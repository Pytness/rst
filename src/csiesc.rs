use std::ptr::null_mut;

use crate::config::VTIDEN;
use crate::sixel::{DECSIXEL_HEIGHT_MAX, DECSIXEL_PALETTE_MAX, DECSIXEL_WIDTH_MAX};
use crate::snprintf;
use crate::term_state::{CursorMovement, TermMode, TermState};
use crate::win::{TermWindow, WinMode};

pub const UTF_INVALID: usize = 0xFFFD;
pub const UTF_SIZ: usize = 4;
pub const ESC_BUF_SIZ: usize = 128 * UTF_SIZ;
pub const ESC_ARG_SIZ: usize = 16;
pub const STR_BUF_SIZ: usize = ESC_BUF_SIZ;
pub const STR_ARG_SIZ: usize = ESC_ARG_SIZ;
pub const STR_TERM_ST: &[u8] = b"\x1b\\\0";
pub const STR_TERM_BEL: &[u8] = b"\x07\0";

macro_rules! DEFAULT {
    ($src:expr, $value:expr) => {
        if $src == 0 {
            $src = $value;
        }
    };
}

#[derive(Debug)]
pub struct CSIEscape {
    pub buf: [u8; ESC_BUF_SIZ], // raw string
    pub len: usize,             // raw string length
    private: bool,

    pub arg: [i32; ESC_ARG_SIZ],
    // sub is true if it's a subparameter
    pub sub: [bool; ESC_ARG_SIZ],
    pub narg: usize, // nb of args
    pub mode: [u8; 2],
}

impl Default for CSIEscape {
    fn default() -> Self {
        Self {
            buf: [0; ESC_BUF_SIZ],
            len: 0,
            private: false,
            arg: [0; ESC_ARG_SIZ],
            sub: [false; ESC_ARG_SIZ],
            narg: 0,
            mode: [b'\0'; 2],
        }
    }
}

impl CSIEscape {
    pub fn parse(&mut self) {
        self.narg = 0;

        let mut bytes: &[u8] = &self.buf[..self.len];

        if bytes.first() == Some(&b'?') {
            self.private = true;
            bytes = &bytes[1..];
        }

        let mut sep = b';';

        while !bytes.is_empty() {
            let (value, rest) = parse_arg(bytes);
            bytes = rest;

            self.arg[self.narg] = value;
            self.narg += 1;

            let next = bytes.first().copied();
            match next {
                Some(b @ (b';' | b':')) if self.narg < ESC_ARG_SIZ => {
                    self.sub[self.narg] = b == b':';
                    bytes = &bytes[1..];
                }
                _ => break,
            }
        }

        self.mode[0] = bytes.get(0).copied().unwrap_or(0);
        self.mode[1] = bytes.get(1).copied().unwrap_or(0);
    }

