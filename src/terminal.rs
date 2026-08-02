use std::ffi::{CStr, CString};
use std::ptr::null_mut;

use crate::colors::{Color, ColorRegistry};
use crate::config::{DEFAULTBG, VTIDEN};
use crate::csiesq::{CSIEscape, STR_TERM_BEL, STR_TERM_ST};
use crate::stresq::StrEscape;
use crate::term_state::{CMDFD, CursorMovement, IOFD, PID, SU, TermMode, TermState};
pub use crate::term_state::{IS_TRUECOL, TWRITE_ABORTED};
use crate::win::{TermWindow, WinMode};
use crate::{BETWEEN, config};
use bitflags::bitflags;
use signal_hook::consts::SIGCHLD;
use signal_hook::iterator::Signals;

use crate::term_state::Charset;

const STR_BUF_SIZ: usize = 128 * 4;
const UTF_SIZ: usize = 4;

fn is_control_c0(c: char) -> bool {
    BETWEEN!(c, '\0', '\u{1F}') || c == '\u{7F}'
}

fn is_control_c1(c: char) -> bool {
    BETWEEN!(c, '\u{80}', '\u{9F}')
}

fn is_control(c: char) -> bool {
    is_control_c0(c) || is_control_c1(c)
}

/// Decodes a single Unicode scalar value from the start of `buffer`.
///
/// On success, returns the decoded `char` along with the number of bytes it
/// occupied in `buffer`.
///
/// If `buffer` starts with an invalid or malformed UTF-8 sequence, returns
/// [`char::REPLACEMENT_CHARACTER`] along with the number of bytes that
/// sequence should be skipped.
///
/// Returns `None` if `buffer` is empty or it starts with a truncated and
/// potentially valid sequence once more bytes arrive.
fn utf8decode(buffer: &[u8]) -> Option<(char, usize)> {
    let probe = &buffer[..buffer.len().min(4)];

    match std::str::from_utf8(probe) {
        Ok(s) => {
            let c = s.chars().next()?;
            Some((c, c.len_utf8()))
        }
        Err(e) if e.valid_up_to() > 0 => {
            let valid_bytes = &probe[..e.valid_up_to()];
            let utf = unsafe { std::str::from_utf8_unchecked(valid_bytes) };

            let c = utf.chars().next()?;

            Some((c, c.len_utf8()))
        }
        Err(e) => e.error_len().map(|n| (char::REPLACEMENT_CHARACTER, n)),
    }
}

bitflags! {
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
    pub struct EscapeState: u32 {
        const ESC_START      = 1;
        const ESC_CSI        = 2;
        const ESC_STR        = 4;   /* DCS, OSC, PM, APC */
        const ESC_ALTCHARSET = 8;
        const ESC_STR_END    = 16;  /* a final string was encountered */
        const ESC_TEST       = 32;  /* Enter in test mode */
        const ESC_UTF8       = 64;
        const ESC_DCS        = 128; /* Device Control String */
    }
}

/* Internal representation of the screen */

#[derive(Default)]
pub struct Term {
    pub colors: ColorRegistry,
    pub state: TermState,
    pub strescseq: StrEscape,
    pub csiescseq: CSIEscape,
    esc: EscapeState,
    pub win: TermWindow,
    // HACK: NEED TO REMOVE THIS ASAP
    pub draw: Option<Box<dyn FnMut()>>,
}

impl Term {
    pub fn new(col: usize, row: usize) -> Self {
        let mut term = Self::default();

        term.state.tresize(col, row);
        term.state.treset();

        term
    }

