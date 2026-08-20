use ahash::RandomState;
use std::collections::HashMap;
use std::mem::{offset_of, size_of};
use std::rc::Rc;
use std::sync::LazyLock;

use fontconfig_sys::FcMatrix;
use freetype::bitmap::PixelMode;
use freetype::{Library, face::LoadFlag};
use glow::HasContext;

use crate::font_registry::{FontRegistry, FontStyle, ShapedGlyph};
use crate::macros::macs::include_shader;
use crate::text_manager::{TermGlyph, TextManager};
use crate::win::{CursorStyle, TermWindow};

static FT_LIB: LazyLock<Library> =
    LazyLock::new(|| Library::init().expect("failed to initialize FreeType library"));

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 3],
    is_color: f32,
}

/// Fixed size (in pixels) of the shared glyph atlas texture that every
/// cached glyph is packed into, so a whole row can be drawn with a single
/// `draw_arrays` call instead of one draw call (and texture bind) per glyph.
const ATLAS_SIZE: i32 = 2048;
/// Gap left between packed glyphs to avoid bilinear filtering sampling
/// texels from a neighbouring glyph at the edge of its UV rect.
const ATLAS_PADDING: i32 = 1;

unsafe impl bytemuck::Pod for Vertex {}
unsafe impl bytemuck::Zeroable for Vertex {}

#[derive(Clone, Copy)]
pub struct FontSize {
    pub width: f32,
    pub height: f32,
    pub ascender: f32,
    pub descender: f32,
}

#[derive(Clone, Copy)]
struct GlyphTexture {
    atlas_x: i32,
    atlas_y: i32,
    width: i32,
    height: i32,
    left: i32,
    top: i32,
    is_color: bool,
    scale: f32,
    matrix: FcMatrix,
    cell_width: usize,
}

type GlyphKey = (usize, u32, FontStyle);

pub struct TextRenderer<'a> {
    gl: Rc<glow::Context>,
    font_registry: &'a FontRegistry,

    pub text_manager: TextManager,

    glyphs: HashMap<GlyphKey, GlyphTexture, RandomState>,

    atlas_tex: glow::NativeTexture,
    atlas_cursor_x: i32,
    atlas_cursor_y: i32,
    atlas_shelf_height: i32,

    program: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    vbo: glow::NativeBuffer,

    u_proj: Option<glow::NativeUniformLocation>,
    u_tex: Option<glow::NativeUniformLocation>,
    font_size_px: FontSize,
    px_size: f32,
}

impl<'a> TextRenderer<'a> {
    pub fn units_per_em(&self) -> f32 {
        self.font_registry
            .get_fonts()
            .first()
            .expect("no fonts registered")
            .regular()
            .units_per_em() as f32
    }

    /// Returns (width, height) of the font at the current pixel size.
    /// This is not the same as the maximum glyph size, but can be used for layout purposes.
    pub fn font_size(&self) -> FontSize {
        self.font_size_px
    }

    pub fn update_font_size(&mut self, px_size: u32, dpi: u32) {
        self.font_registry
            .set_char_size(px_size as isize, Some(dpi));

        let metrics = self
            .font_registry
            .size_metrics()
            .expect("failed to get size metrics");

        let cell_width = metrics.max_advance as f32 / 64.0;
        let ascender = metrics.ascender as f32 / 64.0;
        let descender = metrics.descender as f32 / 64.0;
        let cell_height = ascender - descender;

        self.font_size_px = FontSize {
            width: cell_width,
            height: cell_height,
            ascender,
            descender,
        };

        self.px_size = px_size as f32;

        self.text_manager = TextManager::new(
            cell_width.ceil() as i32,
            cell_height.ceil() as i32,
            self.text_manager.window_width,
            self.text_manager.window_height,
            4,
        );

        self.glyphs.clear();
        self.atlas_cursor_x = 0;
        self.atlas_cursor_y = 0;
        self.atlas_shelf_height = 0;
    }

