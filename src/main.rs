use glutin::config::ConfigTemplateBuilder;
use glutin_winit::DisplayBuilder;
use winit::event_loop::EventLoop;

use self::app::App;
use self::gl_handler::window_attributes;
use self::terminal::Term;

mod app;
mod boxdraw;
mod colors;
mod config;
mod csiesq;
mod font_registry;
mod gl_handler;
mod glyph;
mod graphics;
mod keymap;
mod kitty;
mod macros;
mod renderers;
mod sixel;
mod stresq;
mod term_state;
mod terminal;
mod text_manager;
mod win;

fn main() {
    let cols = 80;
    let rows = 24;

    let mut term = Term::new(cols, rows);
    // FIX: why oh why
    term.state._term_ptr = &mut term as *mut Term;

    // xinit

    let event_loop = EventLoop::new().expect("failed to create winit event loop");

    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(8)
        .with_transparency(true);

    let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attributes()));

    let mut app = App::new(term, template, display_builder);

    match event_loop.run_app(&mut app) {
        Ok(_) => (),
        Err(e) => eprintln!("Application error: {e}"),
    };
}

fn xinit(_cols: usize, _rows: usize) {}
