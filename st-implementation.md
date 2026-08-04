# st (with kitty-graphics/sixel patches) — Implementation Reference

This document describes the implementation of this `st` fork: every function
and struct/type, grouped by file, with a short description of its purpose and
how it's used. It is meant as a checklist/reference for porting the codebase
to another language (e.g. Rust) — it does not include code, only structure
and behavior.

This fork of `st` (the suckless terminal) adds: 24-bit "decor" (underline
style/color) attributes, SIXEL graphics, a subset of the kitty graphics
protocol (images, animations, Unicode placeholders), OSC 8-less clickable URL
highlighting, HarfBuzz text shaping, box-drawing/braille glyph rendering, and
Xft-DPI-aware fonts.

## File Map

| File | Role |
|---|---|
| `st.h` | Shared types/macros/globals used across the whole program (Term, Glyph, TCursor, Window structs, function declarations). |
| `win.h` | Interface the terminal core (`st.c`) uses to talk to the X11 frontend (`x.c`). |
| `st.c` | Terminal emulation core: PTY I/O, ANSI/DEC escape sequence parsing and execution, screen buffer management, selection, UTF-8. |
| `x.c` | X11 frontend: window/font management, glyph rendering via Xft, input (keyboard/mouse), event loop, `main()`. |
| `graphics.h` / `graphics.c` | Kitty graphics protocol implementation: image/frame/placement lifecycle, on-disk cache, RAM budget eviction, animation timing, protocol command parsing. |
| `sixel.h` / `sixel.c` | DEC SIXEL bitmap graphics parser (adapted from mintty/xterm) producing `ImageList` cell images. |
| `sixel_hls.c` / `sixel_hls.h` | HLS→RGB color conversion used by the SIXEL color-introducer command. |
| `boxdraw_data.h` / `boxdraw.c` | Box-drawing and braille Unicode block glyph rendering (drawn as rectangles instead of font glyphs). |
| `hb.h` / `hb.c` | HarfBuzz text-shaping integration (ligatures, cluster-aware glyph positioning) with a font cache. |
| `rowcolumn_diacritics_helpers.c` | Lookup table mapping Unicode "row/column diacritics" combining marks to sequential numbers (kitty Unicode-placeholder image protocol). |
| `khash.h` / `kvec.h` | Third-party generic hash-map / vector macros (klib), used internally by `graphics.c`. Not project-specific. |
| `arg.h` | Minimal suckless-style CLI argument parsing macros used by `x.c`'s `main()`. |
| `config.def.h` / `config.h` | Compile-time configuration: fonts, colors, keybindings, mouse shortcuts, graphics limits, behavior toggles. `config.h` is the user's copy of `config.def.h`. |

---

## `st.h` — Shared Types and Macros

### Key macros
- `MIN`, `MAX`, `LEN`, `BETWEEN`, `DIVCEIL`, `DEFAULT`, `LIMIT`, `MODBIT`, `TIMEDIFF` — generic helpers.
- `ATTRCMP(a, b)` — compares two glyphs' mode/fg/bg/decor to see if they can be batched in one draw call (ignores `ATTR_WRAP`).
- `TRUECOLOR(r,g,b)` / `IS_TRUECOL(x)` — pack/detect a 24-bit RGB color into a 32-bit int using bit 24 as a "is truecolor" flag.
- `DECOR_DEFAULT_COLOR` — sentinel value in `decor` meaning "use the fg color for the underline."

### Enums
- `glyph_attribute` — bitflags stored in `Glyph.mode`: bold, faint, italic, underline, blink, reverse, invisible, struck, wrap, wide, wdummy (2nd cell of a wide glyph), boxdraw, image (this cell is part of an image placeholder), url, sixel (covered by a sixel image).
- `screen` — `S_PRI` / `S_ALL` / `S_ALT`, used to scope keybindings/mouse shortcuts to primary/alt screen.
- `drawing_mode` — `DRAW_BG` / `DRAW_FG`, controls whether `xdrawglyphfontspecs` renders background or foreground in a given pass.
- `selection_mode` — `SEL_IDLE` / `SEL_EMPTY` / `SEL_READY`.
- `selection_type` — `SEL_REGULAR` / `SEL_RECTANGULAR`.
- `selection_snap` — `SNAP_WORD` / `SNAP_LINE` (double/triple click).
- `underline_style` — straight/double/curly/dotted/dashed.

### Structs / typedefs
- **`ImageList`** — a doubly-linked list node representing one row-slice of a legacy SIXEL image anchored to the terminal grid (`x`,`y` cell position, `pixels` RGBA buffer, cached `pixmap`/`clipmask`, `cols`/`cw`/`ch`, `transparent` flag). Distinct from the kitty-protocol `Image`/`ImagePlacement` structs in `graphics.c`; used for classic sixel rendering and scrolling.
- **`Glyph`** — one terminal cell: `u` (rune), `mode` (attribute bitflags), `fg`/`bg` (color, plain index or truecolor via `TRUECOLOR`), `decor` (packed underline color+style, or repurposed to store image placement id — see accessors below).
- **`Line`** — `Glyph *`, one row.
- **`TCursor`** — cursor position (`x`,`y`), current attribute template (`attr`), and `state` (wrapnext/origin flags).
- **`Term`** — the whole terminal state: primary/alt screen (`line`/`alt`), per-row `dirty` flags, cursor, scroll region (`top`/`bot`), `mode` (term_mode flags from `st.c`), escape-parsing state (`esc`), charset translation table, tab stops, `images`/`images_alt` (sixel image lists per screen), `lastc` (last printed rune, for REP).
- **`Arg`** — tagged union (`int`/`uint`/`float`/`const void*`/`const char*`) used to pass a single parameter to shortcut/mouse-shortcut callback functions.
- **`TermWindow`** — pixel-level window geometry: tty/window width & height, border padding, cell width/height, mode flags, cursor style.
- **`XWindow`** — all X11 handles: `Display`, `Colormap`, `Window`, back-buffer `Drawable`, glyph-spec scratch buffer, various atoms, input-method (`ime`) state, `Visual`, geometry.
- **`XSelection`** — clipboard/selection state: target atom, `primary`/`clipboard` text, double/triple-click timestamps.
- **`Shortcut`** — one keyboard shortcut binding: modifier mask, `KeySym`, callback, `Arg`, and `screen` scoping (config.h data).
- **`MouseShortcut`** — one mouse-button binding: modifier, button, callback, `Arg`, whether it fires on press or release, screen scoping.
- **`Key`** — one custom keymap entry: `KeySym`, modifier mask, output string, three-valued app-keypad/app-cursor mode gating.
- **`Font`** — a loaded Xft font plus metrics (height/width/ascent/descent, bad-slant/bad-weight flags, `XftFont*`, `FcFontSet*`, `FcPattern*`).
- **`DC`** ("Drawing Context") — allocated `Color` palette array, the four `Font` variants (regular/bold/italic/bold-italic), and a `GC`.

### Decoration / image-placeholder bit-packing accessors (inline functions)
`Glyph.decor` doubles as either an underline color+style (`tgetdecorcolor`, `tgetdecorstyle`, `tsetdecorcolor`, `tsetdecorstyle`) or an image placement id (`tgetimgplacementid`, `tsetimgplacementid`), disambiguated by `DECOR_DEFAULT_COLOR`.

`Glyph.u` doubles as either a real rune, or (when `ATTR_IMAGE` is set and it's a *classic* placeholder) a packed encoding of: 1-based row (9 bits), 1-based column (9 bits), most-significant-byte-of-image-id-plus-1 (9 bits), diacritic count (2 bits), classic-vs-unicode flag (1 bit). Accessors: `tgetimgrow`/`tsetimgrow`, `tgetimgcol`/`tsetimgcol`, `tgetimgid4thbyteplus1`/`tsetimg4thbyteplus1`, `tgetimgdiacriticcount`/`tsetimgdiacriticcount`, `tgetisclassicplaceholder`/`tsetisclassicplaceholder`. `tgetimgid`/`tsetimgid` compose/decompose the full 32-bit image id from the 24-bit `fg` field plus the 4th-byte-plus-1 bits (the "naive" implementation — doesn't infer the MSB from neighboring cells).

### Extern globals
`dc`, `xw`, `xsel`, `win`, `term` (the core state singletons, defined in `x.c`), plus config.h-provided globals (`utmp`, `scroll`, `stty_args`, `vtiden`, `worddelimiters`, `allowaltscreen`, `allowwindowops`, `termname`, `tabspaces`, `defaultfg/bg/cs`, `urlhandler`, `urlchars`, `urlprefixes`, `nurlprefixes`) and `boxdraw`/`boxdraw_bold`/`boxdraw_braille` toggles.

