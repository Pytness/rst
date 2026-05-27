use std::collections::BTreeMap;
use std::hash::Hash;
use std::sync::atomic::AtomicUsize;

static mut IMAGE_ID: AtomicUsize = AtomicUsize::new(0);
static mut IMAGE_LIST: BTreeMap<usize, ImageList> = BTreeMap::new();

fn get_image_list() -> &'static mut BTreeMap<usize, ImageList> {
    // unsafe { &mut IMAGE_LIST }
    todo!();
}

fn get_next_image_id() -> usize {
    // unsafe { IMAGE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst) }
    todo!();
}

fn add_image(image: ImageList) {
    let id = get_next_image_id();
    let mut image = image;
    image.id = id;

    get_image_list().insert(id, image);
}

fn delete_image(image: ImageList) {
    get_image_list().remove(&image.id);
}

fn delete_image_by_id(id: usize) {
    get_image_list().remove(&id);
}

#[derive(Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct ImageList {
    id: usize,
    pub pixels: Vec<u8>,
    pub pixmap: Option<()>,
    pub clipmask: Option<()>,
    pub width: i32,
    pub height: i32,
    pub x: i32,
    pub y: i32,
    pub cols: i32,
    pub cw: i32,
    pub ch: i32,
    pub transparent: bool,
}

impl Hash for ImageList {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

pub const DECSIXEL_PARAMS_MAX: usize = 16;
pub const DECSIXEL_PALETTE_MAX: usize = 1024;
pub const DECSIXEL_PARAMVALUE_MAX: u16 = 65535;
pub const DECSIXEL_WIDTH_MAX: usize = 4096;
pub const DECSIXEL_HEIGHT_MAX: usize = 4096;

type SixelColorNo = u16;
type SixelColor = u32;

pub struct SixelImage {
    data: Vec<SixelColorNo>,
    width: usize,
    height: usize,
    palette: [SixelColor; DECSIXEL_PALETTE_MAX],
    ncolors: usize,
    palette_modified: bool,
    use_private_register: bool,
}

impl SixelImage {
    pub fn new(
        width: usize,
        height: usize,
        fgcolor: u32,
        bgcolor: u32,
        use_private_register: bool,
    ) -> Self {
        let size = width * height;

        let mut image = SixelImage {
            data: vec![0; size],
            width,
            height,
            palette: [0; DECSIXEL_PALETTE_MAX],
            ncolors: 0,
            palette_modified: false,
            use_private_register,
        };

        image.palette[0] = bgcolor;

        if image.use_private_register {
            image.palette[1] = fgcolor;
        }

        image
    }

    pub fn set_default_color(&mut self) {
        let mut i = 1;

        // palette initialization
        for _ in 1..17 {
            self.palette[i] = SIXEL_DEFAULT_COLOR_TABLE[i - 1];
            i += 1;
        }

        // color 17-232 are a 6x6x6 color cube
        for r in 0..6 {
            for g in 0..6 {
                for b in 0..6 {
                    self.palette[i] = sixel_rgb(r * 51, g * 51, b * 51);
                    i += 1;
                }
            }
        }

        // color 233-256 are a grayscale ramp, intentionally leaving out black and white
        for n in 0..24 {
            self.palette[i] = sixel_gray(n);
            i += 1;
        }

        for n in i..DECSIXEL_PALETTE_MAX {
            self.palette[n] = sixel_gray(255);
        }
    }