    pub fn handle(&mut self, state: &mut TermState, win: &mut TermWindow) {
        let maxcol = state.col;

        let unknown = || {
            eprint!("erresc: unkown csi ");
            self.dump();
        };

        match self.mode[0] {
            // ICH -- Insert Characters: CSI Ps @
            // - Ps: blank characters to insert at the cursor, shifting the rest of the line right. Default 1.
            b'@' => {
                DEFAULT!(self.arg[0], 1);
                state.tinsertblank(self.arg[0] as usize);
            }

            // CUU -- Cursor Up: CSI Ps A
            // - Ps: rows to move up. Default 1.
            b'A' => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(state.c.x, state.c.y.saturating_sub(self.arg[0] as usize));
            }

            // CUD -- Cursor Down: CSI Ps B
            // VPR -- Line Position Relative: CSI Ps e
            // - Ps: rows to move down. Default 1.
            b'B' | // CUD
            b'e'   // VPR
            => {
                DEFAULT!(self.arg[0], 1);
                let y = state.c.y + self.arg[0].max(0) as usize;
                state.tmoveto(state.c.x, y);
            }

            // MC -- Media Copy: CSI Ps i
            // - Ps = 0: print the screen. Default.
            // - Ps = 1: print the cursor line (DEC form: CSI ? 1 i).
            // - Ps = 2: print the selection (st extension).
            // - Ps = 4: turn off printer controller mode.
            // - Ps = 5: turn on printer controller mode.
            b'i' => {
                match self.arg[0] {
                    0 => state.tdump(),
                    1 => state.tdumpline(state.c.y),
                    2 => state.tdumpsel(),
                    4 => state.mode.remove(TermMode::Print),
                    5 => state.mode.insert(TermMode::Print),
                    _ => {}
                }
            }

            // DA -- Primary Device Attributes: CSI Ps c
            // - Ps = 0: request the attributes. Default.
            // Reply: VTIDEN, CSI ? 62 ; 4 c (VT220 with sixel graphics).
            b'c' => {
                if self.arg[0] == 0 {
                    state.ttywrite_pty(VTIDEN, VTIDEN.len());
                }
            }

            // REP -- Repeat: CSI Ps b
            // - Ps: times to repeat the last printed graphic character. Default 1.
            b'b' => {
                self.arg[0] = self.arg[0].max(1).min(65535);

                if state.lastc != '\0' {
                    for _ in 0..self.arg[0] {
                        state.tputc_char(state.lastc);
                    }
                }
            }

            // CUF -- Cursor Forward: CSI Ps C
            // HPR -- Character Position Relative: CSI Ps a
            // - Ps: columns to move right. Default 1.
            b'C' | // CUF
            b'a'  // HPR
            => {
                DEFAULT!(self.arg[0], 1);
                let x = state.c.x + self.arg[0].max(0) as usize;
                state.tmoveto(x, state.c.y);
            }

            // CUB -- Cursor Backward: CSI Ps D
            // - Ps: columns to move left. Default 1.
            b'D' => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(state.c.x.saturating_sub(self.arg[0] as usize), state.c.y);
            }

            // CNL -- Cursor Next Line: CSI Ps E
            // - Ps: rows to move down, then go to the first column. Default 1.
            b'E' => {
                DEFAULT!(self.arg[0], 1);
                let y = state.c.y + self.arg[0].max(0) as usize;
                state.tmoveto(0, y);
            }

