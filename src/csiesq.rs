// typedef struct {
// 	char buf[ESC_BUF_SIZ]; /* raw string */
// 	size_t len;            /* raw string length */
// 	char priv;
// 	int arg[ESC_ARG_SIZ];
// 	int narg; /* nb of args */
// 	char mode[2];
// };

use std::ptr::null_mut;

const UTF_INVALID: usize = 0xFFFD;
const UTF_SIZ: usize = 4;
const ESC_BUF_SIZ: usize = 128 * UTF_SIZ;
const ESC_ARG_SIZ: usize = 16;
const STR_BUF_SIZ: usize = ESC_BUF_SIZ;
const STR_ARG_SIZ: usize = ESC_ARG_SIZ;
// const STR_TERM_ST  : usize = "\033\\"
// const STR_TERM_BEL : usize = "\007";

#[derive(Debug)]
pub struct CSIEscape {
    pub buf: [char; ESC_BUF_SIZ], // raw string
    pub len: usize,               // raw string length
    private: char,

    pub arg: [i32; ESC_ARG_SIZ],
    pub narg: usize, // nb of args
    pub mode: [char; 2],
}

impl Default for CSIEscape {
    fn default() -> Self {
        Self {
            buf: ['\0'; ESC_BUF_SIZ],
            len: 0,
            private: '\0',
            arg: [0; ESC_ARG_SIZ],
            narg: 0,
            mode: ['\0'; 2],
        }
    }
}

impl CSIEscape {
    pub fn parse(&mut self) {
        unsafe {
            let mut p = self.buf.as_ptr();
            let mut sep = ';';

            self.narg = 0;

            if *p == '?' {
                self.private = '?';
                p = p.add(1);
            }

            self.buf[self.len] = '\0';

            while p < self.buf.as_mut_ptr().add(self.len) {
                let mut np = null_mut();

                let mut v = libc::strtol(p as *const i8, &mut np, 10);

                if np as *const char == p {
                    v = 0;
                }

                if v == i64::MIN || v == i64::MAX {
                    v = -1;
                }

                self.arg[self.narg] = v as i32;
                self.narg += 1;
                p = np as *mut char;

                if sep == ';' && *p == ':' {
                    sep = ':'; // allow override to colon once
                }
                if *p != sep || self.narg == ESC_ARG_SIZ {
                    break;
                }
                p = p.add(1);
            }

            self.mode[0] = *p;
            p = p.add(1);

            let read = p.offset_from(self.buf.as_ptr());
            self.mode[1] = if read < self.len as isize { *p } else { '\0' };
        }
    }
}
