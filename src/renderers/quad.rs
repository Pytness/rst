use std::rc::Rc;

use glow::HasContext;

use crate::macros::macs::include_shader;

pub struct QuadRenderer {
    gl: Rc<glow::Context>,
    program: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    fbo: glow::NativeFramebuffer,
    color_tex: glow::NativeTexture,
}

impl QuadRenderer {
    pub unsafe fn new(gl: Rc<glow::Context>, width: i32, height: i32) -> Self {
        unsafe {
            let program = include_shader!(gl, "quad");
            let vao = gl.create_vertex_array().unwrap();
            let vbo = gl.create_buffer().unwrap();
            let fbo = gl.create_framebuffer().unwrap();
            let color_tex = gl.create_texture().unwrap();

            // Fullscreen quad vertex data: (pos.x, pos.y, uv.x, uv.y) x 6
            #[rustfmt::skip]
            let vertices: [f32; 24] = [
                //  x,     y,   u, v
                -1.0, -1.0, 0.0, 0.0,
                 1.0, -1.0, 1.0, 0.0,
                 1.0,  1.0, 1.0, 1.0,
                -1.0, -1.0, 0.0, 0.0,
                 1.0,  1.0, 1.0, 1.0,
                -1.0,  1.0, 0.0, 1.0,
            ];

            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&vertices),
                glow::STATIC_DRAW,
            );
            let stride = 4 * std::mem::size_of::<f32>() as i32;

            // position attribute (location=0)
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
            // uv attribute (location=1)
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(
                1,
                2,
                glow::FLOAT,
                false,
                stride,
                2 * std::mem::size_of::<f32>() as i32,
            );
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);
            gl.delete_buffer(vbo);

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.bind_texture(glow::TEXTURE_2D, Some(color_tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                width,
                height,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
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
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(color_tex),
                0,
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);

            Self {
                gl,
                program,
                vao,
                fbo,
                color_tex,
            }
        }
    }

    pub unsafe fn render(&self) {
        let gl = &self.gl;

        unsafe {
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.color_tex));

            if let Some(loc) = gl.get_uniform_location(self.program, "u_tex") {
                gl.uniform_1_i32(Some(&loc), 0);
            }

            gl.draw_arrays(glow::TRIANGLES, 0, 6);

            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    }

    pub unsafe fn with(&self, f: impl FnOnce()) {
        let gl = &self.gl;

        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));

            f();

            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
    }

    pub unsafe fn clear_section(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: (f32, f32, f32, f32),
    ) {
        let gl = &self.gl;

        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(x, y, width, height);
            gl.clear_color(color.0, color.1, color.2, color.3);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.disable(glow::SCISSOR_TEST);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
    }
}

impl Drop for QuadRenderer {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_program(self.program);
            self.gl.delete_vertex_array(self.vao);
            self.gl.delete_framebuffer(self.fbo);
            self.gl.delete_framebuffer(self.fbo);
            self.gl.delete_texture(self.color_tex);
        }
    }
}
