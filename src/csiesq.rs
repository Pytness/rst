use std::ptr::null_mut;

use crate::config::VTIDEN;
use crate::sixel::{DECSIXEL_HEIGHT_MAX, DECSIXEL_PALETTE_MAX, DECSIXEL_WIDTH_MAX};
use crate::term_state::{CursorMovement, TermMode, TermState};
use crate::win::TermWindow;

pub const UTF_INVALID: usize = 0xFFFD;
pub const UTF_SIZ: usize = 4;
pub const ESC_BUF_SIZ: usize = 128 * UTF_SIZ;
pub const ESC_ARG_SIZ: usize = 16;
pub const STR_BUF_SIZ: usize = ESC_BUF_SIZ;
pub const STR_ARG_SIZ: usize = ESC_ARG_SIZ;
pub const STR_TERM_ST: &[u8] = b"\x1b\\";
pub const STR_TERM_BEL: &[u8] = b"\007";

macro_rules! DEFAULT {
    ($src:expr, $value:expr) => {
        if $src == 0 {
            $src = $value;
        }
    };
}

macro_rules! snprintf {
    ($buffer:expr, $format:expr, $($arg:expr),*) => {
        unsafe {
            libc::snprintf(
                $buffer.as_mut_ptr() as *mut i8,
                $buffer.len(),
                $format.as_ptr() as *const i8,
                $($arg),*
            )
        }
    };
}

#[derive(Debug)]
pub struct CSIEscape {
    pub buf: [u8; ESC_BUF_SIZ], // raw string
    pub len: usize,             // raw string length
    private: bool,

    pub arg: [i32; ESC_ARG_SIZ],
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
            narg: 0,
            mode: [b'\0'; 2],
        }
    }
}

impl CSIEscape {
    pub fn parse(&mut self) {
        let mut p: *const u8 = self.buf.as_ptr();
        let mut np: *mut u8;

        let mut v;
        let mut sep = b';'; // colon or semi-colon, but not both

        self.narg = 0;

        unsafe {
            if *p == b'?' {
                self.private = true;
            }
            self.buf[self.len] = 0;

            while p < self.buf.as_ptr().add(self.len) {
                np = null_mut();
                v = libc::strtol(p as *const i8, (&raw mut np) as *mut *mut i8, 10);

                if (np as *const u8).eq(&p) {
                    v = 0;
                }

                if v == i64::MAX || v == i64::MIN {
                    v = -1;
                }

                self.arg[self.narg] = v as i32;
                self.narg += 1;

                p = np as *const u8;

                if sep == b';' && *p as u8 == b':' {
                    sep = b':'; // allow override to colon once
                }

                if *p as u8 != sep || self.narg == ESC_ARG_SIZ {
                    break;
                }

                p = p.add(1);
            }

            self.mode[0] = *p;
            self.mode[1] = if p < self.buf.as_ptr().add(self.len) {
                *p
            } else {
                0
            };
        }
    }

    pub fn handle(&mut self, state: &mut TermState, win: &mut TermWindow) {
        let maxcol = state.col;

        let unknown = || {
            eprint!("erresc: uknown csi ");
            self.dump();
        };

        match self.mode[0] {
            // ICH -- Insert <n> blank char
            b'@' => {
                DEFAULT!(self.arg[0], 1);
                state.tinsertblank(self.arg[0] as usize);
            }

            // CUU -- Cursor <n> Up
            b'A' => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(state.c.x, state.c.y.saturating_sub(self.arg[0] as usize));
            }

            b'B' | // CUD -- Cursor <n> Down
            b'e'   // VPR -- Cursor <n> Down
            => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(state.c.x, state.c.y - self.arg[0] as usize);
            }

            // MC -- Media Copy
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

            // dA -- Device Attributes
            b'c' => {
                if self.arg[0] == 0 {
                    state.ttywrite_pty(VTIDEN, VTIDEN.len());
                }
            }

            // REP -- if last char is printable print it <n> more times
            b'b' => {
                self.arg[0] = self.arg[0].max(1).min(65535);

                if state.lastc != '\0' {
                    for _ in 0..self.arg[0] {
                        state.tputc_char(state.lastc);
                    }
                }
            }

