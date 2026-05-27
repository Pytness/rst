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

static FT_LIB: LazyLock<Library> =
    LazyLock::new(|| Library::init().expect("failed to initialize FreeType library"));

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
}

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
    tex: glow::NativeTexture,
    width: i32,
    height: i32,
    left: i32,
    top: i32,
    is_color: bool,
    scale: f32,
    matrix: FcMatrix,
    cell_width: usize,
}

pub struct TextRenderer<'a> {
    gl: Rc<glow::Context>,
    font_registry: &'a FontRegistry,

    pub text_manager: TextManager,

    glyphs: HashMap<(usize, u32, FontStyle), GlyphTexture>,

    program: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    vbo: glow::NativeBuffer,

    u_proj: Option<glow::NativeUniformLocation>,
    u_color: Option<glow::NativeUniformLocation>,
    u_background_color: Option<glow::NativeUniformLocation>,
    u_is_color: Option<glow::NativeUniformLocation>,
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
            println!("Warning: failed to set LCD filter (error code {})", err);
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

        let vao = unsafe { gl.create_vertex_array().unwrap() };
        let vbo = unsafe { gl.create_buffer().unwrap() };

        let u_proj = unsafe { gl.get_uniform_location(program, "u_proj") };
        let u_color = unsafe { gl.get_uniform_location(program, "u_text_color") };
        let u_background_color = unsafe { gl.get_uniform_location(program, "u_background_color") };
        let u_is_color = unsafe { gl.get_uniform_location(program, "u_is_color") };
        let u_tex = unsafe { gl.get_uniform_location(program, "u_font") };

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
            glyphs: HashMap::new(),
            program,
            vao,
            vbo,
            u_proj,
            u_background_color,
            u_is_color,
            u_color,
            u_tex,
            font_size_px,
            px_size: px_size as f32,
        }
    }

    pub fn clear_section(&self, x: i32, y: i32, width: i32, height: i32, color: [f32; 4]) {
        let y = self.text_manager.window_height - y; // Convert from top-left to bottom-left origin

        unsafe {
            let gl = self.gl.as_ref();
            gl.enable(glow::SCISSOR_TEST);

            gl.scissor(x, y, width, height);
            gl.clear_color(color[0], color[1], color[2], 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.disable(glow::SCISSOR_TEST);
        }
    }

    /// Ensures the glyph is loaded and cached.
    /// Returns a tuple of (left, top, width, height, tex) to avoid holding a
    /// reference into `self.glyphs` across subsequent `self` accesses.
    fn ensure_glyph(&mut self, glyph: &ShapedGlyph, style: FontStyle) -> Option<&GlyphTexture> {
        let key = (glyph.font_index, glyph.glyph_id, style);
        if !self.glyphs.contains_key(&key) {
            let texture = unsafe { self.load_glyph_texture(glyph, style)? };
            self.glyphs.insert(key, texture);
        }

        self.glyphs.get(&key)
    }

    /// Rasterises a single glyph with FreeType and uploads it to a GL texture.
    unsafe fn load_glyph_texture(
        &self,
        glyph: &ShapedGlyph,
        style: FontStyle,
    ) -> Option<GlyphTexture> {
        let style = self.font_registry.get_fonts()[glyph.font_index].style(style);
        let ft_face = &style.ft_face;
        let matrix = style.matrix;
        // style.set_transform();

        ft_face
            .load_glyph(
                glyph.glyph_id,
                LoadFlag::RENDER
                    | LoadFlag::FORCE_AUTOHINT
                    | LoadFlag::TARGET_NORMAL
                    | LoadFlag::COLOR,
            )
            .expect("freetype load_glyph failed");

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

        let internal_format = match pixel_mode {
            PixelMode::Gray => glow::R8,
            PixelMode::Lcd | PixelMode::LcdV => glow::RGB8,
            PixelMode::Bgra => glow::RGBA8,
            _ => glow::RED,
        } as i32;

        let format = match pixel_mode {
            PixelMode::Gray => glow::RED,
            PixelMode::Lcd | PixelMode::LcdV => glow::RGB,
            PixelMode::Bgra => glow::BGRA,
            _ => glow::RED,
        };

        let cell_width = if is_color {
            2
        } else {
            unicode_width::UnicodeWidthChar::width(glyph.char).unwrap_or(1)
        };
        let scale = (height as f32 / self.font_size_px.height).max(1.0);

        unsafe {
            let gl = self.gl.as_ref();
            let tex = gl.create_texture().ok().expect("failed to create texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));

            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, alignment);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                internal_format,
                width,
                height,
                0,
                format,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(bitmap.buffer())),
            );

            gl.generate_mipmap(glow::TEXTURE_2D);

            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_BORDER as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_BORDER as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR_MIPMAP_LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );

            Some(GlyphTexture {
                tex,
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
                term_g.bg_color.0 as f32 / 255.0,
                term_g.bg_color.1 as f32 / 255.0,
                term_g.bg_color.2 as f32 / 255.0,
            ];

            self.clear_section(
                cell_box.x,
                cell_box.y,
                cell_box.width * cell_width,
                cell_box.height,
                [bg_color[0], bg_color[1], bg_color[2], 1.0],
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

        let text = glyphs.iter().map(|g| g.char).collect::<Vec<char>>();
        let shaped = self.font_registry.shape_text(&text);

        let units_per_em = self.units_per_em();
        let px_size = self.px_size;

        let stride = size_of::<Vertex>() as i32;
        let uv_offset = offset_of!(Vertex, uv) as i32;

        let glyphs_iter = glyphs.iter().zip(shaped.iter());

        let glyph_widths: Vec<usize> = shaped
            .iter()
            .zip(glyphs.iter())
            .map(|(s, g)| {
                self.ensure_glyph(s, g.font_style)
                    .map(|t| t.cell_width)
                    .unwrap_or(1)
            })
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
            for (term_g, shaped_g) in glyphs_iter {
                let Some(&GlyphTexture {
                    left,
                    mut top,
                    width,
                    height,
                    tex,
                    is_color,
                    scale,
                    matrix,
                    cell_width,
                }) = self.ensure_glyph(shaped_g, term_g.font_style)
                else {
                    println!("Warning: glyph ID {} not found in font", shaped_g.glyph_id);
                    continue;
                };

                // println!(
                //     "Drawing glyph ({})='{}': left={}, top={}, width={}, height={}, is_color={}, scale={}",
                //     shaped_g.glyph_id,
                //     shaped_g.char, left, top, width, height, is_color, scale
                // );

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
                    term_g.fg_color.0 as f32 / 255.0,
                    term_g.fg_color.1 as f32 / 255.0,
                    term_g.fg_color.2 as f32 / 255.0,
                ];
                let bg_color = [
                    term_g.bg_color.0 as f32 / 255.0,
                    term_g.bg_color.1 as f32 / 255.0,
                    term_g.bg_color.2 as f32 / 255.0,
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

                    let vertex_uvs = [
                        (0.0, 0.0),
                        (1.0, 0.0),
                        (1.0, 1.0),
                        (0.0, 0.0),
                        (1.0, 1.0),
                        (0.0, 1.0),
                    ];

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

                    let vertices: Vec<Vertex> = vertex_positions
                        .iter()
                        .zip(vertex_uvs.iter())
                        .map(|((px, py), (u, v))| Vertex {
                            pos: [*px, *py],
                            uv: [*u, *v],
                        })
                        .collect();

                    let gl = self.gl.as_ref();

                    gl.uniform_3_f32(self.u_color.as_ref(), fg_color[0], fg_color[1], fg_color[2]);
                    gl.uniform_3_f32(
                        self.u_background_color.as_ref(),
                        bg_color[0],
                        bg_color[1],
                        bg_color[2],
                    );
                    gl.active_texture(glow::TEXTURE0);
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex));

                    gl.buffer_data_u8_slice(
                        glow::ARRAY_BUFFER,
                        bytemuck::cast_slice(&vertices),
                        glow::DYNAMIC_DRAW,
                    );
                    gl.program_uniform_1_u32(
                        self.program,
                        self.u_is_color.as_ref(),
                        if is_color { 1 } else { 0 },
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

                    gl.draw_arrays(glow::TRIANGLES, 0, 6);
                    gl.disable(glow::SCISSOR_TEST);
                }

                // NOTE: due to the way text is rendered in a terminal (in a fixed grid),
                // we ignore the actual x_advance and just move the pen by the cell width.
                pen_x += self.font_size_px.width * cell_width as f32;
            }

            let gl = self.gl.as_ref();
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.use_program(None);
        }
    }

    pub fn set_viewport(&mut self, width: i32, height: i32) {
        self.text_manager.set_window_size(width, height);
    }
}

impl<'a> Drop for TextRenderer<'a> {
    fn drop(&mut self) {
        unsafe {
            let gl = self.gl.as_ref();

            for glyph in self.glyphs.values() {
                gl.delete_texture(glyph.tex);
            }

            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
            gl.delete_program(self.program);
        }
    }
}

fn hb_to_px(v: f32, px_size: f32, units_per_em: f32) -> f32 {
    v * (px_size / units_per_em)
}
