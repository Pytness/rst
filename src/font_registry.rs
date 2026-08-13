use fontconfig::CharSet;
use fontconfig::FC_FAMILY;
use fontconfig::FC_MATRIX;
use fontconfig::FC_SCALABLE;
use fontconfig::FC_SLANT;
use fontconfig::FC_SLANT_ITALIC;
use fontconfig::FC_WEIGHT;
use fontconfig::FC_WEIGHT_BOLD;
use fontconfig::Fontconfig;
use fontconfig::Pattern;
use fontconfig_sys::FcFontSet;
use fontconfig_sys::FcMatrix;
use fontconfig_sys::FcPattern;
use fontconfig_sys::FcResultNoMatch;
use fontconfig_sys::statics::LIB;
use freetype::face::StyleFlag;
use std::cell::Ref;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::CString;
use std::mem::ManuallyDrop;
use std::sync::LazyLock;

use freetype::{Face, Library};
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
    // Regular is required
    pub regular: FontFace,
    pub italic: Option<FontFace>,
    pub bold: Option<FontFace>,
    pub italic_bold: Option<FontFace>,
}

impl FontEntry {
    fn new(
        regular: FontFace,
        italic: Option<FontFace>,
        bold: Option<FontFace>,
        italic_bold: Option<FontFace>,
    ) -> Self {
        Self {
            regular,
            italic,
            bold,
            italic_bold,
        }
    }

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

type FallbackKey = (Option<String>, bool, bool);

pub struct FontRegistry {
    fonts: RefCell<Vec<FontEntry>>,
    shape_buffer: RefCell<Option<UnicodeBuffer>>,
    char_size: RefCell<Option<(isize, u32)>>,
    /// (family, italic, bold) + character combinations Fontconfig has confirmed no
    /// installed font covers, so `register_by_charcode` doesn't repeat the search.
    no_coverage: RefCell<HashSet<(FallbackKey, char)>>,
    /// `FcFontSort` result per (family, italic, bold), reused across every
    /// single-character fallback lookup for that style (mirrors st's `Font.set`
    /// cache in x.c). Never freed: it lives for the process's lifetime, same as
    /// the registered fonts themselves.
    fallback_sets: RefCell<HashMap<FallbackKey, *mut FcFontSet>>,
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
}

impl Drop for FontFace {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.bytes);
        }
    }
}

/// Reads the `FC_MATRIX` fontconfig set on a matched pattern (e.g. a synthetic
/// oblique shear substituted for a missing italic face), defaulting to identity.
fn pattern_matrix(pattern: &Pattern) -> FcMatrix {
    let mut matrix: *mut FcMatrix = std::ptr::null_mut();
    // fn FcPatternGetMatrix( *mut FcPattern, *const c_char, c_int, *mut *mut FcMatrix) -> FcResult,
    unsafe {
        (LIB.FcPatternGetMatrix)(
            pattern.as_ptr() as *mut FcPattern,
            FC_MATRIX.as_ptr() as *const i8,
            0,
            &mut matrix as *mut *mut FcMatrix,
        )
    };

    if matrix.is_null() {
        FcMatrix {
            xx: 1.0,
            xy: 0.0,
            yx: 0.0,
            yy: 1.0,
        }
    } else {
        unsafe { *matrix }
    }
}

fn match_pattern(pattern: &Pattern) -> Option<(String, FcMatrix)> {
    let mut pattern = pattern.clone();
    let fmatch = pattern.font_match().ok()?;
    let matrix = pattern_matrix(&fmatch);

    Some((
        fmatch
            .filename()
            .expect("fontconfig match has no filename")
            .to_string(),
        matrix,
    ))
}

impl FontRegistry {
    pub fn new() -> Self {
        FontRegistry {
            fonts: RefCell::new(Vec::new()),
            shape_buffer: RefCell::new(Some(UnicodeBuffer::new())),
            char_size: RefCell::new(None),
            no_coverage: RefCell::new(HashSet::new()),
            fallback_sets: RefCell::new(HashMap::new()),
        }
    }