            b'C' | // CUF -- Cursor <n> Forward
            b'a'  // HPR -- Cursor <n> Forward
            => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(state.c.x + self.arg[0] as usize, state.c.y);
            }

            // CUB  -- Cursor <n> Backward
            b'D' => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(state.c.x - self.arg[0] as usize, state.c.y);
            }

            // CNL -- Cursor <n> Down and first col
            b'E' => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(0, state.c.y + self.arg[0] as usize);
            }

            // CPL -- Cursor <n> Up and first col
            b'F' => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(0, state.c.y - self.arg[0] as usize);
            }

            // TBC -- Tabulation clear
            b'g' => {
                match self.arg[0] {
                    // clear current tab sotp
                    0 => state.tabs[state.c.x] = 0,
                    // clear all the tabs
                    3 => state.tabs.iter_mut().for_each(|t| *t = 0),
                    _ =>  unknown(),

                }
            }

            b'G' | // CHA -- Move to <col>
            b'`'   // HPA
            => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(self.arg[0] as usize - 1, state.c.y);
            }

            b'H' | // CUP -- Move to <row> <column>
            b'f'   // HVP
            => {
                DEFAULT!(self.arg[0], 1);
                DEFAULT!(self.arg[1], 1);
                state.tmoveato(self.arg[1] as usize - 1, self.arg[0] as usize - 1);
            }

            // CHT -- CUrsor Forwar Tabulation <n> tab stops
            b'I' => {
                DEFAULT!(self.arg[0], 1);
                state.tputtab(self.arg[0] as isize);
            }

            // ED -- Clear screen
            b'J' => {
                match self.arg[0] {
                    // below
                    0 => {
			state.tclearregion(state.c.x, state.c.y, maxcol - 1, state.c.y);
                        if state.c.y < state.row - 1 {
                            state.tclearregion(0, state.c.y + 1, maxcol - 1, state.row - 1);
                        }
                    }
                    // above
                    1 => {
                        if state.c.y > 0 {
                            state.tclearregion(0, 0, maxcol - 1, state.c.y - 1);
                        }
                        state.tclearregion(0, state.c.y, state.c.x, state.c.y);
                    }
                    // screen
                    2 => {
                        state.tclearregion(0, 0, maxcol - 1, state.row - 1);
                        state.tdeleteimages();
                    }
                    // scrollback
                    3 => {
                        // for (im = term.images; im; im = next) {
                        // 	next = im->next;
                        // 	if (im->y < 0) {
                        // 		delete_image(im);
                        // 	}
                        // }
                    }
                    // sixels
                    6 => {
                        state.tdeleteimages();
                        state.tfulldirt();
                    }
                    _ => unknown(),
                }
            }
            // EL -- Clear line
            b'K' => {
                match self.arg[0] {
                    // right
                    0 => state.tclearregion(state.c.x, state.c.y, maxcol - 1, state.c.y),
                    // left
                    1 => state.tclearregion(0, state.c.y, state.c.x, state.c.y),
                    // all
                    2 => state.tclearregion(0, state.c.y, maxcol - 1, state.c.y),
                    _ => {}
                }
            }

            // Su -- Scroll <n> line up ; XTSMGRAPHICS
            b'S' => {
                if self.private {
                    if self.narg > 1 {
                        // XTSMGRAPHICS
                        let pi = self.arg[0];
                        let pa = self.arg[1];
                        let pa_is_valid = pa == 1 || pa == 2 || pa == 4;

                        // TODO: replace snprintf if possible
                        let mut buffer = [0u8; 40];
                        if pi == 1 && pa_is_valid {
                            // number of sixel color registers
                            // (read, reset and read the maximum value give the same response)
                            let n = snprintf!(buffer, b"\x1b[?1;0;%dS\0", DECSIXEL_PALETTE_MAX);
                            state.ttywrite_pty(&buffer, n as usize);
                        } else if pi == 2 && pa_is_valid {
                            // sixel graphics geometry (in pixels)
                            // (read, reset and read the maximum value give the same response)

                            let n = snprintf!(buffer, b"\x1b[?2;0;%d;%dS\0",
                                    (state.col * win.cw as usize).min(DECSIXEL_WIDTH_MAX),
                                    (state.row * win.ch as usize).min(DECSIXEL_HEIGHT_MAX)
                                );

                            state.ttywrite_pty(&buffer, n as usize);
                        } else {
                            // the number of color registers and sixel geometry can't be changed
                            // failure
                            let n = snprintf!(buffer, b"\x1b[?%d;3;0S\0", pi);

                            state.ttywrite_pty(&buffer, n as usize);
                            unknown();
                        }
                    } else {
                        unknown();
                    }
                }

                DEFAULT!(self.arg[0], 1);
                state.tscrollup(state.top, self.arg[0] as usize);
            }

            // SD -- Scroll Mn> line down
            b'T' => {
                DEFAULT!(self.arg[0], 1);
                state.tscrolldown(state.top, self.arg[0] as usize);
            }

            // IL -- Insert <n> blank line(s)
            b'L' => {
                DEFAULT!(self.arg[0], 1);
                state.tinsertblankline(self.arg[0] as usize);
            }

            // RM -- Reset Mode
            b'l' => {
                state.tsetmode(self.private, 0, &self.arg, self.narg);
            },

            // DL -- Delete Mn> lines
            b'M' => {
                DEFAULT!(self.arg[0], 1);
                state.tdeleteline(self.arg[0] as usize);
            }

            // ECH -- Erase <n> char
            b'X' => {
                DEFAULT!(self.arg[0], 1);
                state.tclearregion(state.c.x, state.c.y, state.c.x + (self.arg[0] - 1) as usize, state.c.y);
            }

            // DCH -- Delete <n> char
            b'P' => {
                DEFAULT!(self.arg[0], 1);
                state.tdeletechar(self.arg[0] as usize);
            }

            // CBT -- Cursor Backward Tabulation <n> tab stops
            b'Z' => {
                DEFAULT!(self.arg[0], 1);
                state.tputtab(-self.arg[0] as isize);
            }

            // VPA -- Move to <row>
            b'd' => {
                DEFAULT!(self.arg[0], 1);
                state.tmoveto(state.c.x, self.arg[0] as usize - 1);
            }

            // SM -- Set terminal mode
            b'h' => {
                state.tsetmode(self.private, 1, &self.arg, self.narg);
            }

            // SGR - Terminal attribute (color)
            b'm' => {
                state.tsetattr(&self.arg, self.narg);
            }

            // DSR -- Device Status Report
            b'n' => {
                match self.arg[0] {
                    // Status Report "OK" `0n`
                    5 => {
                        const TEXT: &[u8] = b"\x1b[0n";
                        state.ttywrite_pty(TEXT, TEXT.len());
                    },
                    // Report Cursor Position (CPR) "<row>;<column>R"
                    6 => {
                        let mut buffer = [0u8; 40];
                        let len = snprintf!(buffer, b"\x1b[%i;%iR\0", state.c.y + 1, state.c.x + 1);
                        state.ttywrite_pty(&buffer, len as usize);
                    }
                    _ => unknown(),

                }
            }

            // DECSTBM -- Set scrolling region
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

            // DECSC -- Save Cursor Position (ANIS.SYS)
            b's' => {
                state.tcursor(CursorMovement::CursorSave);
            }

            // DECRC -- Restore cursor position (ANIS.SYS)
            b'u' => {
                if self.private {
                    unknown();
                } else {
                    state.tcursor(CursorMovement::CursorLoad);
                }
            }

            b' ' => {
                match self.mode[1] {
                    // DECSCUSR -- Set Cursor Style
                    b'q' => {
                        // TODO: implement this
                        // let r = xsetcursor(self.arg[0]);
                        // if r != 0 {
                        //     unknown();
                        // }
                    }
                    _ => unknown(),
                }
            }

            b'>' => {
                match self.mode[1] {
                    // XTVERSION -- Print terminal name and version
                    b'q' => {
                        // TODO: implement better version reporting
                        const TEXT: &[u8] = b"\x1bP>|rst(0.1)\x1b\\";
                        state.ttywrite_pty(TEXT, TEXT.len());
                    }
                    _ => unknown(),
                }
            }

            // XTWINOPS -- Window manipulation
            b't' => {
                let mut buffer = [0u8; 40];
                match self.arg[0] {
                    // Report text area size in pixels
                    14 => {
                        let len = snprintf!(buffer, b"\x1b[4;%i;%it\0", state.pixh, state.pixw);
                        state.ttywrite_pty(&buffer, len as usize);
                    }

                    // Report character cell sie in pixels
                    16 => {
			let len = snprintf!(buffer, "\033[6;%i;%it", state.pixh / state.row, state.pixw / state.col);
                        state.ttywrite_pty(&buffer, len as usize);
                    }

                    // Report the size of the text area in characters
                    18 => {
                        let len = snprintf!(buffer, "\033[8;%i;%it", state.row, state.col);
                        state.ttywrite_pty(&buffer, len as usize);
                    }

                    _ => unknown(),
                }
            }

            // DSR-EXT -- Device Status Report (Extended)
            b'$' => {
                match self.mode[1] {
                    b'p' => {
                        let feature_mode = match self.arg[0] {
                            // Synchronized updates
                            2026 => {
                                // Supported and screen updates are shown as usual
                                // (e.g. as soon as they arrive)
                                2
                            }
                            _ => {
                                eprintln!("erresc: unknown DSR-EXT {}", self.arg[0]);
                                0
                            }
                        };

                        let mut buffer = [0u8; 40];
                        let len = snprintf!(buffer, "\033[?%d;%d$y", self.arg[0], feature_mode);
                        state.ttywrite_pty(&buffer, len as usize);
                    }
                    _ => unknown(),
                }
            }

            _ => unknown(),
        }
    }

    pub fn dump(&self) {
        eprintln!("ESC[");

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
