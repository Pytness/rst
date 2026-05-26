use std::ptr::null_mut;

use crate::sixel::{DECSIXEL_HEIGHT_MAX, DECSIXEL_PALETTE_MAX, DECSIXEL_WIDTH_MAX};
use crate::terminal::{CursorMovement, CursorState, Term, TermMode, vtiden};
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

    pub fn handle(&mut self, term: *mut Term, win: *mut TermWindow) {
        let term = unsafe { &mut *term };
        let win = unsafe { &mut *win };
        let maxcol = term.col;

        let unknown = || {
            eprint!("erresc: uknown csi ");
            self.dump();
        };

        match self.mode[0] {
            // ICH -- Insert <n> blank char
            b'@' => {
                DEFAULT!(self.arg[0], 1);
                term.tinsertblank(self.arg[0] as usize);
            }

            // CUU -- Cursor <n> Up
            b'A' => {
                DEFAULT!(self.arg[0], 1);
                term.tmoveto(term.c.x, term.c.y - self.arg[0] as usize);
            }

            b'B' | // CUD -- Cursor <n> Down
            b'e'   // VPR -- Cursor <n> Down
            => {
                DEFAULT!(self.arg[0], 1);
                term.tmoveto(term.c.x, term.c.y - self.arg[0] as usize);
            }

            // MC -- Media Copy
            b'i' => {
                match self.arg[0] {
                    0 => term.tdump(),
                    1 => term.tdumpline(term.c.y),
                    2 => term.tdumpsel(),
                    4 => term.mode.remove(TermMode::MODE_PRINT),
                    5 => term.mode.insert(TermMode::MODE_PRINT),
                    _ => {}
                }
            }

            // dA -- Device Attributes
            b'c' => {
                if self.arg[0] == 0 {
                    term.ttywrite(vtiden, vtiden.len(), false);
                }
            }

            // REP -- if last char is printable print it <n> more times
            b'b' => {
                self.arg[0] = self.arg[0].max(1).min(65535);

                if term.lastc != '\0' {
                    for _ in 0..self.arg[0] {
                        term.tputc(term.lastc);
                    }
                }
            }

            b'C' | // CUF -- Cursor <n> Forward
            b'a'  // HPR -- Cursor <n> Forward
            => {
                DEFAULT!(self.arg[0], 1);
                term.tmoveto(term.c.x + self.arg[0] as usize, term.c.y);
            }

            // CUB  -- Cursor <n> Backward
            b'D' => {
                DEFAULT!(self.arg[0], 1);
                term.tmoveto(term.c.x - self.arg[0] as usize, term.c.y);
            }

            // CNL -- Cursor <n> Down and first col
            b'E' => {
                DEFAULT!(self.arg[0], 1);
                term.tmoveto(0, term.c.y + self.arg[0] as usize);
            }

            // CPL -- Cursor <n> Up and first col
            b'F' => {
                DEFAULT!(self.arg[0], 1);
                term.tmoveto(0, term.c.y - self.arg[0] as usize);
            }

            // TBC -- Tabulation clear
            b'g' => {
                match self.arg[0] {
                    // clear current tab sotp
                    0 => term.tabs[term.c.x] = 0,
                    // clear all the tabs
                    3 => term.tabs.iter_mut().for_each(|t| *t = 0),
                    _ =>  unknown(),

                }
            }

            b'G' | // CHA -- Move to <col>
            b'`'   // HPA
            => {
                DEFAULT!(self.arg[0], 1);
                term.tmoveto(self.arg[0] as usize - 1, term.c.y);
            }

            b'H' | // CUP -- Move to <row> <column>
            b'f'   // HVP
            => {
                DEFAULT!(self.arg[0], 1);
                DEFAULT!(self.arg[1], 1);
                term.tmoveato(self.arg[1] as usize - 1, self.arg[0] as usize - 1);
            }

            // CHT -- CUrsor Forwar Tabulation <n> tab stops
            b'I' => {
                DEFAULT!(self.arg[0], 1);
                term.tputtab(self.arg[0] as isize);
            }

            // ED -- Clear screen
            b'J' => {
                match self.arg[0] {
                    // below
                    0 => {
			term.tclearregion(term.c.x, term.c.y, maxcol - 1, term.c.y);
                        if term.c.y < term.row - 1 {
                            term.tclearregion(0, term.c.y + 1, maxcol - 1, term.row - 1);
                        }
                    }
                    // above
                    1 => {
                        if term.c.y > 0 {
                            term.tclearregion(0, 0, maxcol - 1, term.c.y - 1);
                        }
                        term.tclearregion(0, term.c.y, term.c.x, term.c.y);
                    }
                    // screen
                    2 => {
                        term.tclearregion(0, 0, maxcol - 1, term.row - 1);
                        term.tdeleteimages();
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
                        term.tdeleteimages();
                        term.tfulldirt();
                    }
                    _ => unknown(),
                }
            }
            // EL -- Clear line
            b'K' => {
                match self.arg[0] {
                    // right
                    0 => term.tclearregion(term.c.x, term.c.y, maxcol - 1, term.c.y),
                    // left
                    1 => term.tclearregion(0, term.c.y, term.c.x, term.c.y),
                    // all
                    2 => term.tclearregion(0, term.c.y, maxcol - 1, term.c.y),
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
                            term.ttywrite(&buffer, n as usize, true);
                        } else if pi == 2 && pa_is_valid {
                            // sixel graphics geometry (in pixels)
                            // (read, reset and read the maximum value give the same response)

                            let n = snprintf!(buffer, b"\x1b[?2;0;%d;%dS\0",
                                    (term.col * win.cw as usize).min(DECSIXEL_WIDTH_MAX),
                                    (term.row * win.ch as usize).min(DECSIXEL_HEIGHT_MAX)
                                );

                            term.ttywrite(&buffer, n as usize, true);
                        } else {
                            // the number of color registers and sixel geometry can't be changed
                            // failure
                            let n = snprintf!(buffer, b"\x1b[?%d;3;0S\0", pi);

                            term.ttywrite(&buffer, n as usize, true);
                            unknown();
                        }
                    } else {
                        unknown();
                    }
                }

                DEFAULT!(self.arg[0], 1);
                term.tscrollup(term.top, self.arg[0] as usize);
            }

            // SD -- Scroll Mn> line down
            b'T' => {
                DEFAULT!(self.arg[0], 1);
                term.tscrolldown(term.top, self.arg[0] as usize);
            }

            // IL -- Insert <n> blank line(s)
            b'L' => {
                DEFAULT!(self.arg[0], 1);
                term.tinsertblankline(self.arg[0] as usize);
            }

            // RM -- Reset Mode
            b'l' => {
                term.tsetmode(self.private, 0, &self.arg, self.narg);
            },

            // DL -- Delete Mn> lines
            b'M' => {
                DEFAULT!(self.arg[0], 1);
                term.tdeleteline(self.arg[0] as usize);
            }

            // ECH -- Erase <n> char
            b'X' => {
                DEFAULT!(self.arg[0], 1);
                term.tclearregion(term.c.x, term.c.y, term.c.x + (self.arg[0] - 1) as usize, term.c.y);
            }

            // DCH -- Delete <n> char
            b'P' => {
                DEFAULT!(self.arg[0], 1);
                term.tdeletechar(self.arg[0] as usize);
            }

            // CBT -- Cursor Backward Tabulation <n> tab stops
            b'Z' => {
                DEFAULT!(self.arg[0], 1);
                term.tputtab(-self.arg[0] as isize);
            }

            // VPA -- Move to <row>
            b'd' => {
                DEFAULT!(self.arg[0], 1);
                term.tmoveto(term.c.x, self.arg[0] as usize - 1);
            }

            // SM -- Set terminal mode
            b'h' => {
                term.tsetmode(self.private, 1, &self.arg, self.narg);
            }

            // SGR - Terminal attribute (color)
            b'm' => {
                term.tsetattr(&self.arg, self.narg);
            }

            // DSR -- Device Status Report
            b'n' => {
                match self.arg[0] {
                    // Status Report "OK" `0n`
                    5 => {
                        const TEXT: &[u8] = b"\x1b[0n";
                        term.ttywrite(TEXT, TEXT.len(), false);
                    },
                    // Report Cursor Position (CPR) "<row>;<column>R"
                    6 => {
                        let mut buffer = [0u8; 40];
                        let len = snprintf!(buffer, b"\x1b[%i;%iR\0", term.c.y + 1, term.c.x + 1);
                        term.ttywrite(&buffer, len as usize, true);
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
                    DEFAULT!(self.arg[1], term.row as i32);
                    term.tsetscroll(self.arg[0] as usize - 1, self.arg[1] as usize - 1);
                    term.tmoveato(0, 0);
                }
            }

            // DECSC -- Save Cursor Position (ANIS.SYS)
            b's' => {
                term.tcursor(CursorMovement::CURSOR_SAVE);
            }

            // DECRC -- Restore cursor position (ANIS.SYS)
            b'u' => {
                if self.private {
                    unknown();
                } else {
                    term.tcursor(CursorMovement::CURSOR_LOAD);
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
                        term.ttywrite(TEXT, TEXT.len(), false);
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
                        let len = snprintf!(buffer, b"\x1b[4;%i;%it\0", term.pixh, term.pixw);
                        term.ttywrite(&buffer, len as usize, false);
                    }

                    // Report character cell sie in pixels
                    16 => {
			let len = snprintf!(buffer, "\033[6;%i;%it", term.pixh / term.row, term.pixw / term.col);
                        term.ttywrite(&buffer, len as usize, false);
                    }

                    // Report the size of the text area in characters
                    18 => {
                        let len = snprintf!(buffer, "\033[8;%i;%it", term.row, term.col);
                        term.ttywrite(&buffer, len as usize, false);
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
                        term.ttywrite(&buffer, len as usize, false);
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
}
