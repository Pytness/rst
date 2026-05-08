use super::data::{
    BBD, BBL, BBQ, BBR, BBS, BBU, BDA, BDB, BDL, BL, BOXDATA, BR, BRL, BrailleDots, DD, DL, DR, DU,
    LD, LL, LR, LU, TL, TR,
};

macro_rules! mix_shade {
    ($a: expr, $b: expr, $d: expr) => {
        ($a * $d + $b * (4 - $d)) / 4
    };
}

pub struct RenderColor {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
    pub alpha: u16,
}

pub struct XftColor {
    pub pixel: u32,
    pub color: RenderColor,
}

// TODO: either make this configurable or remove it after copy boxdraw.c.
const BOXDRAW: bool = true;
const BOXDRAW_BRAILLE: bool = true;

fn as_block(u: char) -> u32 {
    u as u32 & !0xff
}

pub fn isboxdraw(u: char) -> bool {
    let block = as_block(u);

    (block == 0x2500 && BOXDATA[u as usize] != 0) || block == 0x2800
}

pub fn boxdrawindex(u: char) -> u32 {
    let block = as_block(u);
    let u = u as u32;

    if BOXDRAW_BRAILLE && block == 0x2800 {
        return BRL | u;
    }

    if BOXDRAW && block == 0x2500 {
        return BDB | BOXDATA[u as usize];
    }

    return BOXDATA[u as usize];
}

pub fn drawboxes(
    x: usize,
    y: usize,
    cw: usize,
    ch: usize,
    fgcolor: u32,
    bgcolor: u32,
    specs: (),
    len: usize,
) {
    let mut x = x;
    for i in 0..len {
        // TODO: advance specs (FontSpec *specs, specs++)

        // TODO:  drawbox(x, y, cw, ch, fg, bg, (ushort)specs->glyph);
        // drawbox(x, y , cw, ch, fgcolor, bgcolor, );
    }
}

fn drawbox(x: usize, y: usize, w: usize, h: usize, fg: XftColor, bg: XftColor, glyph: char) {
    let w = w as u32;
    let h = h as u32;

    let bd = glyph as u32;
    let cat = bd & !(BDB | 0xff);

    if (bd & (BDL | BDA)) != 0 {
        /* lines (light/double/heavy/arcs) */
        drawboxlines(x as i32, y as i32, w as i32, h as i32, &fg, bd);
    } else if cat == BBD {
        /* lower (8-X)/8 block */
        let d = (bd * h) / 8;
        // XftDrawRect(xd, fg, x, y + d, w, h - d);
    } else if cat == BBU {
        /* upper X/8 block */
        // XftDrawRect(xd, fg, x, y, w, (bd * h)/ 8);
    } else if cat == BBL {
        /* left X/8 block */
        // XftDrawRect(xd, fg, x, y, (bd * w)/ 8, h);
    } else if cat == BBR {
        /* right (8-X)/8 block */
        let d = (bd * w) / 8;
        // XftDrawRect(xd, fg, x + d, y, w - d, h);
    } else if cat == BBQ {
        /* Quadrants */
        let w2 = w / 2;
        let h2 = h / 2;
        if (bd & TL) != 0 {
            // XftDrawRect(xd, fg, x, y, w2, h2);
        }
        if (bd & TR) != 0 {
            // XftDrawRect(xd, fg, x + w2, y, w - w2, h2);
        }
        if (bd & BL) != 0 {
            // XftDrawRect(xd, fg, x, y + h2, w2, h - h2);
        }
        if (bd & BR) != 0 {
            // XftDrawRect(xd, fg, x + w2, y + h2, w - w2, h - h2);
        }
    } else if (bd & BBS) != 0 {
        /* Shades - data is 1/2/3 for 25%/50%/75% alpha, respectively */
        let d = bd as u16;
        // let xfc; //:XftColor
        let mut xrc: RenderColor = RenderColor {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0,
        }; // XRenderColor: xrc = {.alpha = 0xffff};

        xrc.red = mix_shade!(fg.color.red, bg.color.red, d);
        xrc.green = mix_shade!(fg.color.green, bg.color.green, d);
        xrc.blue = mix_shade!(fg.color.blue, bg.color.blue, d);

        // XftColorAllocValue(xdpy, xvis, xcmap, &xrc, &xfc);
        // XftDrawRect(xd, &xfc, x, y, w, h);
        // XftColorFree(xdpy, xvis, xcmap, &xfc);
    } else if cat == BRL {
        /* braille, each data bit corresponds to one dot at 2x4 grid */
        let w1 = (w) / 2;
        let h1 = (h) / 4;
        let h2 = (h) / 2;
        let h3 = (3 * h) / 4;

        if bd & BrailleDots::BRAILLE_TOP_LEFT as u32 != 0 {
            // XftDrawRect(xd, fg, x, y, w1, h1);
        }
        if (bd & BrailleDots::BRAILLE_MIDDLE_LEFT as u32) != 0 {
            // XftDrawRect(xd, fg, x, y + h1, w1, h2 - h1);
        }
        if (bd & BrailleDots::BRAILLE_BOTTOM_LEFT as u32) != 0 {
            // XftDrawRect(xd, fg, x, y + h2, w1, h3 - h2);
        }
        if (bd & BrailleDots::BRAILLE_TOP_RIGHT as u32) != 0 {
            // XftDrawRect(xd, fg, x + w1, y, w - w1, h1);
        }
        if (bd & BrailleDots::BRAILLE_MIDDLE_RIGHT as u32) != 0 {
            // XftDrawRect(xd, fg, x + w1, y + h1, w - w1, h2 - h1);
        }
        if (bd & BrailleDots::BRAILLE_BOTTOM_RIGHT as u32) != 0 {
            // XftDrawRect(xd, fg, x + w1, y + h2, w - w1, h3 - h2);
        }
        if (bd & BrailleDots::BRAILLE_LOWER_LEFT as u32) != 0 {
            // XftDrawRect(xd, fg, x, y + h3, w1, h - h3);
        }
        if (bd & BrailleDots::BRAILLE_LOWER_RIGHT as u32) != 0 {
            // XftDrawRect(xd, fg, x + w1, y + h3, w - w1, h - h3);
        }
    }
}

