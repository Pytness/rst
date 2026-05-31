use crate::BETWEEN;
use crate::glyph::Glyph;
use crate::term_state::TRUECOLOR;

const DECOR_DEFAULT_COLOR: u32 = 0x0FFFFFF;

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

fn tsetimgplacementid(_g: &Glyph, _placement_id: usize) {
    todo!()
}

fn gr_get_glyph_underneath_image(
    _image_id: u32,
    _placement_id: u32,
    _col: u32,
    _row: u32,
) -> Option<&'static Glyph> {
    todo!()
}

pub fn tgetdecorcolor(g: &Glyph) -> u32 {
    g.decoration as u32 & 0x1FFFFFF
}

pub fn tgetdecorstyle(g: &Glyph) -> u32 {
    (g.decoration as u32 >> 25) & 0x7
}

pub fn tsetdecorcolor(g: &mut Glyph, color: u32) {
    g.decoration = (g.decoration & !0x1FFFFFF) | (color & 0x1FFFFFF);
}

pub fn tsetdecorstyle(g: &mut Glyph, style: u32) {
    g.decoration = (g.decoration & !(0x7 << 25)) | ((style & 0x7) << 25);
}

pub fn tdefcolor(attr: &[i32], npar: &mut usize, l: usize) -> i32 {
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