    pub fn ttynew(
        &mut self,
        line: Option<&str>,
        cmd: Option<&CStr>,
        out: Option<&str>,
        args: Option<&[&CStr]>,
    ) -> i32 {
        crate::term_state::init_tty_logs();

        if let Some(out) = out {
            self.state.mode.insert(TermMode::Print);
            unsafe {
                IOFD = if out == "-" {
                    1
                } else {
                    libc::open(
                        out.as_ptr() as *const libc::c_char,
                        libc::O_WRONLY | libc::O_CREAT,
                        0o666,
                    )
                };

                if IOFD < 0 {
                    panic!("Error opening {}:{}", out, std::io::Error::last_os_error());
                }
            }
        }

        if let Some(line) = line {
            unsafe {
                CMDFD = libc::open(line.as_ptr() as *const libc::c_char, libc::O_RDWR);

                if CMDFD < 0 {
                    panic!(
                        "open line '{}' failed: {}",
                        line,
                        std::io::Error::last_os_error()
                    );
                }

                libc::dup2(CMDFD, 0);
                // TODO: stty(args);

                return CMDFD;
            }
        }

        let mut m = 0;
        let mut s = 0;

        unsafe {
            if libc::openpty(&mut m, &mut s, null_mut(), null_mut(), null_mut()) < 0 {
                panic!("openpty failed: {}", std::io::Error::last_os_error());
            }
        }

        unsafe {
            PID = libc::fork();

            match PID {
                -1 => {
                    panic!("fork failed: {}", std::io::Error::last_os_error());
                }

                0 => {
                    libc::close(IOFD);
                    libc::close(m);
                    libc::setsid();
                    libc::dup2(s, 0);
                    libc::dup2(s, 1);
                    // libc::dup2(s, 2);

                    if libc::ioctl(s, libc::TIOCSCTTY, 0) < 0 {
                        panic!(
                            "ioctl TIOCSCTTY failed: {}",
                            std::io::Error::last_os_error()
                        );
                    }

                    if s > 2 {
                        libc::close(s);
                    }

                    execsh(cmd, args);
                }

                _ => {
                    libc::close(s);
                    CMDFD = m;
                    install_sigchld_handler();
                }
            }

            return CMDFD;
        }
    }

    pub fn ttywrite(&mut self, buffer: &[u8], len: usize, may_echo: bool) {
        if may_echo && self.state.mode.contains(TermMode::Echo) {
            self.twrite(&buffer, len, true);
        }

        if !self.state.mode.contains(TermMode::Crlf) {
            self.ttywriteraw_pty(buffer, len);
            return;
        }

        self.state.ttywrite_pty(buffer, len);
    }

    fn twrite(&mut self, buffer: &[u8], buflen: usize, show_ctrl: bool) -> usize {
        let mut charsize = 0;
        let mut i = 0;
        let mut u: char;
        let su0 = unsafe { SU };

        unsafe { TWRITE_ABORTED = false };

        while i < buflen {
            /* TODO: sixel_st.state != PS_ESC */
            if self.state.mode.contains(TermMode::Sixel) {
                // charsize = sixel_parser_parse(&sixel_st, (const unsigned char *)buf + n, buflen - n);
                continue;
            } else if self.state.mode.contains(TermMode::Utf8) {
                // FIXME: assumes all chars are properly encoded

                let u_opt = utf8decode(&buffer[i..buflen]);

                if let Some((u_decoded, decoded_len)) = u_opt {
                    u = u_decoded;
                    charsize = decoded_len;
                } else {
                    break;
                }
            } else {
                eprintln!("Non-UTF8 mode is not supported in this implementation");
                u = (buffer[i] & 0xFF) as char;
                charsize = 1;
            }

            if su0 != 0 && unsafe { SU == 0 } {
                unsafe { TWRITE_ABORTED = true };
                break; // ESU - allow rendering before a new BSU
            }

            if show_ctrl && is_control(u as char) {
                if u as u8 & 0x80 != 0 {
                    u = (u as u8 & 0x7F) as char;
                    self.tputc('^');
                    self.tputc('[');
                } else if u != '\n' && u != '\r' && u != '\t' {
                    u = (u as u8 ^ 0x40) as char;
                    self.tputc('^');
                }
            }

            self.tputc(u);

            i += charsize;
        }

        return i;
    }

