use crate::glyph::{Glyph, GlyphAttribute};
use crate::{BETWEEN, config};
use bitflags::bitflags;

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

enum Charset {
    CS_GRAPHIC0,
    CS_GRAPHIC1,
    CS_UK,
    CS_USA,
    CS_MULTI,
    CS_GER,
    CS_FIN,
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
    attr: Glyph, // current char attributes
    x: usize,
    y: usize,
    state: CursorState,
}

// Temp structs
pub type Line = Box<[Glyph]>;

#[derive(Debug, Clone, Copy)]
pub struct Image;

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
    row: usize,
    col: usize,
    pixw: usize,
    pixh: usize,
    line: Vec<Line>,
    alt: Vec<Line>,
    dirty: Vec<bool>,
    c: TCursor,
    ocx: usize,
    ocy: usize,
    top: usize,
    bot: usize,
    mode: TermMode,
    esc: u32,
    trantbl: [u8; 4],
    charset: u32,
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

        self.trantbl = [Charset::CS_USA as u8; 4];
        self.charset = 0;

        for i in 0..2 {
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

    pub fn tresize(&mut self, col: usize, row: usize) {
        let minrow = row.min(self.row);
        let mincol = col.min(self.col);

        if col < 1 || row < 1 {
            // ERR: invalid size
            return;
        }

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

    fn tclearregion(&mut self, x1: usize, y1: usize, x2: usize, y2: usize) {
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

    fn selected(&self, x: usize, y: usize) -> bool {
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
        todo!()
    }
}