    pub unsafe fn new(
        gl: Rc<glow::Context>,
        font_registry: &'a FontRegistry,
        px_size: u32,
        size: (i32, i32),
    ) -> Self {
        // BUG:
        // freetype-sys uses an incorrect value for FT_LCD_FILTER_LIGHT (3 instead of 2)
        // which causes the call to set_lcd_filter to fail with "Invalid argument".
        //
        // ```
        // FT_LIB
        //     .set_lcd_filter(freetype::LcdFilter::LcdFilterLight)
        //     .expect("failed to set LCD filter");
        // ```
        let err = unsafe { freetype::ffi::FT_Library_SetLcdFilter(FT_LIB.raw(), 2) };
        if err != freetype::ffi::FT_Err_Ok {
            eprintln!("Warning: failed to set LCD filter (error code {})", err);
        }

        font_registry.set_char_size(px_size as isize, None);

        let metrics = font_registry
            .size_metrics()
            .expect("failed to get size metrics");

        let cell_width = metrics.max_advance as f32 / 64.0;
        let ascender = metrics.ascender as f32 / 64.0;
        let descender = metrics.descender as f32 / 64.0;
        let cell_height = ascender - descender;
        let font_size_px = FontSize {
            width: cell_width,
            height: cell_height,
            ascender,
            descender,
        };

        // Explicit unsafe block required by Rust 2024: unsafe fn bodies no longer
        // implicitly permit unsafe calls without an unsafe{} block.
        let program = unsafe { include_shader!(gl, "font") };

        let vao = unsafe {
            gl.create_vertex_array()
                .expect("failed to create glyph VAO")
        };
        let vbo = unsafe { gl.create_buffer().expect("failed to create glyph VBO") };

        let u_proj = unsafe { gl.get_uniform_location(program, "u_proj") };
        let u_tex = unsafe { gl.get_uniform_location(program, "u_font") };

        let atlas_tex = unsafe {
            let atlas_tex = gl
                .create_texture()
                .ok()
                .expect("failed to create glyph atlas texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(atlas_tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                ATLAS_SIZE,
                ATLAS_SIZE,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            atlas_tex
        };

        let text_manager = TextManager::new(
            font_size_px.width.ceil() as i32,
            font_size_px.height.ceil() as i32,
            size.0,
            size.1,
            4,
        );

        Self {
            gl,
            font_registry,
            text_manager,
            glyphs: HashMap::default(),
            atlas_tex,
            atlas_cursor_x: 0,
            atlas_cursor_y: 0,
            atlas_shelf_height: 0,
            program,
            vao,
            vbo,
            u_proj,
            u_tex,
            font_size_px,
            px_size: px_size as f32,
        }
    }

    /// Allocates a `width`x`height` region in the shared glyph atlas using a
    /// simple shelf packer, returning its (x, y) pixel offset. Panics if the
    /// atlas runs out of room, since a fixed size was chosen to comfortably
    /// hold a session's glyph set.
    fn alloc_atlas_region(&mut self, width: i32, height: i32) -> (i32, i32) {
        let w = width + ATLAS_PADDING;
        let h = height + ATLAS_PADDING;

        if self.atlas_cursor_x + w > ATLAS_SIZE {
            self.atlas_cursor_x = 0;
            self.atlas_cursor_y += self.atlas_shelf_height;
            self.atlas_shelf_height = 0;
        }

        assert!(
            self.atlas_cursor_y + h <= ATLAS_SIZE,
            "glyph atlas exhausted (increase ATLAS_SIZE)"
        );

        let pos = (self.atlas_cursor_x, self.atlas_cursor_y);
        self.atlas_cursor_x += w;
        self.atlas_shelf_height = self.atlas_shelf_height.max(h);
        pos
    }

    pub fn clear_section(&self, x: i32, y: i32, width: i32, height: i32, color: [f32; 4]) {
        let y = self.text_manager.window_height - y; // Convert from top-left to bottom-left origin

        unsafe {
            let gl = self.gl.as_ref();
            gl.enable(glow::SCISSOR_TEST);

            gl.scissor(x, y, width, height);
            gl.clear_color(color[0], color[1], color[2], color[3]);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.disable(glow::SCISSOR_TEST);
        }
    }

    /// Shapes contiguous of same styled cells separately.
    fn shape_glyph_runs(&self, glyphs: &[TermGlyph]) -> Vec<ShapedGlyph> {
        let mut shaped = Vec::with_capacity(glyphs.len());
        let mut i = 0;

        while i < glyphs.len() {
            let style = glyphs[i].font_style;
            let mut j = i + 1;
            while j < glyphs.len() && glyphs[j].font_style == style {
                j += 1;
            }

            let run_chars: Vec<char> = glyphs[i..j].iter().map(|g| g.char).collect();
            shaped.extend(self.font_registry.shape_text(&run_chars, style));

            i = j;
        }

        shaped
    }

    /// Ensures the glyph is loaded and cached.
    /// Returns a tuple of (left, top, width, height, tex) to avoid holding a
    /// reference into `self.glyphs` across subsequent `self` accesses.
    fn ensure_glyph(&mut self, glyph: &ShapedGlyph, style: FontStyle) -> Option<&GlyphTexture> {
        let key = (glyph.font_index, glyph.glyph_id, style);

        if !self.glyphs.contains_key(&key) {
            let texture = unsafe { self.load_glyph_into_atlas(glyph, style)? };
            self.glyphs.insert(key, texture);
        }

        self.glyphs.get(&key)
    }

    /// Rasterises a single glyph with FreeType and uploads it into the shared glyph atlas.
    unsafe fn load_glyph_into_atlas(
        &mut self,
        glyph: &ShapedGlyph,
        style: FontStyle,
    ) -> Option<GlyphTexture> {
        let fonts = self.font_registry.get_fonts();
        let font = fonts[glyph.font_index].style(style);
        let ft_face = &font.ft_face;
        let matrix = font.matrix;

        let load_glyph_result = ft_face.load_glyph(
            glyph.glyph_id,
            LoadFlag::RENDER | LoadFlag::FORCE_AUTOHINT | LoadFlag::TARGET_NORMAL | LoadFlag::COLOR,
        );

        if let Err(err) = load_glyph_result {
            eprintln!(
                "Failed to load glyph {}: {:?} | from ft_face {} with style {:?}",
                glyph.glyph_id,
                err,
                ft_face.family_name().unwrap_or("unknown".into()),
                style
            );
            return None;
        }

        let glyph_slot = ft_face.glyph();
        let bitmap = glyph_slot.bitmap();
        let pixel_mode = bitmap.pixel_mode().unwrap_or(PixelMode::None);

        #[rustfmt::skip]
        let (width, height) = {
            let w = bitmap.width();
            let h = bitmap.rows();

            match pixel_mode {
                PixelMode::Gray => (w    , h    ), // Single-channel: one byte per pixel
                PixelMode::Lcd  => (w / 3, h    ), // Three bytes per pixel (R, G, B) per horizontal pixel
                PixelMode::LcdV => (w    , h / 3), // Three bytes per pixel (R, G, B) per vertical pixel
                // Four bytes per pixel (B, G, R, A) per horizontal pixel
                // but the width and height are not multiplied by 4
                PixelMode::Bgra => (w, h),
                _ => (w, h),
            }
        };

        let left = glyph_slot.bitmap_left();
        let top = glyph_slot.bitmap_top();

        let is_color = pixel_mode == PixelMode::Bgra;

        let alignment = match pixel_mode {
            PixelMode::Gray => 1,
            PixelMode::Lcd | PixelMode::LcdV => 3,
            PixelMode::Bgra => 4,
            _ => 1,
        };

        let format = match pixel_mode {
            PixelMode::Gray => glow::RED,
            PixelMode::Lcd | PixelMode::LcdV => glow::RGB,
            PixelMode::Bgra => glow::BGRA,
            _ => glow::RED,
        };

        let cell_width = unicode_width::UnicodeWidthChar::width(glyph.char).unwrap_or(1);
        let scale = (height as f32 / self.font_size_px.height).max(1.0);

        let (atlas_x, atlas_y) = if width > 0 && height > 0 {
            self.alloc_atlas_region(width, height)
        } else {
            (0, 0)
        };

        unsafe {
            let gl = self.gl.as_ref();

            if width > 0 && height > 0 {
                gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas_tex));
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, alignment);
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    atlas_x,
                    atlas_y,
                    width,
                    height,
                    format,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(bitmap.buffer())),
                );
            }

            Some(GlyphTexture {
                atlas_x,
                atlas_y,
                width,
                height,
                left,
                top,
                is_color,
                scale,
                matrix,
                cell_width,
            })
        }
    }

    pub unsafe fn draw_glyphs_bg(
        &self,
        term_glyphs: &[TermGlyph],
        widths: &[usize],
        row: i32,
        col: i32,
    ) {
        let mut advance_x = 0;

        // println!(
        //     "Drawing background for row {}, col {}: term_glyphs={}, widths={:?}",
        //     row,
        //     col,
        //     term_glyphs.len(),
        //     widths
        // );
        //
        for (term_g, cell_width) in term_glyphs.iter().zip(widths.iter()) {
            let cell_width = *cell_width as i32;

            let cell_box = self.text_manager.get_cell_box(row, col + advance_x);
            let bg_color = [
                term_g.bg_color.red as f32 / 255.0,
                term_g.bg_color.green as f32 / 255.0,
                term_g.bg_color.blue as f32 / 255.0,
                term_g.bg_color.alpha as f32 / 255.0,
            ];

            self.clear_section(
                cell_box.x,
                cell_box.y,
                cell_box.width * cell_width,
                cell_box.height,
                [bg_color[0], bg_color[1], bg_color[2], bg_color[3]],
            );

            advance_x += cell_width;
        }
    }

    pub unsafe fn draw_glyphs(
        &mut self,
        glyphs: &[TermGlyph],
        row: i32,
        col: i32,
        proj: &[f32; 16],
    ) {
        let cell_box = self.text_manager.get_cell_box(row, col);

        let mut pen_x: f32 = cell_box.x as f32;
        let baseline_y: f32 = cell_box.y as f32;

        let shaped = self.shape_glyph_runs(glyphs);

        let units_per_em = self.units_per_em();
        let px_size = self.px_size;

        let stride = size_of::<Vertex>() as i32;
        let uv_offset = offset_of!(Vertex, uv) as i32;
        let color_offset = offset_of!(Vertex, color) as i32;
        let is_color_offset = offset_of!(Vertex, is_color) as i32;

        let cached_glyphs: Vec<_> = glyphs
            .iter()
            .zip(shaped.iter())
            .map(|(term_g, shaped_g)| self.ensure_glyph(shaped_g, term_g.font_style).cloned())
            .collect();

        let glyphs_iter = glyphs.iter().zip(shaped.iter().zip(cached_glyphs.iter()));
        let glyph_widths: Vec<usize> = cached_glyphs
            .iter()
            .map(|g| g.map(|t| t.cell_width).unwrap_or(1))
            .collect();

        unsafe {
            self.draw_glyphs_bg(glyphs, &glyph_widths, row, col);
            let gl = self.gl.as_ref();

            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));

            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);

            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, uv_offset);

            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 3, glow::FLOAT, false, stride, color_offset);

            gl.enable_vertex_attrib_array(3);
            gl.vertex_attrib_pointer_f32(3, 1, glow::FLOAT, false, stride, is_color_offset);

            gl.use_program(Some(self.program));

            gl.enable(glow::BLEND);
            gl.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );

            gl.uniform_matrix_4_f32_slice(self.u_proj.as_ref(), false, proj);
            gl.uniform_1_i32(self.u_tex.as_ref(), 0);

            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas_tex));

            let mut all_vertices: Vec<Vertex> = Vec::with_capacity(glyphs.len() * 6);
            for (term_g, (shaped_g, cached_g)) in glyphs_iter {
                let &Some(GlyphTexture {
                    atlas_x,
                    atlas_y,
                    left,
                    mut top,
                    width,
                    height,
                    is_color,
                    scale,
                    matrix,
                    cell_width,
                }) = cached_g
                else {
                    continue;
                };

                // Scale top
                if is_color {
                    top = (top as f32 / scale) as i32;
                }

                let x_offset = hb_to_px(shaped_g.x_offset, px_size, units_per_em);
                let y_offset = hb_to_px(shaped_g.y_offset, px_size, units_per_em);

                let w = width as f32;
                let h = height as f32;

                let x = pen_x + x_offset + left as f32;
                let y = baseline_y - y_offset - top as f32 + self.font_size_px.descender; // Adjust for descender

                let fg_color = [
                    term_g.fg_color.red as f32 / 255.0,
                    term_g.fg_color.green as f32 / 255.0,
                    term_g.fg_color.blue as f32 / 255.0,
                ];

                if w > 0.0 && h > 0.0 {
                    #[rustfmt::skip]
                    let mut vertex_positions = [
                        (0.0, 0.0),
                        (  w, 0.0),
                        (  w,   h),
                        (0.0, 0.0),
                        (  w,   h),
                        (0.0,   h),
                    ];

                    // UV rect of this glyph's region within the shared atlas texture.
                    let u0 = atlas_x as f32 / ATLAS_SIZE as f32;
                    let v0 = atlas_y as f32 / ATLAS_SIZE as f32;
                    let u1 = (atlas_x + width) as f32 / ATLAS_SIZE as f32;
                    let v1 = (atlas_y + height) as f32 / ATLAS_SIZE as f32;

                    let vertex_uvs = [(u0, v0), (u1, v0), (u1, v1), (u0, v0), (u1, v1), (u0, v1)];

                    let shear = if matrix.xx != 0.0 {
                        (matrix.xy / matrix.xx) as f32
                    } else {
                        0.0
                    };

                    // Transform vertex_uvs to match terminal cell height and width
                    if is_color {
                        // vertex_uvs = vertex_uvs.map(|(x, y)| {
                        //     let x = x * scale;
                        //     let y = y * scale;
                        //     (x, y)
                        // });

                        vertex_positions = vertex_positions.map(|(x, y)| {
                            let x = x / scale;
                            let y = y / scale;

                            (x - y * shear, y)
                        });

                        let min_x = vertex_positions
                            .iter()
                            .map(|(x, _)| *x)
                            .fold(f32::INFINITY, f32::min);

                        vertex_positions = vertex_positions.map(|(x, y)| (x - min_x, y)); // Shift left to align with cell start
                    }

                    vertex_positions =
                        vertex_positions.map(|(local_x, local_y)| (local_x + x, local_y + y));

                    let is_color_f = if is_color { 1.0 } else { 0.0 };

                    all_vertices.extend(vertex_positions.iter().zip(vertex_uvs.iter()).map(
                        |((px, py), (u, v))| Vertex {
                            pos: [*px, *py],
                            uv: [*u, *v],
                            color: fg_color,
                            is_color: is_color_f,
                        },
                    ));
                }

                // NOTE: due to the way text is rendered in a terminal (in a fixed grid),
                // we ignore the actual x_advance and just move the pen by the cell width.
                pen_x += self.font_size_px.width * cell_width as f32;
            }

            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&all_vertices),
                glow::DYNAMIC_DRAW,
            );

            // FIX:
            // Limit drawing to the row to prevent glyphs from bleeding into adjacent rows
            // while allowing ligatures and diacritics to render correctly
            // within neighbouring cells.
            // gl.enable(glow::SCISSOR_TEST);
            // gl.scissor(
            //     pen_x as i32,
            //     self.text_manager.window_height - cell_box.y,
            //     width + left,
            //     cell_box.height,
            // );

            gl.draw_arrays(glow::TRIANGLES, 0, all_vertices.len() as i32);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);

            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.use_program(None);
        }
    }

    pub unsafe fn draw_cursor(
        &mut self,
        row: i32,
        col: i32,
        color: [f32; 4],
        cursor_style: CursorStyle,
        thickness: u32,
    ) {
        let cell_box = self.text_manager.get_cell_box(row, col);

        match cursor_style {
            CursorStyle::BlinkingUnderline | CursorStyle::SteadyUnderline => {
                let underline_height = thickness as i32;
                let y = cell_box.y + cell_box.height - underline_height;
                self.clear_section(cell_box.x, y, cell_box.width, underline_height, color);
            }
            CursorStyle::BlinkingBar | CursorStyle::SteadyBar => {
                let bar_width = thickness as i32;
                self.clear_section(cell_box.x, cell_box.y, bar_width, cell_box.height, color);
            }

            _ => {}
        }
    }

    pub fn set_viewport(&mut self, win: &TermWindow) {
        self.text_manager.set_window_size(win);
    }
}

impl<'a> Drop for TextRenderer<'a> {
    fn drop(&mut self) {
        unsafe {
            let gl = self.gl.as_ref();

            gl.delete_texture(self.atlas_tex);

            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
            gl.delete_program(self.program);
        }
    }
}

fn hb_to_px(v: f32, px_size: f32, units_per_em: f32) -> f32 {
    v * (px_size / units_per_em)
}
