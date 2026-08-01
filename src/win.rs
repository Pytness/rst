use bitflags::bitflags;

bitflags! {
    #[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct WinMode: u32 {
        const Visible     = 1 << 0;
        const Focused     = 1 << 1;
        const AppKeypad   = 1 << 2;
        const MouseButton    = 1 << 3;
        const MouseMotion = 1 << 4;
        const Reverse     = 1 << 5;
        const KbdLock     = 1 << 6;
        const Hide        = 1 << 7;
        const AppCursor   = 1 << 8;
        const MouseSGR    = 1 << 9;
        const EightBit        = 1 << 10;
        const Blink       = 1 << 11;
        const FBlink      = 1 << 12;
        const Focus       = 1 << 13;
        const MouseX10    = 1 << 14;
        const MouseMany   = 1 << 15;
        const BracketedPaste  = 1 << 16;
        const NumLock     = 1 << 17;

        const MODE_MOUSE =
              Self::MouseButton.bits()
            | Self::MouseMotion.bits()
            | Self::MouseX10.bits()
            | Self::MouseMany.bits()
            | Self::MouseSGR.bits();
    }
}

#[rustfmt::skip]
#[derive(Default, Debug)]
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

#[derive(Default, Debug)]
pub struct TermWindow {
    pub tw: u32,             // tty width
    pub th: u32,             // tty height
    pub w: u32,              // window width
    pub h: u32,              // window height
    pub hborderpx: u32,      // horizontal border in pixels
    pub vborderpx: u32,      // vertical border in pixels
    pub ch: u32,             // char height
    pub cw: u32,             // char width
    pub mode: WinMode,       // window state/mode flags
    pub cursor: CursorStyle, // cursor style
}
