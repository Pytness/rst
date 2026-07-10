use crate::BETWEEN;
use crate::config::DEFAULTBG;

pub const COLORS: [u32; 512] = {
    let mut arr = [0; 512];
    // 8 normal colors
    // 8 bright colors
    arr[0] = 0x000000; // 0
    arr[1] = 0xd86464; // 1
    arr[2] = 0x57d36d; // 2
    arr[3] = 0xd0d06a; // 3
    arr[4] = 0x6464ce; // 4
    arr[5] = 0xd763cc; // 5
    arr[6] = 0x56d2d2; // 6
    arr[7] = 0xd9d9d9; // 7

    // 8 bright colors
    arr[8] = 0x000000; // 0
    arr[9] = 0xd86464; // 1
    arr[10] = 0x57d36d; // 2
    arr[11] = 0xd0d06a; // 3
    arr[12] = 0x6464ce; // 4
    arr[13] = 0xd763cc; // 5
    arr[14] = 0x56d2d2; // 6
    arr[15] = 0xd9d9d9; // 7

    arr[256] = 0xcccccc;
    arr[257] = 0x555555;
    arr[258] = 0xe5e5e5;
    arr[259] = 0x000000;

    arr
};

#[derive(Default, Clone)]
pub struct Color {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
    pub alpha: u16,
}

pub struct ColorRegistry {
    loaded: bool,
    colors: Vec<Color>,
}

impl ColorRegistry {
    pub fn load_colors(&mut self) {
        if self.loaded {
            for color in self.colors.iter() {
                // XftColorFree(xw.dpy, xw.vis, xw.cmap, cp);
            }
        } else {
            let len = COLORS.len().max(256);
            self.colors = vec![Color::default(); len];
        }

        for i in 0..self.colors.len() {
            if let Some(color) = self.load_color(i, None) {
                self.colors[i] = color;
            }
        }

        // TODO: check if this is needed. Maybe opengl makes this obsolete

        // dc.col[defaultbg].color.alpha = (unsigned short)(0xffff * alpha);
        // dc.col[defaultbg].pixel &= 0x00FFFFFF;
        // dc.col[defaultbg].pixel |= (unsigned char)(0xff * alpha) << 24;

        self.loaded = true;
    }

    pub fn load_color(&mut self, i: usize, name: Option<&str>) -> Option<Color> {
        let mut name = name;
        let mut color = Color::default();

        if let Some(name) = name {
            todo!("Implement color name parsing for '{}'", name);
        }

        const XTERM_SIZE: u16 = 6 * 6 * 6 + 16;

        if name.is_none() {
            if BETWEEN!(i, 16, 255) {
                // 256 color
                if i < XTERM_SIZE as usize {
                    // same colors as xterm
                    color.red = sixd_to_16bit(((i - 16) / 36) % 6);
                    color.green = sixd_to_16bit(((i - 16) / 6) % 6);
                    color.blue = sixd_to_16bit(((i - 16) / 1) % 6);
                } else {
                    // TODO: un-magic this
                    color.red = 0x0808 + 0x0a0a * (i as u16 - XTERM_SIZE);
                    color.green = color.red;
                    color.blue = color.red;
                }

                return Some(color);
            } else {
                // TODO: parse the color from `name`
                // c code: name = colorname[i];
                name = Some(COLORS[i].to_string().as_str());
            }
        }

        // TODO: implement color parsing from name

        return Some(color);
    }

    pub fn get_color(&self, i: usize) -> Option<&Color> {
        self.colors.get(i)
    }

    pub fn set_color_name(&mut self, i: usize, name: &str) -> bool {
        if i >= self.colors.len() {
            return false;
        }

        if let Some(color) = self.load_color(i, Some(name)) {
            self.colors[i] = color;

            if i == DEFAULTBG as usize {
                // NOTE: this originaly was multiplied by `alpha`, a configurable constant
                // TODO: check if this is needed. Maybe opengl makes this obsolete
                // dc.col[defaultbg].color.alpha = (unsigned short)(0xffff * alpha);
                // dc.col[defaultbg].pixel &= 0x00FFFFFF;
                // dc.col[defaultbg].pixel |= (unsigned char)(0xff * alpha) << 24;
            }
        } else {
            return false;
        }

        return true;
    }
}

// TODO: un-magic this
fn sixd_to_16bit(x: usize) -> u16 {
    if x == 0 {
        0
    } else {
        0x3737 + 0x2828 * x as u16
    }
}
