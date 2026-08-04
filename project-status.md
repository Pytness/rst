# rst vs st: Implementation Comparison

## Severity key

- Crash: reachable panic or UB from normal or lightly adversarial input.
- Bug: wrong behavior but no crash (inverted logic, wrong direction, silently ignored).
- Missing feature: C has it, Rust has nothing (stub, TODO, or just absent).
- Minor: narrow edge case or cosmetic difference, low real-world impact.

## 1. Crash and panic bugs

These are all reachable just by having a program print an escape sequence. No malicious input required; several of these are things shells, editors, and tmux do routinely.

### 1.1 `tscrollup` panics when the scroll count exceeds the region

C (`st.c:1284`): `for (i = orig; i <= term.bot - n; i++)`. If `term.bot - n` goes negative, signed arithmetic just makes the loop not run.

Rust (`src/term_state.rs:375`): `for i in orig..=(self.bot.saturating_sub(n))`. When `orig == 0` and `n > self.bot`, `saturating_sub` floors at 0, so the loop still runs once with `i = 0`, and `self.line.swap(0, 0 + n)` indexes out of bounds.

Repro: `printf '\e[9999S'` (SU, scroll up) on the default region.

### 1.2 `tinsertblank` (ICH, `CSI @`) has no bounds clamp

C (`st.c:1584-1597`) clamps with `LIMIT(n, 0, term.col - term.c.x)` before computing `dst`/`size`.

Rust (`src/term_state.rs:1071-1080`) has no equivalent clamp, so `size = self.col - dst` underflows as a `usize` when `n` is large.

Repro: `printf '\e[999@'`. Easy to trigger. `tdeletechar` right below it does clamp correctly, so this reads like an isolated omission rather than a deliberate choice.

### 1.3 CUB/CPL cursor-back and cursor-up: unguarded subtraction

C: `case 'D'`/`case 'F'` build a signed delta and let `tmoveto`'s `LIMIT` clamp it.

Rust (`src/csiesq.rs:171-186`): `state.c.x - self.arg[0] as usize` and `state.c.y - self.arg[0] as usize`, plain unsigned subtraction, no `saturating_sub`. CUU (the `A` case) was already fixed to use `saturating_sub`; `D`/`F` were just missed.

Repro: `printf '\e[999D'`, a very common "move cursor to column 0" idiom in shells. Panics in debug, corrupts cursor position in release. Same bug class shows up in RI handling (`src/terminal.rs:750-755`, `self.state.c.y - 1`) when a scroll region with `top > 0` is active.

### 1.4 OSC 104 (reset color) dereferences a null pointer

C (`st.c:2407-2431`): `p` is only set when `par == 4` (a color set). For `par == 104` (reset) it stays NULL and the code correctly skips string handling.

Rust (`src/stresq.rs:178-218`): `p` also stays null for `par == 104`, but the code unconditionally runs `CStr::from_ptr(p as *const i8)` on it, which is UB/segfault on a null pointer.

Repro: a plain `ESC ] 104 BEL` (reset all colors, something shell themes and `tput` send routinely on exit) crashes the terminal.

### 1.5 `tdefcolor` (kitty colored-underline SGR extension) can read one past a 16-slot array

C (`st.c:1620-1673`) reads `attr[*npar+1]` from a fixed, zero-filled 16-slot array. Since C doesn't bounds-check, reading the boundary slot is harmless there; it just reads adjacent memory.

Rust (`src/kitty/mod.rs:24-30`): same `attr[*npar + 1]` read, but on a bounds-checked `[i32; 16]`. If the SGR sequence has exactly `ESC_ARG_SIZ` (16) arguments and the last one is `38`, `48`, or `58`, `attr[16]` is out of bounds and Rust panics, in release builds too, not just debug.

Repro: a crafted 16-argument SGR sequence ending in a color introducer. Narrow, but a real DoS via terminal output, for example from `cat`ing a file or fuzzed pty data.