### Function declarations (implemented in `.c` files, see below)
`die`, `redraw`, `draw`, `drawregion`, `tfulldirt`, `printscreen`, `printsel`, `sendbreak`, `toggleprinter`, `tattrset`, `tisaltscr`, `tnew`, `tresize`, `tsetdirtattr`, `ttyhangup`, `ttynew`, `ttyread`, `ttyresize`, `ttywrite`, `resettitle`, `selclear`, `selinit`, `selremove`, `selstart`, `selextend`, `selected`, `getsel`, `getglyphat`, `highlighturls`, `unhighlighturls`, `followurl`, `utf8encode`, `xmalloc`, `xrealloc`, `xstrdup`, `isboxdraw`, `boxdrawindex`, `boxdraw_xinit`, `drawboxes`, `xgetcolor`.

---

## `win.h` — Terminal-core → X11-frontend Interface

### `enum win_mode`
Window-level mode bitflags (distinct from `term_mode` in `st.c`): visible, focused, app-keypad, mouse button/motion/x10/many reporting, reverse video, kbd-lock, hide, app-cursor, SGR mouse mode, 8-bit, blink/fblink, focus-event reporting, bracketed paste, numlock. `MODE_MOUSE` is a convenience OR of all mouse-reporting submodes.

### Declared functions (implemented in `x.c`)
`xbell`, `xclipcopy`, `xdrawcursor`, `xdrawline`, `xfinishdraw`, `xloadcols`, `xsetcolorname`, `xgetcolor`, `xseticontitle`, `xsettitle`, `xsetcursor`, `xsetmode`, `xsetpointermotion`, `xsetsel`, `xstartdraw`, `xximspot`, `xstartimagedraw`, `xfinishimagedraw` — the abstract "screen" API `st.c` calls into without knowing about X11 details.

---

## `st.c` — Terminal Emulation Core

### Local types
- **`Selection`** — mouse-selection state: `mode`/`type`/`snap`, normalized (`nb`/`ne`) and original (`ob`/`oe`) begin/end coordinates, and `alt` (which screen the selection belongs to).
- **`CSIEscape`** — an in-progress/parsed CSI (`ESC [`) sequence: raw buffer, `priv` (`?` prefix), parsed integer args, final mode byte(s).
- **`STREscape`** — an in-progress/parsed string-type sequence (DCS/OSC/PM/APC): type char, growable raw buffer, `;`-split `args`, terminator (`ST` or `BEL`).

### Enums (internal)
`term_mode` (wrap, insert, altscreen, crlf, echo, print, utf8, sixel, sixel-cursor-right, sixel-display-mode), `cursor_movement` (save/load), `cursor_state` (default/wrapnext/origin), `charset` (G0-G3 designations), `escape_state` (bitflags tracking which escape-sequence type is being accumulated).

### Synchronized-update ("sync") helpers
- `tsync_begin` / `tsync_end` / `tinsync` — implement terminal synchronized-output (BSU/ESU, mode 2026): suppresses redraws between begin/end (with a timeout guard) so multi-part updates appear atomically.

### UTF-8 / base64
- `utf8decode`, `utf8decodebyte`, `utf8encode` (also declared in `st.h`, used externally), `utf8encodebyte`, `utf8validate` — a standard from-scratch UTF-8 codec.
- `base64dec_getc`, `base64dec` — base64 decoder used for OSC 52 clipboard payloads (skips non-printable filler bytes).

### Low-level utilities
- `xwrite` — retrying `write()` wrapper.
- `xmalloc`, `xrealloc`, `xstrdup` — allocation wrappers that call `die()` on failure.
- `die` — prints message to stderr and `exit(1)`.

### Selection
- `selinit` — reset selection to idle/empty.
- `tlinelen` — visible length of a row (trims trailing spaces unless the row ends with `ATTR_WRAP`).
- `selstart` — begin a new selection at a cell, with a snap mode.
- `selextend` — update the end of an in-progress selection; finalizes it if `done`.
- `selnormalize` — computes normalized begin/end (`nb`/`ne`) from original (`ob`/`oe`), applying regular vs rectangular semantics and snap expansion, and extends selection across wrapped lines.
- `selected` — is `(x,y)` inside the current selection (regular or rectangular hit test), also checks the selection is on the currently active screen.
- `selsnap` — expands a coordinate outward to the nearest word or line boundary depending on `sel.snap` (used by double/triple click).
- `getsel` — serializes the selected text to a freshly allocated UTF-8 string (encodes image cells as a placeholder char; converts internal `\n` semantics; drops trailing spaces of wrapped lines correctly).
- `selclear` / `selremove` — clear the current selection (external vs. internal entry points).
- `selscroll` — adjusts/clears the selection when the buffer scrolls (called from `tscrollup`/`tscrolldown`).

### URL highlighting (custom feature, not upstream st)
- `strstrany` — first substring match among a NULL-terminated array of candidate strings.
- `highlighturls` — scans all rows for any `urlprefixes` substring and tags matching runs of `urlchars` with `ATTR_URL` (invoked when Ctrl is held).
- `unhighlighturls` — clears `ATTR_URL` from all cells (invoked on Ctrl release).
- `followurl` — given a click position, expands left/right over `urlchars` to find the URL boundaries, validates a known prefix is present, then forks/execs `urlhandler` with the URL as the sole argument.

### PTY / process management
- `execsh` — sets up environment and `execvp`s the shell/command in the child process (also handles `scroll`/`utmp` wrapper programs).
- `sigchld` — SIGCHLD handler; calls `die()` once the child exits/is signaled.
- `stty` — builds and runs an `stty` command line from `stty_args` + extra args (used for `-l` line mode).
- `ttynew` — opens a PTY (or a given tty line), forks, and execs the shell in the child; sets up SIGCHLD handling in the parent. Returns the master fd.
- `ttyread_pending` — returns whether an aborted `twrite` still has bytes to process (used by the event loop to avoid blocking on `select`).
- `ttyread` — reads available bytes from the pty into a static buffer and feeds them to `twrite`, handling partial UTF-8 sequences and recursive-call protection.
- `ttywrite` — public write entry point; optionally echoes locally, and expands `\r`→`\r\n` if `MODE_CRLF` is set.
- `ttywriteraw` — low-level throttled write to the pty using `pselect` to interleave reading (to avoid deadlock/buffer-clogging on slow lines).
- `ttyresize` — issues `TIOCSWINSZ` to inform the kernel/pty of the new terminal size (rows/cols/pixel dims).
- `ttyhangup` — sends `SIGHUP` to the child process.

### Screen/attribute state queries and mutation
- `tattrset` — true if any cell in the visible screen has a given attribute bit set (used e.g. to decide whether blink redraw timers are needed).
- `tsetdirt` — marks a range of rows dirty for redraw.
- `tsetdirtattr` — marks dirty any row containing a cell with a given attribute.
- `tsetsixelattr` — sets `ATTR_SIXEL` across a column range of a row (marks cells as "covered by a sixel image" for erase/redraw bookkeeping).
- `tfulldirt` — ends any in-progress sync and marks the whole screen dirty.
- `tcursor` — save/load cursor state to/from a small per-altscreen static array (DECSC/DECRC and similar).
- `tresetcursor` — resets cursor to origin with default attributes.
- `treset` — full terminal reset (RIS): resets cursor, tabs, scroll region, mode flags, charset table; clears and deletes images on both screens.
- `tnew` — initializes a fresh `Term` with default colors and calls `tresize`+`treset`.
- `tisaltscr` — whether alt screen is active.
- `tswapscreen` — swaps primary/alt screen line arrays and their respective image lists; toggles `MODE_ALTSCREEN`; marks fully dirty.
- `tscrolldown` / `tscrollup` — scroll a region of rows down/up by `n`, shuffling `Line` pointers, clearing the vacated rows, moving/deleting `ImageList` entries that fall (partially) outside the scrolled region, and adjusting the selection via `selscroll`.
- `tnewline` — moves cursor to next line (optionally first column for CRLF-style), scrolling if at the bottom margin.

