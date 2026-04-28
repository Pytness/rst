use bitflags::bitflags;

bitflags! {
    pub struct WinMode: u32 {
        const MODE_VISIBLE     = 1 << 0;
        const MODE_FOCUSED     = 1 << 1;
        const MODE_APPKEYPAD   = 1 << 2;
        const MODE_MOUSEBTN    = 1 << 3;
        const MODE_MOUSEMOTION = 1 << 4;
        const MODE_REVERSE     = 1 << 5;
        const MODE_KBDLOCK     = 1 << 6;
        const MODE_HIDE        = 1 << 7;
        const MODE_APPCURSOR   = 1 << 8;
        const MODE_MOUSESGR    = 1 << 9;
        const MODE_8BIT        = 1 << 10;
        const MODE_BLINK       = 1 << 11;
        const MODE_FBLINK      = 1 << 12;
        const MODE_FOCUS       = 1 << 13;
        const MODE_MOUSEX10    = 1 << 14;
        const MODE_MOUSEMANY   = 1 << 15;
        const MODE_BRCKTPASTE  = 1 << 16;
        const MODE_NUMLOCK     = 1 << 17;

        const MODE_MOUSE =
              Self::MODE_MOUSEBTN.bits()
            | Self::MODE_MOUSEMOTION.bits()
            | Self::MODE_MOUSEX10.bits()
            | Self::MODE_MOUSEMANY.bits()
            | Self::MODE_MOUSESGR.bits();
    }
}

#[rustfmt::skip]
enum CursorStyle {
    BlinkingBlock =        0, // blinking block
    BlinkingBlockDefault = 1, // blinking block (default)
    SteadyBlock          = 2, // steady block ("█")
    BlinkingUnderline    = 3, // blinking underline
    SteadyUnderline      = 4, // steady underline ("_")
    BlinkingBar          = 5, // blinking bar
    SteadyBar            = 6, // steady bar ("|")
}

struct TermWindow {
    // int tw, th; /* tty width and height */
    // int w, h;   /* window width and height */
    // int hborderpx, vborderpx;
    // int ch;     /* char height */
    // int cw;     /* char width  */
    // int mode;   /* window state/mode flags */
    // int cursor; /* cursor style */
    tw: i32,
    th: i32,
    w: i32,
    h: i32,
    hborderpx: i32,
    vborderpx: i32,
    ch: i32,
    cw: i32,
    mode: WinMode,
    cursor: CursorStyle,
}