    pub fn register_font(&mut self, name: &str) {
        let fontconfig = Fontconfig::new().expect("failed to create fontconfig instance");

        let pattern_ptr = CString::new(name).expect("font name contained a NUL byte");

        let base = unsafe {
            Pattern::from_pattern(
                &fontconfig,
                (LIB.FcNameParse)(pattern_ptr.as_ptr() as *const u8) as *mut FcPattern,
            )
        };

        // slant/weight left as `None` are unconstrained, matching whatever
        // Fontconfig considers closest to `base` rather than a specific style.
        let styled = |slant: Option<i32>, weight: Option<i32>| {
            let mut pattern = base.clone();
            if let Some(slant) = slant {
                pattern.add_integer(FC_SLANT, slant).ok();
            }
            if let Some(weight) = weight {
                pattern.add_integer(FC_WEIGHT, weight).ok();
            }
            match_pattern(&pattern)
        };

        let Some(regular) = styled(None, None) else {
            eprintln!("Warning: failed to find regular style for font '{}'", name);
            return;
        };
        let italic = styled(Some(FC_SLANT_ITALIC), None);
        let bold = styled(Some(0), Some(FC_WEIGHT_BOLD));
        let italic_bold = styled(Some(FC_SLANT_ITALIC), Some(FC_WEIGHT_BOLD));

        self.fonts.get_mut().push(FontEntry::new(
            FontFace::new(regular),
            italic.map(FontFace::new),
            bold.map(FontFace::new),
            italic_bold.map(FontFace::new),
        ));
    }

