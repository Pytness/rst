use std::ptr::null_mut;

use crate::boxdraw::boxdraw::isboxdraw;
use crate::glyph::{Glyph, GlyphAttribute};
use crate::{BETWEEN, config};
use bitflags::bitflags;
use libc::{getenv, pselect};

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

const DECOR_DEFAULT_COLOR: u32 = 0x0FFFFFF;
const IMAGE_PLACEHOLDER_CHAR: char = '\u{10EEEE}';
const IMAGE_PLACEHOLDER_CHAR_OLD: char = '\u{EEEE}';

// TODO: handle globals properly
static mut su: usize = 0;
static mut twrite_aborted: bool = false;

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
enum CursorMovement {
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

enum EscapeState {
    ESC_START = 1,
    ESC_CSI = 2,
    ESC_STR = 4, /* DCS, OSC, PM, APC */
    ESC_ALTCHARSET = 8,
    ESC_STR_END = 16, /* a final string was encountered */
    ESC_TEST = 32,    /* Enter in test mode */
    ESC_UTF8 = 64,
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

#[derive(Default, Debug)]
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
    pixw: usize,
    pixh: usize,
    pub line: Vec<Line>,
    alt: Vec<Line>,
    pub dirty: Vec<bool>,
    pub c: TCursor,
    pub ocx: usize,
    pub ocy: usize,
    top: usize,
    bot: usize,
    pub mode: TermMode,
    esc: u32,
    trantbl: [Charset; 4],
    charset: usize,
    icharset: u32,
    tabs: Vec<usize>,
    images: Vec<Image>,
    images_alt: Vec<Image>,
    lastc: char,

    // fields added on rewrite
    sel: Selection,
}

impl Term {
    pub fn new(col: usize, row: usize) -> Self {
        let mut term = Term {
            c: TCursor::default(),
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

    fn tscrollup(&mut self, orig: usize, n: usize) {
        // ImageList *im, *next;

        let n = n.min(self.bot - orig + 1);

        self.tclearregion(0, orig, self.col - 1, orig + n - 1);
        self.tsetdirt(orig + n, self.bot);

        for i in orig..(self.bot - n) {
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

    pub fn tnewline(&mut self, first_col: usize) {
        let mut y = self.c.y;

        if y == self.bot {
            self.tscrollup(self.top, 1);
        } else {
            y += 1;
        }

        let col = if first_col <= 0 { 0 } else { self.c.x };

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
                self.tnewline(0);
            }
        }

        if do_not_move_cursor {
            self.tmoveto(self.c.x, self.c.y - rows + 1);
        } else {
            // Move the cursor beyond the last column, as required by the
            // protocol. If the cursor goes beyond the screen edge, insert a
            // newline to match the behavior of kitty.
            if self.c.x + cols >= self.col {
                self.tnewline(1);
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

        // resize each row to new width, zero-pad if needed
        // for y in 0..minrow {
        //     self.line[y].resize(col, Glyph::default());
        //     self.alt[y].resize(col, Glyph::default());
        // }

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

    fn tdeleteimages(&self) {
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

        // TODO:
        // struct winsize w;
        // //
        // w.ws_row    = term.row;
        // w.ws_col    = term.col;
        // w.ws_xpixel = tw;
        // w.ws_ypixel = th;
        // if (ioctl(cmdfd, TIOCSWINSZ, &w) < 0) {
        // 	fprintf(stderr, "Couldn't set window size: %s\n", strerror(errno));
        // }
    }

    pub fn ttywrite(&self, buffer: &[u8], len: usize, may_echo: bool) {
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

    fn twrite(&self, buffer: &[u8], buflen: usize, show_ctrl: bool) -> usize {
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

    fn ttywriteraw(&self, buffer: &[u8], len: usize) {
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

                if libc::FD_ISSET(cmdfd, &mut rfd) {
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

                if libc::FD_ISSET(cmdfd, &mut wfd) {
                    lim = self.ttyread();
                }
            }
        }
    }

    fn tputc(&self, arg: char) {
        // println!("tputc called with '{}'", arg);
    }

    pub fn ttyread(&self) -> usize {
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
            println!("ttyread after read");

            println!(
                "read: {:?}",
                &BUF[BUF_WRITTEN..(BUF_WRITTEN + ret as usize)]
            );

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
}

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

fn gr_get_glyph_underneath_image(
    image_id: u32,
    placement_id: u32,
    col: u32,
    row: u32,
) -> Option<&'static Glyph> {
    todo!()
}
