use std::ptr::{null, null_mut};

use crate::boxdraw::boxdraw::isboxdraw;
use crate::csiesq::{CSIEscape, STR_TERM_ST};
use crate::glyph::{Glyph, GlyphAttribute};
use crate::win::{TermWindow, WinMode};
use crate::{BETWEEN, config};
use bitflags::bitflags;
use libc::pselect;
use unicode_width::UnicodeWidthChar;

const STR_BUF_SIZ: usize = 128 * 4; // ESC_BUF_SIZ
const UTF_SIZ: usize = 4;

fn TRUECOLOR(r: u8, g: u8, b: u8) -> u32 {
    1 << 24 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

pub fn IS_TRUECOL(c: u32) -> bool {
    (c & (1 << 24)) != 0
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

// #define ISCONTROLC0(c) (BETWEEN(c, 0, 0x1f) || (c) == 0x7f)
// #define ISCONTROLC1(c) (BETWEEN(c, 0x80, 0x9f))
// #define ISCONTROL(c)   (ISCONTROLC0(c) || ISCONTROLC1(c))

fn ISCONTROLC0(c: char) -> bool {
    BETWEEN!(c, '\0', '\u{1F}') || c == '\u{7F}'
}

fn ISCONTROLC1(c: char) -> bool {
    BETWEEN!(c, '\u{80}', '\u{9F}')
}

fn ISCONTROL(c: char) -> bool {
    ISCONTROLC0(c) || ISCONTROLC1(c)
}

static mut iofd: i32 = 0;
static mut cmdfd: i32 = 0;
static mut pid: i32 = 0;
// TODO: move this to config
pub static vtiden: &[u8] = b"\x1b[?62;4c";

const DECOR_DEFAULT_COLOR: u32 = 0x0FFFFFF;
const IMAGE_PLACEHOLDER_CHAR: char = '\u{10EEEE}';
const IMAGE_PLACEHOLDER_CHAR_OLD: char = '\u{EEEE}';

// TODO: handle globals properly
static mut su: usize = 0;
pub static mut twrite_aborted: bool = false;

bitflags! {
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TermMode: u32 {
        const MODE_WRAP = 1 << 0;
        const MODE_INSERT = 1 << 1;
        const MODE_ALTSCREEN = 1 << 2;
        const MODE_CRLF = 1 << 3;
        const MODE_ECHO = 1 << 4;
        const MODE_PRINT = 1 << 5;
        const MODE_UTF8 = 1 << 6;
        const MODE_SIXEL        = 1 << 7;
        const MODE_SIXEL_CUR_RT = 1 << 8;
        const MODE_SIXEL_SDM    = 1 << 9;
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CursorMovement {
    CURSOR_SAVE,
    CURSOR_LOAD,
}

bitflags! {
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CursorState: u32 {
        const CURSOR_DEFAULT = 0;
        const CURSOR_WRAPNEXT = 1;
        const CURSOR_ORIGIN = 2;
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq)]
enum Charset {
    #[default]
    CS_GRAPHIC0 = 0,
    CS_GRAPHIC1 = 1,
    CS_UK = 2,
    CS_USA = 3,
    CS_MULTI = 4,
    CS_GER = 5,
    CS_FIN = 6,
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

#[derive(Default, Debug, Clone, Copy)]
pub struct Vec2 {
    x: isize,
    y: isize,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    #[default]
    SEL_IDLE = 0,
    SEL_EMPTY = 1,
    SEL_READY = 2,

    // This could be replaced with SEL_IDLE, but gotta test first
    SEL_REMOVED = 3,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionType {
    #[default]
    SEL_REGULAR = 1,
    SEL_RECTANGULAR = 2,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Selection {
    mode: SelectionMode,
    type_: SelectionType,
    snap: i32,
    /*
     * Selection variables:
     * nb – normalized coordinates of the beginning of the selection
     * ne – normalized coordinates of the end of the selection
     * ob – original coordinates of the beginning of the selection
     * oe – original coordinates of the end of the selection
     */
    nb: Vec2,
    ne: Vec2,
    ob: Vec2,
    oe: Vec2,

    alt: bool,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct TCursor {
    pub attr: Glyph, // current char attributes
    pub x: usize,
    pub y: usize,
    pub state: CursorState,
}

// Temp structs
pub type Line = Box<[Glyph]>;

#[derive(Debug, Clone, Copy)]
pub struct Image {
    x: usize,
    y: usize,
    cols: usize,
    rows: usize,
}

/* Internal representation of the screen */

#[derive(Default)]
pub struct Term {
    // int row;         /* nb row */
    // int col;         /* nb col */
    // int pixw;        /* width of the text area in pixels */
    // int pixh;        /* height of the text area in pixels */
    // Line *line;      /* screen */
    // Line *alt;       /* alternate screen */
    // int *dirty;      /* dirtyness of lines */
    // TCursor c;       /* cursor */
    // int ocx;         /* old cursor col */
    // int ocy;         /* old cursor row */
    // int top;         /* top    scroll limit */
    // int bot;         /* bottom scroll limit */
    // int mode;        /* terminal mode flags */
    // int esc;         /* escape state flags */
    // char trantbl[4]; /* charset table translation */
    // int charset;     /* current charset */
    // int icharset;    /* selected charset for sequence */
    // int *tabs;
    // ImageList *images;     /* sixel images */
    // ImageList *images_alt; /* sixel images for alternate screen */
    // Rune lastc;            /* last printed char outside of sequence, 0 if control */
    pub row: usize,
    pub col: usize,
    pub pixw: usize,
    pub pixh: usize,
    pub line: Vec<Line>,
    alt: Vec<Line>,
    pub dirty: Vec<bool>,
    pub c: TCursor,
    pub ocx: usize,
    pub ocy: usize,
    pub top: usize,
    pub bot: usize,
    pub mode: TermMode,
    esc: EscapeState,
    trantbl: [Charset; 4],
    charset: usize,
    icharset: u32,
    // TODO: make this bool
    pub tabs: Vec<usize>,
    images: Vec<Image>,
    images_alt: Vec<Image>,
    pub lastc: char,

    // fields added on rewrite
    sel: Selection,
    strescseq: StrEscape,
    csiescseq: CSIEscape,
    win: *mut TermWindow,
    // HACK: NEED TO REMOVE THIS ASAP
    pub draw: Option<Box<dyn FnMut()>>,
}

impl Term {
    pub fn new(col: usize, row: usize, win: *mut TermWindow) -> Self {
        let mut term = Term {
            win,
            ..Default::default()
        };

        term.tresize(col, row);
        term.treset();

        term
    }

    /// Check if any cell in the terminal has the specified attribute set.
    pub fn tattrset(&mut self, attr: GlyphAttribute) -> bool {
        for row in 0..self.row {
            for col in 0..self.col {
                if self.line[row][col].mode.contains(attr) {
                    return true;
                }
            }
        }

        return false;
    }

    pub fn tsetdirt(&mut self, top: usize, bot: usize) {
        let top = top.min(self.row - 1);
        let bot = bot.min(self.row - 1);

        for i in top..=bot {
            self.dirty[i] = true;
        }
    }

    /// Mark all lines containing any glyph with the specified attribute as dirty.
    pub fn tsetdirtattr(&mut self, attr: GlyphAttribute) {
        for row in 0..self.row {
            for col in 0..self.col {
                if self.line[row][col].mode.contains(attr) {
                    self.tsetdirt(row, row);
                    break;
                }
            }
        }
    }

    pub fn tsetsixelattr(line: &mut Line, x1: usize, x2: usize) {
        for x in x1..=x2 {
            line[x].mode.insert(GlyphAttribute::ATTR_SIXEL);
        }
    }

    pub fn tfulldirt(&mut self) {
        self.tsync_end();
        self.tsetdirt(0, self.row - 1);
    }

    pub fn tcursor(&mut self, mode: CursorMovement) {
        static mut C: [Option<TCursor>; 2] = [None, None];

        let alt = if self.mode.contains(TermMode::MODE_ALTSCREEN) {
            1
        } else {
            0
        };

        unsafe {
            match mode {
                CursorMovement::CURSOR_SAVE => {
                    C[alt] = Some(self.c);
                }
                CursorMovement::CURSOR_LOAD => {
                    if let Some(c) = C[alt] {
                        self.c = c;
                        self.tmoveto(c.x, c.y);
                    }
                }
            }
        }
    }

    pub fn tresetcursor(&mut self) {
        self.c = TCursor::default();
    }

    pub fn treset(&mut self) {
        self.tresetcursor();

        // memset
        self.tabs.iter_mut().for_each(|t| *t = 0);

        for i in (config::TABSPACES..self.col).step_by(config::TABSPACES) {
            self.tabs[i] = 1;
        }

        self.top = 0;
        self.bot = self.row - 1;
        self.mode = TermMode::MODE_WRAP | TermMode::MODE_UTF8;

        self.trantbl = [Charset::CS_USA; 4];
        self.charset = 0;

        for _ in 0..2 {
            self.tmoveto(0, 0);
            self.tcursor(CursorMovement::CURSOR_SAVE);
            self.tclearregion(0, 0, self.col - 1, self.row - 1);
            self.tdeleteimages();
            self.tswapscreen();
        }
    }

    pub fn tisaltscr(&self) -> bool {
        self.mode.contains(TermMode::MODE_ALTSCREEN)
    }

    pub fn tswapscreen(&mut self) {
        std::mem::swap(&mut self.line, &mut self.alt);
        std::mem::swap(&mut self.images, &mut self.images_alt);
        self.mode.toggle(TermMode::MODE_ALTSCREEN);
        self.tfulldirt();
    }

    pub fn tscrolldown(&mut self, orig: usize, n: usize) {
        // ImageList *im, *next;

        let n = n.min(self.bot - orig + 1);

        self.tsetdirt(orig, self.bot - n);
        self.tclearregion(0, self.bot - n + 1, self.col - 1, self.bot);

        // TODO: Check if this range is correct
        for i in (self.bot..orig + n).rev() {
            self.line.swap(i, i - n);
        }

        /* move images, if they are inside the scrolling region */
        // let itop = orig;
        // let ibot = bot;
        // for (im = term.images; im; im = next) {
        // 	next = im->next;
        // 	if (im->y >= itop && im->y <= ibot) {
        // 		im->y += n;
        // 		if (im->y > ibot) {
        // 			delete_image(im);
        // 		}
        // 	}
        // }
        //

        self.selscroll(orig, n as isize);
    }

    pub fn tscrollup(&mut self, orig: usize, n: usize) {
        // ImageList *im, *next;

        let n = n.min(self.bot - orig + 1);

        self.tclearregion(0, orig, self.col - 1, orig + n - 1);
        self.tsetdirt(orig + n, self.bot);

        for i in orig..(self.bot.saturating_sub(n)) {
            // let temp = self.line[i].to_owned();
            // self.line[i] = self.line[i + n];
            // self.line[i + n] = temp;

            self.line.swap(i, i + n);
        }

        /* move images, if they are inside the scrolling region */
        // let itop = orig;
        // let ibot = bot;
        // for (im = self.images; im; im = next) {
        // 	next = im->next;
        // 	if (im->y >= itop && im->y <= ibot) {
        // 		im->y -= n;
        // 		if (im->y < itop) {
        // 			delete_image(im);
        // 		}
        // 	}
        // }

        self.selscroll(orig, -(n as isize));
    }

    pub fn selscroll(&mut self, orig: usize, n: isize) {
        let sel = &mut self.sel;

        if sel.mode == SelectionMode::SEL_REMOVED
            || sel.alt != self.mode.contains(TermMode::MODE_ALTSCREEN)
        {
            return;
        }

        if BETWEEN!(sel.nb.y as usize, orig, self.bot)
            != BETWEEN!(sel.ne.y as usize, orig, self.bot)
        {
            self.selclear();
        } else if BETWEEN!(sel.nb.y as usize, orig, self.bot) {
            sel.ob.y += n;
            sel.oe.y += n;

            if sel.ob.y < self.top as isize
                || sel.ob.y > self.bot as isize
                || sel.oe.y < self.top as isize
                || sel.oe.y > self.bot as isize
            {
                self.selclear();
            } else {
                self.selnormalize();
            }
        }
    }

    pub fn tnewline(&mut self, first_col: bool) {
        let mut y = self.c.y;

        if y == self.bot {
            self.tscrollup(self.top, 1);
        } else {
            y += 1;
        }

        let col = if first_col { 0 } else { self.c.x };

        self.tmoveto(col, y);
    }

    pub fn tmoveato(&mut self, x: usize, y: usize) {
        let origin = if self.c.state.contains(CursorState::CURSOR_ORIGIN) {
            self.top
        } else {
            0
        };

        self.tmoveto(x, y + origin);
    }

    pub fn tmoveto(&mut self, x: usize, y: usize) {
        let (miny, maxy) = if self.c.state.contains(CursorState::CURSOR_ORIGIN) {
            (self.top, self.bot)
        } else {
            (0, self.row - 1)
        };

        self.c.state.remove(CursorState::CURSOR_WRAPNEXT);
        self.c.x = x.max(0).min(self.col - 1);
        self.c.y = y.max(miny).min(maxy);
    }

    pub fn tsetchar(&mut self, u: char, attr: &Glyph, x: usize, y: usize) {
        #[rustfmt::skip]
        const VT100_0: [char; 62] = [
	        /* 0x41 - 0x7e */
	        '↑', '↓', '→', '←', '█', '▚', '☃',      /* A - G */
	        '\0',   '\0',   '\0',   '\0',   '\0',   '\0',   '\0',   '\0',   /* H - O */
	        '\0',   '\0',   '\0',   '\0',   '\0',   '\0',   '\0',   '\0',   /* P - W */
	        '\0',   '\0',   '\0',   '\0',   '\0',   '\0',   '\0',   ' ', /* X - _ */
	        '◆', '▒', '␉', '␌', '␍', '␊', '°', '±', /* ` - g */
	        '␤', '␋', '┘', '┐', '┌', '└', '┼', '⎺', /* h - o */
	        '⎻', '─', '⎼', '⎽', '├', '┤', '┴', '┬', /* p - w */
	        '│', '≤', '≥', 'π', '≠', '£', '·',      /* x - ~ */
	];

        // The table is proudly stolen from rxvt (and from st)

        if self.trantbl[self.charset] == Charset::CS_GRAPHIC0 && BETWEEN!(u, 'A', '~') {
            self.line[y][x].u = VT100_0[(u as usize) - 0x41];
        }

        if self.line[y][x].mode.contains(GlyphAttribute::ATTR_WIDE) {
            if x + 1 < self.col {
                self.line[y][x + 1].u = ' ';
                self.line[y][x + 1].mode &= !GlyphAttribute::ATTR_WDUMMY;
            }
        } else if self.line[y][x].mode.contains(GlyphAttribute::ATTR_WDUMMY) {
            self.line[y][x - 1].u = ' ';
            self.line[y][x - 1].mode &= !GlyphAttribute::ATTR_WDUMMY;
        }

        let is_classic_placeholder = tgetisclassicplaceholder(&self.line[y][x]);

        if u == ' '
            && self.line[y][x].mode.contains(GlyphAttribute::ATTR_IMAGE)
            && is_classic_placeholder
        {
            self.line[y][x].bg = attr.bg;
            self.dirty[y] = true;
            return;
        }

        self.dirty[y] = true;
        self.line[y][x] = *attr;
        self.line[y][x].u = u;

        if u == IMAGE_PLACEHOLDER_CHAR || u == IMAGE_PLACEHOLDER_CHAR_OLD {
            self.line[y][x].u = 0 as char;
            self.line[y][x].mode.insert(GlyphAttribute::ATTR_IMAGE);
        } else if isboxdraw(u) {
            self.line[y][x].mode.insert(GlyphAttribute::ATTR_BOXDRAW);
        }
    }

    pub fn tclearregion(&mut self, x1: usize, y1: usize, x2: usize, y2: usize) {
        let (x1, x2) = if x1 > x2 { (x2, x1) } else { (x1, x2) };
        let (y1, y2) = if y1 > y2 { (y2, y1) } else { (y1, y2) };

        let x1 = x1.min(self.col - 1);
        let x2 = x2.min(self.col - 1);
        let y1 = y1.min(self.row - 1);
        let y2 = y2.min(self.row - 1);

        for y in y1..=y2 {
            self.dirty[y] = true;

            for x in x1..=x2 {
                if self.selected(x, y) {
                    self.selclear();
                }

                let gp = &mut self.line[y][x];

                gp.fg = self.c.attr.fg;
                gp.bg = self.c.attr.bg;
                gp.decoration = self.c.attr.decoration;
                gp.mode = GlyphAttribute::empty();
                gp.u = ' ';
            }
        }
    }

    /// Fills a rectangle area with an image placeholder. The starting point is the
    /// cursor. Adds empty lines if needed. The placeholder will be marked as
    /// classic.
    pub fn tcreateimgplaceholder(
        &mut self,
        image_id: u32,
        placement_id: usize,
        cols: usize,
        rows: usize,
        do_not_move_cursor: bool,
        mut text_underneath: Option<&mut [Glyph]>,
    ) {
        for row in 0..rows {
            let y = self.c.y;
            self.dirty[y] = true;

            for col in 0..cols {
                let x = self.c.x + col;

                if x >= self.col {
                    break;
                }

                if self.selected(x, y) {
                    self.selclear();
                }

                let gp = &mut self.line[y][x];

                if let Some(ref mut text_underneath) = text_underneath {
                    let mut to_save = gp as &Glyph;

                    // If there is already a classic placeholder,
                    // use the text underneath it. This will leave
                    // holes in images, but at least we are
                    // guaranteed to restore the original text.

                    if gp.mode.contains(GlyphAttribute::ATTR_IMAGE) && tgetisclassicplaceholder(gp)
                    {
                        let under = gr_get_glyph_underneath_image(
                            tgetimgid(gp),
                            tgetimgplacementid(gp),
                            tgetimgcol(gp),
                            tgetimgrow(gp),
                        );

                        if let Some(under) = under {
                            to_save = under;
                        }
                    }

                    text_underneath[cols * row + col] = *to_save;
                }

                gp.mode = GlyphAttribute::ATTR_IMAGE;
                gp.u = 0 as char;
                tsetimgrow(gp, row + 1);
                tsetimgcol(gp, col + 1);
                tsetimgid(gp, image_id);
                tsetimgplacementid(gp, placement_id);
                tsetimgdiacriticcount(gp, 3);
                tsetisclassicplaceholder(gp, 1);
            }

            if do_not_move_cursor && y == self.row - 1 {
                break;
            }

            if row != rows - 1 {
                self.tnewline(false);
            }
        }

        if do_not_move_cursor {
            self.tmoveto(self.c.x, self.c.y - rows + 1);
        } else {
            // Move the cursor beyond the last column, as required by the
            // protocol. If the cursor goes beyond the screen edge, insert a
            // newline to match the behavior of kitty.
            if self.c.x + cols >= self.col {
                self.tnewline(true);
            } else {
                self.tmoveto(self.c.x + cols, self.c.y);
            }
        }
    }

    pub fn tresize(&mut self, col: usize, row: usize) {
        let minrow = row.min(self.row);
        let mincol = col.min(self.col);

        if col < 1 || row < 1 {
            // ERR: invalid size
            return;
        }

        // scroll both screens independently
        if row < self.row {
            self.tcursor(CursorMovement::CURSOR_SAVE);
            self.tsetscroll(0, self.row - 1);

            for i in 0..2 {
                if (self.c.y >= row) {
                    self.tscrollup(0, self.c.y - row + 1);
                }

                for j in row..self.row {
                    // free(self.line[j]);
                }

                self.tswapscreen();
                self.tcursor(CursorMovement::CURSOR_LOAD);
            }
        }

        self.line
            .resize_with(row, || vec![Glyph::default(); col].into_boxed_slice());
        self.alt
            .resize_with(row, || vec![Glyph::default(); col].into_boxed_slice());
        self.dirty.resize(row, true);
        self.tabs.resize(col, 0);

        fn resize_boxed_sliced(line: &mut Line, new_len: usize) {
            let mut vec = line.to_owned().to_vec();
            vec.resize(new_len, Glyph::default());
            *line = vec.into_boxed_slice();
        }

        // resize each row to new width, zero-pad if needed
        for y in 0..minrow {
            resize_boxed_sliced(&mut self.line[y], col);
            resize_boxed_sliced(&mut self.alt[y], col);
        }

        // allocate any new rows
        for y in minrow..row {
            self.line[y] = vec![Glyph::default(); col].into_boxed_slice();
            self.alt[y] = vec![Glyph::default(); col].into_boxed_slice();
        }

        if col > self.col {
            self.tabs[self.col..]
                .iter_mut()
                .step_by(config::TABSPACES)
                .for_each(|t| *t = 0);

            for i in (config::TABSPACES..col).step_by(config::TABSPACES) {
                self.tabs[i] = 1;
            }
        }

        self.col = col;
        self.row = row;

        // reset scrolling region
        self.tsetscroll(0, row - 1);

        // clearing both screens (it makes dirty all lines)

        for i in 0..2 {
            self.tmoveto(self.c.x, self.c.y);
            self.tcursor(CursorMovement::CURSOR_SAVE);

            if mincol < col && 0 < minrow {
                self.tclearregion(mincol, 0, col - 1, minrow - 1);
            }

            if 0 < col && minrow < row {
                self.tclearregion(0, minrow, col - 1, row - 1);
            }
        }

        // expand images into new terxt cells

        for i in 0..2 {
            for image in &self.images {
                if image.y < 0 || image.y >= self.row {
                    // TODO:  delete_image(image);
                    continue;
                }

                let line = self.line[image.y].as_mut();
                let x2 = (image.x + image.cols).min(self.col) - 1;

                if mincol < col && x2 >= mincol && image.x < col {
                    // TODO: self.tsetsixelattr(line, image.x.max(mincol), x2);
                }
            }

            self.tswapscreen();
        }
    }

    pub fn tsetscroll(&mut self, t: usize, b: usize) {
        let temp;

        let mut t = t.max(0).min(self.row - 1);
        let mut b = b.max(0).min(self.row - 1);

        if t > b {
            temp = t;
            t = b;
            b = temp;
        }

        self.top = t;
        self.bot = b;
    }

    fn selclear(&mut self) {
        if self.sel.mode == SelectionMode::SEL_REMOVED {
            return;
        }

        self.selremove();
        self.tsetdirt(self.sel.nb.y as usize, self.sel.ne.y as usize);
    }

    fn selnormalize(&mut self) {
        let sel = &mut self.sel;

        if sel.type_ == SelectionType::SEL_REGULAR && sel.ob.y != sel.oe.y {
            sel.nb.x = if sel.ob.y < sel.oe.y {
                sel.ob.x
            } else {
                sel.oe.x
            };

            sel.ne.x = if sel.ob.y < sel.oe.y {
                sel.oe.x
            } else {
                sel.ob.x
            };
        } else {
            sel.nb.x = sel.ob.x.min(sel.oe.x);
            sel.ne.x = sel.ob.x.max(sel.oe.x);
        }
        sel.nb.y = sel.ob.y.min(sel.oe.y);
        sel.ne.y = sel.ob.y.max(sel.oe.y);

        // selsnap(&sel.nb.x, &sel.nb.y, -1);
        // selsnap(&sel.ne.x, &sel.ne.y, +1);

        /* expand selection over line breaks */
        if sel.type_ == SelectionType::SEL_RECTANGULAR {
            return;
        }

        let sel = &self.sel;

        let i = self.tlinelen(sel.nb.y as usize) as isize;
        let ne_len = self.tlinelen(sel.ne.y as usize) as isize;

        let sel = &mut self.sel;

        if i < sel.nb.x {
            sel.nb.x = i;
        }

        if ne_len <= sel.ne.x {
            sel.ne.x = (self.col - 1) as isize;
        }
    }

    fn selremove(&mut self) {
        self.sel.mode = SelectionMode::SEL_REMOVED;
    }

    fn tlinelen(&self, y: usize) -> usize {
        let mut i = self.col;

        if self.line[y][i - 1].mode.contains(GlyphAttribute::ATTR_WRAP) {
            return i;
        }

        while i > 0 && self.line[y][i - 1].u == ' ' {
            i -= 1;
        }

        return i;
    }

    fn tsync_end(&self) {
        // static void tsync_end() { su = 0; }
        // int tinsync(uint timeout) {
        // 	struct timespec now;
        // 	if (su && !clock_gettime(CLOCK_MONOTONIC, &now) && TIMEDIFF(now, sutv) >= timeout) {
        // 		su = 0;
        // 	}
        // 	return su;
        // }

        return;
    }

    pub fn selected(&self, x: usize, y: usize) -> bool {
        let sel = &self.sel;

        // sel.ob.x == -1 => SelectionMode::SEL_REMOVED
        if sel.mode == SelectionMode::SEL_EMPTY || sel.ob.x == -1 {
            return false;
        }

        if sel.alt != self.mode.contains(TermMode::MODE_ALTSCREEN) {
            return false;
        }

        if sel.type_ == SelectionType::SEL_RECTANGULAR {
            return BETWEEN!(y as isize, sel.nb.y, sel.ne.y)
                && BETWEEN!(x as isize, sel.nb.x, sel.ne.x);
        }

        return BETWEEN!(y as isize, sel.nb.y, sel.ne.y)
            && (y as isize != sel.nb.y || x as isize >= sel.nb.x)
            && (y as isize != sel.ne.y || x as isize <= sel.ne.x);
    }

    pub fn tdeleteimages(&self) {
        // TODO: delete all images in the current screen
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
            self.mode.insert(TermMode::MODE_PRINT);
            unsafe {
                iofd = if out == "-" {
                    1
                } else {
                    libc::open(
                        out.as_ptr() as *const libc::c_char,
                        libc::O_WRONLY | libc::O_CREAT,
                        0o666,
                    )
                };

                if iofd < 0 {
                    panic!("Error opening {}:{}", out, std::io::Error::last_os_error());
                }
            }
        }

        if let Some(line) = line {
            unsafe {
                cmdfd = libc::open(line.as_ptr() as *const libc::c_char, libc::O_RDWR);

                if cmdfd < 0 {
                    panic!(
                        "open line '{}' failed: {}",
                        line,
                        std::io::Error::last_os_error()
                    );
                }

                libc::dup2(cmdfd, 0);
                // TODO: stty(args);

                return cmdfd;
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
            pid = libc::fork();

            match pid {
                -1 => {
                    panic!("fork failed: {}", std::io::Error::last_os_error());
                }

                0 => {
                    libc::close(iofd);
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
                    cmdfd = m;
                    libc::sigemptyset(&mut sa.sa_mask);
                    libc::sigaction(libc::SIGCHLD, &sa, null_mut());
                }
            }

            return cmdfd;
        }
    }

    pub fn ttyresize(&mut self, tw: usize, th: usize) {
        self.pixw = tw;
        self.pixh = th;

        let w = libc::winsize {
            ws_row: self.row as u16,
            ws_col: self.col as u16,
            ws_xpixel: tw as u16,
            ws_ypixel: th as u16,
        };

        unsafe {
            if libc::ioctl(cmdfd, libc::TIOCSWINSZ, &w) < 0 {
                panic!(
                    "Couldn't set window size: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
    }

    pub fn ttywrite(&mut self, buffer: &[u8], len: usize, may_echo: bool) {
        if may_echo && self.mode.contains(TermMode::MODE_ECHO) {
            self.twrite(&buffer, len, true);
        }

        if !self.mode.contains(TermMode::MODE_CRLF) {
            self.ttywriteraw(buffer, len);
            return;
        }

        // This is similar to how the kernel handles ONLCR for ttys
        let mut i = 0;
        // TODO: check if this is correct
        while i < len {
            let c = buffer[i];

            if c == b'\r' {
                i += 1;
                // self.ttwriteraw(&"\r\n", 2);
            } else {
                let next = buffer[i..]
                    .iter()
                    .position(|&x| x == b'\r')
                    .unwrap_or(len - i)
                    + i;

                self.ttywriteraw(&buffer[i..], next - i);
            }
        }
    }

    fn twrite(&mut self, buffer: &[u8], buflen: usize, show_ctrl: bool) -> usize {
        let mut charsize = 0;
        let mut i = 0;
        let mut u: char = '\0';
        let su0 = unsafe { su };

        unsafe { twrite_aborted = false };

        while i < buflen {
            if self.mode.contains(TermMode::MODE_SIXEL)
            /* TODO: sixel_st.state != PS_ESC */
            {
                // charsize = sixel_parser_parse(&sixel_st, (const unsigned char *)buf + n, buflen - n);
                // continue;
            } else if self.mode.contains(TermMode::MODE_UTF8) {
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

            if su0 != 0 && unsafe { su == 0 } {
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

    fn ttywriteraw(&mut self, buffer: &[u8], len: usize) {
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
                libc::FD_SET(cmdfd, &mut wfd);
                libc::FD_SET(cmdfd, &mut rfd);

                if pselect(
                    cmdfd + 1,
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

                if libc::FD_ISSET(cmdfd, &mut wfd) {
                    /*
                     * Only write the bytes written by ttywrite() or the
                     * default of 256. This seems to be a reasonable value
                     * for a serial line. Bigger values might clog the I/O.
                     */
                    let count = if n < lim { n } else { lim };
                    let r = libc::write(cmdfd, s, count);

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

                if libc::FD_ISSET(cmdfd, &mut rfd) {
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

        if (u as u32) < 127 && !self.mode.contains(TermMode::MODE_UTF8) {
            width = 1;
            len = 1;
        } else {
            len = u.len_utf8();

            width = u.width().unwrap_or(0);

            if !control && width == 0 {
                width = 1;
            }
        }

        if self.mode.contains(TermMode::MODE_PRINT) {
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
        let mut check_control_code = false;

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
            if self.mode.contains(TermMode::MODE_UTF8) && ISCONTROLC1(u) {
                return;
            }

            self.tcontrolcode(u as u8);

            if self.esc.is_empty() {
                self.lastc = '\0';
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

        if self.selected(self.c.x, self.c.y) {
            self.selclear();
        }

        if width == 0 {
            // Combining character – not properly supported; handle image diacritics
            if self.c.y == 0 && self.c.x == 0 {
                self.lastc = u;
                return;
            }

            let (gx, gy): (usize, usize);
            if self.c.x == 0 {
                gy = self.c.y - 1;
                gx = self.col - 1;
            } else if self.c.state.contains(CursorState::CURSOR_WRAPNEXT) {
                gy = self.c.y;
                gx = self.c.x;
            } else {
                gy = self.c.y;
                gx = self.c.x - 1;
            }

            let num = diacritic_to_num(u);
            if num != 0 && self.line[gy][gx].mode.contains(GlyphAttribute::ATTR_IMAGE) {
                let diaccount = tgetimgdiacriticcount(&self.line[gy][gx]);
                if diaccount == 0 {
                    tsetimgrow(&mut self.line[gy][gx], num as usize);
                } else if diaccount == 1 {
                    tsetimgcol(&mut self.line[gy][gx], num as usize);
                } else if diaccount == 2 {
                    tsetimg4thbyteplus1(&mut self.line[gy][gx], num);
                }
                tsetimgdiacriticcount(&mut self.line[gy][gx], diaccount as i32 + 1);
            }
            self.lastc = u;
            return;
        }

        if self.mode.contains(TermMode::MODE_WRAP)
            && self.c.state.contains(CursorState::CURSOR_WRAPNEXT)
        {
            let (cx, cy) = (self.c.x, self.c.y);
            self.line[cy][cx].mode |= GlyphAttribute::ATTR_WRAP;
            self.tnewline(true);
        }

        if self.mode.contains(TermMode::MODE_INSERT) && (self.c.x + width as usize) < self.col {
            let cx = self.c.x;
            let cy = self.c.y;
            let move_count = self.col - cx - width as usize;
            self.line[cy].copy_within(cx..cx + move_count, cx + width as usize);
            self.line[cy][cx].mode &= !GlyphAttribute::ATTR_WIDE;
        }

        if self.c.x + width as usize > self.col {
            if self.mode.contains(TermMode::MODE_WRAP) {
                self.tnewline(true);
            } else {
                let w = width as usize;
                let col = self.col;
                self.tmoveto(col - w, self.c.y);
            }
        }

        let (cx, cy) = (self.c.x, self.c.y);
        self.tsetchar(u, &self.c.attr.clone(), cx, cy);
        self.lastc = u;

        if width == 2 {
            let (cx, cy) = (self.c.x, self.c.y);
            self.line[cy][cx].mode |= GlyphAttribute::ATTR_WIDE;
            if cx + 1 < self.col {
                if self.line[cy][cx + 1].mode == GlyphAttribute::ATTR_WIDE && cx + 2 < self.col {
                    self.line[cy][cx + 2].u = ' ';
                    self.line[cy][cx + 2].mode &= !GlyphAttribute::ATTR_WDUMMY;
                }
                self.line[cy][cx + 1].u = '\0';
                self.line[cy][cx + 1].mode = GlyphAttribute::ATTR_WDUMMY;
            }
        }

        let (cx, cy) = (self.c.x, self.c.y);
        if cx + (width as usize) < self.col {
            self.tmoveto(cx + width as usize, cy);
        } else {
            self.c.state |= CursorState::CURSOR_WRAPNEXT;
        }
    }

    fn tprinter(&self, s: &[u8], len: usize) {
        unsafe {
            if iofd >= 0 && xwrite(iofd, s, len) < 0 {
                eprintln!("Error writing to output file");
                libc::close(iofd);
                iofd = -1;
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
                let x = self.c.x;
                let y = self.c.y;
                self.tmoveto(x.saturating_sub(1), y);
            }

            // CR
            b'\r' => {
                self.tmoveto(0, self.c.y);
            }

            0x0C  | // LF (\f)
            0x0B  | // VT (\v)
            b'\n'   // LF (\n)
            => {
                self.tnewline(self.mode.contains(TermMode::MODE_CRLF));
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
                self.charset = 1;
            }
            // SI (LS0 -- Locking shift 0)
            0x0f => {
                self.charset = 0;
            }

            // SUB
            0x1A => {
                let g = self.c.attr.clone();
                self.tsetchar('?', &g, self.c.x, self.c.y);
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
                self.tnewline(true);
                interrupt_sequence = true;
            },

            0x86 | // TODO:  SSA
            0x87   // TODO:  ESA
            => {
                interrupt_sequence = true;
            }

            // HTS -- Horizontal tab stop
            0x88 => {
                self.tabs[self.c.x] = 1;
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
                self.ttywrite(vtiden, vtiden.len(), false);
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
        let mut x = self.c.x;

        if count > 0 {
            while x < self.col && count > 0 {
                x += 1;
                while x < self.col && self.tabs[x] == 0 {
                    x += 1;
                }
            }
        } else if count < 0 {
            while x > 0 && count < 0 {
                x -= 1;
                while x > 0 && self.tabs[x] == 0 {
                    x -= 1;
                }
            }
        }

        self.c.x = x.min(self.col - 1)
    }

    fn tdefutf8(&mut self, u: char) {
        match u {
            'G' => self.mode.insert(TermMode::MODE_UTF8),
            '@' => self.mode.remove(TermMode::MODE_UTF8),
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
            self.trantbl[self.icharset as usize] = VCSMAP[idx];
        } else {
            eprintln!("esc unhandled charset: '{}'", u);
        }
    }

    fn tdectest(&mut self, c: char) {
        // DEC screen alignment test
        if c == '8' {
            for y in 0..self.row {
                for x in 0..self.col {
                    self.tsetchar('E', &self.c.attr.clone(), x, y);
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
                self.charset = 2 + (u as u8 - b'n') as usize;
            }
            '('| // GZD4 -- set primary charset G0
            ')'| // G1D4 -- set secondary charset G1
            '*'| // G2D4 -- set tertiary charset G2
            '+'  // G3D4 -- set quaternary charset G3
            => {
                self.icharset = (u as u8 - b'(') as u32;
                self.esc.insert(EscapeState::ESC_ALTCHARSET);
            }
            // IND -- Linefeed
            'D' => {
                if self.c.y == self.bot {
                    self.tscrollup(self.top, 1);
                } else {
                    self.tmoveto(self.c.x, self.c.y + 1);
                }
            }
            // NEL -- Next line
            'E' => {
                self.tnewline(true); // always go to first col
            }
            // HTS -- Horizontal tab stop
            'H' => {
                self.tabs[self.c.x] = 1;
            }
            // RI -- Reverse index
            'M' => {
                if self.c.y == self.top {
                    self.tscrolldown(self.top, 1);
                } else {
                    self.tmoveto(self.c.x, self.c.y - 1);
                }
            }
            // DECID -- Identify Terminal
            'Z' => {
                self.ttywrite(vtiden, vtiden.len(), false);
            }
            // RIS -- Reset to initial state
            'c' => {
                self.treset();
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
                self.tcursor(CursorMovement::CURSOR_SAVE);
            }
            // DESRC -- Restore Cursor
            '8' => {
                self.tcursor(CursorMovement::CURSOR_LOAD);
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
        let term_ptr: *mut Self = self;
        let win_ptr: *mut TermWindow = self.win;

        self.csiescseq.handle(term_ptr, win_ptr);
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
                libc::read(cmdfd, b.add(BUF_WRITTEN), BUF_SIZE - BUF_WRITTEN)
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
    fn xsetmode(&mut self, set: i32, mode: WinMode) {
        let win = unsafe { &mut *self.win };
        let mode = win.mode;

        if set != 0 {
            win.mode.insert(mode);
        } else {
            win.mode.remove(mode);
        }

        if (win.mode & WinMode::MODE_REVERSE) != (mode & WinMode::MODE_REVERSE) {
            self.redraw();
        }
    }

    // TODO:
    fn xloadcols(&self) {}

    pub fn tinsertblank(&mut self, n: usize) {
        let src = self.c.x;
        let dst = self.c.x + n;

        let size = self.col - dst;
        let line = &mut self.line[self.c.y];

        line.copy_within(src..src + size, dst);
        self.tclearregion(src, self.c.y, dst - 1, self.c.y);
    }

    pub fn tinsertblankline(&mut self, n: usize) {
        if BETWEEN!(self.c.y, self.top, self.bot) {
            self.tscrollup(self.c.y, n);
        }
    }

    pub fn tdumpline(&self, n: usize) {
        // TODO: implement this
        // char buf[UTF_SIZ];
        // const Glyph *bp, *end;
        //
        // bp  = &term.line[n][0];
        // end = &bp[MIN(tlinelen(n), term.col) - 1];
        // if (bp != end || bp->u != ' ') {
        // 	for (; bp <= end; ++bp) {
        // 		tprinter(buf, utf8encode(bp->u, buf));
        // 	}
        // }
        // tprinter("\n", 1);
    }

    pub fn tdump(&self) {
        for i in 0..self.row {
            self.tdumpline(i);
        }
    }

    pub fn tdumpsel(&self) {
        // TODO:
        // char *ptr;
        //
        // if ((ptr = getsel())) {
        // 	tprinter(ptr, strlen(ptr));
        // 	free(ptr);
        // }
    }

    pub fn tsetmode(&self, private: bool, set: i32, args: &[i32], narg: usize) {}

    pub fn tdeleteline(&mut self, n: usize) {
        if BETWEEN!(self.c.y, self.top, self.bot) {
            self.tscrollup(self.c.y, n);
        }
    }

    pub fn tdeletechar(&mut self, n: usize) {
        let n = n.min(self.col - self.c.x);

        let dst = self.c.x;
        let src = self.c.x + n;
        let size = self.col - src;
        let line = &mut self.line[self.c.y];

        // TODO: check if this is correct
        // memmove(&line[dst], &line[src], size * sizeof(Glyph));
        line.copy_within(src..src + size, dst);
        self.tclearregion(self.col - n, self.c.y, self.col - 1, self.c.y);
    }

    pub fn tsetattr(&mut self, attr: &[i32], l: usize) {
        let mut index = 0;

        let mut i = 0;
        while i < l {
            let a = attr[i] as u32;

            match a {
                0 => {
                    self.c.attr.mode &= !(GlyphAttribute::ATTR_BOLD
                        | GlyphAttribute::ATTR_FAINT
                        | GlyphAttribute::ATTR_ITALIC
                        | GlyphAttribute::ATTR_UNDERLINE
                        | GlyphAttribute::ATTR_BLINK
                        | GlyphAttribute::ATTR_REVERSE
                        | GlyphAttribute::ATTR_INVISIBLE
                        | GlyphAttribute::ATTR_STRUCK);

                    self.c.attr.fg = config::defaultfg;
                    self.c.attr.bg = config::defaultbg;
                    self.c.attr.decoration = DECOR_DEFAULT_COLOR;
                }

                1 => {
                    self.c.attr.mode |= GlyphAttribute::ATTR_BOLD;
                }

                2 => {
                    self.c.attr.mode |= GlyphAttribute::ATTR_FAINT;
                }

                3 => {
                    self.c.attr.mode |= GlyphAttribute::ATTR_ITALIC;
                }

                4 => {
                    self.c.attr.mode |= GlyphAttribute::ATTR_UNDERLINE;

                    if i + 1 < l {
                        i += 1;
                        let idx = attr[i] as u32;

                        if BETWEEN!(idx, 1, 5) {
                            let g = &mut self.c.attr as *mut Glyph;

                            self.tsetdecorstyle(g, idx);
                        } else if idx == 0 {
                            self.c.attr.mode.remove(GlyphAttribute::ATTR_UNDERLINE);

                            let g = &mut self.c.attr as *mut Glyph;
                            self.tsetdecorstyle(g, 0);
                        } else {
                            eprintln!("erresc: unknown underline style {}", idx);
                        }
                    }
                }

                // TODO: implement slow and rapid blink
                5 | // slow blink
                6   // rapid blink
                => {
                    self.c.attr.mode |= GlyphAttribute::ATTR_BLINK;
                }

                7 => {
                    self.c.attr.mode |= GlyphAttribute::ATTR_REVERSE;
                }

                8 => {
                    self.c.attr.mode |= GlyphAttribute::ATTR_INVISIBLE;
                }

                9 => {
                    self.c.attr.mode |= GlyphAttribute::ATTR_STRUCK;
                }

                22 => {
                    self.c.attr.mode.remove(GlyphAttribute::ATTR_BOLD | GlyphAttribute::ATTR_FAINT);
                }

                23 => {
                    self.c.attr.mode.remove(GlyphAttribute::ATTR_ITALIC);
                }

                24 => {
                    self.c.attr.mode.remove(GlyphAttribute::ATTR_UNDERLINE);

                    let g = &mut self.c.attr as *mut Glyph;
                    self.tsetdecorstyle(g, 0);
                }

                25 => {
                    self.c.attr.mode.remove(GlyphAttribute::ATTR_BLINK);
                }

                27 => {
                    self.c.attr.mode.remove(GlyphAttribute::ATTR_REVERSE);
                }

                28 => {
                    self.c.attr.mode.remove(GlyphAttribute::ATTR_INVISIBLE);
                }

                29 => {
                    self.c.attr.mode.remove(GlyphAttribute::ATTR_STRUCK);
                }

                38 => {
                    let idx = self.tdefcolor(&attr, &mut i, l);

                    if idx >= 0 {
                        self.c.attr.fg = idx as u32;
                    }
                }

                39 => {
                    self.c.attr.fg = config::defaultfg;
                }

                48 => {
                    let idx = self.tdefcolor(&attr, &mut i, l);

                    if idx >= 0 {
                        self.c.attr.bg = idx as u32;
                    }
                }

                49 => {
                    self.c.attr.bg = config::defaultbg;
                }

                // underline decoration color
                58 => {
                    let idx = self.tdefcolor(&attr, &mut i, l);

                    if idx >= 0 {
                        let g = &mut self.c.attr as *mut Glyph;
                        self.tsetdecorcolor(g, idx as u32);
                    }
                }

                59 => {
                    let g = &mut self.c.attr as *mut Glyph;
                    self.tsetdecorcolor(g, DECOR_DEFAULT_COLOR);
                }

                _ => {
                    if BETWEEN!(a, 30, 37) {
                        self.c.attr.bg = a - 30;
                    } else if BETWEEN!(a, 40, 47) {
                        self.c.attr.fg = a - 40;
                    } else if BETWEEN!(a, 90, 97) {
                        self.c.attr.bg = a - 90 + 8;
                    } else if BETWEEN!(a, 100, 107) {
                        self.c.attr.fg = a - 100 + 8;
                    } else {
                        eprintln!("erresc(default): gfx attr {} unkwnon", a);
                        // TODO: CSI DUMP
                    }
                }
            }

            i += 1;
        }
    }

    fn tsetdecorstyle(&self, g: *mut Glyph, style: u32) {
        let g = unsafe { &mut *g };
        g.decoration = (g.decoration & !(0x7 << 25)) | ((style & 0x7) << 25);
    }

    fn tdefcolor(&self, attr: &[i32], npar: &mut usize, l: usize) -> i32 {
        let mut idx = -1;
        let mut r = 0;
        let mut g = 0;
        let mut b = 0;

        match attr[*npar + 1] {
            // direct color in RGB space
            2 if *npar + 4 >= l => {
                eprintln!("erresc(38): Incorrect number of parameters ({})", *npar);
            }
            2 => {
                if attr[*npar] == 58 {
                    r = attr[*npar + 3] as u32;
                    g = attr[*npar + 4] as u32;
                    b = attr[*npar + 5] as u32;

                    *npar += 5;
                } else {
                    r = attr[*npar + 2] as u32;
                    g = attr[*npar + 3] as u32;
                    b = attr[*npar + 4] as u32;

                    *npar += 4;
                }

                let valid_color = r <= 255 && g <= 255 && b <= 255;
                if !valid_color {
                    eprintln!("erresc: invalid rgb color ({}, {}, {})", r, g, b);
                } else {
                    idx = TRUECOLOR(r as u8, g as u8, b as u8) as i32;
                }
            }

            // indexed color
            5 if *npar + 2 >= l => {
                eprintln!("erresc(38): Incorrect number of parameters ({})", *npar);
            }
            5 => {
                *npar += 2;

                if !BETWEEN!(attr[*npar], 0, 255) {
                    eprintln!("erresc: bad fgcolor ({})", attr[*npar]);
                } else {
                    idx = attr[*npar];
                }
            }

            0 | // Implemented defined (only foreground)
            1 | // TODO: transparent
            3 | // direct color in CMY space
            4 | // direct color in CMYK space
            _ => {
                eprintln!("erresc(38): gfx attr {} unkown", attr[*npar]);
            }
        }

        idx
    }

    fn tsetdecorcolor(&self, g: *mut Glyph, color: u32) {
        let g = unsafe { &mut *g };
        g.decoration = (g.decoration & !0x1ffffff) | (color & 0x1ffffff);
    }

    fn redraw(&mut self) {
        self.tfulldirt();
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
        let r = libc::execvp(sh, args.as_ptr());
        println!("execvp failed: {}", std::io::Error::last_os_error());
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

fn tgetdecorcolor(g: &Glyph) -> u32 {
    todo!()
}

fn tsetimgplacementid(g: &Glyph, placement_id: usize) {
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
    image_id: u32,
    placement_id: u32,
    col: u32,
    row: u32,
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