            // CPL -- Cursor Preceding Line: CSI Ps F
            // - Ps: rows to move up, then go to the first column. Default 1.
            b'F' => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(0, state.c.y - self.arg[0] as usize);
            }

            // TBC -- Tab Clear: CSI Ps g
            // - Ps = 0: clear the tab stop at the cursor column. Default.
            // - Ps = 3: clear all tab stops.
            b'g' => {
                match self.arg[0] {
                    0 => state.tabs[state.c.x] = 0,
                    3 => state.tabs.iter_mut().for_each(|t| *t = 0),
                    _ =>  unknown(),

                }
            }

            // CHA -- Cursor Horizontal Absolute: CSI Ps G
            // HPA -- Character Position Absolute: CSI Ps `
            // - Ps: 1-based column. Default 1.
            b'G' | // CHA
            b'`'   // HPA
            => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(self.arg[0] as usize - 1, state.c.y);
            }

            // CUP -- Cursor Position: CSI Ps ; Ps H
            // HVP -- Horizontal and Vertical Position: CSI Ps ; Ps f
            // - Ps 1: 1-based row, relative to the top margin in origin mode. Default 1.
            // - Ps 2: 1-based column. Default 1.
            b'H' | // CUP
            b'f'   // HVP
            => {
                DEFAULT!(self.arg[0], 1);
                DEFAULT!(self.arg[1], 1);
                state.tmoveato(self.arg[1] as usize - 1, self.arg[0] as usize - 1);
            }

            // CHT -- Cursor Forward Tabulation: CSI Ps I
            // - Ps: tab stops to move forward. Default 1.
            b'I' => {
                DEFAULT!(self.arg[0], 1);
                state.tputtab(self.arg[0] as isize);
            }

            // ED -- Erase in Display: CSI Ps J
            // - Ps = 0: erase from the cursor to the end of the screen. Default.
            // - Ps = 1: erase from the start of the screen to the cursor.
            // - Ps = 2: erase the whole screen.
            // - Ps = 3: erase the saved lines (scrollback).
            // - Ps = 6: delete all sixel images (non-standard).
            b'J' => {
                match self.arg[0] {
                    0 => {
                        state.tclearregion(state.c.x, state.c.y, maxcol - 1, state.c.y);
                        if state.c.y < state.row - 1 {
                            state.tclearregion(0, state.c.y + 1, maxcol - 1, state.row - 1);
                        }
                    }
                    1 => {
                        if state.c.y > 0 {
                            state.tclearregion(0, 0, maxcol - 1, state.c.y - 1);
                        }
                        state.tclearregion(0, state.c.y, state.c.x, state.c.y);
                    }
                    2 => {
                        state.tclearregion(0, 0, maxcol - 1, state.row - 1);
                        state.tdeleteimages();
                    }
                    3 => {
                        // for (im = term.images; im; im = next) {
                        // 	next = im->next;
                        // 	if (im->y < 0) {
                        // 		delete_image(im);
                        // 	}
                        // }
                    }
                    6 => {
                        state.tdeleteimages();
                        state.tfulldirt();
                    }
                    _ => unknown(),
                }
            }

            // EL -- Erase in Line: CSI Ps K
            // - Ps = 0: erase from the cursor to the end of the line. Default.
            // - Ps = 1: erase from the start of the line to the cursor.
            // - Ps = 2: erase the whole line.
            b'K' => {
                match self.arg[0] {
                    0 => state.tclearregion(state.c.x, state.c.y, maxcol - 1, state.c.y),
                    1 => state.tclearregion(0, state.c.y, state.c.x, state.c.y),
                    2 => state.tclearregion(0, state.c.y, maxcol - 1, state.c.y),
                    _ => {}
                }
            }

            // SU -- Scroll Up: CSI Ps S
            // - Ps: lines to scroll up inside the scroll region. Default 1.
            //
            // XTSMGRAPHICS -- Set or Request Graphics Attribute: CSI ? Pi ; Pa ; Pv S
            // - Pi = 1: number of color registers. Pi = 2: sixel geometry in pixels. Pi = 3: ReGIS geometry.
            // - Pa = 1: read. Pa = 2: reset to default. Pa = 3: set to Pv. Pa = 4: read the maximum.
            // Reply: CSI ? Pi ; Ps ; Pv S, where Ps = 0 success, 1 bad Pi, 2 bad Pa, 3 failure.
            b'S' => {
                if self.private {
                    if self.narg > 1 {
                        // XTSMGRAPHICS
                        let pi = self.arg[0];
                        let pa = self.arg[1];
                        let pa_is_valid = pa == 1 || pa == 2 || pa == 4;

                        let term = unsafe {&mut *state._term_ptr};

                        // TODO: replace snprintf if possible
                        let mut buffer = [0u8; 40];
                        if pi == 1 && pa_is_valid {
                            // number of sixel color registers
                            // (read, reset and read the maximum value give the same response)
                            let n = snprintf!(buffer, b"\x1b[?1;0;%dS\0", DECSIXEL_PALETTE_MAX);
                            term.ttywrite(&buffer, n as usize, true);
                        } else if pi == 2 && pa_is_valid {
                            // sixel graphics geometry (in pixels)
                            // (read, reset and read the maximum value give the same response)

                            let n = snprintf!(buffer, b"\x1b[?2;0;%d;%dS\0",
                                    (state.col * win.cw as usize).min(DECSIXEL_WIDTH_MAX),
                                    (state.row * win.ch as usize).min(DECSIXEL_HEIGHT_MAX)
                                );

                            term.ttywrite(&buffer, n as usize, true);
                        } else {
                            // the number of color registers and sixel geometry can't be changed
                            // failure
                            let n = snprintf!(buffer, b"\x1b[?%d;3;0S\0", pi);

                            state.ttywrite_pty(&buffer, n as usize);
                        }
                    }

                    unknown();
                    return;
                }

                DEFAULT!(self.arg[0], 1);
                state.tscrollup(state.top, self.arg[0] as usize);
            }

            // SD -- Scroll Down: CSI Ps T
            // - Ps: lines to scroll down inside the scroll region. Default 1.
            b'T' => {
                DEFAULT!(self.arg[0], 1);
                state.tscrolldown(state.top, self.arg[0] as usize);
            }

            // IL -- Insert Lines: CSI Ps L
            // - Ps: blank lines to insert at the cursor row, inside the scroll region. Default 1.
            b'L' => {
                DEFAULT!(self.arg[0], 1);
                state.tinsertblankline(self.arg[0] as usize);
            }

            // RM -- Reset Mode: CSI Pm l
            // DECRST -- DEC Private Mode Reset: CSI ? Pm l
            // - Pm: modes to reset, see `TermState::tsetmode`.
            b'l' => {
                state.tsetmode(self.private, false, &self.arg, self.narg);
            },

            // DL -- Delete Lines: CSI Ps M
            // - Ps: lines to delete at the cursor row, inside the scroll region. Default 1.
            b'M' => {
                DEFAULT!(self.arg[0], 1);
                state.tdeleteline(self.arg[0] as usize);
            }

            // ECH -- Erase Characters: CSI Ps X
            // - Ps: characters to erase from the cursor, without shifting the line. Default 1.
            b'X' => {
                DEFAULT!(self.arg[0], 1);
                let n = (self.arg[0] - 1).max(0) as usize;
                state.tclearregion(state.c.x, state.c.y, state.c.x + n, state.c.y);
            }

            // DCH -- Delete Characters: CSI Ps P
            // - Ps: characters to delete at the cursor, shifting the rest of the line left. Default 1.
            b'P' => {
                DEFAULT!(self.arg[0], 1);
                state.tdeletechar(self.arg[0] as usize);
            }

            // CBT -- Cursor Backward Tabulation: CSI Ps Z
            // - Ps: tab stops to move back. Default 1.
            b'Z' => {
                DEFAULT!(self.arg[0], 1);
                state.tputtab(-self.arg[0] as isize);
            }

            // VPA -- Line Position Absolute: CSI Ps d
            // - Ps: 1-based row. Default 1.
            b'd' => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(state.c.x, self.arg[0] as usize - 1);
            }

            // SM -- Set Mode: CSI Pm h
            // DECSET -- DEC Private Mode Set: CSI ? Pm h
            // - Pm: modes to set, see `TermState::tsetmode`.
            b'h' => {
                state.tsetmode(self.private, true, &self.arg, self.narg);
            }

            // SGR -- Select Graphic Rendition: CSI Pm m
            // - Pm: attributes to apply, see `TermState::tsetattr`. Default 0 (reset).
            b'm' => {
                state.tsetattr(&self.arg, &self.sub, self.narg);
            }

            // DSR -- Device Status Report: CSI Ps n
            // - Ps = 5: status report. Reply: CSI 0 n (OK).
            // - Ps = 6: cursor position report (CPR). Reply: CSI r ; c R, 1-based.
            b'n' => {
                let term = unsafe {&mut *state._term_ptr};
                match self.arg[0] {
                    5 => {
                        const TEXT: &[u8] = b"\x1b[0n";
                        term.ttywrite(TEXT, TEXT.len(), false);
                    },
                    6 => {
                        let mut buffer = [0u8; 40];
                        let len = snprintf!(buffer, b"\x1b[%i;%iR\0", state.c.y + 1, state.c.x + 1);
                        term.ttywrite(&buffer, len as usize, false);
                    }
                    _ => unknown(),

                }
            }

            // DECSTBM -- Set Top and Bottom Margins: CSI Ps ; Ps r
            // - Ps 1: 1-based top row. Default 1.
            // - Ps 2: 1-based bottom row. Default: last row.
            // Moves the cursor to the home position.
            b'r' => {
                if self.private {
                    unknown();
                } else {
                    DEFAULT!(self.arg[0], 1);
                    DEFAULT!(self.arg[1], state.row as i32);
                    state.tsetscroll(self.arg[0] as usize - 1, self.arg[1] as usize - 1);
                    state.tmoveato(0, 0);
                }
            }

            // SCOSC -- Save Cursor: CSI s
            // Saves the cursor like DECSC (ESC 7).
            b's' => {
                state.tcursor(CursorMovement::CursorSave);
            }

            // SCORC -- Restore Cursor: CSI u
            // Restores the cursor like DECRC (ESC 8).
            b'u' => {
                if self.private {
                    unknown();
                } else {
                    state.tcursor(CursorMovement::CursorLoad);
                }
            }

            b' ' => {
                match self.mode[1] {
                    // DECSCUSR -- Set Cursor Style: CSI Ps SP q
                    // - Ps = 0 or 1: blinking block. Default.
                    // - Ps = 2: steady block.
                    // - Ps = 3: blinking underline. Ps = 4: steady underline.
                    // - Ps = 5: blinking bar. Ps = 6: steady bar.
                    b'q' => {
                        // TODO: implement this
                        win.set_cursor(self.arg[0]);
                        // if r != 0 {
                        //     unknown();
                        // }
                    }
                    _ => unknown(),
                }
            }

            b'>' => {
                match self.mode[1] {
                    // XTVERSION -- Report Terminal Name and Version: CSI > Ps q
                    // - Ps = 0: request the version. Default.
                    // Reply: DCS > | text ST.
                    b'q' => {
                        // TODO: implement better version reporting
                        const TEXT: &[u8] = b"\x1bP>|rst(0.1)\x1b\\";
                        state.ttywrite_pty(TEXT, TEXT.len());
                    }
                    _ => unknown(),
                }
            }

            // XTWINOPS -- Window Manipulation: CSI Ps ; Ps ; Ps t
            // - Ps = 14: report the text area size in pixels. Reply: CSI 4 ; height ; width t.
            // - Ps = 16: report the character cell size in pixels. Reply: CSI 6 ; height ; width t.
            // - Ps = 18: report the text area size in characters. Reply: CSI 8 ; rows ; cols t.
            b't' => {
                let term = unsafe {&mut *state._term_ptr};
                let mut buffer = [0u8; 40];
                match self.arg[0] {
                    14 => {
                        let len = snprintf!(buffer, b"\x1b[4;%i;%it\0", state.pixh, state.pixw);
                        term.ttywrite(&buffer, len as usize, false);
                    }

                    16 => {
                        let len = snprintf!(
                            buffer,
                            b"\x1b[6;%i;%it\0",
                            state.pixh / state.row,
                            state.pixw / state.col
                        );
                        term.ttywrite(&buffer, len as usize, false);
                    }

                    18 => {
                        let len = snprintf!(buffer, b"\x1b[8;%i;%it\0", state.row, state.col);
                        term.ttywrite(&buffer, len as usize, false);
                    }

                    _ => unknown(),
                }
            }

            // DECRQM -- Request Mode: CSI Ps $ p (ANSI) or CSI ? Ps $ p (DEC private)
            // - Ps: mode to query.
            // Reply (DECRPM): CSI ? Ps ; Pm $ y, where Pm = 0 not recognized, 1 set, 2 reset,
            // 3 permanently set, 4 permanently reset.
            b'$' => {
                match self.mode[1] {
                    b'p' => {
                        let feature_mode = match self.arg[0] {
                            // Synchronized output
                            2026 => {
                                // Supported and screen updates are shown as usual
                                // (e.g. as soon as they arrive)
                                2
                            }

                            // In-band resize notifications
                            2048 => {
                                if win.mode.contains(WinMode::ResizeNotification) {
                                    1
                                } else {
                                    2
                                }
                            }
                            _ => {
                                eprintln!("erresc: unknown DSR-EXT {}", self.arg[0]);
                                0
                            }
                        };

                        let mut buffer = [0u8; 40];
                        let len =
                            snprintf!(buffer, b"\x1b[?%d;%d$y\0", self.arg[0], feature_mode);
                        state.ttywrite_pty(&buffer, len as usize);
                    }
                    _ => unknown(),
                }
            }

            _ => unknown(),
        }
    }

    pub fn dump(&self) {
        eprint!("ESC[");

        for i in 0..self.len {
            let c = self.buf[i] & 0xFF;

            let is_printable = c.is_ascii_alphabetic()
                || c.is_ascii_digit()
                || c.is_ascii_punctuation()
                || c == b' ';
            if is_printable {
                eprint!("{}", c as char);
            } else if c == b'\n' {
                eprint!("(\\n)");
            } else if c == b'\r' {
                eprint!("(\\r)");
            } else if c == 0x1b {
                eprint!("(\\e)");
            } else {
                eprint!("({:02X})", c);
            }
        }

        eprint!("\n");
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

fn parse_arg(bytes: &[u8]) -> (i32, &[u8]) {
    let negative = bytes.first() == Some(&b'-');
    let digits = if negative { &bytes[1..] } else { bytes };

    let digit_count = digits.iter().take_while(|b| b.is_ascii_digit()).count();
    if digit_count == 0 {
        return (0, bytes);
    }

    let magnitude = digits[..digit_count].iter().fold(0i64, |acc, &b| {
        let digit = (b - b'0') as i64;
        acc.saturating_mul(10).saturating_add(digit)
    });

    let value = if negative { -magnitude } else { magnitude };
    let value = i32::try_from(value).unwrap_or(-1);

    (value, &digits[digit_count..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq(payload: &[u8]) -> CSIEscape {
        let mut c = CSIEscape::default();
        c.buf[..payload.len()].copy_from_slice(payload);
        c.len = payload.len();
        c
    }

    #[test]
    fn parse_zero_length_buffer_is_safe() {
        let mut c = seq(b"");
        c.parse();
        assert_eq!(c.narg, 0);
        assert_eq!(c.mode, [0, 0]);
        assert!(!c.private);
    }

    #[test]
    fn parse_final_byte_only_defaults_arg_to_zero() {
        // e.g. bare "\x1b[A" (CUU with no explicit count): parse() itself
        // leaves the arg at 0; the DEFAULT! macro applies the real default
        // of 1 later, in handle().
        let mut c = seq(b"A");
        c.parse();
        assert_eq!(c.narg, 1);
        assert_eq!(c.arg[0], 0);
        assert_eq!(c.mode, [b'A', 0]);
    }

    #[test]
    fn parse_single_numeric_arg() {
        let mut c = seq(b"5A");
        c.parse();
        assert_eq!(c.narg, 1);
        assert_eq!(c.arg[0], 5);
        assert_eq!(c.mode, [b'A', 0]);
    }

    #[test]
    fn parse_multiple_args_split_on_semicolon() {
        let mut c = seq(b"1;2H");
        c.parse();
        assert_eq!(c.narg, 2);
        assert_eq!(&c.arg[..2], &[1, 2]);
        assert_eq!(c.mode, [b'H', 0]);
    }

    #[test]
    fn parse_missing_middle_arg_defaults_to_zero() {
        let mut c = seq(b"1;H");
        c.parse();
        assert_eq!(c.narg, 2);
        assert_eq!(&c.arg[..2], &[1, 0]);
        assert_eq!(c.mode, [b'H', 0]);
    }

    #[test]
    fn parse_private_marker_sets_flag_and_is_not_counted_as_an_arg_digit() {
        let mut c = seq(b"?1049h");
        c.parse();
        assert!(c.private);
        assert_eq!(c.narg, 1);
        assert_eq!(c.arg[0], 1049);
        assert_eq!(c.mode, [b'h', 0]);
    }

    #[test]
    fn parse_switches_to_colon_separator_for_sgr_subparams() {
        // e.g. truecolor SGR "\x1b[38:2:255:0:0m"
        let mut c = seq(b"38:2:255:0:0m");
        c.parse();
        assert_eq!(c.narg, 5);
        assert_eq!(&c.arg[..5], &[38, 2, 255, 0, 0]);
        assert_eq!(c.mode, [b'm', 0]);
    }

    #[test]
    fn parse_two_byte_intermediate_and_final() {
        // e.g. DECSCUSR "\x1b[0 q"
        let mut c = seq(b"0 q");
        c.parse();
        assert_eq!(c.narg, 1);
        assert_eq!(c.arg[0], 0);
        assert_eq!(c.mode, [b' ', b'q']);
    }

    #[test]
    fn parse_caps_args_at_esc_arg_siz() {
        let payload = (0..20).map(|i| i.to_string()).collect::<Vec<_>>().join(";") + "m";
        let mut c = seq(payload.as_bytes());
        c.parse();
        assert_eq!(c.narg, ESC_ARG_SIZ);
        let expected: Vec<i32> = (0..ESC_ARG_SIZ as i32).collect();
        assert_eq!(&c.arg[..ESC_ARG_SIZ], expected.as_slice());
    }

    #[test]
    fn reset_clears_all_state() {
        let mut c = seq(b"?1;2m");
        c.parse();
        assert!(c.narg > 0);
        assert!(c.private);

        c.reset();
        assert_eq!(c.len, 0);
        assert_eq!(c.narg, 0);
        assert!(!c.private);
        assert_eq!(c.arg, [0; ESC_ARG_SIZ]);
        assert_eq!(c.mode, [0, 0]);
        assert!(c.buf.iter().all(|&b| b == 0));
    }
}
