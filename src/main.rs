use std::ffi::CString;

use glutin::config::ConfigTemplateBuilder;
use glutin::display::GetGlDisplay;
use glutin::prelude::{GlDisplay, PossiblyCurrentGlContext};
use glutin_winit::{DisplayBuilder, GlWindow};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

use self::gl_handler::{GlHandler, window_attributes};
use self::terminal::Term;

mod boxdraw;
mod config;
mod csiesq;
mod gl_handler;
mod glyph;
mod graphics;
mod kitty;
mod macros;
mod sixel;
mod terminal;
mod win;

fn main() {
    let mut event_loop = EventLoop::new().unwrap();

    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(8)
        .with_transparency(true);

    let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attributes()));

    let mut app = App::new(template, display_builder);

    event_loop.run_app(&mut app);
    // run();
}

fn run() {
    let cols = 80;
    let rows = 24;

    let term = Term::new(cols, rows);
    xinit(cols, rows);
}

fn xinit(cols: usize, rows: usize) {}

struct App {
    gl_handler: GlHandler,
}

impl App {
    pub fn new(template: ConfigTemplateBuilder, display_builder: DisplayBuilder) -> Self {
        Self {
            gl_handler: GlHandler::new(template, display_builder),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let gl_window = self.gl_handler.get_or_create_gl_window(event_loop);

        if gl_window.is_none() {
            return;
        }

        let (window, gl_config) = gl_window.unwrap();

        let attrs = window
            .build_surface_attributes(Default::default())
            .expect("Failed to build surface attributes");

        let gl_surface = unsafe {
            gl_config
                .display()
                .create_window_surface(&gl_config, &attrs)
                .expect("Failed to create surface")
        };

        let gl_context = self.gl_handler.gl_context.as_ref().unwrap();
        gl_context.make_current(&gl_surface).unwrap();

        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                let symbol = CString::new(s).unwrap();
                gl_config.display().get_proc_address(symbol.as_c_str())
            })
        };

        // self.gl = Some(Rc::new(gl));
        //
        // self.triangle_renderer.get_or_insert_with(|| unsafe {
        //     renderers::TriangleRenderer::new(self.gl.as_ref().unwrap().clone())
        // });
        // self.text_renderer.get_or_insert_with(|| unsafe {
        //     TextRenderer::new(
        //         self.gl.as_ref().unwrap().clone(),
        //         &self.font_registry,
        //         FONT_SIZE,
        //     )
        // });

        // FontRegistry must outlive TextRenderer
        // self.text_renderer.get_or_insert_with(|| unsafe {
        //     // This is safe because the font registry is owned by the App struct
        //     // and will not be dropped while the TextRenderer is still in use.
        //     let font_registry: &'a FontRegistry = &*(&self.font_registry as *const _);
        //
        //     let width = window.inner_size().width as i32;
        //     let height = window.inner_size().height as i32;
        //     TextRenderer::<'a>::new(
        //         self.gl.as_ref().unwrap().clone(),
        //         font_registry,
        //         self.conf_font_size_px,
        //         (width, height),
        //     )
        // });
        //
        // self.quad_renderer.get_or_insert_with(|| unsafe {
        //     renderers::QuadRenderer::new(
        //         self.gl.as_ref().unwrap().clone(),
        //         window.inner_size().width as i32,
        //         window.inner_size().height as i32,
        //     )
        // });
        //
        // self.state = Some(AppState { gl_surface, window });
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(size) if size.width != 0 && size.height != 0 => {
                // self.on_resize(&size);
            }
            WindowEvent::CloseRequested => {
                // event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // let start = Instant::now();
                // if let Some(AppState { gl_surface, window }) = &self.state {
                //     let gl_context = self.gl_handler.gl_context.as_ref().unwrap();
                //
                //     unsafe {
                //         let text_manager = &self.text_renderer.as_ref().unwrap().text_manager;
                //         let cell_size = (
                //             text_manager.font_width as usize,
                //             text_manager.font_height as usize,
                //         );
                //
                //         let offset = (
                //             text_manager.border_x_px as usize,
                //             text_manager.border_y_px as usize,
                //         );
                //
                //         let inner_size = window.inner_size();
                //         let size = (inner_size.width as usize, inner_size.height as usize);
                //
                //         self.quad_renderer.as_ref().unwrap().render();
                //
                //         self.grid_renderer
                //             .as_ref()
                //             .unwrap()
                //             .render(cell_size, offset, size);
                //     }
                //
                //     gl_surface.swap_buffers(gl_context).unwrap();
                // }
                //
                // let duration = start.elapsed();
                // println!("Redrawn in {} ms", duration.as_millis());
            }
            WindowEvent::KeyboardInput {
                device_id: _,
                event,
                is_synthetic: _,
            } => {
                //conf_font_size_px
                if event.repeat || event.state != winit::event::ElementState::Pressed {
                    return;
                }
                //
                // match event.physical_key {
                //     winit::keyboard::PhysicalKey::Code(KeyCode::Equal) => {
                //         self.conf_font_size_px += 1;
                //         println!("Increasing font size to {}", self.conf_font_size_px);
                //     }
                //     winit::keyboard::PhysicalKey::Code(KeyCode::Minus) => {
                //         if self.conf_font_size_px > 2 {
                //             self.conf_font_size_px -= 1;
                //             println!("Decreasing font size to {}", self.conf_font_size_px);
                //         }
                //     }
                //     _ => {}
                // }
                //
                // if let Some(AppState {
                //     gl_surface: _gl_surface,
                //     window,
                // }) = self.state.as_ref()
                // {
                //     self.text_renderer
                //         .as_mut()
                //         .unwrap()
                //         .update_font_size(self.conf_font_size_px, 96);
                //
                //     let size = window.inner_size();
                //     let physical_size = PhysicalSize::new(size.width, size.height);
                //     window.request_redraw();
                //     self.on_resize(&physical_size);
                // }
            }
            _ => (),
        }
    }
}
