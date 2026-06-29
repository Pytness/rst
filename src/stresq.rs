use crate::config;
use crate::csiesq::STR_BUF_SIZ;
use crate::term_state::TermState;
use std::ffi::CString;
use std::ptr::null;

pub const STR_ARG_SIZ: usize = 16;

struct OscEntry {
    idx: u32,
    str: &'static str,
}

const OSC_TABLE: [OscEntry; 3] = [
    OscEntry {
        idx: config::DEFAULTFG,
        str: "foreground",
    },
    OscEntry {
        idx: config::DEFAULTBG,
        str: "background",
    },
    OscEntry {
        idx: config::DEFAULTCS,
        str: "cursor",
    },
];

/// Holds the current STR/DCS/OSC/APC/PM escape sequence being accumulated.
#[derive(Debug)]
pub struct StrEscape {
    /// The type byte of the escape sequence (e.g. b'P' for DCS)
    pub type_: u8, // ESC type
    pub buf: Vec<u8>, // allocated raw string
    pub size: usize,
    pub len: usize,                     // raw string length
    pub args: [*const u8; STR_ARG_SIZ], // parsed arguments
    pub narg: usize,                    // number of arguments
    pub term: *const u8,                // terminator: ST or BEL
}

impl Default for StrEscape {
    fn default() -> Self {
        Self {
            type_: 0,
            buf: Vec::with_capacity(STR_BUF_SIZ),
            size: 0,
            len: 0,
            args: [null(); STR_ARG_SIZ],
            narg: 0,
            term: null(),
        }
    }
}

impl StrEscape {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.type_ = 0;
        self.buf.clear();
        self.len = 0;
        self.size = 0;
        self.term = null();
    }

    pub fn handle(&mut self, state: &mut TermState) {
        // FIX:
        // term.esc &= ~(ESC_STR_END | ESC_STR);

        self.parse();

        let narg = self.narg;
        let par: i32 = if self.narg > 0 {
            unsafe {
                let cstr = CString::from_raw(self.args[0] as *mut i8);
                let par = cstr.to_str().unwrap_or_default();
                par.parse::<i32>().unwrap_or(0)
            }
        } else {
            0
        };

        match self.type_ {
            // OSC -- Operating System Command
            b']' => match par {
                0 => {
                    if narg > 1 {
                        // TODO:
                        // xsettitle(self.args[1]);
                        // xseticontitle(self.args[1]);
                    }
                    return;
                }
                1 => {
                    if narg > 1 {
                        // TODO:
                        // xseticontitle(self.args[1]);
                    }
                    return;
                }
                2 => {
                    if narg > 1 {
                        // TODO:
                        // xsettitle(self.args[1]);
                    }
                    return;
                }
                52 => {
                    // TODO:
                    // if (narg > 2 && allowwindowops) {
                    //     dec = base64dec(self.args[2]);
                    //     if (dec) {
                    //         xsetsel(dec);
                    //         xclipcopy();
                    //     } else {
                    //         fprintf(stderr, "erresc: invalid base64\n");
                    //     }
                    // }
                    return;
                }
                /* Clear Hyperlinks */
                8 => {
                    return;
                }

                10 | 11 | 12 if narg >= 2 => {}
                _ => {}
            },
            _ => {}
        }
    }

    pub fn parse(&mut self) {
        let mut c = 0;
        let mut p = self.buf.as_mut_ptr();

        self.narg = 0;
        self.buf[self.len] = 0;

        unsafe {
            if *p == 0 {
                return;
            }

            // preserve semicolon in window titles, icon names and OSC 7 sequences
            if self.type_ == b']' && (self.buf[0] <= b'2') && self.buf[1] == b';' {
                self.args[self.narg] = p;
                self.args[self.narg + 1] = p.add(2);
                self.buf[1] = 0;

                self.narg += 2;

                return;
            }

            while self.narg < STR_ARG_SIZ {
                self.args[self.narg] = p;
                self.narg += 1;

                while *p != b';' && *p != 0 {
                    p = p.add(1);
                }

                if *p == 0 {
                    break;
                }

                *(p) = 0;
                p = p.add(1);
            }
        }
    }

    fn strdump(&self) {
        eprintln!("ESC{}", self.type_ as char);
        for i in 0..self.len {
            let c = self.buf[i] as char;

            if c == '\0' {
                eprint!("\n");
                return;
            }

            if c == '\n' {
                eprint!("(\\n)");
            } else if c == '\r' {
                eprint!("(\\r)");
            } else if c == '\x1b' {
                eprint!("(\\e");
            } else if c.is_ascii_graphic() {
                eprint!("{}", c);
            } else {
                eprint!("({:02x})", c as u8);
            }
        }

        let terminator = unsafe { if *self.term == 0x1b { "ESC\\" } else { "BEL" } };
        eprintln!("{}", terminator);
    }
}
