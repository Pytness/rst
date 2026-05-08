use self::terminal::Term;

mod boxdraw;
mod config;
mod csiesq;
mod glyph;
mod graphics;
mod kitty;
mod macros;
mod sixel;
mod terminal;
mod win;
mod xlib;

fn main() {
    run();
}

fn run() {
    let cols = 80;
    let rows = 24;

    let term = Term::new(cols, rows);
    xinit(cols, rows);
}

fn xinit(cols: usize, rows: usize) {}