## 2. Correctness bugs, no crash

### 2.1 `CSI L` (insert blank line) scrolls the wrong direction

C (`st.c:1599-1603`): `tinsertblankline()` calls `tscrolldown()`.

Rust (`src/term_state.rs:1082-1086`) calls `self.tscrollup()` instead, so IL and DL (`CSI M`, which correctly uses `tscrollup`) end up doing the same thing. Very visible in any TUI that uses IL for scroll-region optimization: vim, less, htop, tmux.

### 2.2 `tscrolldown` (reverse-index, `CSI T`) is effectively a no-op

C (`st.c:1250`): `for (i = term.bot; i >= orig + n; i--)`.

Rust (`src/term_state.rs:346`): `for i in (self.bot..orig + n).rev()`. That half-open range only has elements when `self.bot < orig + n`, which is false in the normal case, so the range is empty. Only the pre-clear runs; nothing actually shifts. Combined with 2.1, which relies on `tscrolldown`, reverse scrolling is broken end to end. The fix is `(orig + n..=self.bot).rev()`.

### 2.3 DECTCEM (`CSI ?25h`/`CSI ?25l`, cursor show/hide) is inverted

C (`st.c:1836-1838`): `xsetmode(!set, MODE_HIDE)`, note the negation.

Rust (`src/term_state.rs:1185-1188`): `term.xsetmode(set, WinMode::Hide)`, no negation, so showing the cursor hides it and vice versa. This affects every program that toggles cursor visibility: vim, less, htop, readline. Rust's own RIS handler (`src/terminal.rs:766`) calls this correctly, which confirms this particular call site is the outlier and not a deliberate design choice.

### 2.4 Private-marked `CSI ?S` (XTSMGRAPHICS) falls through into scroll-up

C (`st.c:2106-2135`): the private branch always ends in `break` or `goto unknown`; `tscrollup` is never reached when the sequence is private-marked.

Rust (`src/csiesq.rs:274-314`): the `if self.private { ... }` block has no early return, so after answering the capability query it falls into `state.tscrollup(...)` anyway. Any app probing sixel or graphics geometry with `CSI ?1;1S` also scrolls the screen.

### 2.5 OSC 4 (palette set): inverted query check plus inverted success/failure convention

C: `st.c:2407-2431`. Rust: `src/stresq.rs:178-218`. Three things compound here:

The query check is `!p.is_null() && p_str != "?"`, when it should test `p_str == "?"`. As written, an actual color set takes the query branch, and an actual query falls into the set branch.

`colors.set_color_name()` (`src/colors.rs:149-169`) returns `true` on success, but the call site was ported using C's `xsetcolorname()` convention, where truthy means failure. So a successful set logs "invalid color" and skips repainting, while a failed one repaints as if it worked.

Net effect: OSC 4 palette manipulation doesn't function. (OSC 104 additionally crashes, see 1.4.)

### 2.6 OSC 10/11/12 (dynamic fg/bg/cursor color): same inverted success/failure bug as 2.5

C: `st.c:2388-2406`. Rust: `src/stresq.rs:143-176`. Same root cause as the second point in 2.5: successful sets don't visibly repaint, and failures are silently treated as success.

### 2.7 256-color cube and grayscale ramp aren't scaled to real RGB

C (`x.c:816-824`): cube indices map through xterm's step table (0, 95, 135, 175, 215, 255); the grayscale ramp is `8 + 10*i`.

Rust (`src/colors.rs:117-130`): cube channel values are just the raw 0-5 index, and grayscale is the raw `i-232` (0-23), with no scaling applied. Palette indices 16-231 and 232-255 render as near-black instead of the actual xterm 256-color palette.

### 2.8 Named/hex color resolution silently produces black

C (`x.c:813-829`): `xloadcolor` parses a name or hex string through `XftColorAllocName`.

