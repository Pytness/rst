use crate::config;
use crate::csiesq::STR_BUF_SIZ;
use crate::term_state::TermState;
use std::ptr::null;

pub const STR_ARG_SIZ: usize = 16;

struct OscEntry {
    idx: u32,
    str: String,
}

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
        let mut osc_table = vec![
            OscEntry {
                idx: config::DEFAULTFG,
                str: "foreground".to_string(),
            },
            OscEntry {
                idx: config::DEFAULTBG,
                str: "background".to_string(),
            },
            OscEntry {
                idx: config::DEFAULTCS,
                str: "cursor".to_string(),
            },
        ];

        // FIX:
        // term.esc &= ~(ESC_STR_END | ESC_STR);

        self.parse()
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
}
