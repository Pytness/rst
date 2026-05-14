use bitflags::bitflags;

use crate::font_registry::FontStyle;

bitflags! {
    pub struct TermGlyphMode: u8 {
        const Normal = 0;
        // const Bold = 1 << 0;
        // const Italic = 1 << 1;
        // const Underline = 1 << 2;
        // const Blink = 1 << 3;
        // const Inverse = 1 << 4;
        // const Invisible = 1 << 5;
    }
}
pub enum TermGlyphDecoration {}

#[derive(Clone, Copy)]
pub struct TermGlyph {
    pub char: char,
    pub fg_color: (u8, u8, u8),
    pub bg_color: (u8, u8, u8),
    pub font_style: FontStyle,
    // pub mode: TermGlyphMode,
    // pub decoration: TermGlyphDecoration,
}

pub struct CellPosition {
    pub x: i32,
    pub y: i32,
}

pub struct CellBox {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub struct TextManager {
    pub font_width: i32,
    pub font_height: i32,
    pub window_width: i32,
    pub window_height: i32,
    pub rows: i32,
    pub cols: i32,
    min_border_px: i32,
    pub border_x_px: i32,
    pub border_y_px: i32,
}

impl TextManager {
    pub fn new(
        font_width: i32,
        font_height: i32,
        window_width: i32,
        window_height: i32,
        min_border_px: i32,
    ) -> Self {
        let cols = (window_width - 2 * min_border_px) / font_width;
        let rows = (window_height - 2 * min_border_px) / font_height;

        let border_y_px = (window_height - (rows * font_height)) / 2;
        let border_x_px = (window_width - (cols * font_width)) / 2;
        Self {
            font_width,
            font_height,
            window_width,
            window_height,
            rows,
            cols,
            min_border_px,
            border_x_px,
            border_y_px,
        }
    }

    pub fn set_window_size(&mut self, width: i32, height: i32) {
        let cols = (width - 2 * self.min_border_px) / self.font_width;
        let rows = (height - 2 * self.min_border_px) / self.font_height;

        self.border_y_px = (height - (rows * self.font_height)) / 2;
        self.border_x_px = (width - (cols * self.font_width)) / 2;
        self.window_width = width;
        self.window_height = height;
        self.rows = rows;
        self.cols = cols;
    }

    pub fn get_cell_position(&self, row: i32, col: i32) -> CellPosition {
        let x = col * self.font_width + self.border_x_px;
        // Add font_height to y to account for rendering starting from the baseline,
        // so we want to position the text such that it fits within the cell.
        let y = row * self.font_height + self.font_height + self.border_y_px;

        CellPosition { x, y }
    }

    pub fn get_cell_box(&self, row: i32, col: i32) -> CellBox {
        let CellPosition { x, y } = self.get_cell_position(row, col);

        CellBox {
            x,
            y,
            width: self.font_width,
            height: self.font_height,
        }
    }
}