Rust (`src/colors.rs:107-143`, `load_color`): when a `name` is passed, which is the path OSC 4/10/11/12 use, the function falls through both branches of its `is_none()` check and returns `Color::default()` (black) without ever parsing the string. This compounds 2.5/2.6: even once those inversions are fixed, the resulting color would still come out black.

### 2.9 Wrong attribute cleared when a wide character's dummy cell gets overwritten

C (`st.c:1411-1413`) clears `ATTR_WIDE` on the actual wide glyph to the left.

Rust (`src/term_state.rs:488-490`) clears `ATTR_WDUMMY` instead, a flag that cell doesn't even carry, so `ATTR_WIDE` never gets cleared. That leaves a stale "this is wide" flag around that can desync cursor-width and redraw logic.

### 2.10 VT100 line-drawing charset (`ESC ( 0`) remap gets clobbered immediately

C (`st.c:1402-1427`): the remapped glyph is written once and sticks.

Rust (`src/term_state.rs:479-506`) writes the remapped character, but a later unconditional `self.line[y][x] = *attr; self.line[y][x].u = u;` overwrites it with the original, un-remapped character. So `tput smacs` or ncurses box-drawing through the classic VT100 charset renders literal ASCII instead of line-drawing glyphs. Worth noting this is a different code path from `boxdraw.rs` (see 3.9): both are broken, independently of each other.

### 2.11 `ttyresize` gets called twice with inconsistent pixel dimensions

C (`x.c:775-796`): `cresize()` computes the text-area pixel size once and calls `ttyresize` exactly once.

Rust (`src/app.rs:271-303`) computes it correctly and calls `ttyresize`, but the caller `resize()` (`src/app.rs:215-217`) calls it again afterward with the raw window pixel size, padding included, overwriting the correct value. Any program that queries `TIOCGWINSZ` pixel dimensions, image preview tools or `CSI 14 t` responders for instance, gets a value off by the border padding.

## 3. Missing features

### Input layer

**3.1 The special-key table is almost entirely absent.** `src/keymap.rs` (`KEYMAPS`) has four entries, all for Enter. C's `config.def.h:341-554` has around 190 entries covering arrows, Home/End/PageUp/PageDown/Insert/Delete, all the F-keys, keypad keys, with full modifier-combination and appcursor/appkeypad/numlock branching (`kmap()` in `x.c:2369`). None of those mode bits are even read by `kmap` in Rust. In practice, arrow keys, navigation keys, and function keys probably produce no tty output at all.

**3.2 All keyboard shortcuts are non-functional.** `config::shortcuts` (`src/config.rs:55`) is a permanently empty `Vec`; `src/app.rs:149-157` loops over it with a literal TODO body. The backing functions in `src/x.rs` (`clipcopy`, `clippaste`, `selpaste`, `zoom`, `numlock`, and so on) are all empty stubs, and `x.rs` isn't even declared as a module in `main.rs`, so it doesn't compile into the binary at all. Zoom, clipboard shortcuts, sendbreak, printsel: all dead.

**3.3 Mouse handling is entirely absent.** `bpress`/`brelease`/`bmotion` (`src/app.rs:309-311`) are empty stubs, and the winit event dispatch (`src/app.rs:765-810`) never matches `MouseInput`, `CursorMoved`, or `MouseWheel`; they fall into a catch-all no-op. No mouse-report escape sequences (X10, normal, button-event, any-event, SGR modes) ever get generated, even though the corresponding `WinMode` bits are correctly tracked from DECSET/DECRST (`src/term_state.rs:1199-1222`). The state is tracked, just never consumed.

**3.4 Mouse-driven text selection is entirely absent.** There's no `selstart`/`selextend`/`getsel`/`selinit` equivalent anywhere in the crate (C: `st.c:423-664`). The supporting internals exist (`selnormalize`, `selclear`, `selscroll`, `selected` in `term_state.rs:753-866`), but nothing ever creates a selection since there's no mouse-drag entry point. Word/line double/triple-click snap (`selsnap`) is stubbed out even where the code exists (calls commented out at `term_state.rs:784-785`), and `worddelimiters` (C: `config.def.h:55`) has no Rust equivalent at all.