    pub fn ttywriteraw_pty(&mut self, buffer: &[u8], len: usize) {
        let mut rfd: libc::fd_set = unsafe { std::mem::zeroed() };
        let mut wfd: libc::fd_set = unsafe { std::mem::zeroed() };

        let mut n = len;
        let mut s: *const libc::c_void = buffer.as_ptr() as *const libc::c_void;
        let mut lim: usize = 256;
        let mut retries_left = 100;

        /*
         * Remember that we are using a pty, which might be a modem line.
         * Writing too much will clog the line. That's why we are doing this
         * dance.
         * FIXME: Migrate the world to Plan 9.
         */

        while n > 0 {
            retries_left -= 1;
            if retries_left <= 0 {
                eprintln!("Could not write {} bytes to tty", n);
                break;
            }

            unsafe {
                libc::FD_ZERO(&mut wfd);
                libc::FD_ZERO(&mut rfd);
                libc::FD_SET(CMDFD, &mut wfd);
                libc::FD_SET(CMDFD, &mut rfd);

                if libc::pselect(
                    CMDFD + 1,
                    &mut rfd,
                    &mut wfd,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                ) < 0
                {
                    if *libc::__errno_location() == libc::EINTR {
                        continue;
                    }
                    panic!("select failed: {}", std::io::Error::last_os_error());
                }

                if libc::FD_ISSET(CMDFD, &mut wfd) {
                    /*
                     * Only write the bytes written by ttywrite() or the
                     * default of 256. This seems to be a reasonable value
                     * for a serial line. Bigger values might clog the I/O.
                     */
                    let count = if n < lim { n } else { lim };
                    let r = libc::write(CMDFD, s, count);

                    if r < 0 {
                        panic!("write failed on tty: {}", std::io::Error::last_os_error());
                    }

                    if r > 0 {
                        crate::term_state::log_tty_write(std::slice::from_raw_parts(
                            s as *const u8,
                            r as usize,
                        ));
                    }

                    if r < n as isize {
                        /*
                         * We weren't able to write out everything.
                         * This means the buffer is getting full
                         * again. Empty it.
                         */
                        if n < lim {
                            lim = self.ttyread();
                        }

                        n -= r as usize;
                        s = s.add(r as usize);
                    } else {
                        // All bytes have been written
                        break;
                    }
                } else {
                    eprintln!("select returned but cmdfd is not writable");
                }

                if libc::FD_ISSET(CMDFD, &mut rfd) {
                    lim = self.ttyread();
                }
            }
        }
    }

    // TODO: refactor this
    pub fn tputc(&mut self, u: char) {
        let mut utfbuf = [0u8; 4];
        let control = is_control(u);

        let len = if (u as u32) < 127 || !self.state.mode.contains(TermMode::Utf8) {
            utfbuf[0] = u as u8;
            1
        } else {
            u.encode_utf8(&mut utfbuf);
            u.len_utf8()
        };

        if self.state.mode.contains(TermMode::Print) {
            self.tprinter(&utfbuf, len);
        }

        /*
         * STR sequence must be checked before anything else
         * because it uses all following characters until it
         * receives a ESC, a SUB, a ST or any other C1 control
         * character.
         */
        if self.esc.contains(EscapeState::ESC_STR) {
            'pre_check_control_code: {
                let is_control = match u as u8 {
                    0o7 | 0o30 | 0o32 | 0o33 => true,
                    _ => is_control_c1(u),
                };

                if is_control {
                    self.esc.remove(
                        EscapeState::ESC_START | EscapeState::ESC_STR | EscapeState::ESC_DCS,
                    );
                    self.esc.insert(EscapeState::ESC_STR_END);

                    break 'pre_check_control_code;
                }

                if self.esc.contains(EscapeState::ESC_DCS) {
                    break 'pre_check_control_code;
                }

                if self.strescseq.len + len >= self.strescseq.size {
                    /*
                     * Here is a bug in terminals. If the user never sends
                     * some code to stop the str or esc command, then st
                     * will stop responding. But this is better than
                     * silently failing with unknown characters. At least
                     * then users will report back.
                     *
                     * In the case users ever get fixed, here is the code:
                     */
                    /*
                     * term.esc = 0;
                     * strhandle();
                     */
                    if self.strescseq.size > (usize::MAX - UTF_SIZ) / 2 {
                        return;
                    }

                    self.strescseq.size *= 2;
                    eprintln!(
                        "esc: ESC_STR: strescseq.buf.reserve({})",
                        self.strescseq.size
                    );
                    self.strescseq.buf.reserve(self.strescseq.size);
                }

                self.strescseq.buf.extend_from_slice(&utfbuf[..len]);
                self.strescseq.len += len;
                return;
            }
        }

        // check_control_code:
        if control {
            /* in UTF-8 mode ignore handling C1 control characters */
            if self.state.mode.contains(TermMode::Utf8) && is_control_c1(u) {
                return;
            }

            self.tcontrolcode(u as u8);

            if self.esc.is_empty() {
                self.state.lastc = '\0';
            }

            return;
        }

