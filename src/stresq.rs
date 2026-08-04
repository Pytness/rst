use crate::colors::ColorRegistry;
use crate::config;
use crate::csiesq::STR_BUF_SIZ;
use crate::term_state::TermMode;
use crate::term_state::TermState;
use crate::terminal::EscapeState;
use crate::terminal::Term;
use std::ffi::CStr;
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
        self.size = STR_BUF_SIZ;
        self.len = 0;
        self.args = [null(); STR_ARG_SIZ];
        self.narg = 0;
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
                let cstr = CStr::from_ptr(self.args[0] as *const i8);
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

                    if p_str == "?" {
                        self.osc_color_response(
                            &colors,
                            par,
                            OSC_TABLE[j as usize].idx,
                            false,
                            term_ptr,
                        );
                    } else if !colors
                        .set_color_name(OSC_TABLE[j as usize].idx as usize, Some(p_str))
                    {
                        eprintln!(
                            "erresc: invalid {} color: {}",
                            OSC_TABLE[j as usize].str, p_str
                        );
                    } else {
                        unsafe {
                            (*term_ptr).state.tfulldirt();
                        }
                    }

                    return;
                }

                // color set
                4 | 104 => 'color_set: {
                    let mut p = null();

                    if par == 4 {
                        if self.narg < 3 {
                            break 'color_set;
                        }

                        p = self.args[2];
                    }

                    let j = if self.narg > 1 {
                        let cstr = unsafe { CStr::from_ptr(self.args[1] as *const i8) };
                        let par = cstr.to_str().unwrap_or_default();
                        par.parse::<i32>().unwrap_or(0)
                    } else {
                        -1
                    };

                    let p_str = if !p.is_null() {
                        let v =
                            unsafe { CStr::from_ptr(p as *const i8).to_str().unwrap_or_default() };
                        Some(v)
                    } else {
                        None
                    };

                    if !p.is_null() && p_str == Some("?") {
                        self.osc_color_response(colors, j, 0, true, term_ptr);
                    } else if j >= 0 && !colors.set_color_name(j as usize, p_str) {
                        if par == 104 && self.narg <= 1 {
                            colors.load_colors();
                            return;
                        }

                        eprintln!("erresc: invalid color j={}, p={:?}", j, p_str);
                    } else {
                        // TODO: if defaulbg color is changed, borders are dirty
                        unsafe {
                            (*term_ptr).state.tfulldirt();
                        }
                    }

                    return;
                }

                x => {
                    eprintln!("erresc: unknown osc par {}", x);
                }
            },

            // old title set compatibility
            b'k' => {
                // TODO
                // xsettitle(strescseq.args[0]);
                return;
            }

            // DCS -- Device Control String
            b'P' => {
                let term_mode = unsafe { &mut (*term_ptr).state.mode };

                if term_mode.contains(TermMode::Sixel) {
                    term_mode.remove(TermMode::Sixel);

                    // TODO: continue sixel decoding
                }
            }

            // APC -- Application Program Command
            b'_' => {
                // TODO: implement APC handling
            }

            // PM -- Privacy Message
            b'^' => {
                return;
            }

            _ => {}
        }

        eprintln!("erresc: unknown str ");
        self.strdump();
    }

    pub fn parse(&mut self) {
        self.narg = 0;
        // buf holds exactly `len` bytes (no reserved slot for a
        // terminator), so grow it by one before writing the sentinel
        // instead of indexing one past the end.
        self.buf.resize(self.len + 1, 0);
        let mut p = self.buf.as_mut_ptr();

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

                let mut c = *p;
                while *p != b';' && c != 0 {
                    c = *p;
                    p = p.add(1);
                }

                if c == 0 {
                    return;
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

        let terminator: &str = unsafe {
            CStr::from_ptr(self.term as *const i8)
                .to_str()
                .expect("valid terminator sequence on osc_color_response")
        };

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::EscapeState;
    use crate::terminal::Term;

    fn seq(type_: u8, payload: &[u8]) -> StrEscape {
        let mut s = StrEscape::new();
        s.type_ = type_;
        s.buf = payload.to_vec();
        s.len = payload.len();
        s
    }

    fn arg_str(s: &StrEscape, i: usize) -> &str {
        unsafe {
            CStr::from_ptr(s.args[i] as *const i8)
                .to_str()
                .expect("parsed arg is not valid UTF-8")
        }
    }

    #[test]
    fn parse_empty_payload_has_no_args() {
        let mut s = seq(b'P', b"");
        s.parse();
        assert_eq!(s.narg, 0);
    }

    #[test]
    fn parse_single_arg() {
        let mut s = seq(b'P', b"abc");
        s.parse();
        assert_eq!(s.narg, 1);
        assert_eq!(arg_str(&s, 0), "abc");
    }

    #[test]
    fn parse_splits_on_semicolons() {
        let mut s = seq(b'P', b"1;22;333");
        s.parse();
        assert_eq!(s.narg, 3);
        assert_eq!(arg_str(&s, 0), "1");
        assert_eq!(arg_str(&s, 1), "22");
        assert_eq!(arg_str(&s, 2), "333");
    }

    #[test]
    fn parse_trailing_semicolon_yields_empty_final_arg() {
        let mut s = seq(b'P', b"1;");
        s.parse();
        assert_eq!(s.narg, 2);
        assert_eq!(arg_str(&s, 0), "1");
        assert_eq!(arg_str(&s, 1), "");
    }

    #[test]
    fn parse_osc_title_preserves_semicolons_in_remainder() {
        // OSC sequences with a first arg starting with '0', '1' or '2' (window/icon
        // title, OSC 7, ...) keep the remainder of the payload intact, including any
        // embedded ';', instead of splitting it into further args.
        let mut s = seq(b']', b"0;my;title;here");
        s.parse();
        assert_eq!(s.narg, 2);
        assert_eq!(arg_str(&s, 0), "0");
        assert_eq!(arg_str(&s, 1), "my;title;here");
    }

    #[test]
    fn parse_non_title_osc_splits_normally() {
        // par >= '3' doesn't hit the title special-case, so ';' still splits args.
        let mut s = seq(b']', b"4;1;red");
        s.parse();
        assert_eq!(s.narg, 3);
        assert_eq!(arg_str(&s, 0), "4");
        assert_eq!(arg_str(&s, 1), "1");
        assert_eq!(arg_str(&s, 2), "red");
    }

    #[test]
    fn parse_caps_args_at_str_arg_siz() {
        let payload = (0..20)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(";")
            .into_bytes();
        let mut s = seq(b'P', &payload);
        s.parse();
        assert_eq!(s.narg, STR_ARG_SIZ);
    }

    #[test]
    fn reset_clears_all_state() {
        let mut s = seq(b'P', b"1;2;3");
        s.parse();
        assert!(s.narg > 0);

        s.reset();
        assert_eq!(s.type_, 0);
        assert_eq!(s.len, 0);
        assert_eq!(s.narg, 0);
        assert!(s.buf.is_empty());
        assert!(s.args.iter().all(|p| p.is_null()));
        assert!(s.term.is_null());
    }

    #[test]
    fn handle_clears_escape_state() {
        let mut s = seq(b'k', b"");
        let mut esc = EscapeState::ESC_STR | EscapeState::ESC_STR_END;
        let mut state = TermState::default();
        let mut colors = ColorRegistry::default();
        let mut term = Term::default();

        s.handle(&mut esc, &mut state, &mut colors, &mut term as *mut Term);

        assert!(!esc.contains(EscapeState::ESC_STR));
        assert!(!esc.contains(EscapeState::ESC_STR_END));
    }

    #[test]
    fn handle_osc4_color_set_query_does_not_crash() {
        // Regression: `par == 4`/`par == 104` (previously `self.type_ == 4`,
        // which could never be true here) gates whether `p` is read from
        // self.args[2]. Getting that wrong left `p` null unconditionally,
        // and the unconditional `CStr::from_ptr(p)` a few lines down
        // segfaulted on every OSC 4 / OSC 104 sequence.
        let mut s = seq(b']', b"4;1;red");
        let mut esc = EscapeState::ESC_STR | EscapeState::ESC_STR_END;
        let mut state = TermState::default();
        let mut colors = ColorRegistry::default();
        let mut term = Term::default();

        s.handle(&mut esc, &mut state, &mut colors, &mut term as *mut Term);

        assert!(!esc.contains(EscapeState::ESC_STR));
    }

    #[test]
    fn handle_with_parsed_args_does_not_double_free() {
        // Regression test: handle() used to take ownership of self.args[0]
        // (a pointer into self.buf's own allocation) via CString::from_raw,
        // so dropping it freed memory self.buf still owned -- a double free
        // whenever narg > 0 (i.e. essentially any real OSC/DCS/PM sequence).
        let mut s = seq(b']', b"0;window title");
        let mut esc = EscapeState::ESC_STR | EscapeState::ESC_STR_END;
        let mut state = TermState::default();
        let mut colors = ColorRegistry::default();
        let mut term = Term::default();

        s.handle(&mut esc, &mut state, &mut colors, &mut term as *mut Term);

        assert!(!esc.contains(EscapeState::ESC_STR));
    }
}