### CSI parsing/handling
- `csiparse` — tokenizes the raw CSI buffer into `priv`, integer args (`;` or `:` separated), and the final mode byte(s).
- `tmoveato` / `tmoveto` — absolute cursor moves, honoring DECOM (origin mode) scroll-region clamping; clears wrapnext.
- `tsetchar` — writes one rune+attributes into a cell; handles VT100 graphics-charset substitution table, wide-char dummy-cell cleanup, the "don't overwrite classic image placeholder with a space" workaround, and tagging `ATTR_IMAGE`/`ATTR_BOXDRAW` as appropriate.
- `tclearregion` — clears a rectangular region to the current cursor background/decor, clearing selection if it overlapped.
- `tcreateimgplaceholder` — fills a rectangle starting at the cursor with a *classic* (kitty-protocol) image placeholder, optionally saving the previously-underneath glyphs (recursing through `gr_get_glyph_underneath_image` if a placeholder was already present), inserting newlines as needed, and positioning the cursor per protocol rules (`do_not_move_cursor`).
- `gr_for_each_image_cell` (declared in `graphics.h`, implemented here since it needs `term`) — invokes a callback for every cell tagged `ATTR_IMAGE`, marking the row dirty if the callback mutated it.
- `gr_schedule_image_redraw_by_id` — marks dirty every row containing a cell whose image id matches, for animation/redraw scheduling from the graphics module.
- `tdeletechar` / `tinsertblank` — DCH/ICH: shift a row's cells left/right by `n`, clearing the vacated cells.
- `tinsertblankline` / `tdeleteline` — IL/DL: delegate to `tscrolldown`/`tscrollup` when inside the scroll region.
- `tdeleteimages` — deletes every `ImageList` node on the current screen (used by ED-2/6 style screen clears).
- `tdefcolor` — parses SGR 38/48/58 extended color subparameters (`5;n` indexed or `2;r;g;b` truecolor, including the colon-separated ITU form), returns packed color or -1 on error.
- `tsetattr` — applies SGR parameters to the cursor's pending attribute (`term.c.attr`), including reset, bold/faint/italic/underline(+style subparam)/blink/reverse/invisible/struck, individual attribute resets, extended fg/bg/decor color (38/48/58/59), and standard/bright 8-color palette codes.
- `tsetscroll` — sets the scroll region top/bottom (normalizing order).
- `tsetmode` — handles both DEC private (`?`) and ANSI SM/RM mode-setting for the full range of supported modes: cursor keys, reverse video, origin, autowrap, cursor visibility, all mouse-tracking variants, focus events, SGR mouse extension, 8-bit, alt-screen switch variants (47/1047/1049/1048 with cursor save/load), bracketed paste, synchronized-update (2026), keyboard lock, insert mode, send/receive (echo), linefeed/newline mode, sixel display-mode toggle (80), and sixel-cursor-right-after-scroll toggle (8452).
- `csihandle` — the big CSI dispatch switch: cursor movement (`A`-`H`, `a`,`d`,`e`,`f`), tab handling (`I`,`Z`,`g`), REP (`b`), device attributes/status (`c`,`n`), media-copy/print (`i`), erase-in-display/-line (`J`,`K`), scroll (`S`,`T`), insert/delete line/char (`L`,`M`,`P`,`X`,`@`), set/reset mode (`h`,`l`), SGR (`m`), scroll region (`r`), cursor save/restore (`s`,`u`), cursor style (` q`), XTVERSION (`>q`), window-ops pixel/char size queries (`t`), DSR-EXT synchronized-updates feature query (`$p`), and XTSMGRAPHICS sixel-registers/geometry queries (private `S`). Unknown sequences fall through to `csidump`.
- `csidump` — prints the raw CSI sequence to stderr for diagnostics.
- `csireset` — zeroes the `CSIEscape` accumulator.
- `osc_color_response` — builds and sends an OSC "rgb:RRRR/GGGG/BBBB" response for color queries (OSC 4/10/11/12 with `?` argument).

### STR (OSC/DCS/PM/APC) parsing/handling
- `strhandle` — dispatches a completed string-type sequence: OSC (window/icon title 0/1/2, OSC 52 clipboard set via base64, OSC 4/10/11/12/104 color get/set/reset with `osc_color_response`), old-style title (`k`), DCS (`P`) — finalizes an in-progress sixel image via `sixel_parser_finalize`, merges/replaces overlapping old `ImageList` entries, positions the new image(s) (handling `MODE_SIXEL_SDM` display-mode vs normal cursor-relative placement, and `MODE_SIXEL_CUR_RT` cursor-after-graphic mode), and also recognizes BSU/ESU (`=1s`/`=2s`) synchronized-update DCS commands; APC (`_`) — delegates to `gr_parse_command` (kitty graphics protocol) and, if the response indicates a placeholder should be created, calls `tcreateimgplaceholder`, writes any protocol response, and marks the screen dirty if requested; PM (`^`) is ignored.
- `strparse` — splits the accumulated STR buffer on `;` into `args` (with a special case preserving embedded `;` in OSC 0/1/2 title text and OSC 7).
- `strdump` — prints the raw string sequence to stderr for diagnostics.
- `strreset` — (re)allocates the STR accumulator buffer.

### Printing / misc user-facing actions
- `sendbreak` — sends a TTY break.
- `tprinter` — writes bytes to the print/output fd (`-o` option or MODE_PRINT).
- `toggleprinter`, `printscreen`, `printsel` — shortcut-callback wrappers around MODE_PRINT toggling and `tdump`/`tdumpsel`.
- `tdumpsel` — prints the current selection via `tprinter`.
- `tdumpline` — prints one row (trimmed) via `tprinter`.
- `tdump` — prints every row.

### Tabs / charsets / DEC private sequences
- `tputtab` — moves the cursor forward/backward by `n` tab stops.
- `tdefutf8` — `%G`/`%@` — enable/disable UTF-8 mode.
- `tdeftran` — `ESC ( / ) / * / +` — designates a charset (graphics/USA) into `trantbl`.
- `tdectest` — DEC screen-alignment test (`ESC # 8`): fills the screen with 'E'.
- `tstrsequence` — begins accumulating a DCS/APC/PM/OSC string sequence, mapping C1 single-byte forms to their two-byte ESC equivalents.

### Main input dispatch
- `tcontrolcode` — handles all C0/C1 control codes: tab, backspace, CR, LF/VT/FF, BEL (also finalizes a BEL-terminated string sequence), ESC (starts new escape), SO/SI (charset shift), SUB/CAN (abort sequence), and various mostly-ignored/TODO C1 codes, dispatching OSC/DCS/PM/APC starters to `tstrsequence`.
- `dcshandle` — currently only handles DECSIXEL (`q`): computes the background color (from truecolor or the color table, honoring alpha for the default bg), calls `sixel_parser_init`, and sets `MODE_SIXEL`.
- `eschandle` — handles two-character (non-CSI) escape sequences: enters CSI/TEST/UTF8/DCS/APC/PM/OSC accumulation modes, locking-shift (`n`/`o`), charset designators (`(`,`)`,`*`,`+`), IND/NEL/HTS/RI cursor-and-scroll operations, DECID, RIS (full reset + retitle + reload colors), DECPAM/DECPNM keypad mode, DECSC/DECRC, and ST (string terminator, finalizes a pending string sequence).
- `tputc` — the core character-ingestion state machine: computes UTF-8 width, handles MODE_PRINT echo, accumulates STR-type sequences until a terminator, dispatches control codes immediately (even mid-sequence), otherwise accumulates ESC/CSI/DCS bytes or dispatches to the appropriate escape handler; for printable text, handles zero-width combining characters (as row/column/id diacritics for Unicode image placeholders, via `diacritic_to_num`), autowrap, insert-mode shifting, wide-character dummy-cell bookkeeping, and advances the cursor.
- `twrite` — the top-level buffer-consuming loop: routes bytes into the sixel parser while `MODE_SIXEL` is active, otherwise UTF-8-decodes (or passes raw bytes) and calls `tputc`, with support for the "show control chars as `^X`" debug mode and cooperative abort (`twrite_aborted`) to interrupt a batch when a synchronized update ends (ESU) mid-parse.

### Resize / draw entry points
- `tresize` — resizes both screens to new col/row counts: scrolls content up if shrinking, reallocates row arrays and tab stops, clears newly exposed area, and re-tags sixel-attribute cells and deletes/clips `ImageList` entries that fall outside the new bounds.
- `resettitle` — resets the window title to the default.
- `drawregion` — redraws dirty rows in `[y1,y2)` between columns `[x1,x2)`, wrapping the image-drawing start/finish calls around the loop.
- `draw` — top-level frame redraw: clamps/dedumbs old and new cursor positions (wide-dummy cells), redraws the whole region, draws the cursor overlay, and notifies the input method of cursor movement (`xximspot`).
- `redraw` — marks everything dirty then calls `draw`.
- `getglyphat` — returns a copy of the `Glyph` at `(col,row)` (used by mouse-click image-preview/info shortcuts in `x.c`).

---

## `x.c` — X11 Frontend

### Local types
- **`Fontcache`** entry — a fallback font (`XftFont*`) plus flags (`FRC_NORMAL`/`ITALIC`/`BOLD`/`ITALICBOLD`) and the codepoint it was resolved for; `frc`/`frclen`/`frccap` form a growable array used when the primary font is missing a glyph.
- Local re-declarations of several `st.h` types are present but commented out (kept as documentation of the shared ABI, actual definitions live in `st.h`).

### Globals defined here
`dc`, `term`, `xw`, `win`, `xsel` (the singleton state structs declared `extern` in `st.h`), `mouse_col`/`mouse_row` (last mouse cell position, used by image preview/info actions), font-cache/geometry statics, CLI option strings (`opt_*`), `buttons` (pressed-button bitmask), `cursorblinks`.

