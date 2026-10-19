use super::data::{BDB, BOXDATA, BRL};

// TODO: either make this configurable or remove it after copy boxdraw.c.
const BOXDRAW: bool = true;
const BOXDRAW_BOLD: bool = true;
const BOXDRAW_BRAILLE: bool = true;

fn as_block(u: char) -> u32 {
    u as u32 & !0xff
}

/// Whether `u` is drawn by the box drawing shader instead of the font.
pub fn isboxdraw(u: char) -> bool {
    match as_block(u) {
        0x2500 => BOXDRAW && BOXDATA[u as usize & 0xff] != 0,
        0x2800 => BOXDRAW_BRAILLE,
        _ => false,
    }
}

/// Expects `isboxdraw(u)`.
pub fn boxdrawindex(u: char, bold: bool) -> u32 {
    let block = as_block(u);
    let u = u as u32;

    if BOXDRAW_BRAILLE && block == 0x2800 {
        return BRL | (u & 0xff);
    }

    let data = BOXDATA[(u & 0xff) as usize];

    if BOXDRAW_BOLD && bold {
        return BDB | data;
    }

    data
}
