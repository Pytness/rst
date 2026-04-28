use bitflags::bitflags;

bitflags! {
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
    pub struct GlyphAttribute: u32 {
        const ATTR_NULL = 0;
        const ATTR_BOLD = 1 << 0;
        const ATTR_FAINT = 1 << 1;
        const ATTR_ITALIC = 1 << 2;
        const ATTR_UNDERLINE = 1 << 3;
        const ATTR_BLINK = 1 << 4;
        const ATTR_REVERSE = 1 << 5;
        const ATTR_INVISIBLE = 1 << 6;
        const ATTR_STRUCK = 1 << 7;
        const ATTR_WRAP = 1 << 8;
        const ATTR_WIDE = 1 << 9;
        const ATTR_WDUMMY = 1 << 10;
        const ATTR_BOXDRAW = 1 << 11;
        const ATTR_BOLD_FAINT = Self::ATTR_BOLD.bits() | Self::ATTR_FAINT.bits();
        const ATTR_IMAGE = 1 << 14;
        const ATTR_URL = 1 << 15;
        const ATTR_SIXEL = 1 << 16;

    }
}

pub enum SelectionMode {
    SelIdle,
    SelEmpty,
    SelReady,
}

pub enum SelectionType {
    SelRegular,
    SelRectangular,
}

pub enum SelectionSnap {
    SnapWord,
    SnapLine,
}

pub enum UnderlineStyle {
    UnderlineStraight = 1,
    UnderlineDouble = 2,
    UnderlineCurly = 3,
    UnderlineDotted = 4,
    UnderlineDashed = 5,
}

#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    pub u: char,              // character code
    pub mode: GlyphAttribute, // attributes
    pub fg: u32,              // foreground color
    pub bg: u32,              // background color
    pub decoration: u32,      // used for underline color and style
}

impl Default for Glyph {
    fn default() -> Self {
        Self {
            u: '\0',
            mode: GlyphAttribute::ATTR_NULL,
            fg: 0,
            bg: 0,
            decoration: 0,
        }
    }
}

pub struct Decoration(u32);

impl Decoration {
    const COLOR_MASK: u32 = 0x1ffffff; // 25 bits for color

    pub fn new(color: u32, style: UnderlineStyle) -> Self {
        Self((color & Self::COLOR_MASK) | ((style as u32 & 0x7) << 25))
    }

    fn style_bits(&self) -> u32 {
        (self.0 >> 25) & 0x7
    }

    pub fn color(&self) -> u32 {
        self.0 & Self::COLOR_MASK
    }

    pub fn set_color(&mut self, color: u32) {
        self.0 = (self.0 & !Self::COLOR_MASK) | (color & Self::COLOR_MASK);
    }

    pub fn style(&self) -> UnderlineStyle {
        match self.style_bits() {
            1 => UnderlineStyle::UnderlineStraight,
            2 => UnderlineStyle::UnderlineDouble,
            3 => UnderlineStyle::UnderlineCurly,
            4 => UnderlineStyle::UnderlineDotted,
            5 => UnderlineStyle::UnderlineDashed,
            _ => UnderlineStyle::UnderlineStraight, // Default to straight if invalid
        }
    }

    pub fn set_style(&mut self, style: UnderlineStyle) {
        self.0 = (self.0 & !(0x7 << 25)) | ((style as u32 & 0x7) << 25);
    }
}

// Accessors to decoration properties stored in `decor`.
// The 25-th bit is used to indicate if it's a 24-bit color.
// static inline uint32_t tgetdecorcolor(Glyph *g) { return g->decor & 0x1ffffff; }
// static inline uint32_t tgetdecorstyle(Glyph *g) { return (g->decor >> 25) & 0x7; }
// static inline void tsetdecorcolor(Glyph *g, uint32_t color) {
// 	g->decor = (g->decor & ~0x1ffffff) | (color & 0x1ffffff);
// }
// static inline void tsetdecorstyle(Glyph *g, uint32_t style) {
// 	g->decor = (g->decor & ~(0x7 << 25)) | ((style & 0x7) << 25);
// }