### Shortcut-callback functions (bound via `config.h` `shortcuts[]`/`mshortcuts[]`)
- `clipcopy` — copies PRIMARY selection text into CLIPBOARD and claims ownership.
- `clippaste` — requests CLIPBOARD contents be delivered (async, completed in `selnotify`).
- `numlock` — toggles `MODE_NUMLOCK`.
- `selpaste` — requests PRIMARY selection contents.
- `zoom` / `zoomabs` / `zoomreset` — relative/absolute/default font-size change: reloads fonts at the new size, frees old image pixmaps/clipmasks so they get regenerated at the new cell size, and triggers a resize+redraw.
- `ttysend` — writes a fixed string (from the shortcut's `Arg`) to the pty.
- `previewimage` — resolves the image under the last-clicked cell and calls `gr_preview_image` with a user-configured viewer command.
- `showimageinfo` — resolves the image/placement under the last-clicked cell and calls `gr_show_image_info`, which opens a `less` view of debug info (spawns `<argv0> -e less <tmpfile>`).
- `togglegrdebug` — cycles `graphics_debug_mode` (none/log/log+boxes) and redraws.
- `dumpgrstate` — dumps the graphics module's internal state to stderr.
- `unloadimages` — forces the graphics module to unload cached images from RAM.
- `toggleimages` — toggles whether images are actually rendered (vs. just bounding boxes) and redraws.

### Mouse handling
- `evcol` / `evrow` — converts an `XEvent`'s pixel coordinates to a clamped terminal cell column/row.
- `mousesel` — extends or finalizes a text selection during mouse drag, choosing regular/rectangular selection type from the held modifier vs `selmasks`; calls `setsel` on release.
- `mousereport` — encodes and sends X10/SGR/UTF-8-legacy mouse button/motion escape sequences per the currently enabled mouse-tracking mode.
- `buttonmask` — maps an X `Button1..5` constant to its corresponding state-mask bit.
- `mouseaction` — checks Ctrl+Button1 for URL-follow, then matches the event against `mshortcuts[]` (respecting per-screen scoping and forced-mouse-modifier override), invoking the bound callback.
- `bpress` — tracks pressed-button bitmask; if mouse-reporting is active (and not overridden) reports it instead of selecting; otherwise tries `mouseaction`, then (Button1) starts a selection with double/triple-click snap detection based on click timing.
- `brelease` — mirrors `bpress` for release events; finalizes selection via `mousesel(e, 1)`.
- `bmotion` — reports motion if mouse-tracking is active, otherwise extends the in-progress selection.

### Selection / clipboard (X11 side)
- `propnotify` — handles incremental (INCR) selection transfer continuation via `PropertyNotify`.
- `selnotify` — receives selection data (possibly via INCR large-transfer protocol), converts embedded `\n`→`\r`, wraps it in bracketed-paste markers if enabled, and writes it to the pty.
- `xclipcopy` — thin wrapper calling `clipcopy(NULL)` (the `win.h` entry point).
- `selclear_` — X11 event handler wrapper around `selclear` (currently unregistered — commented out in the handler table).
- `selrequest` — services another application's `XConvertSelection` request: replies with supported TARGETS, or the actual PRIMARY/CLIPBOARD text.
- `setsel` / `xsetsel` — takes ownership of PRIMARY with new text.

### Window / drawing setup
- `cresize` — recomputes terminal columns/rows from a new pixel window size (honoring border padding and the anysize alignment), then resizes term, X buffers, and the pty.
- `xresize` — recreates the backing pixmap and Xft draw context at the new pixel size, and grows the glyph-spec scratch buffer.
- `sixd_to_16bit` — converts a 0-5 xterm color-cube coordinate to a 16-bit color channel value.
- `xloadcolor` — resolves a color: named string, xterm 256-color palette entry (6×6×6 cube or grayscale ramp), or a named X color.
- `xloadcols` — (re)allocates and loads the entire color table (`dc.col`), and applies the configured alpha to the default background's pixel/color.
- `xgetcolor` — reads back an allocated color's 8-bit R/G/B (used e.g. by OSC color-query responses and sixel default bg/fg).
- `xsetcolorname` — reassigns a palette slot to a new named color, freeing the old Xft color (re-applies alpha if it's the background slot).
- `xclear` / `xclearwin` — fill a pixel rectangle (or the whole window) with bg/fg depending on reverse-video mode.
- `xhints` — sets WM size hints (min size, resize increments, optionally fixed/geometry-positioned).
- `xgeommasktogravity` — maps `XParseGeometry` negative-position flags to an `X` window gravity constant.
- `ximopen` / `ximinstantiate` / `ximdestroy` / `xicdestroy` — X Input Method lifecycle: opens an XIM, registers destroy callbacks, and re-registers an instantiate callback if the input method server restarts.
- `xloadfont` — loads one Xft font variant from an `FcPattern`, checking for slant/weight substitution mismatches (sets `badslant`/`badweight`) and computing height/width/ascent/descent metrics from a sample string.
- `xloadfonts` — parses the configured font string (XLFD or fontconfig name), applies the requested pixel size (or reads/derives a default), loads the four style variants (regular/italic/bold/bold-italic) via `xloadfont`, and sets `win.cw`/`win.ch` from the metrics × `cwscale`/`chscale`.
- `xloadsparefont` — loads one fallback ("font2") font variant into the font-ring cache.
- `xloadsparefonts` — loads all configured fallback fonts (`font2[]`) in all four style variants, scaling their requested size proportionally if the primary font size deviates from default.
- `xunloadfont` / `xunloadfonts` — free a single font / all primary+fallback fonts and the HarfBuzz font cache.
- `xinit` — the big X11 setup routine: opens the display, picks a 32-bit TrueColor visual if available, initializes fontconfig and loads fonts/colors, computes window size, creates the window (optionally reparented for `-w` embedding), creates the backing pixmap/GC/Xft draw context, sets up input methods, cursor shape/colors, WM protocol atoms (delete-window, `_NET_WM_PID`), maps the window, and initializes the graphics (`gr_init`) and box-drawing (`boxdraw_xinit`) modules.
- `xresetfontsettings` — picks which of the four `Font`s (and FRC flag) to use for a given attribute mode (bold/italic combination).

### Glyph shaping / rendering
- `xmakeglyphfontspecs` — converts a run of `Glyph`s into an array of `XftGlyphFontSpec` (font+glyph-index+position) ready for Xft to draw: selects font per attribute mode, shapes the run through HarfBuzz (`hbtransform`) for ligatures/cluster positioning, substitutes box-drawing glyph indices directly, and falls back to the fontconfig-driven per-codepoint font-ring cache (`frc`) for glyphs missing from the primary/fallback fonts. Skips wide-dummy cells and treats image cells as spaces (images are drawn separately).
- `xdrawunderdashed` — draws a dashed/dotted underline segment pattern across a width.
- `xdrawundercurl` — draws a wavy (curly) underline using a zig-zag polyline via raw Xlib `XDrawLines` with a clip rectangle.
- `xdrawglyphfontspecs` — renders one run of same-attribute glyphs: resolves fg/bg (truecolor or palette, bright-bold substitution, reverse-video swap/invert, faint dimming, blink/invisible hiding), fills background and clears window border slivers adjacent to the run, computes underline metrics and draws the selected underline style (straight/double/dotted/dashed/curly) using the resolved "decoration color", draws glyphs (via `drawboxes` for box-drawing runs, else `XftDrawGlyphFontSpec`), and draws strikethrough.
- `xdrawglyph` — draws a single glyph (used for the block/kitty-style cursor overlay) and, if it's an image cell, also triggers image drawing for that one cell.
- `xdrawcursor` — redraws the line under the old cursor position (to fix ligatures broken by the cursor glyph), then draws the new cursor in the style selected by `win.cursor` (blinking/steady block/underline/bar/"st" custom glyph), honoring reverse-video and `dynamic_cursor` (swap fg/bg instead of using fixed cursor colors) and selection-aware coloring.
- `xdrawimages` — for a horizontal run of cells sharing an image id/placement, infers each cell's row/column (and 4th-id-byte) from diacritic/neighbor information (per the Unicode-placeholder protocol's inheritance rules), splits the run into contiguous image-coordinate stripes, and calls `gr_append_imagerect` for each stripe; also backfills inferred row/col/id-byte into the `Glyph`s for future runs.
- `xdrawoneimagecell` — same idea for exactly one cell (used by the cursor-hover single-glyph draw path).
- `xstartimagedraw` / `xfinishimagedraw` — thin wrappers bracketing a redraw pass with `gr_start_drawing`/`gr_mark_dirty_animations` and `gr_finish_drawing`.
- `xsetenv` — sets the `WINDOWID` environment variable for the pty child.
- `xseticontitle` / `xsettitle` — set the X11 icon/window title properties (UTF-8).
- `xstartdraw` — returns whether the window is currently visible (gates whether `draw()` does anything).
- `xdrawline` — draws one row: batches consecutive cells with identical attributes into a single `xmakeglyphfontspecs`/`xdrawglyphfontspecs` call (run twice, once for `DRAW_BG` and once for `DRAW_FG`, so background fills never overlap adjacent glyph rendering), calling `xdrawimages` instead of glyph-drawing for image runs.
- `xfinishdraw` — the legacy-SIXEL image compositing pass: for each `ImageList` node still on screen, scales/rescales it into an X `Pixmap` (via direct `XPutImage` if cell size is unchanged, else through Imlib2 scaling), builds a 1-bit clip mask for transparent sixels, then blits only the still-un-erased horizontal spans (tracked via `ATTR_SIXEL`) onto the back buffer, deleting fully-erased images; finally blits the back buffer to the window.
- `xximspot` — updates the X Input Method's preedit spot location to just below the cursor.
- `expose` / `visibility` / `unmap` — X event handlers updating `MODE_VISIBLE`/triggering `redraw`.
- `xsetpointermotion` — toggles `PointerMotionMask` on the window's event mask.
- `xsetmode` — sets/clears a `win.mode` flag, forcing a redraw if `MODE_REVERSE` changed.
- `xsetcursor` — validates and sets the cursor style number, updating whether it's a "blinking" style.
- `xseturgency` — sets/clears the X11 urgency window-manager hint.
- `xbell` — sets urgency if unfocused and rings the XKB bell if `bellvolume` is nonzero.
- `focus` — handles FocusIn/FocusOut: manages IME focus, `MODE_FOCUSED`, urgency clear, and sends focus-in/out escape sequences if `MODE_FOCUS` reporting is enabled.
- `match` — checks whether an event's modifier state matches a shortcut's required mask (mask `XK_ANY_MOD` always matches; `ignoremod` bits are ignored).
- `kmap` — resolves a `KeySym`+modifiers to a custom keymap string from `config.h`'s `key[]` table (filtering by app-keypad/app-cursor/numlock three-valued conditions), restricted to X11 "function key" range unless explicitly listed in `mappedkeys`.
- `kpress` — the keyboard event handler: looks up the composed string via XIM or `XLookupString`, highlights/un-highlights URLs on Ctrl press/release, ignores key releases for shortcut purposes, checks `shortcuts[]` first, then `kmap` custom keys, then falls back to the IME/XLookupString composed bytes (with Meta/Alt handling either 8-bit high-bit-set encoding or ESC-prefixing).
- `cmessage` — handles `ClientMessage`s: XEMBED focus in/out, and `WM_DELETE_WINDOW` (hangs up the tty, deinitializes graphics, exits).
- `resize` — `ConfigureNotify` handler; calls `cresize` if the size actually changed.
- `run` — the main event loop: waits for the initial `MapNotify`, starts the pty (`ttynew`) and sizes the window (`cresize`), then loops on `pselect` over the X connection fd and the pty fd, reading/dispatching both, using a min/max-latency debounce scheme to batch bursts of output into fewer redraws, respecting synchronized-update suspension (`tinsync`), driving cursor blink timing, and decreasing the poll timeout when the graphics module has a pending animation redraw (`graphics_next_redraw_delay`).
- `usage` — prints CLI usage and exits.
- `get_dpi` — reads the `Xft.dpi` X resource (falls back to 96 DPI).
- `calculate_dpi` — computes DPI from physical display size (currently just logged, informational).
- `main` — parses CLI args (`arg.h`-based), sets up locale, creates the initial `Term` (`tnew`) and X window (`xinit`), sets `WINDOWID` env, initializes selection state, logs DPI, and runs the event loop.

### Event dispatch table
`handler[LASTEvent]` — maps X11 event types to the handler functions above (KeyPress/Release→`kpress`, ClientMessage→`cmessage`, ConfigureNotify→`resize`, VisibilityNotify→`visibility`, UnmapNotify→`unmap`, Expose→`expose`, FocusIn/Out→`focus`, MotionNotify→`bmotion`, ButtonPress/Release→`bpress`/`brelease`, SelectionNotify→`selnotify`, PropertyNotify→`propnotify`, SelectionRequest→`selrequest`; SelectionClear is commented out).

---

## `graphics.h` / `graphics.c` — Kitty Graphics Protocol

Implements a subset of the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/): image upload (direct/file/shared-memory, raw or PNG-like via Imlib2, optionally zlib-compressed raw pixels), multi-frame animations, placements (classic text-replacing or virtual/Unicode-placeholder), an on-disk cache with RAM/disk budget eviction, and debug/introspection tooling.

