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
mod utils;
mod win;

fn main() {
    let cols = 80;
    let rows = 24;

    let term = Term::new(cols, rows);

    // xinit

    let event_loop = EventLoop::new().expect("failed to create winit event loop");

    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(8)
        .with_transparency(true);

    let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attributes()));

    let mut app = App::new(term, template, display_builder);

    // `term` moves (twice: into App::new, then into the App struct's `term`
    // field) before this point, so its final, stable address is only known
    // once it's a field of `app`, which itself never moves again (only
    // borrowed) for the rest of `main`. Setting this pointer any earlier
    // (e.g. on the pre-move `term` local) leaves it dangling at a stack slot
    // that gets reused by later locals, corrupting memory the first time
    // something dereferences it (e.g. the OSC 10/11/12 color-query handling
    // in stresq.rs, which nvim exercises on startup) — this was a real,
    // intermittent crash.
    app.term.state._term_ptr = &mut app.term as *mut Term;

    match event_loop.run_app(&mut app) {
        Ok(_) => (),
        Err(e) => eprintln!("Application error: {e}"),
    };

    // Drop GL/EGL resources here, on the main thread, before possibly
    // calling process::exit below (which skips destructors) or returning.
    drop(app);

    let child_exit_code = terminal::CHILD_EXIT_CODE.load(std::sync::atomic::Ordering::SeqCst);
    if child_exit_code > 0 {
        std::process::exit(child_exit_code);
    }
}
