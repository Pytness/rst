use fontconfig::FC_MATRIX;
use fontconfig::FC_SIZE;
use fontconfig::FC_SLANT;
use fontconfig::FC_SLANT_ITALIC;
use fontconfig::FC_WEIGHT;
use fontconfig::FC_WEIGHT_BOLD;
use fontconfig::Fontconfig;
use fontconfig::Pattern;
use fontconfig_sys::Fc;
use fontconfig_sys::FcMatrix;
use fontconfig_sys::FcPattern;
use fontconfig_sys::ffi_dispatch;
use fontconfig_sys::statics::{LIB, LIB_RESULT};
use freetype::ffi::FT_Matrix;
use freetype::ffi::FT_Vector;
use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::sync::LazyLock;

use freetype::face::LoadFlag;
use freetype::{Face, GlyphSlot, Library};
use rustybuzz::{Face as RbFace, UnicodeBuffer};

const DEFAULT_DPI: u32 = 96;

static FT_LIB: LazyLock<Library> =
    LazyLock::new(|| Library::init().expect("failed to initialize FreeType library"));

pub struct ShapedGlyph {
    pub glyph_id: u32,
    pub char: char,
    pub font_index: usize,
    pub x_offset: f32,
    pub y_offset: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Regular,
    Italic,
    Bold,
    ItalicBold,
}

pub struct FontEntry {
    pub name: String,
    // Regular is required
    pub regular: FontFace,
    pub italic: Option<FontFace>,
    pub bold: Option<FontFace>,
    pub italic_bold: Option<FontFace>,
}

impl FontEntry {
    pub fn regular(&self) -> &FontFace {
        &self.regular
    }

    pub fn italic(&self) -> &FontFace {
        self.italic.as_ref().unwrap_or(&self.regular)
    }

    pub fn bold(&self) -> &FontFace {
        self.bold.as_ref().unwrap_or(&self.regular)
    }

    pub fn italic_bold(&self) -> &FontFace {
        self.italic_bold.as_ref().unwrap_or(&self.regular)
    }

    pub fn style(&self, style: FontStyle) -> &FontFace {
        match style {
            FontStyle::Regular => self.regular(),
            FontStyle::Italic => self.italic(),
            FontStyle::Bold => self.bold(),
            FontStyle::ItalicBold => self.italic_bold(),
        }
    }

    pub fn styles(&self) -> Vec<&FontFace> {
        [
            Some(&self.regular),
            self.italic.as_ref(),
            self.bold.as_ref(),
            self.italic_bold.as_ref(),
        ]
        .iter()
        .flatten()
        .cloned()
        .collect()
    }
}

pub struct FontRegistry {
    fonts: Vec<FontEntry>,
    shape_buffer: RefCell<Option<UnicodeBuffer>>,
}

pub struct FontFace {
    bytes: ManuallyDrop<&'static [u8]>,
    pub ft_face: Face,
    pub matrix: FcMatrix,
    pub rb_face: RbFace<'static>,
}

impl FontFace {
    fn new(match_: (String, FcMatrix)) -> Self {
        let (path, matrix) = match_;

        let bytes = std::fs::read(path).expect("failed to read font file");
        let bytes = ManuallyDrop::new(Box::leak(bytes.into_boxed_slice()) as &'static [u8]);

        let ft_face = FT_LIB
            .new_memory_face(bytes.to_vec(), 0)
            .expect("failed to load freetype face");

        let rb_face = RbFace::from_slice(&bytes, 0).expect("failed to load rustybuzz face");

        Self {
            bytes,
            ft_face,
            matrix,
            rb_face,
        }
    }

    pub fn units_per_em(&self) -> i32 {
        self.rb_face.units_per_em()
    }

    pub fn set_transform(&self) {
        let matrix = self.matrix;
        let mut matrix = FT_Matrix {
            xx: (matrix.xx * 0x10000 as f64) as i64,
            xy: (matrix.xy * 0x10000 as f64) as i64,
            yx: (matrix.yx * 0x10000 as f64) as i64,
            yy: (matrix.yy * 0x10000 as f64) as i64,
        };

        let mut vector = FT_Vector { x: 0, y: 0 };
        println!(
            "Setting transform for font '{}': matrix xx={:?}, xy={:?}, yx={:?}, yy={:?}",
            self.ft_face.family_name().unwrap_or("unknown".to_string()),
            matrix.xx,
            matrix.xy,
            matrix.yx,
            matrix.yy
        );
        self.ft_face.set_transform(&mut matrix, &mut vector);
    }
}

impl Drop for FontFace {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.bytes);
        }
    }
}