### Core data model (see struct summaries above under st.h's ImageList note — these are separate, richer structures specific to the kitty protocol)
- **`ImageFrame`** — one frame of an image: pointer back to its `Image`, 1-based `index`, `atime`, background color/frame for composition, expected/actual disk size, pixel format, on-disk dimensions and offset (for animation composition), compression mode, `status` (`ImageStatus`), `uploading_failure` reason, `quiet` level, `blend` flag, open file handle during upload, disk size, and the composed (unscaled) `imlib_object`.
- **`Image`** — a whole image: client `image_id`, `query_id` (for `a=q` probing), `image_number` (an alternate, possibly-reused identifier), `atime`, animation `total_duration`, `total_disk_size`, `global_command_index` (recency tiebreaker), `current_frame`/`animation_state`/`current_frame_time`/`last_redraw`/`next_redraw` (animation playback state), unscaled `pix_width`/`pix_height`, `first_frame` plus a `kvec` of additional frames, a `khash` of placements, `default_placement`, and `initial_placement_id`.
- **`ImagePlacement`** — one placement of an image on the grid: back-pointer to `Image`, `placement_id`, `atime`, `protected_frame` (pinned against eviction), `virtual` flag (Unicode-placeholder-only, invisible classic cell), `scale_mode`, `rows`/`cols`, source rectangle (`src_pix_*`), `first_pixmap` plus a `kvec` of additional per-frame pixmaps, the cell size the pixmaps were scaled for (`scaled_cw`/`scaled_ch`), `do_not_move_cursor`, and `text_underneath` (saved glyphs for classic placements, restored on deletion).
- **`ImageRect`** — one queued rectangle to blit: image/placement id, screen position (pixel and row), the source sub-rectangle in image cells, current cell size, and a `reverse`-video flag. A small fixed-size array (`image_rects[MAX_IMAGE_RECTS]`) accumulates and merges adjacent same-image stripes across a redraw pass before they're actually drawn, to reduce X server round-trips.
- **`GraphicsCommand`** — a fully parsed protocol command (all `key=value` fields from the APC payload), covering every documented key (`a`,`q`,`f`,`o`,`t`,`d`,`s`/`v` (dual-purpose: frame size or animation state/loop count depending on action), `x`/`y`, `w`/`h`, `r`/`c` (dual-purpose: rows/cols or edit-frame/current-frame), `i`,`I`,`p`,`m`,`is_direct_transmission_continuation`, `S`,`O`,`U`,`C`, plus animation-only `X`,`Y`,`c` (background frame),`r` (edit frame),`z` (gap),`s`/`v` (state/loops)).
- **`KeyAndValue`** — a raw `key_start`/`val_start`/lengths pair produced by the first parse pass, before being interpreted into `GraphicsCommand` fields (interpretation is context-dependent on `action`, hence the two-pass parse).
- **`DeletionData`** — accumulator used while walking all on-screen image cells to perform a delete command: target `image_id`/`placement_id` filter and the deduplicated list of placements found so far.
- **`UnloadableObject`** — a scored candidate for RAM eviction: either an `ImageFrame`'s imlib object or one pixmap (`frameidx`) of an `ImagePlacement`, with a `score` derived from access recency (see eviction section).
- **Enums**: `ScaleMode` (fill/contain/none/none-or-contain), `AnimationState` (stopped/loading/looping), `ImageStatus` (uninitialized→uploading→(error|success)→ram-loading→(error|success)), `ImageUploadingFailure` (size-limit/cannot-open-file/unexpected-size/cannot-copy/cannot-open-shm), `GraphicsDebugMode` (none/log/log+boxes, public via `graphics.h`).
- **`GraphicsCommandResult`** (public, in `graphics.h`) — output of `gr_parse_command`: whether to redraw, the protocol response string to send back, error flag, and (if the terminal needs to materialize a classic placeholder) the placeholder's image/placement id, rows/cols, cursor-move flag, and a pointer to the saved-text buffer.
- **`foreach_frame` / `foreach_pixmap`** — code-generating macros iterating an image's frames or a placement's pixmaps (each stored as "first" + a `kvec` tail, to avoid an allocation for the common single-frame case).

### Time helpers
- `gr_timediff_ms`, `gr_now_ms` — millisecond timestamps relative to module-init time (`initialization_time`), used throughout for `atime`/animation scheduling.

