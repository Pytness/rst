use std::ptr::null;

use bitflags::bitflags;
use unicode_width::UnicodeWidthChar;

use crate::BETWEEN;
use crate::boxdraw::boxdraw::isboxdraw;
use crate::config;
use crate::glyph::{Glyph, GlyphAttribute};

pub static mut IOFD: i32 = 0;
pub static mut CMDFD: i32 = 0;
pub static mut PID: i32 = 0;
pub static mut SU: usize = 0;
pub static mut twrite_aborted: bool = false;

pub const DECOR_DEFAULT_COLOR: u32 = 0x0FFFFFF;
pub const IMAGE_PLACEHOLDER_CHAR: char = '\u{10EEEE}';
pub const IMAGE_PLACEHOLDER_CHAR_OLD: char = '\u{EEEE}';

pub fn TRUECOLOR(r: u8, g: u8, b: u8) -> u32 {
    1 << 24 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

pub fn IS_TRUECOL(c: u32) -> bool {
    (c & (1 << 24)) != 0
}

bitflags! {
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TermMode: u32 {
        const MODE_WRAP         = 1 << 0;
        const MODE_INSERT       = 1 << 1;
        const MODE_ALTSCREEN    = 1 << 2;
        const MODE_CRLF         = 1 << 3;
        const MODE_ECHO         = 1 << 4;
        const MODE_PRINT        = 1 << 5;
        const MODE_UTF8         = 1 << 6;
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
        const CURSOR_DEFAULT  = 0;
        const CURSOR_WRAPNEXT = 1;
        const CURSOR_ORIGIN   = 2;
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub(crate) enum Charset {
    #[default]
    CS_GRAPHIC0 = 0,
    CS_GRAPHIC1 = 1,
    CS_UK = 2,
    CS_USA = 3,
    CS_MULTI = 4,
    CS_GER = 5,
    CS_FIN = 6,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Vec2 {
    pub x: isize,
    pub y: isize,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    #[default]
    SEL_IDLE = 0,
    SEL_EMPTY = 1,
    SEL_READY = 2,
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
    pub mode: SelectionMode,
    pub type_: SelectionType,
    pub snap: i32,
    /*
     * Selection variables:
     * nb – normalized coordinates of the beginning of the selection
     * ne – normalized coordinates of the end of the selection
     * ob – original coordinates of the beginning of the selection
     * oe – original coordinates of the end of the selection
     */
    pub nb: Vec2,
    pub ne: Vec2,
    pub ob: Vec2,
    pub oe: Vec2,
    pub alt: bool,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct TCursor {
    pub attr: Glyph,
    pub x: usize,
    pub y: usize,
    pub state: CursorState,
}

pub type Line = Box<[Glyph]>;

#[derive(Debug, Clone, Copy)]
pub struct Image {
    pub x: usize,
    pub y: usize,
    pub cols: usize,
    pub rows: usize,
}

// ── TermState ─────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct TermState {
    pub row: usize,             // number of rows
    pub col: usize,             // number of columns
    pub pixw: usize,            // width of the text area in pixels
    pub pixh: usize,            // height of the text area in pixels
    pub line: Vec<Line>,        // the main screen buffer
    pub alt: Vec<Line>,         // the alternate screen buffer
    pub dirty: Vec<bool>,       // dirtyness of lines
    pub c: TCursor,             // cursor
    pub ocx: usize,             // old cursor col
    pub ocy: usize,             // old cursor row
    pub top: usize,             // top scroll limit
    pub bot: usize,             // bottom scroll limit
    pub mode: TermMode,         // terminal mode flags
    pub trantbl: [Charset; 4],  // charset table translation
    pub charset: usize,         // current charset
    pub icharset: u32,          // selected charset for sequence
    pub images: Vec<Image>,     // Sixel images
    pub images_alt: Vec<Image>, // Sixel images in the alternate screen
    pub lastc: char,            // last printed char outside of sequence, 0 if control
    pub sel: Selection,
    pub tabs: Vec<u8>,
}

impl TermState {
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

            for _ in 0..2 {
                if self.c.y >= row {
                    self.tscrollup(0, self.c.y - row + 1);
                }

                for _j in row..self.row {
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

        for _ in 0..2 {
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

        for _ in 0..2 {
            for image in &self.images {
                if image.y >= self.row {
                    // TODO:  delete_image(image);
                    continue;
                }

                let _line = self.line[image.y].as_mut();
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
        self.pixw = tw;
        self.pixh = th;

        let w = libc::winsize {
            ws_row: self.row as u16,
            ws_col: self.col as u16,
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

    /// Write bytes directly to the pty (no echo processing).
    pub fn ttywrite_pty(&mut self, buffer: &[u8], len: usize) {
        if !self.mode.contains(TermMode::MODE_CRLF) {
            self.ttywriteraw_pty(buffer, len);
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

                self.ttywriteraw_pty(&buffer[i..], next - i);
                i = next;
            }
        }
    }

    fn ttywriteraw_pty(&mut self, buffer: &[u8], len: usize) {
        let mut wfd: libc::fd_set = unsafe { std::mem::zeroed() };
        let mut rfd: libc::fd_set = unsafe { std::mem::zeroed() };

        let mut n = len;
        let mut s: *const libc::c_void = buffer.as_ptr() as *const libc::c_void;
        let lim: usize = 256;
        let mut retries = 100;

        while n > 0 {
            retries -= 1;
            if retries <= 0 {
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
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                ) < 0
                {
                    if *libc::__errno_location() == libc::EINTR {
                        continue;
                    }
                    panic!("select failed: {}", std::io::Error::last_os_error());
                }
                if libc::FD_ISSET(CMDFD, &mut wfd) {
                    let count = n.min(lim);
                    let r = libc::write(CMDFD, s, count);
                    if r < 0 {
                        panic!("write failed on tty: {}", std::io::Error::last_os_error());
                    }
                    if r < n as isize {
                        n -= r as usize;
                        s = s.add(r as usize);
                    } else {
                        break;
                    }
                }
            }
        }
    }
}

// ── free helper functions ─────────────────────────────────────────────────────

pub(crate) unsafe fn tprinter(s: &[u8], len: usize) {
    if IOFD >= 0 && xwrite(IOFD, s, len) < 0 {
        eprintln!("Error writing to output file");
        libc::close(IOFD);
        IOFD = -1;
    }
}

pub(crate) fn xwrite(fd: i32, buffer: &[u8], len: usize) -> isize {
    let total = len;
    let mut remaining = len;
    while remaining > 0 {
        let s = buffer[total - remaining..].as_ptr() as *const libc::c_void;
        let r = unsafe { libc::write(fd, s, remaining) };
        if r < 0 {
            return r;
        }
        remaining -= r as usize;
    }
    total as isize
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
    ((g.u as usize) >> 29) & 0x1 != 0
}

fn tsetimgrow(g: &mut Glyph, row: usize) {
    let v = (g.u as u32 & !0x1ff) | (row as u32 & 0x1ff);
    g.u = char::from_u32(v).unwrap_or('\0');
}
fn tsetimgcol(g: &mut Glyph, col: usize) {
    let v = (g.u as u32 & !(0x1ff << 9)) | ((col as u32 & 0x1ff) << 9);
    g.u = char::from_u32(v).unwrap_or('\0');
}
fn tsetimg4thbyteplus1(g: &mut Glyph, byteplus1: u32) {
    let v = (g.u as u32 & !(0x1ff << 18)) | ((byteplus1 & 0x1ff) << 18);
    g.u = char::from_u32(v).unwrap_or('\0');
}
fn tsetimgdiacriticcount(g: &mut Glyph, count: i32) {
    let v = (g.u as u32 & !(0x3 << 27)) | (((count as u32) & 0x3) << 27);
    g.u = char::from_u32(v).unwrap_or('\0');
}
fn tsetisclassicplaceholder(g: &mut Glyph, is_classic: i32) {
    let v = (g.u as u32 & !(0x1 << 29)) | (((is_classic as u32) & 0x1) << 29);
    g.u = char::from_u32(v).unwrap_or('\0');
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
