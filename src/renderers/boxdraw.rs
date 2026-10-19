use std::mem::{offset_of, size_of};
use std::rc::Rc;

use glow::HasContext;

use crate::macros::macs::include_shader;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    local: [f32; 2],
    color: [f32; 4],
    code: u32,
}

unsafe impl bytemuck::Pod for Vertex {}
unsafe impl bytemuck::Zeroable for Vertex {}

/// A box drawing cell, in window pixels with a top-left origin.
pub struct BoxCell {
    pub x: f32,
    pub y: f32,
    /// A `boxdraw::boxdrawindex` code.
    pub code: u32,
    pub color: [f32; 4],
}

pub struct BoxDrawRenderer {
    gl: Rc<glow::Context>,
    program: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    vbo: glow::NativeBuffer,

    u_proj: Option<glow::NativeUniformLocation>,
    u_cell_size: Option<glow::NativeUniformLocation>,
}

impl BoxDrawRenderer {
    pub unsafe fn new(gl: Rc<glow::Context>) -> Self {
        unsafe {
            let program = include_shader!(gl, "boxdraw");
            let vao = gl
                .create_vertex_array()
                .expect("failed to create boxdraw VAO");
            let vbo = gl.create_buffer().expect("failed to create boxdraw VBO");

            let stride = size_of::<Vertex>() as i32;

            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));

            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);

            gl.enable_vertex_attrib_array(1);
            let local_offset = offset_of!(Vertex, local) as i32;
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, local_offset);

            gl.enable_vertex_attrib_array(2);
            let color_offset = offset_of!(Vertex, color) as i32;
            gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, stride, color_offset);

            gl.enable_vertex_attrib_array(3);
            let code_offset = offset_of!(Vertex, code) as i32;
            gl.vertex_attrib_pointer_i32(3, 1, glow::UNSIGNED_INT, stride, code_offset);

            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);

            Self {
                gl: gl.clone(),
                program,
                vao,
                vbo,
                u_proj: gl.get_uniform_location(program, "u_proj"),
                u_cell_size: gl.get_uniform_location(program, "u_cell_size"),
            }
        }
    }

    /// Cell positions and size must be whole pixels so shapes stay crisp.
    pub unsafe fn draw(
        &self,
        cells: &[BoxCell],
        cell_width: f32,
        cell_height: f32,
        proj: &[f32; 16],
    ) {
        if cells.is_empty() {
            return;
        }

        let vertices: Vec<Vertex> = cells
            .iter()
            .flat_map(|cell| {
                let &BoxCell { x, y, code, color } = cell;
                let (w, h) = (cell_width, cell_height);
                let (right, bottom) = (x + w, y + h);

                #[rustfmt::skip]
                let quad = [
                    ([x,     y     ], [0.0, 0.0]),
                    ([right, y     ], [w,   0.0]),
                    ([right, bottom], [w,   h  ]),
                    ([x,     y     ], [0.0, 0.0]),
                    ([right, bottom], [w,   h  ]),
                    ([x,     bottom], [0.0, h  ]),
                ];

                quad.map(|(pos, local)| Vertex {
                    pos,
                    local,
                    color,
                    code,
                })
            })
            .collect();

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
            gl.uniform_2_f32(self.u_cell_size.as_ref(), cell_width, cell_height);

            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&vertices),
                glow::DYNAMIC_DRAW,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, vertices.len() as i32);

            gl.disable(glow::BLEND);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.use_program(None);
        }
    }
}

impl Drop for BoxDrawRenderer {
    fn drop(&mut self) {
        unsafe {
            let gl = self.gl.as_ref();

            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
            gl.delete_program(self.program);
        }
    }
}
