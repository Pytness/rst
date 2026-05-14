// typedef struct {
// 	char buf[ESC_BUF_SIZ]; /* raw string */
// 	size_t len;            /* raw string length */
// 	char priv;
// 	int arg[ESC_ARG_SIZ];
// 	int narg; /* nb of args */
// 	char mode[2];
// };

use std::ptr::null_mut;

pub const UTF_INVALID: usize = 0xFFFD;
pub const UTF_SIZ: usize = 4;
pub const ESC_BUF_SIZ: usize = 128 * UTF_SIZ;
pub const ESC_ARG_SIZ: usize = 16;
pub const STR_BUF_SIZ: usize = ESC_BUF_SIZ;
pub const STR_ARG_SIZ: usize = ESC_ARG_SIZ;
pub const STR_TERM_ST: &[u8] = b"\x1b\\";
pub const STR_TERM_BEL: &[u8] = b"\007";

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
            mode: ['\0'; 2],
        }
    }
}

impl CSIEscape {
    pub fn parse(&mut self) {
        let mut v = 0;
        let mut sep = b';'; // colon or semi-colon, but not both

        let mut p: *const u8 = self.buf.as_ptr();
        let mut np: *mut u8 = null_mut();

        self.narg = 0;

        unsafe {
            if *p == b'?' {
                self.private = true;
            }
            self.buf[self.len] = 0;

            while p < self.buf.as_ptr().add(self.len) {
                v = libc::strtol(p as *const i8, (np) as *mut *mut i8, 10);

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

    pub fn handle(&self) {
        // TODO:
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
