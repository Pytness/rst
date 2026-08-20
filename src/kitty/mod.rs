use crate::BETWEEN;
use crate::glyph::Glyph;
use crate::term_state::TRUECOLOR;

const DECOR_DEFAULT_COLOR: u32 = 0x0FFFFFF;

fn gr_get_glyph_underneath_image(
    _image_id: u32,
    _placement_id: u32,
    _col: u32,
    _row: u32,
) -> Option<&'static Glyph> {
    todo!()
}

pub fn tsetdecorcolor(g: &mut Glyph, color: u32) {
    g.decoration = (g.decoration & !0x1FFFFFF) | (color & 0x1FFFFFF);
}

pub fn tsetdecorstyle(g: &mut Glyph, style: u32) {
    g.decoration = (g.decoration & !(0x7 << 25)) | ((style & 0x7) << 25);
}

pub fn tdefcolor(attr: &[i32], npar: &mut usize, l: usize) -> i32 {
    let mut idx = -1;
    let r;
    let g;
    let b;

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
