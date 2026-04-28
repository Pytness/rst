type KeySym = u64;

union Arg {
    i: i32,
    ui: u32,
    f: f32,
    v: *mut std::ffi::c_void,
    s: *mut std::ffi::c_char,
}

pub struct Shortcut {
    modifier: u32,
    keysym: KeySym,
    // function
    // arg
}

pub struct MouseShortcut {
    modifier: u32,
    button: u32,
    release: bool,
    // function
    // arg
}

struct Key {
    key: KeySym,
    mask: u32,
    // char *s
    // signed char appkey
    // signed char appcursor
}

fn clipcopy(arg: Args) {}
fn clippaste(arg: Args) {}
fn numlock(arg: Args) {}
fn selpaste(arg: Args) {}
fn zoom(arg: Args) {}
fn zoomabs(arg: Args) {}
fn zoomreset(arg: Args) {}
fn tyysend(arg: Args) {}
fn previewimage(arg: Args) {}
fn shoimageinfo(arg: Args) {}
fn togglegrdebug(arg: Args) {}
fn dumpgrstate(arg: Args) {}
fn unloadimages(arg: Args) {}
fn toggleimages(arg: Args) {}

struct TermWindow {
    term_width: usize,
    term_height: usize,
    window_width: usize,
    window_height: usize,
    height_border_px: usize,
    width_border_px: usize,
}
