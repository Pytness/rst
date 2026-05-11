use glutin::config::ConfigTemplateBuilder;
use glutin_winit::DisplayBuilder;
use winit::event_loop::EventLoop;

use self::app::App;
use self::gl_handler::window_attributes;
use self::terminal::Term;
use self::win::TermWindow;

mod app;
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
    let cols = 80;
    let rows = 24;

    let mut term = Term::new(cols, rows);
    let mut win = TermWindow::default();

    let ttyfd = term.ttynew(None, Some("/bin/zsh"), None, None);
    // xinit

    let event_loop = EventLoop::new().unwrap();

    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(8)
        .with_transparency(true);

    let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attributes()));

    let mut app = App::new(term, win, template, display_builder);

    match event_loop.run_app(&mut app) {
        Ok(_) => (),
        Err(e) => eprintln!("Application error: {e}"),
    };
}

fn xinit(cols: usize, rows: usize) {}
