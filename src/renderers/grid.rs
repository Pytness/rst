use std::rc::Rc;

use glow::HasContext;

use crate::macros::macs::include_shader;

pub struct GridRenderer {
    gl: Rc<glow::Context>,
    program: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    vbo: glow::NativeBuffer,
    u_resolution: glow::NativeUniformLocation,
    u_grid_size: glow::NativeUniformLocation,
    u_offset: glow::NativeUniformLocation,
    u_bg_color: glow::NativeUniformLocation,
    u_line_color: glow::NativeUniformLocation,
    u_line_width: glow::NativeUniformLocation,
}

impl GridRenderer {
    pub unsafe fn new(gl: Rc<glow::Context>) -> Self {
        unsafe {
            let program = include_shader!(gl, "grid");
            let vao = gl.create_vertex_array().expect("Cannot create VAO");
            let vbo = gl.create_buffer().expect("Cannot create VBO");

            // uniform vec2 u_resolution;   // window size in pixels
            // uniform vec2 u_grid_size;    // (W, H) size of each cell in pixels
            // uniform vec2 u_offset;       // grid offset in pixels
            // uniform vec4 u_bg_color;     // background color
            // uniform vec4 u_line_color;   // grid line color
            // uniform float u_line_width;  // line thickness in pixels

            let u_resolution = gl
                .get_uniform_location(program, "u_resolution")
                .expect("Cannot find uniform u_resolution");
            let u_grid_size = gl
                .get_uniform_location(program, "u_grid_size")
                .expect("Cannot find uniform u_grid_size");
            let u_offset = gl
                .get_uniform_location(program, "u_offset")
                .expect("Cannot find uniform u_offset");
            let u_bg_color = gl
                .get_uniform_location(program, "u_bg_color")
                .expect("Cannot find uniform u_bg_color");
            let u_line_color = gl
                .get_uniform_location(program, "u_line_color")
                .expect("Cannot find uniform u_line");
            let u_line_width = gl
                .get_uniform_location(program, "u_line_width")
                .expect("Cannot find uniform u_line_width");

            Self {
                gl,
                program,
                vao,
                vbo,
                u_resolution,
                u_grid_size,
                u_offset,
                u_bg_color,
                u_line_color,
                u_line_width,
            }
        }
    }

    pub unsafe fn gl(&self) -> &glow::Context {
        &self.gl
    }

    pub unsafe fn render(
        &self,
        cell_size: (usize, usize),
        offset: (usize, usize),
        window: (usize, usize),
    ) {
        unsafe {
            let gl = self.gl();
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));

            gl.use_program(Some(self.program));

            gl.uniform_2_f32(Some(&self.u_resolution), window.0 as f32, window.1 as f32);
            gl.uniform_2_f32(
                Some(&self.u_grid_size),
                cell_size.0 as f32,
                cell_size.1 as f32,
            );
            gl.uniform_2_f32(Some(&self.u_offset), offset.0 as f32, offset.1 as f32);
            gl.uniform_4_f32(Some(&self.u_bg_color), 0.1, 0.1, 0.1, 0.0);
            gl.uniform_4_f32(Some(&self.u_line_color), 1.0, 1.0, 1.0, 1.);
            gl.uniform_1_f32(Some(&self.u_line_width), 1.0);

            gl.draw_arrays(glow::TRIANGLES, 0, 6);

            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.use_program(None);
        }
    }
}