        if self.esc.contains(EscapeState::ESC_START) {
            if self.esc.contains(EscapeState::ESC_CSI) {
                let index = self.csiescseq.len;
                self.csiescseq.buf[index] = u as u8;
                self.csiescseq.len += 1;

                let len = self.csiescseq.len;

                if BETWEEN!(u as u8, 0x40, 0x7E) || len >= self.csiescseq.buf.len() - 1 {
                    self.esc = EscapeState::empty();
                    self.csiparse();
                    self.csihandle();
                }

                return;
            } else if self.esc.contains(EscapeState::ESC_DCS) {
                let idx = self.csiescseq.len;
                self.csiescseq.buf[idx] = u as u8;
                self.csiescseq.len += 1;
                let len = self.csiescseq.len;
                if (u >= '\u{0040}' && u <= '\u{007E}') || len >= self.csiescseq.buf.len() - 1 {
                    self.csiparse();
                    self.dcshandle();
                }
                return;
            } else if self.esc.contains(EscapeState::ESC_UTF8) {
                self.tdefutf8(u);
            } else if self.esc.contains(EscapeState::ESC_ALTCHARSET) {
                self.tdeftran(u);
            } else if self.esc.contains(EscapeState::ESC_TEST) {
                self.tdectest(u);
            } else {
                if !self.eschandle(u) {
                    return;
                }
                /* sequence already finished */
            }
            self.esc = EscapeState::empty();
            /*
             * All characters which form part of a sequence are not printed
             */
            return;
        }

