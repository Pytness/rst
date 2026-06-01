use bitflags::bitflags;

bitflags! {
    #[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
#[derive(Default)]
enum CursorStyle {

    BlinkingBlock =        0, // blinking block
    #[default]
    BlinkingBlockDefault = 1, // blinking block (default)
    SteadyBlock          = 2, // steady block ("█")
    BlinkingUnderline    = 3, // blinking underline
    SteadyUnderline      = 4, // steady underline ("_")
    BlinkingBar          = 5, // blinking bar
    SteadyBar            = 6, // steady bar ("|")
}

#[derive(Default)]
pub struct TermWindow {
    pub tw: i32,             // tty width
    pub th: i32,             // tty height
    pub w: u32,              // window width
    pub h: u32,              // window height
    pub hborderpx: u32,      // horizontal border in pixels
    pub vborderpx: u32,      // vertical border in pixels
    pub ch: u32,             // char height
    pub cw: u32,             // char width
    pub mode: WinMode,       // window state/mode flags
    pub cursor: CursorStyle, // cursor style
}