/// Integer division with rounding (rounds to nearest, ties round up).
#[inline]
fn div_round(n: i32, d: i32) -> i32 {
    (n + d / 2) / d
}

// TODO: move this to an opengl shader
fn drawboxlines(x: i32, y: i32, w: i32, h: i32, fg: &XftColor, bd: u32) {
    /* s: stem thickness. width/8 roughly matches underscore thickness. */
    /* We draw bold as 1.5 * normal-stem and at least 1px thicker.      */
    /* doubles draw at least 3px, even when w or h < 3. bold needs 6px. */
    let mwh = w.min(h);
    let base_s = 1_i32.max(div_round(mwh, 8));
    let bold = (bd & BDB) != 0 && mwh >= 6; /* possibly ignore boldness */
    let s = if bold {
        (base_s + 1).max(div_round(3 * base_s, 2))
    } else {
        base_s
    };
    let w2 = div_round(w - s, 2);
    let h2 = div_round(h - s, 2);
    /* the s-by-s square (x + w2, y + h2, s, s) is the center texel.   */
    /* The base length (per direction till edge) includes this square.  */

    let light = bd & (LL | LU | LR | LD);
    let double_ = bd & (DL | DU | DR | DD);

    if light != 0 {
        /* d: additional (negative) length to not-draw the center       */
        /* texel - at arcs and avoid drawing inside (some) doubles      */
        let arc = (bd & BDA) != 0;
        let multi_light = light & (light - 1);
        let multi_double = double_ & (double_ - 1);
        /* light crosses double only at DH+LV, DV+LH (ref. shapes)     */
        let d: i32 = if arc || (multi_double != 0 && multi_light == 0) {
            -s
        } else {
            0
        };

        if (bd & LL) != 0 {
            // XftDrawRect(xd, fg, x, y + h2, w2 + s + d, s);
        }
        if (bd & LU) != 0 {
            // XftDrawRect(xd, fg, x + w2, y, s, h2 + s + d);
        }
        if (bd & LR) != 0 {
            // XftDrawRect(xd, fg, x + w2 - d, y + h2, w - w2 + d, s);
        }
        if (bd & LD) != 0 {
            // XftDrawRect(xd, fg, x + w2, y + h2 - d, s, h - h2 + d);
        }
    }

    /* double lines - also align with light to form heavy when combined */
    if double_ != 0 {
        /*
         * going clockwise, for each double-ray: p is additional length
         * to the single-ray nearer to the previous direction, and n to
         * the next. p and n adjust from the base length to lengths
         * which consider other doubles - shorter to avoid intersections
         * (p, n), or longer to draw the far-corner texel (n).
         */
        let dl = bd & DL;
        let du = bd & DU;
        let dr = bd & DR;
        let dd = bd & DD;

        if dl != 0 {
            let p: i32 = if dd != 0 { -s } else { 0 };
            let n: i32 = if du != 0 {
                -s
            } else if dd != 0 {
                s
            } else {
                0
            };
            // XftDrawRect(xd, fg, x, y + h2 + s, w2 + s + p, s);
            // XftDrawRect(xd, fg, x, y + h2 - s, w2 + s + n, s);
        }
        if du != 0 {
            let p: i32 = if dl != 0 { -s } else { 0 };
            let n: i32 = if dr != 0 {
                -s
            } else if dl != 0 {
                s
            } else {
                0
            };
            // XftDrawRect(xd, fg, x + w2 - s, y, s, h2 + s + p);
            // XftDrawRect(xd, fg, x + w2 + s, y, s, h2 + s + n);
        }
        if dr != 0 {
            let p: i32 = if du != 0 { -s } else { 0 };
            let n: i32 = if dd != 0 {
                -s
            } else if du != 0 {
                s
            } else {
                0
            };
            // XftDrawRect(xd, fg, x + w2 - p, y + h2 - s, w - w2 + p, s);
            // XftDrawRect(xd, fg, x + w2 - n, y + h2 + s, w - w2 + n, s);
        }
        if dd != 0 {
            let p: i32 = if dr != 0 { -s } else { 0 };
            let n: i32 = if dl != 0 {
                -s
            } else if dr != 0 {
                s
            } else {
                0
            };
            // XftDrawRect(xd, fg, x + w2 + s, y + h2 - p, s, h - h2 + p);
            // XftDrawRect(xd, fg, x + w2 - s, y + h2 - n, s, h - h2 + n);
        }
    }
}