        self.state.tputc_char(u);
    }

    fn tprinter(&self, s: &[u8], len: usize) {
        unsafe {
            if IOFD >= 0 && xwrite(IOFD, s, len) < 0 {
                eprintln!("Error writing to output file");
                libc::close(IOFD);
                IOFD = -1;
            }
        }
    }

    fn tcontrolcode(&mut self, u: u8) {
        let mut interrupt_sequence = false;

        match u {
            // HT
            b'\t' => self.state.tputtab(1),

            // BS (\b)
            0x08 => {
                // BS
                let x = self.state.c.x;
                let y = self.state.c.y;
                self.state.tmoveto(x.saturating_sub(1), y);
            }

            // CR
            b'\r' => {
                self.state.tmoveto(0, self.state.c.y);
            }

            0x0C  | // LF (\f)
            0x0B  | // VT (\v)
            b'\n'   // LF (\n)
            => {
                self.state.tnewline(self.state.mode.contains(TermMode::Crlf));
            }

            // BEL (\a)
            0x07 => {
                if self.esc.contains(EscapeState::ESC_STR_END) {
                    self.strescseq.term = STR_TERM_BEL.as_ptr();
                    self.strhandle();
                } else {
                    // TODO: implement ring bell
                    // xbell();
                }

                interrupt_sequence = true;
            }

            // ESC
            0x1B => {
                self.csireset();
                self.esc.remove(EscapeState::ESC_CSI | EscapeState::ESC_ALTCHARSET | EscapeState::ESC_TEST);
                self.esc.insert(EscapeState::ESC_START);
            }

            // SO (LS1 -- Locking shift 1)
            0x0e => {
                self.state.charset = 1;
            }
            // SI (LS0 -- Locking shift 0)
            0x0f => {
                self.state.charset = 0;
            }

            // SUB
            0x1A => {
                let g = self.state.c.attr.clone();
                self.state.tsetchar('?', &g, self.state.c.x, self.state.c.y);
                self.csireset();
                interrupt_sequence = true;
            }

            // CAN
            0x18 => {
                self.csireset();
                interrupt_sequence = true;
            }

            0x05 | // ENQ
            0x00 | // NUL
            0x11 | // XON
            0x13 | // XOFF
            0x7F   // DEL
            => {
                // ignored
            }

            0x80 | // TODO: PAD
            0x81 | // TODO: HOP
            0x82 | // TODO: BPH
            0x83 | // TODO: NBH
            0x84   // TODO: IND
            => {
                interrupt_sequence = true;
            }

            // NEL -- Next line
            0x85 => {
                self.state.tnewline(true);
                interrupt_sequence = true;
            },

            0x86 | // TODO:  SSA
            0x87   // TODO:  ESA
            => {
                interrupt_sequence = true;
            }

            // HTS -- Horizontal tab stop
            0x88 => {
                self.state.tabs[self.state.c.x] = 1;
                interrupt_sequence = true;
            }

            0x89 | // TODO: HTJ
            0x8a | // TODO: VTS
            0x8b | // TODO: PLD
            0x8c | // TODO: PLU
            0x8d | // TODO: RI
            0x8e | // TODO: SS2
            0x8f | // TODO: SS3
            0x91 | // TODO: PU1
            0x92 | // TODO: PU2
            0x93 | // TODO: STS
            0x94 | // TODO: CCH
            0x95 | // TODO: MW
            0x96 | // TODO: SPA
            0x97 | // TODO: EPA
            0x98 | // TODO: SOS
            0x99   // TODO: SGCI
            => {
                interrupt_sequence = true;
            }

            // DECID -- Identify Terminal
            0x9a => {
                self.ttywrite(VTIDEN, VTIDEN.len(), false);
                interrupt_sequence = true;
            }

            0x9b | // TODO: CSI
            0x9c   // TODO: ST
            => {
                interrupt_sequence = true;
            }


            0x90 | // DCS -- Device Control String
            0x9d | // OSC -- Operating System Command
            0x9e | // PM -- Privacy Message
            0x9f   // APC -- Application Program Command
            => {
                self.tstrsequence(u);
            }

            _ => {
                // ignore other control codes
                interrupt_sequence = true;
            }
        }

        // only CAN, SUB, \a and C1 chars interrupt a sequence
        if interrupt_sequence {
            self.esc
                .remove(EscapeState::ESC_STR_END | EscapeState::ESC_STR);
        }
    }

    fn tdefutf8(&mut self, u: char) {
        match u {
            'G' => self.state.mode.insert(TermMode::Utf8),
            '@' => self.state.mode.remove(TermMode::Utf8),
            _ => {}
        }
    }

    fn tdeftran(&mut self, u: char) {
        // TODO: check this:
        // const CS: &[char] = &['0', 'B', 'U', 'K'];
        // const VCSMAP: &[Charset] = &[
        //     Charset::CS_USA,
        //     Charset::CS_GRAPHIC0,
        //     Charset::CS_GRAPHIC1,
        //     Charset::CS_UK,
        // ];

        const CS: &[char] = &['0', 'B'];
        const VCSMAP: &[Charset] = &[Charset::Graphic0, Charset::Usa];

        if let Some(idx) = CS.iter().position(|&c| c == u) {
            self.state.trantbl[self.state.icharset as usize] = VCSMAP[idx];
        } else {
            eprintln!("esc unhandled charset: '{}'", u);
        }
    }

    fn tdectest(&mut self, c: char) {
        // DEC screen alignment test
        if c == '8' {
            for y in 0..self.state.row {
                for x in 0..self.state.col {
                    self.state.tsetchar('E', &self.state.c.attr.clone(), x, y);
                }
            }
        }
    }

    /// Returns true w hen the sequence is finished and it hasn't to read
    /// more characters for this sequence, otherwise false
    fn eschandle(&mut self, u: char) -> bool {
        match u {
            '[' => {
                self.esc.insert(EscapeState::ESC_CSI);
                return false;
            }
            '#' => {
                self.esc.insert( EscapeState::ESC_TEST);
                return false;
            }
            '%' => {
                self.esc.insert(EscapeState::ESC_UTF8);
                return false;
            }
            'P' | // DCS -- Device Control String
            '_' | // APC -- Application Program Command
            '^' | // PM -- Privacy Message
            ']' | // OSC -- Operating System Command
            'k'   // TODO: check if we can remove this: old title set compatibility
            => {
                if u == 'P' {
                    self.esc.insert(EscapeState::ESC_DCS);
                }

                self.tstrsequence(u as u8);
                return false;
            }
            'n' | // LS2 -- Locking shift 2
            'o'   // LS3 -- Locking shift 3
            => {
                self.state.charset = 2 + (u as u8 - b'n') as usize;
            }
            '('| // GZD4 -- set primary charset G0
            ')'| // G1D4 -- set secondary charset G1
            '*'| // G2D4 -- set tertiary charset G2
            '+'  // G3D4 -- set quaternary charset G3
            => {
                self.state.icharset = (u as u8 - b'(') as u32;
                self.esc.insert(EscapeState::ESC_ALTCHARSET);
                return false;
            }
            // IND -- Linefeed
            'D' => {
                if self.state.c.y == self.state.bot {
                    self.state.tscrollup(self.state.top, 1);
                } else {
                    self.state.tmoveto(self.state.c.x, self.state.c.y + 1);
                }
            }
            // NEL -- Next line
            'E' => {
                self.state.tnewline(true); // always go to first col
            }
            // HTS -- Horizontal tab stop
            'H' => {
                self.state.tabs[self.state.c.x] = 1;
            }
            // RI -- Reverse index
            'M' => {
                if self.state.c.y == self.state.top {
                    self.state.tscrolldown(self.state.top, 1);
                } else {
                    self.state.tmoveto(self.state.c.x, self.state.c.y - 1);
                }
            }
            // DECID -- Identify Terminal
            'Z' => {
                self.ttywrite(VTIDEN, VTIDEN.len(), false);
            }
            // RIS -- Reset to initial state
            'c' => {
                self.state.treset();
                self.resettitle();
                self.xloadcols();
                self.xsetmode(false, WinMode::Hide);
            }
            // DECKPAM – application keypad
            '=' => {
                self.xsetmode(true, WinMode::AppKeypad);
            }
            // DECPNM -- Normal keypad
            '>' => {
                self.xsetmode(false, WinMode::AppKeypad);
            }
            // DECSC -- Save Cursor
            '7' => {
                self.state.tcursor(CursorMovement::CursorSave);
            }
            // DESRC -- Restore Cursor
            '8' => {
                self.state.tcursor(CursorMovement::CursorLoad);
            }
            // ST -- String terminator
            '\\' => {
                if self.esc.contains(EscapeState::ESC_STR_END) {
                    self.strescseq.term = STR_TERM_ST.as_ptr();
                    self.strhandle();
                }
            }
            _ => {
                eprintln!("erresc: unknown sequence ESC {:02X} '{}'", u as u32, u);
            }
        }

        true
    }

    fn csiparse(&mut self) {
        self.csiescseq.parse();
    }

    fn csihandle(&mut self) {
        self.csiescseq.handle(&mut self.state, &mut self.win);
    }

    fn csireset(&mut self) {
        self.csiescseq.reset();
    }

    // TODO: implement this
    fn dcshandle(&mut self) {
        match self.csiescseq.mode[0] {
            // DECDIXEL
            b'q' => {
                let transparent = self.csiescseq.narg >= 2 && self.csiescseq.arg[1] == 1;
                let mut r: u8 = 0;
                let mut g: u8 = 0;
                let mut b: u8 = 0;
                let mut a: u8 = 255;

                let bg = self.state.c.attr.bg;
                if IS_TRUECOL(bg) {
                    r = (bg >> 16 & 0xFF) as u8;
                    g = (bg >> 8 & 0xFF) as u8;
                    b = (bg & 0xFF) as u8;
                } else if let Some(color) = self.colors.get_color(bg as usize) {
                    if bg == DEFAULTBG {
                        a = (color.alpha & 0xFF) as u8;
                    }
                }

                let bgcolor = Color::rgba(r, g, b, a);

                // if (sixel_parser_init(&sixel_st, transparent, (255 << 24), bgcolor, 1, win.cw, win.ch) != 0) {
                // 	perror("sixel_parser_init() failed");
                // }

                self.state.mode.insert(TermMode::Sixel);
            }
            _ => {
                eprintln!("erresc: unknown csi ");
                self.csiescseq.dump()
            }
        }
    }

    fn strhandle(&mut self) {
        let term = self as *mut Self;

        self.strescseq
            .handle(&mut self.esc, &mut self.state, &mut self.colors, term);
    }

    pub fn ttyread(&mut self) -> usize {
        const BUF_SIZE: usize = 8192;
        static mut BUF: [u8; BUF_SIZE] = unsafe { std::mem::zeroed() };
        static mut BUF_WRITTEN: usize = 0;
        static mut ALREADY_PROCESSING: bool = false;

        let mut written = 0;

        if unsafe { BUF_WRITTEN >= BUF_SIZE } {
            return 0;
        }

        unsafe {
            // append read bytes to unprocessed bytes
            let ret = if TWRITE_ABORTED {
                1
            } else {
                let b = &raw mut BUF as *mut libc::c_void;
                let n = libc::read(CMDFD, b.add(BUF_WRITTEN), BUF_SIZE - BUF_WRITTEN);

                if n > 0 {
                    crate::term_state::log_tty_read(&BUF[BUF_WRITTEN..BUF_WRITTEN + n as usize]);
                }

                n
            };

            match ret {
                0 => {
                    // EOF: the child hung up. Same reasoning as
                    // CHILD_EXIT_CODE's doc comment - don't call
                    // exit()/_exit() here on the main thread mid-frame; let
                    // the event loop unwind normally so App's GL/EGL Drop
                    // runs before the process actually exits.
                    CHILD_EXIT_CODE.store(0, std::sync::atomic::Ordering::SeqCst);
                    return 0;
                }

                -1 => {
                    panic!("read failed on tty: {}", std::io::Error::last_os_error());
                }

                _ => {
                    BUF_WRITTEN += if TWRITE_ABORTED { 0 } else { ret as usize };

                    if ALREADY_PROCESSING {
                        return ret as usize;
                    }

                    ALREADY_PROCESSING = true;

                    loop {
                        let buflen_before_processing = BUF_WRITTEN;
                        written += self.twrite(&BUF[written..], BUF_WRITTEN - written, false);

                        // If buflen changed during the call to twrite, there is
                        // new data, and we need to keep processing, otherwise
                        // we can exit. This will not loop forever because the
                        // buffer is limited, and we don't clean it in this
                        // loop, so at some point ttywrite will have to drop
                        // some data.
                        if buflen_before_processing == BUF_WRITTEN {
                            break;
                        }
                    }

                    ALREADY_PROCESSING = false;
                    BUF_WRITTEN -= written;

                    // keep any incomplete UTF-8 byte sequence for the next call
                    if BUF_WRITTEN > 0 {
                        let b = &raw mut BUF as *mut libc::c_void;
                        libc::memmove(b, b.add(written), BUF_WRITTEN);
                    }

                    return ret as usize;
                }
            }
        }
    }

    fn tstrsequence(&mut self, c: u8) {
        let mut c = c;
        self.strreset();

        match c as u8 {
            0x90 => {
                c = b'P';
                self.esc.insert(EscapeState::ESC_DCS);
            }
            0x9f => {
                c = b'_';
            }
            0x9e => {
                c = b'^';
            }
            0x9d => {
                c = b']';
            }
            _ => {}
        }

        self.strescseq.type_ = c;
        self.esc.insert(EscapeState::ESC_STR);
    }

    fn strreset(&mut self) {
        self.strescseq.reset();
    }

    fn resettitle(&self) {
        // TODO: xsettitle(NULL);
    }

    // TODO:
    pub fn xsetmode(&mut self, set: bool, flags: WinMode) {
        let mode = self.win.mode;

        self.win.mode.set(flags, set);

        if (self.win.mode & WinMode::Reverse) != (mode & WinMode::Reverse) {
            self.redraw();
        }
    }

    // TODO:
    fn xloadcols(&self) {}
    fn redraw(&mut self) {
        self.state.tfulldirt();
        if let Some(draw) = self.draw.as_mut() {
            draw();
        }
    }
}

