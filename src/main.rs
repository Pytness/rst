mod config;
mod csiesq;
mod glyph;
mod graphics;
mod image;
mod macros;
mod sixel;
mod terminal;
mod win;
mod xlib;

use std::cmp::min;

use glyph::Glyph;

#[derive(Default)]
struct TCursor {
    attr: Glyph,
    x: u32,
    y: u32,
    state: u32,
}

#[derive(Default)]
struct Term {
    // int row;      /* nb row */
    // int col;      /* nb col */
    // int pixw;     /* width of the text area in pixels */
    // int pixh;     /* height of the text area in pixels */
    // Line *line;   /* screen */
    // Line *alt;    /* alternate screen */
    // int *dirty;   /* dirtyness of lines */
    // TCursor c;    /* cursor */
    // int ocx;      /* old cursor col */
    // int ocy;      /* old cursor row */
    // int top;      /* top    scroll limit */
    // int bot;      /* bottom scroll limit */
    // int mode;     /* terminal mode flags */
    // int esc;      /* escape state flags */
    // char trantbl[4]; /* charset table translation */
    // int charset;  /* current charset */
    // int icharset; /* selected charset for sequence */
    // int *tabs;
    // Rune lastc;   /* last printed char outside of sequence, 0 if control */
    row: i32,
    col: i32,
    pixw: u32,
    pixh: u32,
    // line: Line,
    // alt: Line,
    // dirty: Vec<u32>,
    c: TCursor,
    ocx: u32,
    ocy: u32,
    top: u32,
    bot: u32,
    mode: u32,
    esc: u32,
    trantbl: [u8; 4],
    charset: u32,
    icharset: u32,
    // tabs: Vec<u32>,
    lastc: char,
}

// fn tnew(cols: i32, rows: i32) -> Terminal {
//     let mut term = Term::default();
//     tresize(&mut term, cols, rows);
//     treset();
// }

fn tresize(term: &mut Term, cols: i32, rows: i32) {
    let i = 0;
    let minrow = min(rows, term.row);
    let mincol = min(cols, term.col);

    let cursor: TCursor = TCursor::default();

    if cols < 1 || rows < 1 {
        eprintln!("tresize: error resizing to {}x{}", cols, rows);
        return;
    }
}

fn treset() {}

fn xinit(cols: i32, rows: i32) {
    let _xlib = xlib::Xlib::open().unwrap();
    let display = unsafe { (_xlib.XOpenDisplay)(std::ptr::null()) };

    if display.is_null() {
        eprintln!("xinit: cannot open display");
        return;
    }

    let screen = unsafe { XDefaultScreen(display) };
    let visual = unsafe { XDefaultVisual(display, screen) };

    let colormap = unsafe { XDefaultColormap(display, screen) };
    let root = unsafe { XRootWindow(display, screen) };

    let width = 800;
    let height = 600;
    let left_offset = 10;
    let top_offset = 10;

    let window = unsafe {
        XCreateWindow(
            display,
            root,
            left_offset,
            top_offset,
            width,
            height,
            0,
            0,
            0,
            visual,
            0,
            std::ptr::null_mut(),
        )
    };
}

fn main() {
    xinit(80, 24);
}

fn run() {}
