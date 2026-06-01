use std::ops::{Deref, DerefMut};
use std::ptr::{null, null_mut};

use crate::BETWEEN;
use crate::config::VTIDEN;
use crate::csiesq::{CSIEscape, STR_TERM_ST};
use crate::glyph::Glyph;
use crate::term_state::{
    CMDFD, CursorMovement, DECOR_DEFAULT_COLOR, IOFD, PID, SU, TermMode, TermState,
};
pub use crate::term_state::{IS_TRUECOL, twrite_aborted};
use crate::win::{TermWindow, WinMode};
use bitflags::bitflags;
use unicode_width::UnicodeWidthChar;

use crate::term_state::Charset;

const STR_BUF_SIZ: usize = 128 * 4;
const UTF_SIZ: usize = 4;

fn TRUECOLOR(r: u8, g: u8, b: u8) -> u32 {
    1 << 24 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Holds the current STR/DCS/OSC/APC/PM escape sequence being accumulated.
#[derive(Debug)]
pub struct StrEscape {
    /// The type byte of the escape sequence (e.g. b'P' for DCS)
    pub type_: u8,
    /// Raw accumulated bytes of the sequence
    pub buf: Vec<u8>,
    pub len: usize,
    pub size: usize,
    pub term: *const u8,
}

impl Default for StrEscape {
    fn default() -> Self {
        Self {
            type_: 0,
            buf: Vec::with_capacity(STR_BUF_SIZ),
            len: 0,
            size: 0,
            term: null(),
        }
    }
}

fn ISCONTROLC0(c: char) -> bool {
    BETWEEN!(c, '\0', '\u{1F}') || c == '\u{7F}'
}

fn ISCONTROLC1(c: char) -> bool {
    BETWEEN!(c, '\u{80}', '\u{9F}')
}

fn ISCONTROL(c: char) -> bool {
    ISCONTROLC0(c) || ISCONTROLC1(c)
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
        cmd: Option<&str>,
        out: Option<&str>,
        args: Option<&[&str]>,
    ) -> i32 {
        let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };

        if let Some(out) = out {
            self.state.mode.insert(TermMode::MODE_PRINT);
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
                    libc::dup2(s, 2);

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
                    libc::sigemptyset(&mut sa.sa_mask);
                    libc::sigaction(libc::SIGCHLD, &sa, null_mut());
                }
            }

            return CMDFD;
        }
    }

    pub fn ttyresize(&mut self, tw: usize, th: usize) {
        self.state.pixw = tw;
        self.state.pixh = th;

        let w = libc::winsize {
            ws_row: self.state.row as u16,
            ws_col: self.state.col as u16,
            ws_xpixel: tw as u16,
            ws_ypixel: th as u16,
        };

        unsafe {
            if libc::ioctl(CMDFD, libc::TIOCSWINSZ, &w) < 0 {
                panic!(
                    "Couldn't set window size: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
    }

    pub fn ttywrite(&mut self, buffer: &[u8], len: usize, may_echo: bool) {
        if may_echo && self.state.mode.contains(TermMode::MODE_ECHO) {
            self.twrite(&buffer, len, true);
        }

        if !self.state.mode.contains(TermMode::MODE_CRLF) {
            self.ttywriteraw_pty(buffer, len);
            return;
        }

        self.state.ttywrite_pty(buffer, len);
    }

    fn twrite(&mut self, buffer: &[u8], buflen: usize, show_ctrl: bool) -> usize {
        let mut charsize = 0;
        let mut i = 0;
        let mut u: char = '\0';
        let su0 = unsafe { SU };

        unsafe { twrite_aborted = false };

        while i < buflen {
            if self.state.mode.contains(TermMode::MODE_SIXEL)
            /* TODO: sixel_st.state != PS_ESC */
            {
                // charsize = sixel_parser_parse(&sixel_st, (const unsigned char *)buf + n, buflen - n);
                // continue;
            } else if self.state.mode.contains(TermMode::MODE_UTF8) {
                // FIXME: assumes all chars are properly encoded
                for utf_len in 0..4 {
                    let end = (i + utf_len + 1).min(buflen);
                    match str::from_utf8(&buffer[i..end]) {
                        Ok(s) => {
                            u = s.chars().next().unwrap_or('\0');
                            charsize = utf_len + 1;
                            break;
                        }
                        _ => continue,
                    }
                }
            } else {
                println!("Non-UTF8 mode is not supported in this implementation");
                u = (buffer[i] & 0xFF) as char;
                charsize = 1;
            }

            if su0 != 0 && unsafe { SU == 0 } {
                unsafe { twrite_aborted = true };
                break; // ESU - allow rendering before a new BSU
            }

            if show_ctrl && ISCONTROL(u as char) {
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

    fn ttywriteraw_pty(&mut self, buffer: &[u8], len: usize) {
        let mut wfd: libc::fd_set = unsafe { std::mem::zeroed() };
        let mut rfd: libc::fd_set = unsafe { std::mem::zeroed() };

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
                println!("Could not write {} bytes to tty", n);
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

                    println!("write returned {}, {}", r, n);

                    if r < 0 {
                        panic!("write failed on tty: {}", std::io::Error::last_os_error());
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
                    println!("select returned but cmdfd is not writable");
                }

                if libc::FD_ISSET(CMDFD, &mut rfd) {
                    lim = self.ttyread();
                }
            }
        }
    }

    // TODO: refactor this
    pub fn tputc(&mut self, u: char) {
        let control = ISCONTROL(u);
        let mut width = 0;
        let mut len = 0;

        if (u as u32) < 127 && !self.state.mode.contains(TermMode::MODE_UTF8) {
            width = 1;
            len = 1;
        } else {
            len = u.len_utf8();

            width = u.width().unwrap_or(0);

            if !control && width == 0 {
                width = 1;
            }
        }

        if self.state.mode.contains(TermMode::MODE_PRINT) {
            let mut buf = [0u8; 4];
            u.encode_utf8(&mut buf);

            self.tprinter(&buf, len);
        }

        /*
         * STR sequence must be checked before anything else
         * because it uses all following characters until it
         * receives a ESC, a SUB, a ST or any other C1 control
         * character.
         */
        if self.esc.contains(EscapeState::ESC_STR) {
            let is_control = match u as u8 {
                0o7 | 0o30 | 0o32 | 0o33 => true,
                _ => ISCONTROLC1(u),
            };

            if is_control {
                self.esc &= !(EscapeState::ESC_START | EscapeState::ESC_STR | EscapeState::ESC_DCS);
                self.esc |= EscapeState::ESC_STR_END;
            } else if !self.esc.contains(EscapeState::ESC_DCS)
                && self.strescseq.len + len > self.strescseq.size
            {
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
                self.strescseq.buf.resize(self.strescseq.size, 0);
            }

            // memmove(&strescseq.buf[strescseq.len], c, len);
            // strescseq.len += len;
            // return;
            self.strescseq.buf[self.strescseq.len..self.strescseq.len + len]
                .copy_from_slice(&u.to_string().as_bytes()[..len]);

            self.strescseq.len += len;
            return;
        }

        // check_control_code:
        if control {
            /* in UTF-8 mode ignore handling C1 control characters */
            if self.state.mode.contains(TermMode::MODE_UTF8) && ISCONTROLC1(u) {
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
            b'\t' => self.tputtab(1),

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
                self.state.tnewline(self.state.mode.contains(TermMode::MODE_CRLF));
            }

            // BEL (\a)
            0x07 => {
                if self.esc.contains(EscapeState::ESC_STR_END) {
                    // backwards compatibility to xterm
                    // TODO: implemet streescseq handling
                    // strescseq.term = STR_TERM_BEL;
                    // strhandle();
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

            // TODO: check IGNORED
            // case '\005': /* ENQ (IGNORED) */
            // case '\000': /* NUL (IGNORED) */
            // case '\021': /* XON (IGNORED) */
            // case '\023': /* XOFF (IGNORED) */
            // case 0177:   /* DEL (IGNORED) */
            //         return;


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
            }
        }

        // only CAN, SUB, \a and C1 chars interrupt a sequence
        if interrupt_sequence {
            self.esc
                .remove(EscapeState::ESC_STR_END | EscapeState::ESC_STR);
        }
    }

    pub fn tputtab(&mut self, count: isize) {
        let mut x = self.state.c.x;

        if count > 0 {
            while x < self.state.col && count > 0 {
                x += 1;
                while x < self.state.col && self.state.tabs[x] == 0 {
                    x += 1;
                }
            }
        } else if count < 0 {
            while x > 0 && count < 0 {
                x -= 1;
                while x > 0 && self.state.tabs[x] == 0 {
                    x -= 1;
                }
            }
        }

        self.state.c.x = x.min(self.state.col - 1)
    }

    fn tdefutf8(&mut self, u: char) {
        match u {
            'G' => self.state.mode.insert(TermMode::MODE_UTF8),
            '@' => self.state.mode.remove(TermMode::MODE_UTF8),
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
        const VCSMAP: &[Charset] = &[Charset::CS_GRAPHIC0, Charset::CS_USA];

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
                self.esc |= EscapeState::ESC_CSI;
                return false;
            }
            '#' => {
                self.esc |= EscapeState::ESC_TEST;
                return false;
            }
            '%' => {
                self.esc |= EscapeState::ESC_UTF8;
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
                self.xsetmode(0, WinMode::MODE_HIDE);
            }
            // DECKPAM – application keypad
            '=' => {
                self.xsetmode(1, WinMode::MODE_APPKEYPAD);
            }
            // DECPNM -- Normal keypad
            '>' => {
                self.xsetmode(0, WinMode::MODE_APPKEYPAD);
            }
            // DECSC -- Save Cursor
            '7' => {
                self.state.tcursor(CursorMovement::CURSOR_SAVE);
            }
            // DESRC -- Restore Cursor
            '8' => {
                self.state.tcursor(CursorMovement::CURSOR_LOAD);
            }
            // ST -- String terminator
            '\\' => {
                if self.esc.contains(EscapeState::ESC_STR_END) {
                    // TODO: STR_TERM_ST = 0o33
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

    fn dcshandle(&mut self) {
        dcshandle();
    }

    fn strhandle(&mut self) {
        strhandle();
    }

    pub fn ttyread(&mut self) -> usize {
        println!("ttyread called");
        const BUF_SIZE: usize = 256;
        static mut BUF: [u8; 256] = unsafe { std::mem::zeroed() };
        static mut BUF_WRITTEN: usize = 0;
        static mut ALREADY_PROCESSING: bool = false;

        let mut ret = 0;
        let mut written = 0;

        if unsafe { BUF_WRITTEN > BUF_SIZE } {
            return 0;
        }

        unsafe {
            // append read bytes to unprocessed bytes
            println!("ttyread about to read");
            ret = if twrite_aborted {
                1
            } else {
                let b = &raw mut BUF as *mut libc::c_void;
                libc::read(CMDFD, b.add(BUF_WRITTEN), BUF_SIZE - BUF_WRITTEN)
            };

            match ret {
                0 => {
                    libc::exit(0);
                }

                -1 => {
                    panic!("read failed on tty: {}", std::io::Error::last_os_error());
                }

                _ => {
                    BUF_WRITTEN += if twrite_aborted { 0 } else { ret as usize };

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

                    let left = BUF_WRITTEN;
                    println!("Finished processing, {} bytes left in buffer", left);

                    // keep any incomplete UTF-8 byte sequence for the next call
                    if BUF_WRITTEN > 0 {
                        let b = &raw mut BUF as *mut libc::c_void;
                        std::ptr::copy(b.add(written), b, BUF_WRITTEN);
                        std::ptr::write_bytes(b.add(BUF_WRITTEN), 0, BUF_SIZE - BUF_WRITTEN);
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
        self.strescseq = Default::default();
    }

    fn resettitle(&self) {
        // TODO: xsettitle(NULL);
    }

    // TODO:
    fn xsetmode(&mut self, set: i32, flags: WinMode) {
        let mode = self.win.mode;

        if set != 0 {
            self.win.mode.insert(flags);
        } else {
            self.win.mode.remove(flags);
        }

        if (self.win.mode & WinMode::MODE_REVERSE) != (mode & WinMode::MODE_REVERSE) {
            self.redraw();
        }
    }

    // TODO:
    fn xloadcols(&self) {}

    fn tsetdecorstyle(&self, g: *mut Glyph, style: u32) {
        let g = unsafe { &mut *g };
        g.decoration = (g.decoration & !(0x7 << 25)) | ((style & 0x7) << 25);
    }

    fn tsetdecorcolor(&self, g: *mut Glyph, color: u32) {
        let g = unsafe { &mut *g };
        g.decoration = (g.decoration & !0x1ffffff) | (color & 0x1ffffff);
    }

    fn redraw(&mut self) {
        self.state.tfulldirt();
        if let Some(draw) = self.draw.as_mut() {
            draw();
        }
    }
}

fn dcshandle() {}

fn strhandle() {}

fn execsh(cmd: Option<&str>, args: Option<&[&str]>) {
    unsafe {
        let pw = libc::getpwuid(libc::getuid());

        if pw.is_null() {
            panic!("getpwuid: {}", std::io::Error::last_os_error());
        }

        let mut sh = libc::getenv("SHELL".as_ptr() as *const libc::c_char);

        if sh.is_null() {
            sh = if *((*pw).pw_shell) != 0 {
                (*pw).pw_shell
            } else {
                cmd.unwrap_or("/bin/sh").as_ptr() as *mut libc::c_char
            };
        }

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

        // TODO: handle envs
        // unsetenv("COLUMNS");
        // unsetenv("LINES");
        // unsetenv("TERMCAP");
        // setenv("LOGNAME", pw->pw_name, 1);
        // setenv("USER", pw->pw_name, 1);
        // setenv("SHELL", sh, 1);
        // setenv("HOME", pw->pw_dir, 1);
        // setenv("TERM", termname, 1);
        // setenv("COLORTERM", "truecolor", 1);
        // signal(SIGCHLD, SIG_DFL);
        // signal(SIGHUP, SIG_DFL);
        // signal(SIGINT, SIG_DFL);
        // signal(SIGQUIT, SIG_DFL);
        // signal(SIGTERM, SIG_DFL);
        // signal(SIGALRM, SIG_DFL);

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

        unsetenv!("COLUMNS");
        unsetenv!("LINES");
        unsetenv!("TERMCAP");
        setenv!("LOGNAME", (*pw).pw_name);
        setenv!("USER", (*pw).pw_name);
        setenv!("SHELL", sh);
        setenv!("HOME", (*pw).pw_dir);
        setenv!("TERM", "xterm-256color".as_ptr() as *const libc::c_char);

        println!("Exec: {:?} with args: {:?}", sh, args);
        libc::execvp(sh, args.as_ptr());
        libc::_exit(1);
    }
}

fn tgetimgrow(g: &Glyph) -> u32 {
    g.u as u32 & 0x1ff
}

fn tgetimgcol(g: &Glyph) -> u32 {
    (g.u as u32 >> 9) & 0x1ff
}

fn tgetimgid4thbyteplus1(g: &Glyph) -> u32 {
    (g.u as u32 >> 18) & 0x1ff
}

fn tgetimgdiacriticcount(g: &Glyph) -> u32 {
    (g.u as u32 >> 27) & 0x3
}

fn tgetisclassicplaceholder(g: &Glyph) -> bool {
    let v = ((g.u as usize) >> 29) & 0x1;
    return v != 0;
}

fn tsetimgrow(g: &mut Glyph, row: usize) {
    let u = g.u as u32;
    let value = (u & !0x1ff) | (row as u32 & 0x1ff);

    g.u = char::from_u32(value).unwrap_or('\0');
}

fn tsetimgcol(g: &mut Glyph, col: usize) {
    let u = g.u as u32;
    let value = (u & !(0x1ff << 9)) | ((col as u32 & 0x1ff) << 9);

    g.u = char::from_u32(value).unwrap_or('\0');
}

fn tsetimg4thbyteplus1(g: &mut Glyph, byteplus1: u32) {
    let u = g.u as u32;
    let value = (u & !(0x1ff << 18)) | ((byteplus1 & 0x1ff) << 18);

    g.u = char::from_u32(value).unwrap_or('\0');
}

fn tsetimgdiacriticcount(g: &mut Glyph, count: i32) {
    let u = g.u as u32;
    let value = (u & !(0x3 << 27)) | (((count as u32) & 0x3) << 27);

    g.u = char::from_u32(value).unwrap_or('\0');
}

fn tsetisclassicplaceholder(g: &mut Glyph, is_classic: i32) {
    let u = g.u as u32;
    let value = (u & !(0x1 << 29)) | (((is_classic as u32) & 0x1) << 29);

    g.u = char::from_u32(value).unwrap_or('\0');
}

fn tgetimgid(g: &Glyph) -> u32 {
    let mut msb = tgetimgid4thbyteplus1(g);
    if msb != 0 {
        msb -= 1;
    }

    (msb << 24) | (g.fg & 0xFFFFFF)
}

fn tsetimgid(g: &mut Glyph, id: u32) {
    g.fg = (id & 0xFFFFFF) | (1 << 24);
    tsetimg4thbyteplus1(g, ((id >> 24) & 0xFF) + 1);
}

fn tgetimgplacementid(g: &Glyph) -> u32 {
    if tgetdecorcolor(g) == DECOR_DEFAULT_COLOR {
        return 0;
    }

    g.decoration as u32 & 0xFFFFFF
}

fn tgetdecorcolor(_g: &Glyph) -> u32 {
    todo!()
}

fn tsetimgplacementid(_g: &Glyph, _placement_id: usize) {
    todo!()
}

/// Maps a Unicode combining diacritic character to its Kitty image protocol
/// row/column number (1–295). Returns 0 if the character is not a recognized diacritic.
// TODO: check correctness
fn diacritic_to_num(u: char) -> u32 {
    let code = u as u32;
    match code {
        0x305 => code - 0x305 + 1,
        0x30d..=0x30e => code - 0x30d + 2,
        0x310 => code - 0x310 + 4,
        0x312 => code - 0x312 + 5,
        0x33d..=0x33f => code - 0x33d + 6,
        0x346 => code - 0x346 + 9,
        0x34a..=0x34c => code - 0x34a + 10,
        0x350..=0x352 => code - 0x350 + 13,
        0x357 => code - 0x357 + 16,
        0x35b => code - 0x35b + 17,
        0x363..=0x36f => code - 0x363 + 18,
        0x483..=0x487 => code - 0x483 + 31,
        0x592..=0x595 => code - 0x592 + 36,
        0x597..=0x599 => code - 0x597 + 40,
        0x59c..=0x5a1 => code - 0x59c + 43,
        0x5a8..=0x5a9 => code - 0x5a8 + 49,
        0x5ab..=0x5ac => code - 0x5ab + 51,
        0x5af => code - 0x5af + 53,
        0x5c4 => code - 0x5c4 + 54,
        0x610..=0x617 => code - 0x610 + 55,
        0x657..=0x65b => code - 0x657 + 63,
        0x65d..=0x65e => code - 0x65d + 68,
        0x6d6..=0x6dc => code - 0x6d6 + 70,
        0x6df..=0x6e2 => code - 0x6df + 77,
        0x6e4 => code - 0x6e4 + 81,
        0x6e7..=0x6e8 => code - 0x6e7 + 82,
        0x6eb..=0x6ec => code - 0x6eb + 84,
        0x730 => code - 0x730 + 86,
        0x732..=0x733 => code - 0x732 + 87,
        0x735..=0x736 => code - 0x735 + 89,
        0x73a => code - 0x73a + 91,
        0x73d => code - 0x73d + 92,
        0x73f..=0x741 => code - 0x73f + 93,
        0x743 => code - 0x743 + 96,
        0x745 => code - 0x745 + 97,
        0x747 => code - 0x747 + 98,
        0x749..=0x74a => code - 0x749 + 99,
        0x7eb..=0x7f1 => code - 0x7eb + 101,
        0x7f3 => code - 0x7f3 + 108,
        0x816..=0x819 => code - 0x816 + 109,
        0x81b..=0x823 => code - 0x81b + 113,
        0x825..=0x827 => code - 0x825 + 122,
        0x829..=0x82d => code - 0x829 + 125,
        0x951 => code - 0x951 + 130,
        0x953..=0x954 => code - 0x953 + 131,
        0xf82..=0xf83 => code - 0xf82 + 133,
        0xf86..=0xf87 => code - 0xf86 + 135,
        0x135d..=0x135f => code - 0x135d + 137,
        0x17dd => code - 0x17dd + 140,
        0x193a => code - 0x193a + 141,
        0x1a17 => code - 0x1a17 + 142,
        0x1a75..=0x1a7c => code - 0x1a75 + 143,
        0x1b6b => code - 0x1b6b + 151,
        0x1b6d..=0x1b73 => code - 0x1b6d + 152,
        0x1cd0..=0x1cd2 => code - 0x1cd0 + 159,
        0x1cda..=0x1cdb => code - 0x1cda + 162,
        0x1ce0 => code - 0x1ce0 + 164,
        0x1dc0..=0x1dc1 => code - 0x1dc0 + 165,
        0x1dc3..=0x1dc9 => code - 0x1dc3 + 167,
        0x1dcb..=0x1dcc => code - 0x1dcb + 174,
        0x1dd1..=0x1de6 => code - 0x1dd1 + 176,
        0x1dfe => code - 0x1dfe + 198,
        0x20d0..=0x20d1 => code - 0x20d0 + 199,
        0x20d4..=0x20d7 => code - 0x20d4 + 201,
        0x20db..=0x20dc => code - 0x20db + 205,
        0x20e1 => code - 0x20e1 + 207,
        0x20e7 => code - 0x20e7 + 208,
        0x20e9 => code - 0x20e9 + 209,
        0x20f0 => code - 0x20f0 + 210,
        0x2cef..=0x2cf1 => code - 0x2cef + 211,
        0x2de0..=0x2dff => code - 0x2de0 + 214,
        0xa66f => code - 0xa66f + 246,
        0xa67c..=0xa67d => code - 0xa67c + 247,
        0xa6f0..=0xa6f1 => code - 0xa6f0 + 249,
        0xa8e0..=0xa8f1 => code - 0xa8e0 + 251,
        0xaab0 => code - 0xaab0 + 269,
        0xaab2..=0xaab3 => code - 0xaab2 + 270,
        0xaab7..=0xaab8 => code - 0xaab7 + 272,
        0xaabe..=0xaabf => code - 0xaabe + 274,
        0xaac1 => code - 0xaac1 + 276,
        0xfe20..=0xfe26 => code - 0xfe20 + 277,
        0x10a0f => code - 0x10a0f + 284,
        0x10a38 => code - 0x10a38 + 285,
        0x1d185..=0x1d189 => code - 0x1d185 + 286,
        0x1d1aa..=0x1d1ad => code - 0x1d1aa + 291,
        0x1d242..=0x1d244 => code - 0x1d242 + 295,
        _ => 0,
    }
}

fn gr_get_glyph_underneath_image(
    _image_id: u32,
    _placement_id: u32,
    _col: u32,
    _row: u32,
) -> Option<&'static Glyph> {
    todo!()
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