fn delpattern(pattern: &mut Pattern, object: &str) {
    unsafe {
        (LIB.FcPatternDel)(pattern.as_mut_ptr(), object.as_ptr() as *const i8);
    }
}

fn match_pattern(pattern: &Pattern) -> Option<(String, FcMatrix)> {
    let mut pattern = pattern.clone();
    let slant = pattern.get_int(FC_SLANT);
    let weight = pattern.get_int(FC_WEIGHT);

    let fmatch = pattern.font_match();
    let name = fmatch.name().unwrap_or("unknown").to_string();

    let match_slant = fmatch.get_int(FC_SLANT);
    let match_weight = fmatch.get_int(FC_WEIGHT);

    let face_index = fmatch.face_index();
    let filename = fmatch.filename().unwrap_or("unknown");

    println!(
        "Matched font: '{}', requested slant={:?}, weight={:?}, got slant={:?}, weight={:?}, face_index={:?}, filename='{}'",
        name, slant, weight, match_slant, match_weight, face_index, filename
    );
    fmatch.print();

    let mut matrix: *mut FcMatrix = std::ptr::null_mut();
    // fn FcPatternGetMatrix( *mut FcPattern, *const c_char, c_int, *mut *mut FcMatrix) -> FcResult,
    unsafe {
        (LIB.FcPatternGetMatrix)(
            fmatch.as_ptr() as *mut FcPattern,
            FC_MATRIX.as_ptr() as *const i8,
            0,
            &mut matrix as *mut *mut FcMatrix,
        )
    };

    let matrix: FcMatrix = if matrix.is_null() {
        FcMatrix {
            xx: 1.0,
            xy: 0.0,
            yx: 0.0,
            yy: 1.0,
        }
    } else {
        unsafe { *matrix }
    };

    println!(
        "Font matrix: xx={:?}\n, xy={:?}\n, yx={:?}\n, yy={:?}",
        matrix.xx, matrix.xy, matrix.yx, matrix.yy
    );

    // if slant.is_some() && match_slant != slant {
    //     return None;
    // }
    //
    // if weight.is_some() && match_weight != weight {
    //     return None;
    // }

    Some((fmatch.filename().unwrap().to_string(), matrix))
}

impl FontRegistry {
    pub fn new() -> Self {
        FontRegistry {
            fonts: Vec::new(),
            shape_buffer: RefCell::new(Some(UnicodeBuffer::new())),
        }
    }

    pub fn register_font(&mut self, name: &str, bytes: &'static [u8]) {
        let fontconfig = Fontconfig::new().expect("failed to create fontconfig instance");

        let mut pattern = unsafe {
            Pattern::from_pattern(
                &fontconfig,
                (LIB.FcNameParse)(name.as_ptr() as *const u8) as *mut FcPattern,
            )
        };

        pattern.add_integer(FC_SIZE, 10);
        let regular = match_pattern(&pattern);

        pattern.add_integer(FC_SLANT, FC_SLANT_ITALIC);
        let italic = match_pattern(&pattern);

        pattern.add_integer(FC_WEIGHT, FC_WEIGHT_BOLD);
        let italic_bold = match_pattern(&pattern);

        unsafe {
            (LIB.FcPatternDel)(pattern.as_mut_ptr(), FC_SLANT.as_ptr() as *const i8);
        }

        delpattern(&mut pattern, FC_SLANT.to_str().unwrap());
        pattern.add_integer(FC_SLANT, 0);
        let bold = match_pattern(&pattern);

        if regular.is_none() {
            println!("Warning: failed to find regular style for font '{}'", name);

            return;
        }

        self.fonts.push(FontEntry {
            name: name.to_string(),
            regular: regular
                .map(|path| FontFace::new(path))
                .expect("regular style is required"),
            italic: italic.map(|path| FontFace::new(path)),
            bold: bold.map(|path| FontFace::new(path)),
            italic_bold: italic_bold.map(|path| FontFace::new(path)),
        });
    }

    pub fn get_fonts(&self) -> &[FontEntry] {
        &self.fonts
    }