### Image/frame/placement bookkeeping (find/create/delete/touch)
- `gr_last_frame_index`, `gr_get_frame`, `gr_get_last_frame`, `gr_last_uploaded_frame_index` — frame index/lookup helpers (1-based; "last uploaded" skips a frame still mid-transfer).
- `gr_get_frame_pixmap` / `gr_set_frame_pixmap` — indexed pixmap accessors into a placement's first+overflow arrays, growing the overflow `kvec` as needed.
- `gr_find_image` / `gr_find_image_by_number` — id-based hash lookup, or newest-by-`global_command_index` lookup by the (non-unique) image number.
- `gr_find_placement` / `gr_find_image_and_placement` — placement lookup by id, with id 0 resolving to (and caching) a "default placement".
- `gr_get_glyph_underneath_image` (public) — looks up the saved `text_underneath` glyph for a classic placement at a given cell.
- `gr_get_frame_filename` — builds the on-disk cache path for a frame.
- `gr_frame_current_ram_size`, `gr_placement_single_frame_ram_size`, `gr_placement_current_ram_size` — RAM usage estimators used for budget accounting.
- `gr_unload_frame` / `gr_unload_all_frames` — free a frame's/image's in-RAM imlib object(s) (file cache and pixmaps untouched).
- `gr_unload_placement` / `gr_unload_pixmap` — free a placement's pixmap(s) from the X server.
- `gr_delete_imagefile` / `gr_delete_imagefiles` — remove on-disk cache file(s) for a frame/image.
- `gr_delete_placement_keep_id` / `gr_delete_all_placements` / `gr_delete_image_keep_id` / `gr_delete_image` / `gr_delete_placement` / `gr_delete_all_images` — full teardown at various granularities; "keep_id" variants don't remove the hash-table entry (used when about to reinsert with the same id). Deleting a non-virtual placement first calls `gr_erase_placement` to restore on-screen text.
- `gr_touch_image` / `gr_touch_frame` / `gr_touch_placement` — update `atime` (frame/placement touches propagate to the owning image).
- `gr_new_image` — creates (generating a random id if 0 given, avoiding degenerate id patterns) and registers a new `Image`, deleting any prior image with the same id.
- `gr_append_new_frame` — appends a new `ImageFrame` (as the first frame or into the overflow vector).
- `gr_new_placement` — creates (random id if 0) and registers a new `ImagePlacement`, becoming the image's default if it's the first.
- `ceil_div` — integer ceiling division helper.
- `gr_infer_placement_size_maybe` — clamps/defaults the placement's source rectangle to the image bounds, and — if rows/cols weren't specified — computes them from the source size and current cell size, respecting `SCALE_MODE_CONTAIN`'s aspect-preserving single-dimension inference.
- `gr_update_frame_index` — advances `img->current_frame` based on elapsed time and per-frame `gap`, computing `img->next_redraw`; handles stopped/loading/looping animation states, gapless frames, loop-duration shortcuts for long-idle catch-up, and infinite-loop guards.

### RAM/disk budget eviction
- `gr_cmp_frames_by_atime` / `gr_cmp_images_by_atime` / `gr_cmp_placements_by_atime` / `gr_cmp_unloadable_objects` — `qsort` comparators (ties broken by `global_command_index`).
- `gr_get_images_sorted_by_atime` / `gr_get_placements_sorted_by_atime` / `gr_get_frames_sorted_by_atime` — collect+sort all live objects of a kind.
- `gr_recency_threshold` — a "still probably in active use" time window based on an image's animation duration.
- `gr_unloadable_object_for_frame` / `gr_unloadable_object_for_pixmap` — score a frame's imlib object or a placement's pixmap for eviction desirability: recently-touched/active-animation objects are scored artificially high (deprioritized for eviction), with pixmap-vs-imlib size used as a tiebreaker.
- `gr_get_unloadable_objects_sorted_by_score` — collects and sorts all evictable objects across all images/placements.
- `apply_tolerance` — inflates a hard limit by a configurable tolerance ratio (hysteresis to avoid thrashing).
- `gr_check_limits` — enforces, in order: max total images, max total placements, max on-disk cache size, max total RAM — deleting/unloading the least-recently-used objects until back under budget (with tolerance), and logs a summary.
- `gr_unload_images_to_reduce_ram` (public) — user-triggered aggressive unload of every non-protected placement/frame.

### Image loading (disk → RAM)
- `gr_copy_pixels` — converts a raw RGB/RGBA buffer into imlib2's native `0xAARRGGBB` pixel format.
- `gr_load_raw_pixel_data_uncompressed` — streams a raw pixel file into an imlib buffer in chunks.
- `gr_load_raw_pixel_data_compressed` — same, but through a zlib inflate stream (RFC 1950).
- `gr_load_raw_pixel_data` — loads a whole frame's raw-pixel-format on-disk file into a new `Imlib_Image`, dispatching to the compressed/uncompressed loader.
- `gr_load_imlib_object` — loads a frame's on-disk file (raw pixel format directly, or via Imlib2's generic loader for e.g. PNG), composes it onto its background color/frame if it's a non-first animation frame (blend or replace per `X=`), premultiplies alpha, and updates status/errors and RAM accounting.
- `gr_premultiply_alpha` — multiplies RGB channels by the alpha channel in-place (required before uploading as a premultiplied X pixmap).
- `gr_load_pixmap` (public) — ensures a placement's `frameidx`-th pixmap is loaded and correctly scaled to the current cell size: loads the source frame if needed, creates/uploads a scaled X `Pixmap` via Imlib2 (cropped+scaled to `cols*cw × rows*ch`), rescaling if cell dimensions changed since last load.

### Cache directory / init/deinit
- `gr_create_cache_dir` — creates a unique temp directory (`mkdtemp`) from `graphics_cache_dir_template`.
- `gr_make_sure_tmpdir_exists` — recreates the cache dir if it's been removed externally.
- `gr_init` (public) — sets up Imlib2's X11 context (display/visual/colormap), builds the color-inversion lookup table, records init time, creates the image/placement hash table and cache dir.
- `gr_deinit` (public) — deletes all images (cleaning up temp files) and removes the cache directory.

### Debug/introspection
- `gr_ago` — formats a millisecond time delta as a human-readable "Xs ago" string.
- `fprintf_ind` — indented `fprintf`.
- `gr_dump_image_info` / `gr_dump_frame_info` / `gr_dump_placement_info` / `gr_dump_placement_pixmaps` — recursive pretty-printers of the object graph.
- `gr_dump_state` (public) — dumps every image/placement to stderr (bound to a debug shortcut).
- `gr_preview_image` (public) — execs a user-provided command with the path to a cached image file.
- `gr_show_image_info` (public) — writes a temp info file about a specific cell/placement/glyph-underneath and opens it via `st -e less`.

### Drawing
- `gr_displayinfo` — draws a small debug text label (X server text, not Xft) over a rectangle.
- `gr_showrect` — draws a colored bounding-box outline for debug mode.
- `gr_update_next_redraw_time` — records/merges the next scheduled redraw time for a row (used for animation frame pacing), growing the tracking vector as needed.
- `gr_drawimagerect` — actually draws one queued `ImageRect`: advances the image's animation frame if needed, ensures the relevant pixmap is loaded (`gr_load_pixmap`), `XCopyArea`s the (possibly color-inverted, via `reverse_table` and an offscreen inverted copy) sub-rectangle onto the terminal buffer, schedules the next animation redraw, and optionally overlays debug info/boxes.
- `gr_getrectbottom` — bottom-edge pixel Y of a rect (for merge/eviction ordering).
- `gr_freerect` — zeroes an `ImageRect` slot (image_id 0 marks it free) when recycling `image_rects` slots.
- `gr_start_drawing` (public) — records current cell size for later inference.
- `gr_finish_drawing` (public) — flushes/draws all still-queued rectangles at end of frame.
- `gr_mark_dirty_animations` (public) — marks rows dirty whose scheduled redraw time has arrived, resizing the per-row redraw-time tracking vector to match the current row count.
- `gr_append_imagerect` (public) — the main entry point called from `x.c`'s `xdrawimages`/`xdrawoneimagecell`: merges the new stripe into an existing queued rect if it's a perfectly-aligned vertical continuation, otherwise evicts (draws) the oldest existing rect if the queue is full and starts a new one. No-ops for the empty image id or degenerate rectangles.

