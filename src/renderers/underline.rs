use std::mem::{offset_of, size_of};
use std::rc::Rc;

use glow::HasContext;

use crate::config;
use crate::glyph::UnderlineStyle;
use crate::macros::macs::include_shader;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    local: [f32; 2],
}

unsafe impl bytemuck::Pod for Vertex {}
unsafe impl bytemuck::Zeroable for Vertex {}

/// Pixel metrics mirroring st's `xdrawglyphfontspecs`.
#[derive(Clone, Copy)]
pub struct UnderlineMetrics {
    thick: f32,
    /// Between the baseline and the top of the underline.
    gap: f32,
    /// Total height of a double underline.
    doubleh: f32,
    /// Total height of an undercurl, including the stroke.
    curlh: f32,
    dash_period: f32,
    curl_period: f32,
}

impl UnderlineMetrics {
    pub fn new(font_height: f32, cell_width: f32) -> Self {
        let fthick = font_height / 18.0;
        let thick = fthick.round().max(1.0);

        Self {
            thick,
            gap: (fthick * 2.0).round(),
            doubleh: thick * 2.0 + (fthick * 0.5).ceil(),
            curlh: thick * 2.0 + (fthick * 0.75).round(),
            dash_period: (cell_width * 0.9).floor().max(2.0),
            curl_period: cell_width / config::CURLY_UNDERLINE_FREQUENCY_PER_CHAR,
        }
    }

    pub fn underline_top(&self, baseline_y: f32) -> f32 {
        baseline_y + self.gap
    }
}

/// A horizontal underline span, in window pixels with a top-left origin.
pub struct Underline {
    pub x: f32,
    pub width: f32,
    pub style: UnderlineStyle,
    pub color: [f32; 4],
}

pub struct UnderlineRenderer {
    gl: Rc<glow::Context>,
    program: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    vbo: glow::NativeBuffer,

    u_proj: Option<glow::NativeUniformLocation>,
    u_style: Option<glow::NativeUniformLocation>,
    u_thickness: Option<glow::NativeUniformLocation>,
    u_color: Option<glow::NativeUniformLocation>,
    u_band_height: Option<glow::NativeUniformLocation>,
    u_period: Option<glow::NativeUniformLocation>,
    u_dash: Option<glow::NativeUniformLocation>,
}

impl UnderlineRenderer {
    pub fn new(gl: Rc<glow::Context>) -> Self {
        unsafe {
            let program = include_shader!(gl, "underline");
            let vao = gl
                .create_vertex_array()
                .expect("failed to create underline VAO");
            let vbo = gl.create_buffer().expect("failed to create underline VBO");

            let stride = size_of::<Vertex>() as i32;
            let local_offset = offset_of!(Vertex, local) as i32;

            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));

            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);

            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, local_offset);

            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);

            Self {
                gl: gl.clone(),
                program,
                vao,
                vbo,
                u_proj: gl.get_uniform_location(program, "u_proj"),
                u_style: gl.get_uniform_location(program, "u_style"),
                u_thickness: gl.get_uniform_location(program, "u_thickness"),
                u_color: gl.get_uniform_location(program, "u_color"),
                u_band_height: gl.get_uniform_location(program, "u_band_height"),
                u_period: gl.get_uniform_location(program, "u_period"),
                u_dash: gl.get_uniform_location(program, "u_dash"),
            }
        }
    }

    /// `line_top` is where a single-thickness line starts. It and
    /// `cell_bottom` must be whole pixels so strokes stay crisp.
    pub fn draw(
        &self,
        spans: &[Underline],
        metrics: &UnderlineMetrics,
        line_top: f32,
        cell_bottom: f32,
        proj: &[f32; 16],
    ) {
        if spans.is_empty() {
            return;
        }

        unsafe {
            let gl = self.gl.as_ref();

            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.use_program(Some(self.program));

            gl.enable(glow::BLEND);
            gl.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );

            gl.uniform_matrix_4_f32_slice(self.u_proj.as_ref(), false, proj);

            gl.uniform_1_f32(self.u_thickness.as_ref(), metrics.thick);

            for span in spans {
                self.draw_span(span, metrics, line_top, cell_bottom);
            }

            gl.disable(glow::BLEND);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.use_program(None);
        }
    }

    /// Expects the program, VAO and VBO to be bound by `draw`.
    fn draw_span(
        &self,
        underline: &Underline,
        metrics: &UnderlineMetrics,
        line_top: f32,
        cell_bottom: f32,
    ) {
        let Underline {
            x,
            width,
            style,
            color,
        } = *underline;

        if width <= 0.0 {
            return;
        }

        let m = metrics;
        let (height, period, dash) = match style {
            UnderlineStyle::UnderlineDouble => (m.doubleh, 0.0, 0.0),
            UnderlineStyle::UnderlineCurly => (m.curlh, m.curl_period, 0.0),
            UnderlineStyle::UnderlineDotted => (m.thick, m.thick * 2.0, m.thick),
            UnderlineStyle::UnderlineDashed => (
                m.thick,
                m.dash_period,
                (m.dash_period * 0.65).floor().max(1.0),
            ),
            UnderlineStyle::UnderlineStraight => (m.thick, 0.0, 0.0),
        };

        // Fit a single underline in the cell first, then the taller styles,
        // so the next row's background clear does not cut them.
        let mut top = line_top;
        top -= (top + m.thick - cell_bottom).max(0.0);
        top -= (top + height - cell_bottom).max(0.0);

        let bottom = top + height;
        let right = x + width;

        // Local x is the absolute window x so patterns stay continuous
        // across spans drawn separately.
        #[rustfmt::skip]
        let vertices = [
            Vertex { pos: [x,     top   ], local: [x,     0.0   ] },
            Vertex { pos: [right, top   ], local: [right, 0.0   ] },
            Vertex { pos: [right, bottom], local: [right, height] },
            Vertex { pos: [x,     top   ], local: [x,     0.0   ] },
            Vertex { pos: [right, bottom], local: [right, height] },
            Vertex { pos: [x,     bottom], local: [x,     height] },
        ];

        unsafe {
            let gl = self.gl.as_ref();

            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&vertices),
                glow::DYNAMIC_DRAW,
            );

            gl.uniform_1_i32(self.u_style.as_ref(), style as i32);
            gl.uniform_4_f32(
                self.u_color.as_ref(),
                color[0],
                color[1],
                color[2],
                color[3],
            );
            gl.uniform_1_f32(self.u_band_height.as_ref(), height);
            gl.uniform_1_f32(self.u_period.as_ref(), period);
            gl.uniform_1_f32(self.u_dash.as_ref(), dash);

            gl.draw_arrays(glow::TRIANGLES, 0, vertices.len() as i32);
        }
    }
}

impl Drop for UnderlineRenderer {
    fn drop(&mut self) {
        unsafe {
            let gl = self.gl.as_ref();

            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
            gl.delete_program(self.program);
        }
    }
}