    /// Find and set best matching fixed size for the given pixel size.
    fn set_color_size(&self, ft_face: &Face, pixel_size: isize) {
        let raw = ft_face.raw();
        let num = (*raw).num_fixed_sizes;

        if num == 0 {
            println!(
                "Font '{}' does not have fixed sizes, skipping color size setting",
                ft_face.family_name().unwrap_or("unknown".to_string())
            );
            return;
        }

        println!(
            "Font '{}' has {} fixed sizes, selecting best match for pixel size {}",
            ft_face.family_name().unwrap_or("unknown".to_string()),
            num,
            pixel_size
        );

        let availables_sizes =
            unsafe { std::slice::from_raw_parts((*raw).available_sizes, num as usize) };

        let mut best_diff = isize::MAX;
        let mut best_match_index = 0;

        for (i, size) in availables_sizes.iter().enumerate() {
            println!(
                "Available size {}: width={}, height={}, pixel_size={}",
                i, size.width, size.height, size.y_ppem
            );
            let diff = (pixel_size - size.width as isize).abs();

            if diff < best_diff {
                best_diff = diff;
                best_match_index = i;
            }
        }

        ft_face
            .select_size(best_match_index as i32)
            .expect("failed to select color size");
    }

    /// Sets the character size for all registered fonts.
    pub fn set_char_size(&self, char_size: isize, dpi: Option<u32>) {
        let char_size = char_size * 64;
        let dpi = dpi.unwrap_or(DEFAULT_DPI);

        for font in &self.fonts {
            for style in font.styles().iter() {
                println!(
                    "Setting char size for font '{}': char_size={}, dpi={}",
                    style.ft_face.family_name().unwrap_or("unknown".to_string()),
                    char_size,
                    dpi
                );

                if !style.ft_face.has_color() {
                    style
                        .ft_face
                        .set_char_size(0, char_size, dpi, dpi)
                        .expect("failed to set char size");
                } else {
                    self.set_color_size(&style.ft_face, char_size);
                    println!(
                        "Skipping char size setting for font '{}' because it has color glyphs",
                        style.ft_face.family_name().unwrap_or("unknown".to_string())
                    );
                }
            }
        }
    }

    pub fn get_char_index(&self, char_code: char) -> Option<(usize, u32)> {
        for (font_index, entry) in self.fonts.iter().enumerate() {
            let glyph_id = entry.regular().ft_face.get_char_index(char_code as usize);
            println!(
                "Font '{}': char code '{}' (U+{:04X}) maps to glyph ID {:?}",
                entry.name, char_code, char_code as u32, glyph_id
            );

            if let Some(glyph_id) = glyph_id {
                if glyph_id != 0 {
                    println!(
                        "Found glyph ID {} for char code '{}' in font '{}'",
                        glyph_id, char_code, entry.name
                    );
                    return Some((font_index, glyph_id));
                }
            }
        }

        None
    }

    pub fn shape_text(&self, chars: &[char]) -> Vec<ShapedGlyph> {
        let mut buffer = self
            .shape_buffer
            .borrow_mut()
            .take()
            .expect("shape buffer should always be available");

        let text: String = chars.iter().collect();
        buffer.push_str(&text);

        let font = self.fonts.first().expect("no fonts registered");
        let shaped = rustybuzz::shape(&font.regular().rb_face, &[], buffer);

        let infos = shaped.glyph_infos();
        let positions = shaped.glyph_positions();
        let glyphs: Vec<ShapedGlyph> = infos
            .iter()
            .zip(positions.iter())
            .zip(chars.iter())
            .map(|((info, pos), c)| {
                let (font_index, id) = if info.glyph_id != 0 {
                    (0, info.glyph_id)
                } else {
                    self.get_char_index(*c).unwrap_or((0, 0))
                };

                ShapedGlyph {
                    glyph_id: id,
                    char: *c,
                    font_index,
                    // rustybuzz positions are in font units; converted to pixels in draw_text
                    x_offset: pos.x_offset as f32,
                    y_offset: pos.y_offset as f32,
                }
            })
            .collect();

        self.shape_buffer.borrow_mut().replace(shaped.clear());

        glyphs
    }

    pub fn size_metrics(&self) -> Option<freetype::ffi::FT_Size_Metrics> {
        let font = self.fonts.first()?;

        font.regular().ft_face.size_metrics()
    }
}