### Protocol command parsing/handling
- `sanitize_str` / `sanitized_filename` — redact non-printable bytes and truncate (with ellipsis) for safe logging of untrusted filenames/strings.
- `gr_createresponse` — builds the `\033_G...\033\\` APC response string with the appropriate `i=`/`I=`/`p=` echo fields.
- `gr_reportsuccess_cmd` / `gr_reportsuccess_frame` — send "OK" unless quiet or a non-final `m=1` transmission.
- `gr_reporterror_cmd` / `gr_reporterror_frame` — format + log + (unless quiet) send an error response; also sets `graphics_command_result.error`.
- `gr_loadimage_and_report` — loads a frame's imlib object and reports success/failure; discards the image immediately if it was a query (`a=q`) action.
- `gr_reportuploaderror` — maps an `ImageUploadingFailure` code to the matching errno-style error message.
- `gr_display_nonvirtual_placement` — populates `graphics_command_result`'s placeholder-creation fields (columns/rows/cursor-move flag) and allocates the `text_underneath` save buffer, once the placement's size can be inferred and the first frame is loaded.
- `gr_schedule_image_redraw` — dirties all rows containing the image (wrapper around `gr_schedule_image_redraw_by_id`).
- `gr_append_raw_data_to_file` — appends bytes to (creating if needed) a frame's on-disk cache file, updating disk-size accounting.
- `gr_append_data` — appends one chunk of base64-encoded direct-transmission payload to the right frame (inferred from `current_upload_image_id`/`frame_index` if not explicit), enforcing the size limit, and on the final chunk (`!more`) closes the file, validates size, schedules a redraw, loads+reports the image, and displays any pending non-virtual placements.
- `gr_find_image_for_command` — resolves the target image by id, or by number (falling back to the last-uploaded image for a `put` with no id/number).
- `gr_new_image_or_frame_from_command` — for action `f`, finds the existing image and appends a frame to it; otherwise creates a brand-new image (random id for `q`) and its first frame — in both cases copying over format/compression/background/gap/blend/dimensions from the command, and inferring `expected_size` from dimensions when possible (needed for shared-memory transfers).
- `gr_delete_tmp_file` — best-effort `unlink` of a client-supplied temp file, restricted to paths that look like this protocol's own temp files (under `/tmp/` or `$TMPDIR`, containing `tty-graphics-protocol`) as a safety check.
- `gr_handle_transmit_command` — handles `t`/`T`/`f`/`q` data transmission for all three media: **file** (`t`/`f`: stats + symlinks + `cp`s the source file into the cache dir, or reports the specific stat error), **direct** (`d`, default: continues an in-progress upload or starts one and appends the first payload chunk via `gr_append_data`), and **shared memory** (`s`: `shm_open`+`mmap`s the named POSIX shm object at a page-aligned offset, copies the requested bytes into the cache file, then unmaps/reports).
- `gr_handle_put_command` — resolves the target image, creates a placement with the command's placement parameters, picks a default `scale_mode` based on which of rows/cols were specified, displays it if non-virtual, and reports success.
- `gr_deletion_callback` — per-cell callback (via `gr_for_each_image_cell`) used by delete/erase: filters by image-id/placement-id (only affects classic placeholders), restores the saved underneath glyph if known (else blanks the cell), and records the placement as needing full teardown (deduplicated).
- `gr_handle_delete_command` — implements the `d=` delete-specifier space: `a`/unset (all visible placements), `i`/`I`→`n` (by image id, or by image number resolved to its id, optionally scoped to one placement id) — walking the screen via `gr_deletion_callback`, then actually deleting the collected placements (and their owning images too, if the uppercase/"also delete image" variant was requested and no placements remain).
- `gr_erase_placement` — restores on-screen text for one placement (used when a classic placement is implicitly replaced/deleted) via the same cell-walk mechanism.
- `gr_handle_animation_control_command` — implements `a=a`: resolves the target image, optionally edits a specific frame's `gap` (adjusting `total_duration`), sets the current frame index and/or animation state (stopped/loading/looping), and schedules a redraw.
- `gr_handle_command` — top-level action dispatch: transmit (`t`/`q`/`f`/unset), put (`p`), transmit-and-put (`T`, chaining the two unless it was a continuation of a prior direct upload), delete (`d`), animation control (`a`); sets `quiet=2` automatically when neither id nor number was given (nobody could be expecting a response).
- `gr_set_keyvalue` — interprets one raw key/value pair into the correctly-typed, context-sensitive (`action`-dependent for several dual-purpose keys) `GraphicsCommand` field, validating single-char vs numeric value shape per key.
- `gr_parse_command` (public) — the protocol entry point: strips the leading `G`, splits the command into `key=value,key=value...;payload` via a small hand-rolled state machine (collecting raw key/value spans first, then interpreting `a=`/`i=`/`I=` before the rest so later keys can be disambiguated by action), calls `gr_handle_command` unless a parse error already occurred, optionally logs the response, honors quiet-suppression, and returns whether the buffer was a graphics command at all (leading `G` check).

### Base64
- `gr_base64_getc` / `gr_base64dec` (public) — a second, independent base64 decoder (mirrors the one in `st.c`) used for command payloads (filenames, direct-transmission chunks).

---

## `sixel.h` / `sixel.c` — DEC SIXEL Parser

Ported from mintty/xterm; converts an incoming DEC SIXEL bitmap byte stream into one or more `ImageList` row-slices (one per terminal row of height, since sixels can be taller than one cell).

### Types (in `sixel.h`)
- **`sixel_image_t`** (aka `sixel_image_buffer`) — the indexed-color raster being built: `data` (per-pixel palette index), `width`/`height`, `palette` (up to `DECSIXEL_PALETTE_MAX` RGBA entries), `ncolors`, whether the palette was explicitly modified, and whether a private (per-sixel, vs shared) color register set is used.
- **`parse_state_t`** — the sixel parser's state machine states: `PS_ESC` (saw an escape, stop), `PS_DECSIXEL` (normal body), `PS_DECGRA` (raster-attributes `"..."`), `PS_DECGRI` (repeat-introducer `!`), `PS_DECGCI` (color-introducer `#`), `PS_ERROR`.
- **`sixel_state_t`** (aka `parser_context`) — full parser state: current `state`, cursor position (`pos_x`/`pos_y`) and bounds (`max_x`/`max_y`), raster attributes (pan/pad/ph/pv), `transparent` flag, `repeat_count` (from `!`), `color_index`, `bgindex`, target cell `grid_width`/`grid_height` (used to slice the raster into per-row `ImageList` entries), a small parameter accumulator (`param`/`nparams`/`params[]`), and the embedded `sixel_image_t`.