**3.5 Clipboard (OSC 52) is entirely unimplemented.** `src/stresq.rs:125-137` is fully commented out. No `base64dec` equivalent exists anywhere in the crate. `clipcopy`/`clippaste`/`selpaste` are the dead stubs from 3.2. No clipboard crate is even wired in, this isn't a different backend, it's just absent.

**3.6 Focus in/out is not wired.** `WindowEvent::Focused` is never matched in the winit dispatch. `WinMode::Focused`/`Focus` exist but are never toggled by a real event, so DEC private mode 1004 (focus reporting, `\x1b[I`/`\x1b[O`) never fires and focus-triggered urgency-clear never happens.

Architectural note, not a bug: there's no IME preedit/commit handling (winit `Ime` events); input relies solely on `text_with_all_modifiers()`. Given the keymap table is already the dominant issue here, this is a secondary concern.

### Rendering

**3.7 The cursor is not drawn.** `xdrawcursor()` (`src/app.rs:400-420`) only toggles `ATTR_REVERSE` on the old cursor cell if it was selected; it never draws anything at the new cursor position. No block/underline/bar shape switching, no unfocused hollow-box outline, no blink suppression. `CursorStyle` (`src/win.rs:34-46`) is defined but nothing reads it. Net effect: the cursor is probably invisible in the running terminal. (DECSCUSR, `CSI SP q`, which would set the style, is also unimplemented, see 3.12.)

**3.8 DECSCNM (whole-screen reverse video, `CSI ?5h/l`) is tracked but has no visual effect.** `WinMode::Reverse` is set correctly (`term_state.rs:1157-1159`), but `xdrawglyphfontspecs` (`src/app.rs:457-493`) never checks it, only per-glyph `ATTR_REVERSE` is honored.

**3.9 Box-drawing characters are disconnected from the render pipeline.** `boxdraw/boxdraw.rs`'s classification logic (`isboxdraw`/`boxdrawindex`, faithfully ported from `boxdraw_data.h`) looks correct, but every actual drawing call in `drawbox`/`drawboxlines` is commented out, and none of these functions are called from anywhere (checked `renderers/glyphs.rs`, `app.rs`, `terminal.rs` via grep). Box-drawing glyphs render only via whatever a fallback font happens to provide, not st's crisp procedural pixel drawing. There's no `boxdraw` config knob either.