/// Set by the SIGCHLD-watcher thread once the tracked child has exited, to
/// the process exit code the main thread should shut down with. `-1` means
/// "child still running".
///
/// The watcher thread must NOT call `std::process::exit`/`_exit` itself:
/// that races the main thread's normal GL/EGL teardown (dropping `App` at
/// the end of `main`) and the two concurrent driver teardowns can segfault
/// inside the NVIDIA driver (observed via coredump: one thread inside
/// `process::exit`'s atexit handlers, the other inside `drop_in_place::<App>`,
/// both deep in `libnvidia-eglcore.so` at the same time). Only the main
/// thread may tear down the GL context, so it alone is allowed to exit the
/// process; this flag is how the watcher thread asks it to.
pub static CHILD_EXIT_CODE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Reaps the child shell in the background and mirrors st's `sigchld()`:
/// terminate with the child's exit status (or the signal that killed it)
/// once the tracked `pid` shows up in a `waitpid`, so a dead shell doesn't
/// leave the terminal running against a closed pty.
fn install_sigchld_handler() {
    let mut signals = Signals::new([SIGCHLD]).expect("failed to register SIGCHLD handler");

    std::thread::spawn(move || {
        for _ in signals.forever() {
            unsafe {
                loop {
                    let mut stat: i32 = 0;
                    let p = libc::waitpid(-1, &mut stat, libc::WNOHANG);

                    if p <= 0 {
                        break;
                    }

                    if p == PID {
                        if libc::WIFEXITED(stat) && libc::WEXITSTATUS(stat) != 0 {
                            eprintln!("child exited with status {}", libc::WEXITSTATUS(stat));
                            CHILD_EXIT_CODE.store(1, std::sync::atomic::Ordering::SeqCst);
                        } else if libc::WIFSIGNALED(stat) {
                            eprintln!("child terminated due to signal {}", libc::WTERMSIG(stat));
                            CHILD_EXIT_CODE.store(1, std::sync::atomic::Ordering::SeqCst);
                        } else {
                            CHILD_EXIT_CODE.store(0, std::sync::atomic::Ordering::SeqCst);
                        }

                        return;
                    }
                }
            }
        }
    });
}