### Functions
- `scroll_images` (declared here, used by `st.c`'s legacy scroll path — actually superseded by the per-region logic in `tscrollup`/`tscrolldown`, kept for API compatibility) — shifts every `ImageList` node's `y` by `n`, deleting any that scroll above row 0.
- `delete_image` (public) — unlinks an `ImageList` node from `term.images`'s doubly-linked list, frees its X pixmap/clipmask and pixel buffer, and frees the node.
- `set_default_color` (static) — initializes the 256-ish-entry sixel palette: a 16-color ANSI-like table, a 6×6×6 color cube (17-232), a grayscale ramp (233-256), and white for the remainder.
- `sixel_image_init` (static) — allocates and zeroes the indexed pixel buffer for a fresh 1×1 image, seeding palette slot 0 (background) and optionally slot 1 (foreground, for private-register mode).
- `image_buffer_resize` (static) — grows the indexed pixel buffer to a new width/height, copying existing rows and zero-filling new area (used as the raster grows during parsing, since final dimensions aren't always known up front).
- `sixel_image_deinit` (static) — frees the indexed pixel buffer.
- `sixel_parser_init` (public) — resets a `sixel_state_t` to initial values (given fg/bg colors, transparency, private-register flag, and target cell size) and allocates a starting 1×1 `sixel_image_t`.
- `sixel_parser_set_default_color` (public) — thin wrapper calling `set_default_color`.
- `sixel_parser_finalize` (public) — converts the finished indexed raster into one or more `ImageList` nodes (one per `ch`-pixel-tall band, since the terminal only tracks whole-cell-row image anchors): clamps to the actual drawn bounds, re-applies the default palette if needed, computes column/row counts, allocates each `ImageList` with its RGBA `pixels` slice (palette-resolved from the indexed data), and flags each as `transparent` if any pixel resolved to palette index 0 while transparency was requested.
- `sixel_parser_parse` (public) — the main byte-consuming state machine implementing the SIXEL command grammar: body sixel characters (`?`-`~`, encoding a 6-bit vertical bitmask, with buffer auto-growth via `image_buffer_resize` when the cursor would exceed current bounds, and repeat-count expansion), graphics carriage-return/next-line (`$`/`-`), raster-attributes (`"` → `PS_DECGRA`, parses `Pan;Pad;Ph;Pv` and resizes/pre-allocates the buffer to the declared size), repeat-introducer (`!` → `PS_DECGRI`, parses `Pn` repeat count), color-introducer (`#` → `PS_DECGCI`, parses `Pc[;Pu;Px;Py;Pz]` and sets a palette entry via HLS or RGB, or just selects an existing color register), and error/escape termination.
- `sixel_parser_deinit` (public) — frees the parser's indexed pixel buffer.
- `sixel_create_clipmask` (public) — builds a 1-bit-per-pixel X `Pixmap` bitmap (packed per `XBitmapBitOrder`) marking which pixels of an RGBA buffer are non-transparent, used as the clip mask when blitting transparent sixel images.

---

## `sixel_hls.h` / `sixel_hls.c`

- `hls_to_rgb` (public) — converts a DEC-SIXEL-flavored HLS color (hue 0-360 with a 240° offset convention where 0°=blue, sat/lum 0-100) to a packed `0xAARRGGBB`-style RGB value (alpha forced to 255), via standard HSL→RGB sector math; degenerate `sat==0` short-circuits to a gray. Used by the SIXEL color-introducer (`#`) command when the color-space selector requests HLS (`Pu=1`) instead of RGB (`Pu=2`).

---

## `boxdraw_data.h` / `boxdraw.c` — Box-drawing & Braille Glyphs

Renders Unicode box-drawing (`U+2500`-`U+257F`) and braille (`U+2800`-`U+28FF`) glyphs as vector rectangles instead of relying on the font, for crisp, cell-aligned lines regardless of font hinting.

### `boxdraw_data.h`
Defines the per-character shape-encoding bit constants (`BDL`/`BDA`/`BDB` line/arc/bold flags; `LL`/`LU`/`LR`/`LD` light-direction bits; `DL`/`DU`/`DR`/`DD` double-direction bits; `BBD`/`BBU`/`BBL`/`BBR`/`BBQ`/`BBS` block/quadrant/shade categories; `BRL` braille category plus 8 `BRAILLE_*` dot-position bits) and the `boxdata[256]` lookup table mapping each low byte of a box-drawing/braille codepoint to its encoded shape.

### `boxdraw.c`
- `boxdraw_xinit` (public) — stashes the `Display`/`Colormap`/`XftDraw`/`Visual` handles for later use (called once from `xinit`).
- `isboxdraw` (public, declared in `st.h`) — true if a rune is a supported box-drawing char (block `0x2500` with a non-zero `boxdata` entry, if `boxdraw` config is on) or a braille char (block `0x2800`, if `boxdraw_braille` is on).
- `boxdrawindex` (public, declared in `st.h`) — computes the "glyph index" used in place of a real font glyph index: the raw `boxdata` shape code, OR'd with `BRL` for braille or `BDB` for bold box-drawing (if `boxdraw_bold` is enabled and the cell is bold).
- `drawboxes` (public, declared in `st.h`) — draws a run of box-drawing glyph-specs left to right, calling `drawbox` per cell.
- `drawbox` (static) — dispatches one cell's shape code to the appropriate rectangle(s): line/arc shapes → `drawboxlines`; partial block fills (`BBD`/`BBU`/`BBL`/`BBR`, in eighths); quadrant blocks (`BBQ`, any combination of the 4 quadrants); shade blocks (`BBS`, alpha-blends fg/bg at 25/50/75%); braille (`BRL`, up to 8 dot rectangles in a 2×4 grid).
- `drawboxlines` (static) — draws light and/or double line segments in each of the 4 directions from a cell's center, computing stem thickness (thicker for "bold" box-drawing) and carefully shortening/lengthening segments where light and double lines cross so double-line corners/junctions render correctly (per the reference box-drawing shapes).

---

## `hb.h` / `hb.c` — HarfBuzz Text Shaping

Provides ligature- and cluster-aware glyph shaping so combining sequences and font ligatures render correctly, instead of naive one-rune-per-glyph mapping.

### Types (`hb.h`)
- **`HbTransformData`** — the output of shaping one run: the `hb_buffer_t` (owns the memory, must be destroyed), plus convenience pointers to its glyph-info and glyph-position arrays and the glyph `count`.

### Functions (`hb.c`)
- Local **`HbFontMatch`**/**`HbFontCache`** — a linear-scan cache mapping an already-loaded `XftFont*` to its corresponding `hb_font_t*` (avoids re-locking the FreeType face / recreating the HarfBuzz font object every draw).
- Local **`RuneBuffer`** — a reusable growable scratch buffer of `Rune`s used to stage codepoints before feeding them to HarfBuzz (grows in `BUFFER_STEP`-sized increments).
- `hbunloadfonts` (public) — destroys all cached `hb_font_t`s and unlocks their FreeType faces (called from `xunloadfonts`).
- `hbfindfont` — returns the cached `hb_font_t` for an `XftFont*`, creating (locking the FT face, wrapping it with `hb_ft_font_create`) and caching it on first use.
- `hbtransform` (public) — shapes a slice `[start, start+length)` of a `Glyph` run: fills the rune scratch buffer (substituting a space for wide-dummy filler cells), runs `hb_shape` with a (currently empty) fixed `features[]` list in left-to-right, monotone-cluster mode, and returns the resulting glyph infos/positions/count via `HbTransformData`.
- `hbcleanup` (public) — destroys the `hb_buffer_t` created by `hbtransform` and zeroes the struct.

---

## `rowcolumn_diacritics_helpers.c`

- `diacritic_to_num` (declared in `st.c`, used by `tputc`) — a large `switch` lookup table mapping ~280 Unicode combining-mark codepoints (the specific set of "row/column diacritics" used by the kitty Unicode-placeholder image protocol, spanning multiple Unicode blocks: combining diacriticals, Hebrew/Arabic/Syriac marks, combining diacriticals supplement/extended/for-symbols, etc.) to a sequential 1-based number. A diacritic applied to an image-placeholder cell encodes, in order, the row, then column, then most-significant-id-byte of the image the placeholder refers to (see `tputc`'s combining-character handling in `st.c` and the `tgetimgdiacriticcount`/`tsetimgrow`/`tsetimgcol`/`tsetimg4thbyteplus1` accessors in `st.h`). Returns 0 for any codepoint not in the table (not a recognized row/column diacritic).

---

## `khash.h` / `kvec.h` — Third-party Generics (klib)

Not project-specific; vendored generic-programming macro libraries used by `graphics.c`:
- **`khash.h`** — a generic open-addressing hash table (`KHASH_MAP_INIT_INT` instantiates `khash_t(id2image)` mapping `uint32_t → Image*` and `khash_t(id2placement)` mapping `uint32_t → ImagePlacement*`). Provides `kh_init`/`kh_destroy`/`kh_get`/`kh_put`/`kh_del`/`kh_value`/`kh_size`/`kh_foreach_value`/`kh_clear` macros used throughout `graphics.c`'s image/placement bookkeeping.
- **`kvec.h`** — a generic growable vector (`kvec_t(T)`), providing `kv_init`/`kv_destroy`/`kv_push`/`kv_pushp`/`kv_a`/`kv_A`/`kv_size`/`kv_resize`/`kv_max` macros. Used for `frames_beyond_the_first`, `pixmaps_beyond_the_first`, the sorted-object arrays in the eviction logic, `next_redraw_times`, and `placements_to_delete`.

---

## `arg.h`

Minimal suckless-style argv parser macros (`ARGBEGIN`/`ARGEND`/`EARGF`/`ARGC`) used by `main()` in `x.c` to parse single-character flags (`-a`, `-A`, `-c`, `-e`, `-f`, `-g`, `-i`, `-o`, `-l`, `-n`, `-t`/`-T`, `-w`, `-v`) with optional attached/following arguments, including `-e command args...` "consume the rest" semantics.

---

## `config.def.h` / `config.h` — Compile-time Configuration

Not logic, but defines the data tables several functions above iterate over. Key entries (all `static`, replicated into the user's `config.h`):
- `font`, `font2[]` — primary and fallback font (fontconfig/XLFD) strings.
- `borderpx`, `anysize_halign`/`anysize_valign` — window border and alignment when the window size isn't an exact multiple of the cell size.
- `shell`, `stty_args`, `utmp`, `scroll`, `worddelimiters`, `allowaltscreen`, `allowwindowops` — process/behavior config consumed by `st.c`.
- `cwscale`/`chscale` — cell width/height scale factors relative to font metrics.
- `doubleclicktimeout`/`tripleclicktimeout` — selection snap timing.
- `minlatency`/`maxlatency` — `x.c`'s `run()` redraw debounce window.
- `su_timeout` — synchronized-update timeout (`tinsync`).
- `blinktimeout`, `cursorthickness`, `bellvolume` — cursor blink period, bar/underline cursor thickness, XKB bell volume.
- `colorname[]`, `defaultfg`/`defaultbg`/`defaultcs`/`defaultrcs`/`defaultattr` — the color palette and default foreground/background/cursor-color indices.
- `cursorstyle`, `stcursor`, `dynamic_cursor` — default cursor shape, the custom "st" cursor glyph, and whether the cursor swaps fg/bg dynamically.
- `cols`/`rows` — default terminal size.
- `mouseshape`/`mousefg`/`mousebg` — X cursor shape/colors.
- `forcemousemod` — modifier that overrides application mouse-tracking to allow local selection.
- `mshortcuts[]`, `shortcuts[]`, `mappedkeys[]`, `ignoremod`, `key[]`, `selmasks[]` — the `MouseShortcut`/`Shortcut`/`Key` tables consumed by `mouseaction`/`kpress`/`kmap`/`mousesel`.
- `ascii_printable[]` — sample string used to measure average glyph advance width when loading a font.
- Graphics-module tunables (used by `graphics.c`, declared `extern` there): `graphics_cache_dir_template`, `graphics_max_single_image_file_size`, `graphics_total_file_cache_size`, `graphics_max_single_image_ram_size`, `graphics_max_total_ram_size`, `graphics_max_total_placements`, `graphics_excess_tolerance_ratio`, `graphics_animation_min_delay`.
- `alpha`, `sixelbyteorder` — window transparency and sixel/image byte order (endianness) for `XImage` construction.