**3.10 Window/icon title is never set.** Every call site is a commented-out `xsettitle(...)` in `src/stresq.rs:106,121,228` and `src/terminal.rs:969`. No `window.set_title()` call exists anywhere in the crate (also see 2.5's relationship to OSC 0/1/2, this is the window-manager-visible half of the same gap).

**3.11 Bell (audible/visual) and urgency hint are unimplemented.** BEL handling (`src/terminal.rs:526`) is a bare `// TODO: implement ring bell`.

**3.12 DECSCUSR (`CSI SP q`, cursor style) is unimplemented.** The call is commented out in `src/csiesq.rs:418-430`; there's no `xsetcursor` equivalent anywhere.

### Graphics protocols

**3.13 Kitty graphics protocol: effectively 0% implemented.** `graphics.c` is roughly 3900 lines: full command parser, base64 decode, PNG/raw pixel decode, chunked transmission, placements, animation frames, cache/eviction, drawing. `src/graphics.rs` (304 lines) is just struct scaffolding, several fields are literally typed `()` as placeholders, and the stub functions `todo!()` if called. The APC entry point (`src/stresq.rs:244-246`) is a bare `// TODO: implement APC handling`, so every kitty graphics sequence gets silently discarded before it reaches any of the scaffolding. `src/kitty/mod.rs` is unrelated to this protocol, by the way; it implements the SGR colored/styled-underline extension (`tsetdecorcolor`/`tsetdecorstyle`), which is correctly ported.

**3.14 Sixel protocol: about 5% implemented, and it actively hangs the terminal.** `sixel.c` (692 lines, the full DECSIXEL state machine) has no real counterpart. `src/sixel.rs` has struct shells and a correctly-ported palette-generation function, but no byte-consuming parser exists (`SixelParser` has no `parse`/`feed` method), `finalize()` is an empty stub, and `get_image_list()` is `todo!()` (panics if reached). `sixel_hls.c` (HLS to RGB color conversion) has no Rust port at all. Worse than just missing: entering sixel mode sets `TermMode::Sixel` (`src/terminal.rs:812-840`, the actual `sixel_parser_init()` call is commented out), and then the main read loop's sixel-mode branch (`src/terminal.rs:207-211`) does `continue` without advancing the byte pointer. Once a sixel DCS sequence starts, the terminal enters an infinite non-advancing loop and hangs, rather than just failing to render.

**3.15 DCS Sixel finalize / BSU-ESU (synchronized update via `=1s`/`=2s` in sixel DCS) is unimplemented** (`src/stresq.rs:232-241`, TODO stub), consistent with 3.14.

### CLI and config

**3.16 There's no command-line argument parsing at all.** `main.rs` hardcodes `cols=80, rows=24`; `ttynew` is always called with every optional param as `None`. None of st's flags have a Rust equivalent: `-a` (disable altscreen), `-A` (alpha), `-c` (class), `-e` (command), `-f` (font), `-g` (geometry), `-i` (fixed geometry), `-l` (serial/tty line), `-n` (name), `-t`/`-T` (title), `-w` (embed), `-v` (version), and notably `-o` (I/O logging to a chosen file or stdout). The underlying `tprinter`/`out` plumbing is faithfully reimplemented in `ttynew` (`src/terminal.rs:89-129`), but it's dead code since nothing ever passes it a path.

Separate, unrelated issue: the `rst-read.*.log`/`rst-write.*.log` files you'll see in the repo root come from `init_tty_logs`/`log_tty_read`/`log_tty_write` (`src/term_state.rs:23-89`), which unconditionally logs both directions to fixed filenames in the current directory on every run, with no flag to turn it off or redirect it. Don't confuse this with st's `-o`, which is opt-in, single-direction, and path-configurable. This looks more like a debug feature that got left permanently on.

**3.17 Several `config.def.h` knobs have no live Rust equivalent:** `worddelimiters`, `doubleclicktimeout`/`tripleclicktimeout`, `allowaltscreen` (the gate is missing from `tsetmode`'s alt-screen handling in `term_state.rs:1231-1261`; C checks it twice, `st.c:1869,1876`), `allowwindowops`, `bellvolume`, `dynamic_cursor`, `cursorstyle`/`cursorthickness`/the custom snowman cursor, `mouseshape`/`mousefg`/`mousebg`, `cwscale`/`chscale`, `su_timeout`, all the `boxdraw*` knobs, and `mshortcuts[]`/`shortcuts[]` (see 3.2). Also, `termname` is hardcoded to `"xterm-256color"` in Rust versus C's `"xterm-kitty"`, which is a behavioral divergence rather than a plain omission since it affects terminfo-dependent app behavior.

**3.18 Media Copy / printer support (`CSI i`) is unimplemented.** `tdumpline`/`tdumpsel` (`src/term_state.rs:1088-1117`) are empty stubs. Low real-world impact.

**3.19 Synchronized-output mode (`CSI ?2026h/l`) is a stub.** `tsync_begin`/`tsync_end` (`src/term_state.rs:826-844`) are no-ops with the real logic commented out (there's a comment acknowledging this at `src/app.rs:717-726`). Just cosmetic tearing, not correctness-critical.

## 4. Minor divergences

**4.1** `tattrset`/`tsetdirtattr` iterate the full `0..row`/`0..col` in Rust versus C's long-standing `0..row-1`/`0..col-1` quirk (`st.c:1126-1162` vs `term_state.rs:228-259`). Rust is arguably more correct here, but it's an observable difference for the last row/column.

**4.2** `tcursor(CursorLoad)` with no prior save is a no-op in Rust (`static mut C: [Option<TCursor>; 2]` starts as `None`) versus C, which resets to `(0,0)`/`fg=bg=0` from its zero-initialized static. Edge case.

**4.3** The non-UTF-8-mode byte-width check uses `&&` where C uses `||` (`u < 127 && !utf8` vs C's `u < 127 || !MODE_UTF8`, `st.c:2975`). Only matters with UTF-8 mode explicitly disabled and a byte >= 128, a path the Rust code already logs as unsupported.

**4.4** `ttywriteraw_pty` doesn't drain readable pty data while blocked on a write, unlike C's `ttywriteraw` (`st.c:1094-1096`). This reintroduces a write/read deadlock class that C specifically guards against.

**4.5** A handful of uncommon control codes (0x01-0x06, 0x10, 0x12, 0x14-0x1F) fail to interrupt an in-progress OSC/DCS string in Rust, unlike C's catch-all fallthrough (`terminal.rs:639-641` vs `st.c:2843-2844`). Very low real-world impact.

**4.6** Private mode `?9` (X10 mouse) doesn't even track its internal state flag in Rust (commented "IGNORED due to using wayland"), unlike modes 1000/1002/1003/1006 which are implemented. Legacy protocol, low impact.

**4.7** `CSIEscape::dump()` debug output has a stray newline versus C's single-line format (`eprintln!("ESC[")` vs `fprintf` with no newline). Debug-only, no functional impact.

**4.8** Alt-key ESC-prefixing exists but is gated behind a hardcoded `const MOD1: bool = false` (`src/app.rs:185-201`), making the whole block dead code. Alt+key never gets ESC-prefixed or 8th-bit-set.

## 5. Architectural notes (not bugs, legitimate rewrite decisions)

The rendering backend is OpenGL instead of Xlib/Xft/the X11 selection protocol. This is a deliberate architectural choice, not a port gap, and I left it out of the findings above except where it affects behavior (clipboard semantics, for instance, which are simply absent rather than reimplemented differently).

Font shaping uses `rustybuzz` (a Rust port of HarfBuzz) via `shape_text` in `src/font_registry.rs:375-415`, a legitimate equivalent to `hb.c`, not a dropped feature. One minor caveat: shaping always runs against the primary font even when a glyph ultimately renders from a fallback font, so shaping-derived offsets for fallback glyphs use the wrong metrics. A small visual nit for symbol/emoji fallback, not a functional break.

Font fallback chain and bold/italic variant selection are genuinely implemented (`register_font`, `get_char_index`, `glyph_to_font_style` in `app.rs`/`font_registry.rs`), comparable in intent to C's `frc` cache.

## 6. Verified as correct (representative, not exhaustive)

To keep this report honest about what's working, here's what each pass confirmed matches C behavior:

`Glyph`/`GlyphAttribute` bit layout (all 12 flags), `TCursor`, `TermMode`/`WinMode` bitflags, and the `Charset` enum: exact structural match.

`tnew`/`Term::new`, `treset`, `tresetcursor`, `tswapscreen`, `tmoveto`/`tmoveato` (DECOM clamp semantics), `tnewline`, `tclearregion`, `tdeletechar`, `tdeleteline`, `tsetscroll`, `tputtab`, `tfulldirt`/`tsetdirt`, and `tinsertblank`'s core memmove logic (aside from the missing clamp in 1.2).

`tsetattr` (full SGR logic including underline sub-styles, 38/39/48/49/58/59, all color range cases) and `tdefcolor`'s truecolor/indexed dispatch (aside from the array-bound edge case in 1.5).

`tsetmode`: the full DEC private mode table (1, 5, 6, 7, 8, 18, 19, 25, 42, 1000, 1002, 1003, 1004, 1006, 1034, 1048, 2004, 2026, 80, 8452, plus the 47/1047/1049 alt-screen quirk) matches, aside from the missing `allowaltscreen` gate (3.17).

`CSIEscape::parse()`: private marker, sub-parameter separator, `ESC_ARG_SIZ` cap, overflow handling. Matches `csiparse()` faithfully.

Cursor movement CUU/CUD/CUF/HPR/CUP/HVP/CHA/HPA/VPA/CNL, ED/EL/ECH/DCH, DSR, DECSTBM, DECSC/DECRC, XTVERSION, XTWINOPS all match (aside from CUB/CPL in 1.3).

`eschandle` (ESC-prefixed non-CSI dispatch): charset selectors, IND/NEL/HTS/RI/DECID/RIS/DECPAM/DECPNM, ST. All correct.

Default color indices (`defaultfg=258` etc.) and the special-color entries (256-259) match `config.def.h` exactly.

256-color palette-generation structure (the default-16 table, the loop shape) is right; only the cube/grayscale scaling math is wrong (2.7).

## 7. Suggested priority order

1. Fix the five crash bugs (section 1). These can take down the terminal from ordinary output.
2. Fix the inverted-logic bugs (2.1-2.6). Cheap fixes with high behavioral impact, scrolling and cursor visibility are core to any interactive use.
3. Fix color rendering (2.7-2.8). Right now the 256-color palette and any OSC-set colors render wrong or black.
4. Wire up keyboard (3.1) and mouse (3.3-3.4). Without these the terminal is basically unusable interactively beyond plain text entry.
5. Implement cursor rendering (3.7) and window title (3.10). An invisible cursor and a static title are the first things any user will notice.
6. Decide scope for the two image protocols (3.13-3.14). Both are large, thousands of C lines, and currently near 0%. Sixel additionally needs the hang bug neutralized even if it stays unimplemented, for example by not entering `TermMode::Sixel` until parsing exists.
7. CLI argument parsing (3.16) and the remaining config knobs (3.17), needed for the binary to work as a drop-in `st` replacement.

## 8. Fix status checklist

Checked against the commit history from `ea200f4` through the latest fixes at the time of writing. `[x]` means fixed and verified in current source; `[ ]` means still open, including partial fixes, which are noted inline.

- [ ] 1 Crash / panic bugs
    - [x] 1.1 `tscrollup` panic on large scroll count, fixed in `6326420` (`checked_sub` guard)
    - [x] 1.2 `tinsertblank` underflow, fixed in `5ab673f` (clamps `n` to `col - c.x`)
    - [x] 1.3 CUB/CPL/RI unguarded subtraction, fixed in `7c31ddfd`
    - [x] 1.4 OSC 104 null-pointer deref, fixed in `884bb0a` (`p_str` now `Option`, only dereferenced when non-null)
    - [ ] 1.5 kitty `tdefcolor` array OOB is still untouched, `attr[*npar + 1]` still unguarded (`src/kitty/mod.rs:30`)
- [ ] 2 Correctness bugs, no crash
    - [x] 2.1 IL scrolling wrong direction, fixed in `0497ee5` (now calls `tscrolldown`)
    - [x] 2.2 `tscrolldown` no-op, fixed in `4172cb7` (range direction corrected)
    - [x] 2.3 DECTCEM inverted, fixed in `ce9908b5`
    - [x] 2.4 Private `CSI ?S` falls through to scroll-up, fixed in `4e7eaf4f` (adds `unknown(); return;` after the private block, matching C's unconditional `goto unknown`)
    - [x] 2.5 OSC 4 set/query inversion, success/failure convention corrected in `884bb0a`, the '?' check corrected in `baa0faed`
    - [x] 2.6 OSC 10/11/12 success/failure inversion, fixed in `884bb0a`
    - [ ] 2.7 256-color cube/grayscale not scaled to RGB, still untouched (`src/colors.rs:117-130`)
    - [ ] 2.8 Named/hex color resolution produces black, still untouched (`src/colors.rs:107-143`)
    - [x] 2.9 Wrong attribute cleared on wide-glyph dummy overwrite, fixed in `657d1fe`
    - [x] 2.10 VT100 line-drawing remap clobbered, fixed in `6691241`
    - [x] 2.11 `ttyresize` double-called with inconsistent size, fixed in `3d7be591` (wrong pixel dimensions were being passed to ortho and TextRenderer)
- [ ] 3 Missing features
    - [ ] 3.1 Special-key table almost entirely absent, untouched
    - [ ] 3.2 Keyboard shortcuts non-functional, untouched
    - [ ] 3.3 Mouse handling entirely absent, untouched
    - [ ] 3.4 Mouse-driven text selection entirely absent, untouched
    - [ ] 3.5 Clipboard (OSC 52) unimplemented, untouched
    - [x] 3.6 Focus in/out not wired, implemented in `b4d6c7ed`
    - [ ] 3.7 Cursor not drawn, untouched
    - [ ] 3.8 DECSCNM tracked but no visual effect, untouched
    - [ ] 3.9 Box-drawing disconnected from render pipeline, untouched
    - [ ] 3.10 Window/icon title never set, untouched
    - [ ] 3.11 Bell/urgency unimplemented, untouched
    - [ ] 3.12 DECSCUSR unimplemented, untouched
    - [ ] 3.13 Kitty graphics protocol ~0% implemented, untouched
    - [ ] 3.14 Sixel protocol ~5% implemented: partially fixed. The infinite-loop hang is fixed in `313f433` (byte pointer now advances, discarding bytes up to the next DCS terminator instead of looping forever), but the actual DECSIXEL parser/renderer is still unimplemented
    - [ ] 3.15 DCS Sixel finalize / BSU-ESU unimplemented, untouched
    - [ ] 3.16 No CLI argument parsing, untouched
    - [ ] 3.17 Config knobs with no live equivalent, untouched
    - [ ] 3.18 Media Copy (`CSI i`) unimplemented, untouched
    - [ ] 3.19 Synchronized-output mode is a stub, untouched
- [ ] 4 Minor divergences
    - [x] 4.1 `tattrset`/`tsetdirtattr` off-by-one vs C quirk, untouched, and this is arguably a bug on st's side, not ours
    - [x] 4.2 `tcursor(CursorLoad)` no-op with no prior save, fixed in `28bdc31e`
    - [x] 4.3 Non-UTF-8-mode byte-width check `&&` vs `||`, fixed in `da0823d`
    - [x] 4.4 `ttywriteraw_pty` doesn't drain readable pty while blocked on write, fixed in `007a69b`. It deleted the buggy `TermState` duplicate (which lacked the drain) and delegated to `Term::ttywriteraw_pty`, which already had the correct `if FD_ISSET(rfd) { lim = self.ttyread(); }` drain in place
    - [x] 4.5 Uncommon control codes don't interrupt in-progress OSC/DCS, fixed in `fbce093` (`interrupt_sequence = true` added to the catch-all arm)
    - [ ] 4.6 Private mode `?9` (X10 mouse) state not tracked, untouched
    - [x] 4.7 `CSIEscape::dump()` stray newline, fixed in `8e30aee1`
    - [ ] 4.8 Alt-key ESC-prefixing dead code behind `MOD1: bool = false`, untouched

### Fixes in the same commit range with no matching entry above

These landed alongside the fixes above but address things outside this report's original scope:

- `48009ba`: the utf8 decoder didn't distinguish invalid from incomplete sequences, which caused hangs on invalid UTF-8
- `ea200f4`: follow-up fix to the incomplete-UTF-8-sequence retention logic
- `1884a6c`: incorrect `ttyread` buffer size check
- `bc1bffb`: missing ONLCR mode handling (a feature add, not a fix)