fn execsh(cmd: Option<&CStr>, args: Option<&[&CStr]>) {
    unsafe {
        let pw = libc::getpwuid(libc::getuid());

        if pw.is_null() {
            panic!("getpwuid: {}", std::io::Error::last_os_error());
        }

        let default_shell = c"/bin/sh";
        let shell_env = libc::getenv(c"SHELL".as_ptr());

        let sh = if let Some(cmd) = cmd {
            cmd.as_ptr() as *mut libc::c_char
        } else if !shell_env.is_null() {
            shell_env as *mut libc::c_char
        } else {
            if *((*pw).pw_shell) != 0 {
                (*pw).pw_shell
            } else {
                default_shell.as_ptr() as *mut libc::c_char
            }
        };

        let args: Vec<*const libc::c_char> = if let Some(args) = args {
            let mut cargs: Vec<*const libc::c_char> = Vec::with_capacity(args.len() + 2);
            cargs.push(sh);

            for arg in args {
                cargs.push(arg.as_ptr() as *const libc::c_char);
            }
            cargs.push(std::ptr::null());
            cargs
        } else {
            vec![sh, std::ptr::null(), std::ptr::null()]
        };

        eprintln!("Executing shell: {:?}", args);

        libc::signal(libc::SIGCHLD, libc::SIG_DFL);
        libc::signal(libc::SIGHUP, libc::SIG_DFL);
        libc::signal(libc::SIGINT, libc::SIG_DFL);
        libc::signal(libc::SIGQUIT, libc::SIG_DFL);
        libc::signal(libc::SIGTERM, libc::SIG_DFL);
        libc::signal(libc::SIGALRM, libc::SIG_DFL);

        macro_rules! unsetenv {
            ($name:expr) => {
                libc::unsetenv($name.as_ptr() as *const libc::c_char);
            };
        }

        macro_rules! setenv {
            ($name:expr, $value:expr) => {
                libc::setenv($name.as_ptr() as *const libc::c_char, $value, 1)
            };
        }

        unsetenv!(c"COLUMNS");
        unsetenv!(c"LINES");
        unsetenv!(c"TERMCAP");
        setenv!(c"LOGNAME", (*pw).pw_name);
        setenv!(c"USER", (*pw).pw_name);
        setenv!(c"SHELL", sh);
        setenv!(c"HOME", (*pw).pw_dir);
        setenv!(c"TERM", config::TERM.as_ptr());
        setenv!(c"COLORTERM", c"truecolor".as_ptr());

        libc::execvp(sh, args.as_ptr());
        libc::_exit(1);
    }
}

fn xwrite(fd: i32, buffer: &[u8], len: usize) -> isize {
    let total_len: usize = len;
    let mut len = len;
    let mut r = 0;

    while len > 0 {
        let s = buffer[total_len - len..].as_ptr() as *const libc::c_void;
        unsafe {
            r = libc::write(fd, s, len);
        }

        if r < 0 {
            return r;
        }

        len -= r as usize;
    }

    return total_len as isize;
}
