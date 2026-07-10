#[macro_export]
macro_rules! BETWEEN {
    ($x:expr, $a:expr, $b:expr) => {
        ($x >= $a && $x <= $b)
    };
}

#[macro_export]
macro_rules! snprintf {
    ($buffer:expr, $format:expr, $($arg:expr),*) => {
        unsafe {
            libc::snprintf(
                $buffer.as_mut_ptr() as *mut i8,
                $buffer.len(),
                $format.as_ptr() as *const i8,
                $($arg),*
            )
        }
    };
}

pub(crate) mod macs {
    macro_rules! assets_path {
        ($name: literal) => {
            concat!(env!("CARGO_MANIFEST_DIR"), "/assets/", $name)
        };
    }

    macro_rules! include_shader {
        ($gl: ident, $name: literal ) => {{
            let gl = &$gl;

            let vs_source = include_str!(concat!(
                crate::macros::macs::assets_path!("shaders/"),
                $name,
                "/",
                $name,
                ".vert"
            ));

            let fs_source = include_str!(concat!(
                crate::macros::macs::assets_path!("shaders/"),
                $name,
                "/",
                $name,
                ".frag"
            ));

            let program = gl.create_program().expect("Cannot create program");

            let vs = gl
                .create_shader(glow::VERTEX_SHADER)
                .expect("Cannot create vertex shader");

            gl.shader_source(vs, vs_source);
            gl.compile_shader(vs);

            if !gl.get_shader_compile_status(vs) {
                panic!(
                    "Vertex shader compilation failed:\n{}",
                    gl.get_shader_info_log(vs)
                );
            }

            let fs = gl
                .create_shader(glow::FRAGMENT_SHADER)
                .expect("Cannot create fragment shader");
            gl.shader_source(fs, fs_source);
            gl.compile_shader(fs);
            if !gl.get_shader_compile_status(fs) {
                panic!(
                    "Fragment shader compilation failed:\n{}",
                    gl.get_shader_info_log(fs)
                );
            }

            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                panic!("Program link failed:\n{}", gl.get_program_info_log(program));
            }

            gl.delete_shader(vs);
            gl.delete_shader(fs);

            program
        }};
    }

    macro_rules! include_font {
        ($name: literal) => {{ include_bytes!(concat!(crate::macros::macs::assets_path!("fonts/"), $name)) }};
    }

    pub(crate) use {assets_path, include_font, include_shader};
}