    /// Builds the Fontconfig pattern used as the search anchor for a fallback
    /// lookup: same family/slant/weight as the face we failed to find a glyph in.
    fn fallback_base_pattern<'fc>(
        fontconfig: &'fc Fontconfig,
        (family, italic, bold): &FallbackKey,
    ) -> Option<Pattern<'fc>> {
        let family = CString::new(family.as_deref()?).ok()?;

        let mut pattern = Pattern::new(fontconfig).ok()?;
        pattern.add_string(FC_FAMILY, &family).ok();

        if *italic {
            pattern.add_integer(FC_SLANT, FC_SLANT_ITALIC).ok();
        }
        if *bold {
            pattern.add_integer(FC_WEIGHT, FC_WEIGHT_BOLD).ok();
        }

        Some(pattern)
    }

    /// Returns the (cached) `FcFontSort` result closest to `key` - fonts ranked by
    /// similarity to the face that was missing a glyph, i.e. the same search space
    /// st builds once per `Font` (`font->set` in x.c) and reuses for every
    /// single-character fallback lookup in that style.
    fn sorted_fallback_set(&self, key: &FallbackKey, base_pattern: &Pattern) -> *mut FcFontSet {
        if let Some(set) = self.fallback_sets.borrow().get(key) {
            return *set;
        }

        let mut pattern = base_pattern.clone();
        pattern.config_substitute().ok();
        pattern.default_substitute();

        let mut result = FcResultNoMatch;
        let set = unsafe {
            (LIB.FcFontSort)(
                std::ptr::null_mut(),
                pattern.as_mut_ptr(),
                1, // trim: drop fonts whose coverage is a subset of an earlier one
                std::ptr::null_mut(),
                &mut result,
            )
        };

        self.fallback_sets.borrow_mut().insert(key.clone(), set);
        set
    }

    /// Finds and loads a system font covering `char_code`, the same way st's
    /// `xmakeglyphfontspecs` does on a shaping miss (x.c): sort installed fonts by
    /// closeness to the face used for `style` once per (family, italic, bold), then
    /// match that sorted set against a pattern constrained to just this character.
    /// The result is appended to the registry, so later lookups - for this
    /// character and any other the new font happens to cover - are satisfied by
    /// the plain scan at the top of `get_char_index` without asking Fontconfig
    /// again. Characters no font covers are remembered too, so a glyph with no
    /// coverage anywhere doesn't re-trigger this search every time it's drawn.
    pub fn register_by_charcode(&self, char_code: char, style: FontStyle) -> Option<(usize, u32)> {
        let key: FallbackKey = {
            let fonts = self.fonts.borrow();
            let base = fonts.first()?.style(style);
            let flags = base.ft_face.style_flags();
            (
                base.ft_face.family_name(),
                flags.contains(StyleFlag::ITALIC),
                flags.contains(StyleFlag::BOLD),
            )
        };

        if self
            .no_coverage
            .borrow()
            .contains(&(key.clone(), char_code))
        {
            return None;
        }

        let fontconfig = Fontconfig::new()?;
        let base_pattern = Self::fallback_base_pattern(&fontconfig, &key)?;
        let sorted_set = self.sorted_fallback_set(&key, &base_pattern);

        let mut char_pattern = base_pattern.clone();
        let mut charset = CharSet::new(&fontconfig).ok()?;
        charset.add_char(char_code).ok();
        char_pattern.add_charset(charset).ok();
        unsafe {
            (LIB.FcPatternAddBool)(
                char_pattern.as_mut_ptr(),
                FC_SCALABLE.as_ptr() as *const i8,
                1,
            );
        }
        char_pattern.config_substitute().ok();
        char_pattern.default_substitute();

        let mut sets = [sorted_set];
        let mut result = FcResultNoMatch;
        let matched_ptr = unsafe {
            (LIB.FcFontSetMatch)(
                std::ptr::null_mut(),
                sets.as_mut_ptr(),
                sets.len() as i32,
                char_pattern.as_mut_ptr(),
                &mut result,
            )
        };

        if matched_ptr.is_null() {
            self.no_coverage.borrow_mut().insert((key, char_code));
            return None;
        }

        let matched = unsafe { Pattern::from_pattern(&fontconfig, matched_ptr) };
        let Ok(filename) = matched.filename().map(str::to_string) else {
            return None;
        };
        let matrix = pattern_matrix(&matched);

        let face = FontFace::new((filename, matrix));
        if let Some((char_size, dpi)) = *self.char_size.borrow() {
            self.apply_char_size(&face, char_size, dpi);
        }

        let glyph_id = face.ft_face.get_char_index(char_code as usize).unwrap_or(0);

        let font_index = {
            let mut fonts = self.fonts.borrow_mut();
            let font_index = fonts.len();
            fonts.push(FontEntry::new(face, None, None, None));
            font_index
        };

        if glyph_id == 0 {
            self.no_coverage.borrow_mut().insert((key, char_code));
            return None;
        }

        Some((font_index, glyph_id))
    }

    pub fn get_fonts(&self) -> Ref<'_, Vec<FontEntry>> {
        self.fonts.borrow()
    }

    /// Find and set best matching fixed size for the given pixel size.
    fn set_color_size(&self, ft_face: &Face, pixel_size: isize) {
        let raw = ft_face.raw();
        let num = (*raw).num_fixed_sizes;

        if num == 0 {
            return;
        }

        let availables_sizes =
            unsafe { std::slice::from_raw_parts((*raw).available_sizes, num as usize) };

        let mut best_diff = isize::MAX;
        let mut best_match_index = 0;

        for (i, size) in availables_sizes.iter().enumerate() {
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

    /// Sets the character size for all registered fonts, including any fallback
    /// fonts discovered later via `register_by_charcode`.
    pub fn set_char_size(&self, char_size: isize, dpi: Option<u32>) {
        let char_size = char_size * 64;
        let dpi = dpi.unwrap_or(DEFAULT_DPI);
        *self.char_size.borrow_mut() = Some((char_size, dpi));

        for font in self.fonts.borrow().iter() {
            for style in font.styles() {
                self.apply_char_size(style, char_size, dpi);
            }
        }
    }

    fn apply_char_size(&self, face: &FontFace, char_size: isize, dpi: u32) {
        if !face.ft_face.has_color() {
            face.ft_face
                .set_char_size(0, char_size, dpi, dpi)
                .expect("failed to set char size");
        } else {
            self.set_color_size(&face.ft_face, char_size);
        }
    }

    /// Looks up `char_code` in the face used for rendering `style`, expanding the
    /// font list via `register_by_charcode` when no registered font - static or
    /// previously-discovered fallback - covers it.
    pub fn get_char_index(&self, char_code: char, style: FontStyle) -> Option<(usize, u32)> {
        {
            let fonts = self.fonts.borrow();
            for (font_index, entry) in fonts.iter().enumerate() {
                let glyph_id = entry
                    .style(style)
                    .ft_face
                    .get_char_index(char_code as usize);

                if let Some(glyph_id) = glyph_id {
                    if glyph_id != 0 {
                        return Some((font_index, glyph_id));
                    }
                }
            }
        }

        self.register_by_charcode(char_code, style)
    }

    /// Shapes `chars` against the face used for rendering `style`.
    pub fn shape_text(&self, chars: &[char], style: FontStyle) -> Vec<ShapedGlyph> {
        let mut buffer = self
            .shape_buffer
            .borrow_mut()
            .take()
            .expect("shape buffer should always be available");

        let text: String = chars.iter().collect();
        buffer.push_str(&text);

        // Scoped so the borrow is released before `get_char_index` below, which may
        // need to mutably borrow `self.fonts` to register a fallback font.
        let shaped = {
            let fonts = self.fonts.borrow();
            let font = fonts.first().expect("no fonts registered");
            rustybuzz::shape(&font.style(style).rb_face, &[], buffer)
        };

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
                    self.get_char_index(*c, style).unwrap_or((0, 0))
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
        let fonts = self.fonts.borrow();
        let font = fonts.first()?;

        font.regular().ft_face.size_metrics()
    }
}
