use crate::glyph::Glyph;

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

fn tgetdecorcolor(g: &Glyph) -> u32 {
    g.decoration as u32 & 0x1FFFFFF
}

fn tgetdecorstyle(g: &Glyph) -> u32 {
    (g.decoration as u32 >> 25) & 0x7
}

fn tsetdecorcolor(g: &mut Glyph, color: u32) {
    g.decoration = (g.decoration & !0x1FFFFFF) | (color & 0x1FFFFFF);
}

fn tsetdecorstyle(g: &mut Glyph, style: u32) {
    g.decoration = (g.decoration & !(0x7 << 25)) | ((style & 0x7) << 25);
}
