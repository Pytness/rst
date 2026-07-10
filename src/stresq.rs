use crate::colors::ColorRegistry;
use crate::config;
use crate::csiesq::STR_BUF_SIZ;
use crate::term_state::TermState;
use crate::terminal::EscapeState;
use crate::terminal::Term;
use std::ffi::CStr;
use std::ffi::CString;
use std::io::Cursor;
use std::io::Write as _;
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

    pub fn handle(
        &mut self,
        esc: &mut EscapeState,
        state: &mut TermState,
        colors: &mut ColorRegistry,

        // FIX: DONT USE POINTERS!
        term_ptr: *mut Term,
    ) {
        esc.remove(EscapeState::ESC_STR_END | EscapeState::ESC_STR);

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

                10 | 11 | 12 if narg >= 2 => {
                    let p = self.args[1];

                    let j = par - 10;
                    if j < 0 || j >= OSC_TABLE.len() as i32 {
                        return;
                    }

                    let p_str = unsafe { CStr::from_ptr(p as *const i8) }
                        .to_str()
                        .unwrap_or_default();

                    if p_str != "?" {
                        self.osc_color_response(
                            &colors,
                            par,
                            OSC_TABLE[j as usize].idx,
                            false,
                            term_ptr,
                        );
                    } else if colors.set_color_name(OSC_TABLE[j as usize].idx as usize, p_str) {
                        eprintln!(
                            "erresc: invalid {} color: {}",
                            OSC_TABLE[j as usize].str, p_str
                        );
                    } else {
                        unsafe {
                            (*term_ptr).state.tfulldirt();
                        }
                    }
                }

                // color set
                4 | 104 => 'color_set: {
                    let mut p = null();

                    if self.type_ == 4 {
                        if self.narg < 3 {
                            break 'color_set;
                        }

                        p = self.args[2];
                    }

                    let j = if self.narg > 1 {
                        let cstr = unsafe { CString::from_raw(self.args[1] as *mut i8) };
                        let par = cstr.to_str().unwrap_or_default();
                        par.parse::<i32>().unwrap_or(0)
                    } else {
                        -1
                    };

                    let p_str =
                        unsafe { CStr::from_ptr(p as *const i8).to_str().unwrap_or_default() };

                    if !p.is_null() && p_str != "?" {
                        self.osc_color_response(colors, j, 0, true, term_ptr);
                    } else if j >= 0 && colors.set_color_name(j as usize, p_str) {
                        if self.type_ == 104 && self.narg <= 1 {
                            colors.load_colors();
                            return;
                        }

                        eprintln!("erresc: invalid color j={}, p={}", j, p_str,);
                    } else {
                        // TODO: if defaulbg color is changed, borders are dirty
                        unsafe {
                            (*term_ptr).state.tfulldirt();
                        }
                    }
                }
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

    fn osc_color_response(
        &self,
        colors: &ColorRegistry,
        num: i32,
        index: u32,
        is_osc4: bool,
        term_ptr: *mut Term,
    ) {
        let osc_name = if is_osc4 { "osc4" } else { "osc" };
        let color_value = if is_osc4 {
            num as usize
        } else {
            index as usize
        };

        let Some(color) = colors.get_color(color_value) else {
            eprintln!("erresc: failed to fetch {} color {}", osc_name, color_value);

            return;
        };

        let mut buffer = [0u8; 32];
        let mut cursor = Cursor::new(&mut buffer[..]);

        let terminator: &str = unsafe { CStr::from_ptr(self.term as *const i8).to_str().unwrap() };

        let err = write!(
            cursor,
            "\x1b]{}{};rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}{}",
            if is_osc4 { "4;" } else { "" },
            num,
            color.red,
            color.red,
            color.green,
            color.green,
            color.blue,
            color.blue,
            terminator
        );

        let n = cursor.position() as usize;

        // TODO: check n < 0?
        if err.is_err() {
            eprintln!(
                "error: snprintf failed while printing {} response",
                osc_name
            );
        } else if n > buffer.len() {
            eprintln!(
                "error: truncation occurred while printing {} response",
                osc_name
            );
        } else {
            unsafe { (*term_ptr).ttywrite(&buffer, n, true) };
        }
    }
}