    pub fn resize_buffer(&mut self, _width: usize, _height: usize) {}
}

pub enum ParseState {
    Esc = 1,      /* ESC */
    DecSixel = 2, /* DECSIXEL body part ", $, -, ? ... ~ */
    DecGra = 3,   /* DECGRA Set Raster Attributes " Pan; Pad; Ph; Pv */
    DecGri = 4,   /* DECGRI Graphics Repeat Introducer ! Pn Ch */
    DecGci = 5,   /* DECGCI Graphics Color Introducer # Pc; Pu; Px; Py; Pz */
    Error = 6,
}

pub struct SixelParser {
    state: ParseState,
    pos_x: usize,
    pos_y: usize,
    max_x: usize,
    max_y: usize,
    attributed_pan: usize,
    attributed_pad: usize,
    attributed_ph: usize,
    attributed_pv: usize,
    transparent: bool,
    repeat_count: usize,
    color_index: usize,
    bgindex: usize,
    grid_width: usize,
    grid_height: usize,
    param: usize,
    nparams: usize,
    params: [usize; DECSIXEL_PARAMS_MAX],
    image: SixelImage,
}

impl SixelParser {
    pub fn new(
        transparent: bool,
        fgcolor: u32,
        bgcolor: u32,
        use_private_register: bool,
        cell_width: usize,
        cell_height: usize,
    ) -> Self {
        SixelParser {
            state: ParseState::Esc,
            pos_x: 0,
            pos_y: 0,
            max_x: 0,
            max_y: 0,
            attributed_pan: 2,
            attributed_pad: 1,
            attributed_ph: 0,
            attributed_pv: 0,
            transparent,
            repeat_count: 1,
            color_index: 16,
            bgindex: 0,
            grid_width: cell_width,
            grid_height: cell_height,
            nparams: 0,
            param: 0,
            params: [0; DECSIXEL_PARAMS_MAX],
            image: SixelImage::new(1, 1, fgcolor, bgcolor, use_private_register),
        }
    }

    pub fn set_default_color(&mut self) {
        self.image.set_default_color();
    }

    pub fn finalize() {}
}

const fn sixel_rgb(r: u8, g: u8, b: u8) -> u32 {
    255 << 24 | (r as u32) << 16 | (g as u32) << 8 | (b as u32)
}

/// Maps a percentage value (0-100) to a u8 value (0-255)
const fn percent_to_u8(value: u8) -> u8 {
    ((value as u16 * 255 + 50) / 100) as u8
}

const fn sixel_xrgb(r: u8, g: u8, b: u8) -> u32 {
    sixel_rgb(percent_to_u8(r), percent_to_u8(g), percent_to_u8(b))
}

const fn sixel_gray(n: u8) -> u32 {
    sixel_rgb(n, n, n)
}

const SIXEL_DEFAULT_COLOR_TABLE: [u32; 16] = [
    sixel_xrgb(0, 0, 0),    /*  0 Black    */
    sixel_xrgb(20, 20, 80), /*  1 Blue     */
    sixel_xrgb(80, 13, 13), /*  2 Red      */
    sixel_xrgb(20, 80, 20), /*  3 Green    */
    sixel_xrgb(80, 20, 80), /*  4 Magenta  */
    sixel_xrgb(20, 80, 80), /*  5 Cyan     */
    sixel_xrgb(80, 80, 20), /*  6 Yellow   */
    sixel_xrgb(53, 53, 53), /*  7 Gray 50% */
    sixel_xrgb(26, 26, 26), /*  8 Gray 25% */
    sixel_xrgb(33, 33, 60), /*  9 Blue*    */
    sixel_xrgb(60, 26, 26), /* 10 Red*     */
    sixel_xrgb(33, 60, 33), /* 11 Green*   */
    sixel_xrgb(60, 33, 60), /* 12 Magenta* */
    sixel_xrgb(33, 60, 60), /* 13 Cyan*    */
    sixel_xrgb(60, 60, 33), /* 14 Yellow*  */
    sixel_xrgb(80, 80, 80), /* 15 Gray 75% */
];

pub fn scroll_images(n: usize) {
    let mut to_delete = Vec::new();

    for image in get_image_list().values_mut() {
        image.y += n as i32;

        if image.y < 0 {
            // fprintf(stderr, "im@0x%08x exceeded maximum distance\n");
            to_delete.push(image.id);
        }
    }

    to_delete.into_iter().for_each(|id| {
        delete_image_by_id(id);
    });
}

pub fn image_buffer_resize(_image: &mut SixelImage, _width: usize, _height: usize) {}
