use x11_dl::xlib;

struct Lib {
    xlib: xlib::Xlib,
}

impl Lib {
    fn new() -> Self {
        Self {
            xlib: xlib::Xlib::open().unwrap(),
        }
    }

    fn XDefaultScreen(self, display: *mut xlib::Display) -> i32 {
        unsafe { (self.xlib.XDefaultScreen)(display) }
    }

    fn XftFontClose(self, display: *mut xlib::Display, font: *mut xlib::XftFont) {
        unsafe { (self.xlib.XftFontClose)(display, font) }
    }
}
