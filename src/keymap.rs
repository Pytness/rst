use winit::event::KeyEvent;
use winit::keyboard::{self, KeyCode, KeyLocation, ModifiersState};

use crate::macros::macs::or_bitflags as or;
use crate::win::WinMode;

#[derive(Debug, Clone, Copy)]
pub enum ModifiersMatch {
    Empty,
    Exact(ModifiersState),
    Any,
}

impl ModifiersMatch {
    fn matches(&self, other: ModifiersMatch) -> bool {
        match (self, other) {
            (Empty, Empty) => true,
            (Empty, Exact(m)) => m.is_empty(),
            (Exact(m), Empty) => m.is_empty(),
            (Exact(m1), Exact(m2)) => *m1 == m2,
            (Any, _) | (_, Any) => true,
        }
    }
}

pub struct KeyMatch {
    pub modifiers: ModifiersMatch,
    pub key: KeyCode,
}

impl KeyMatch {
    pub const fn new(modifiers: ModifiersMatch, key: KeyCode) -> Self {
        Self { modifiers, key }
    }

    pub fn matches(&self, key: KeyCode, modifiers: ModifiersState) -> bool {
        self.key == key && self.modifiers.matches(Exact(modifiers))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Code(KeyCode),

    // With numlock off the keypad shares physical codes with the digits,
    // so these need their own variants
    NumHome,
    NumUp,
    NumDown,
    NumLeft,
    NumRight,
    NumPrior,
    NumNext,
    NumBegin,
    NumEnd,
    NumInsert,
    NumDelete,
}

impl Key {
    pub fn from_event(event: &KeyEvent, code: KeyCode) -> Self {
        // numlock on yields the digit as a character
        if event.location != KeyLocation::Numpad
            || matches!(event.logical_key, keyboard::Key::Character(_))
        {
            return Code(code);
        }

        match code {
            KeyCode::Numpad7 => NumHome,
            KeyCode::Numpad8 => NumUp,
            KeyCode::Numpad2 => NumDown,
            KeyCode::Numpad4 => NumLeft,
            KeyCode::Numpad6 => NumRight,
            KeyCode::Numpad9 => NumPrior,
            KeyCode::Numpad3 => NumNext,
            KeyCode::Numpad5 => NumBegin,
            KeyCode::Numpad1 => NumEnd,
            KeyCode::Numpad0 => NumInsert,
            KeyCode::NumpadDecimal => NumDelete,
            _ => Code(code),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKeyMode {
    Off,
    Any,
    On,

    // This only applies to appkey, not to appcursor.
    Exclusive,
}

impl AppKeyMode {
    fn allows(self, set: bool) -> bool {
        match self {
            AppKeyMode::Off => !set,
            AppKeyMode::Any => true,
            AppKeyMode::On | AppKeyMode::Exclusive => set,
        }
    }
}

pub struct MappedKey {
    pub modifiers: ModifiersMatch,
    pub key: Key,
    pub output: &'static [u8],
    // Exclusive also requires numlock off
    pub appkey: AppKeyMode,
    pub appcursor: AppKeyMode,
}

impl MappedKey {
    fn matches(&self, key: Key, modifiers: ModifiersState, mode: WinMode) -> bool {
        let numlock = mode.contains(WinMode::NumLock);

        self.key == key
            && self.modifiers.matches(Exact(modifiers))
            && self.appkey.allows(mode.contains(WinMode::AppKeypad))
            && !(numlock && self.appkey == MODE_EXC)
            && self.appcursor.allows(mode.contains(WinMode::AppCursor))
    }
}

pub struct Shortcut {
    pub key_match: KeyMatch,
    pub callback: fn(app: &mut App),
}

pub struct MouseShortcut {
    pub modifiers: ModifiersState,
    pub button: u8,
    pub callback: fn(),
    pub release: bool,
}

macro_rules! key {
    (KeyCode::$code:ident) => {
        Code(KeyCode::$code)
    };
    ($key:ident) => {
        $key
    };
}

macro_rules! k {
    ($modifiers:expr, $($key:ident)::+, $output:expr) => {
        k!($modifiers, $($key)::+, $output, MODE_ANY, MODE_ANY)
    };
    ($modifiers:expr, $($key:ident)::+, $output:expr, $appkey:expr, $appcursor:expr) => {
        MappedKey {
            modifiers: $modifiers,
            key: key!($($key)::+),
            output: $output,
            appkey: $appkey,
            appcursor: $appcursor,
        }
    };
}

pub const SHIFT: ModifiersState = ModifiersState::SHIFT;
pub const CONTROL: ModifiersState = ModifiersState::CONTROL;
pub const ALT: ModifiersState = ModifiersState::ALT;
pub const SUPER: ModifiersState = ModifiersState::SUPER;

pub const SHIFT_ALT: ModifiersState = or!(SHIFT | ALT);
pub const SHIFT_CONTROL: ModifiersState = or!(SHIFT | CONTROL);
pub const CONTROL_ALT: ModifiersState = or!(CONTROL | ALT);
pub const SHIFT_CONTROL_ALT: ModifiersState = or!(SHIFT | CONTROL | ALT);

pub const MODE_OFF: AppKeyMode = AppKeyMode::Off;
pub const MODE_ANY: AppKeyMode = AppKeyMode::Any;
pub const MODE_ON: AppKeyMode = AppKeyMode::On;
pub const MODE_EXC: AppKeyMode = AppKeyMode::Exclusive;

use Key::*;
use ModifiersMatch::*;

use crate::app::App;

#[rustfmt::skip]
const KEYMAPS: &[MappedKey] = &[
    k!(Exact(SHIFT),             NumHome,                      b"\x1b[2J",     MODE_ANY, MODE_OFF),
    k!(Exact(SHIFT),             NumHome,                      b"\x1b[1;2H",   MODE_ANY, MODE_ON),
    k!(Any,                      NumHome,                      b"\x1b[H",      MODE_ANY, MODE_OFF),
    k!(Any,                      NumHome,                      b"\x1b[1~",     MODE_ANY, MODE_ON),
    k!(Any,                      NumUp,                        b"\x1bOx",      MODE_ON,  MODE_ANY),
    k!(Any,                      NumUp,                        b"\x1b[A",      MODE_ANY, MODE_OFF),
    k!(Any,                      NumUp,                        b"\x1bOA",      MODE_ANY, MODE_ON),
    k!(Any,                      NumDown,                      b"\x1bOr",      MODE_ON,  MODE_ANY),
    k!(Any,                      NumDown,                      b"\x1b[B",      MODE_ANY, MODE_OFF),
    k!(Any,                      NumDown,                      b"\x1bOB",      MODE_ANY, MODE_ON),
    k!(Any,                      NumLeft,                      b"\x1bOt",      MODE_ON,  MODE_ANY),
    k!(Any,                      NumLeft,                      b"\x1b[D",      MODE_ANY, MODE_OFF),
    k!(Any,                      NumLeft,                      b"\x1bOD",      MODE_ANY, MODE_ON),
    k!(Any,                      NumRight,                     b"\x1bOv",      MODE_ON,  MODE_ANY),
    k!(Any,                      NumRight,                     b"\x1b[C",      MODE_ANY, MODE_OFF),
    k!(Any,                      NumRight,                     b"\x1bOC",      MODE_ANY, MODE_ON),
    k!(Exact(SHIFT),             NumPrior,                     b"\x1b[5;2~"),
    k!(Any,                      NumPrior,                     b"\x1b[5~"),
    k!(Any,                      NumBegin,                     b"\x1b[E"),
    k!(Exact(CONTROL),           NumEnd,                       b"\x1b[J",      MODE_OFF, MODE_ANY),
    k!(Exact(CONTROL),           NumEnd,                       b"\x1b[1;5F",   MODE_ON,  MODE_ANY),
    k!(Exact(SHIFT),             NumEnd,                       b"\x1b[K",      MODE_OFF, MODE_ANY),
    k!(Exact(SHIFT),             NumEnd,                       b"\x1b[1;2F",   MODE_ON,  MODE_ANY),
    k!(Any,                      NumEnd,                       b"\x1b[4~"),
    k!(Exact(SHIFT),             NumNext,                      b"\x1b[6;2~"),
    k!(Any,                      NumNext,                      b"\x1b[6~"),
    k!(Exact(SHIFT),             NumInsert,                    b"\x1b[2;2~",   MODE_ON,  MODE_ANY),
    k!(Exact(SHIFT),             NumInsert,                    b"\x1b[4l",     MODE_OFF, MODE_ANY),
    k!(Exact(CONTROL),           NumInsert,                    b"\x1b[L",      MODE_OFF, MODE_ANY),
    k!(Exact(CONTROL),           NumInsert,                    b"\x1b[2;5~",   MODE_ON,  MODE_ANY),
    k!(Any,                      NumInsert,                    b"\x1b[4h",     MODE_OFF, MODE_ANY),
    k!(Any,                      NumInsert,                    b"\x1b[2~",     MODE_ON,  MODE_ANY),
    k!(Exact(CONTROL),           NumDelete,                    b"\x1b[M",      MODE_OFF, MODE_ANY),
    k!(Exact(CONTROL),           NumDelete,                    b"\x1b[3;5~",   MODE_ON,  MODE_ANY),
    k!(Exact(SHIFT),             NumDelete,                    b"\x1b[2K",     MODE_OFF, MODE_ANY),
    k!(Exact(SHIFT),             NumDelete,                    b"\x1b[3;2~",   MODE_ON,  MODE_ANY),
    k!(Any,                      NumDelete,                    b"\x1b[P",      MODE_OFF, MODE_ANY),
    k!(Any,                      NumDelete,                    b"\x1b[3~",     MODE_ON,  MODE_ANY),
    k!(Any,                      KeyCode::NumpadMultiply,      b"\x1bOj",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::NumpadAdd,           b"\x1bOk",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::NumpadEnter,         b"\x1bOM",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::NumpadEnter,         b"\r",          MODE_OFF, MODE_ANY),
    k!(Any,                      KeyCode::NumpadSubtract,      b"\x1bOm",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::NumpadDecimal,       b"\x1bOn",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::NumpadDivide,        b"\x1bOo",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::Numpad0,             b"\x1bOp",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::Numpad1,             b"\x1bOq",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::Numpad2,             b"\x1bOr",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::Numpad3,             b"\x1bOs",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::Numpad4,             b"\x1bOt",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::Numpad5,             b"\x1bOu",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::Numpad6,             b"\x1bOv",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::Numpad7,             b"\x1bOw",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::Numpad8,             b"\x1bOx",      MODE_EXC, MODE_ANY),
    k!(Any,                      KeyCode::Numpad9,             b"\x1bOy",      MODE_EXC, MODE_ANY),
    k!(Exact(SHIFT),             KeyCode::ArrowUp,             b"\x1b[1;2A"),
    k!(Exact(ALT),               KeyCode::ArrowUp,             b"\x1b[1;3A"),
    k!(Exact(SHIFT_ALT),         KeyCode::ArrowUp,             b"\x1b[1;4A"),
    k!(Exact(CONTROL),           KeyCode::ArrowUp,             b"\x1b[1;5A"),
    k!(Exact(SHIFT_CONTROL),     KeyCode::ArrowUp,             b"\x1b[1;6A"),
    k!(Exact(CONTROL_ALT),       KeyCode::ArrowUp,             b"\x1b[1;7A"),
    k!(Exact(SHIFT_CONTROL_ALT), KeyCode::ArrowUp,             b"\x1b[1;8A"),
    k!(Any,                      KeyCode::ArrowUp,             b"\x1b[A",      MODE_ANY, MODE_OFF),
    k!(Any,                      KeyCode::ArrowUp,             b"\x1bOA",      MODE_ANY, MODE_ON),
    k!(Exact(SHIFT),             KeyCode::ArrowDown,           b"\x1b[1;2B"),
    k!(Exact(ALT),               KeyCode::ArrowDown,           b"\x1b[1;3B"),
    k!(Exact(SHIFT_ALT),         KeyCode::ArrowDown,           b"\x1b[1;4B"),
    k!(Exact(CONTROL),           KeyCode::ArrowDown,           b"\x1b[1;5B"),
    k!(Exact(SHIFT_CONTROL),     KeyCode::ArrowDown,           b"\x1b[1;6B"),
    k!(Exact(CONTROL_ALT),       KeyCode::ArrowDown,           b"\x1b[1;7B"),
    k!(Exact(SHIFT_CONTROL_ALT), KeyCode::ArrowDown,           b"\x1b[1;8B"),
    k!(Any,                      KeyCode::ArrowDown,           b"\x1b[B",      MODE_ANY, MODE_OFF),
    k!(Any,                      KeyCode::ArrowDown,           b"\x1bOB",      MODE_ANY, MODE_ON),
    k!(Exact(SHIFT),             KeyCode::ArrowLeft,           b"\x1b[1;2D"),
    k!(Exact(ALT),               KeyCode::ArrowLeft,           b"\x1b[1;3D"),
    k!(Exact(SHIFT_ALT),         KeyCode::ArrowLeft,           b"\x1b[1;4D"),
    k!(Exact(CONTROL),           KeyCode::ArrowLeft,           b"\x1b[1;5D"),
    k!(Exact(SHIFT_CONTROL),     KeyCode::ArrowLeft,           b"\x1b[1;6D"),
    k!(Exact(CONTROL_ALT),       KeyCode::ArrowLeft,           b"\x1b[1;7D"),
    k!(Exact(SHIFT_CONTROL_ALT), KeyCode::ArrowLeft,           b"\x1b[1;8D"),
    k!(Any,                      KeyCode::ArrowLeft,           b"\x1b[D",      MODE_ANY, MODE_OFF),
    k!(Any,                      KeyCode::ArrowLeft,           b"\x1bOD",      MODE_ANY, MODE_ON),
    k!(Exact(SHIFT),             KeyCode::ArrowRight,          b"\x1b[1;2C"),
    k!(Exact(ALT),               KeyCode::ArrowRight,          b"\x1b[1;3C"),
    k!(Exact(SHIFT_ALT),         KeyCode::ArrowRight,          b"\x1b[1;4C"),
    k!(Exact(CONTROL),           KeyCode::ArrowRight,          b"\x1b[1;5C"),
    k!(Exact(SHIFT_CONTROL),     KeyCode::ArrowRight,          b"\x1b[1;6C"),
    k!(Exact(CONTROL_ALT),       KeyCode::ArrowRight,          b"\x1b[1;7C"),
    k!(Exact(SHIFT_CONTROL_ALT), KeyCode::ArrowRight,          b"\x1b[1;8C"),
    k!(Any,                      KeyCode::ArrowRight,          b"\x1b[C",      MODE_ANY, MODE_OFF),
    k!(Any,                      KeyCode::ArrowRight,          b"\x1bOC",      MODE_ANY, MODE_ON),
    k!(Exact(SHIFT),             KeyCode::Tab,                 b"\x1b[Z"),
    k!(Exact(SHIFT),             KeyCode::Enter,               b"\x1b[13;2u"),
    k!(Exact(CONTROL),           KeyCode::Enter,               b"\x1b[13;5u"),
    k!(Exact(ALT),               KeyCode::Enter,               b"\x1b[13;3u"),
    k!(Any,                      KeyCode::Enter,               b"\r"),
    k!(Exact(SHIFT),             KeyCode::Insert,              b"\x1b[4l",     MODE_OFF, MODE_ANY),
    k!(Exact(SHIFT),             KeyCode::Insert,              b"\x1b[2;2~",   MODE_ON,  MODE_ANY),
    k!(Exact(CONTROL),           KeyCode::Insert,              b"\x1b[L",      MODE_OFF, MODE_ANY),
    k!(Exact(CONTROL),           KeyCode::Insert,              b"\x1b[2;5~",   MODE_ON,  MODE_ANY),
    k!(Any,                      KeyCode::Insert,              b"\x1b[4h",     MODE_OFF, MODE_ANY),
    k!(Any,                      KeyCode::Insert,              b"\x1b[2~",     MODE_ON,  MODE_ANY),
    k!(Exact(CONTROL),           KeyCode::Delete,              b"\x1b[M",      MODE_OFF, MODE_ANY),
    k!(Exact(CONTROL),           KeyCode::Delete,              b"\x1b[3;5~",   MODE_ON,  MODE_ANY),
    k!(Exact(SHIFT),             KeyCode::Delete,              b"\x1b[2K",     MODE_OFF, MODE_ANY),
    k!(Exact(SHIFT),             KeyCode::Delete,              b"\x1b[3;2~",   MODE_ON,  MODE_ANY),
    k!(Any,                      KeyCode::Delete,              b"\x1b[P",      MODE_OFF, MODE_ANY),
    k!(Any,                      KeyCode::Delete,              b"\x1b[3~",     MODE_ON,  MODE_ANY),
    k!(Empty,                    KeyCode::Backspace,           b"\x7f"),
    k!(Exact(ALT),               KeyCode::Backspace,           b"\x1b\x7f"),
    k!(Exact(SHIFT),             KeyCode::Home,                b"\x1b[2J",     MODE_ANY, MODE_OFF),
    k!(Exact(SHIFT),             KeyCode::Home,                b"\x1b[1;2H",   MODE_ANY, MODE_ON),
    k!(Any,                      KeyCode::Home,                b"\x1b[H",      MODE_ANY, MODE_OFF),
    k!(Any,                      KeyCode::Home,                b"\x1b[1~",     MODE_ANY, MODE_ON),
    k!(Exact(CONTROL),           KeyCode::End,                 b"\x1b[J",      MODE_OFF, MODE_ANY),
    k!(Exact(CONTROL),           KeyCode::End,                 b"\x1b[1;5F",   MODE_ON,  MODE_ANY),
    k!(Exact(SHIFT),             KeyCode::End,                 b"\x1b[K",      MODE_OFF, MODE_ANY),
    k!(Exact(SHIFT),             KeyCode::End,                 b"\x1b[1;2F",   MODE_ON,  MODE_ANY),
    k!(Any,                      KeyCode::End,                 b"\x1b[4~"),
    k!(Exact(CONTROL),           KeyCode::PageUp,              b"\x1b[5;5~"),
    k!(Exact(SHIFT),             KeyCode::PageUp,              b"\x1b[5;2~"),
    k!(Any,                      KeyCode::PageUp,              b"\x1b[5~"),
    k!(Exact(CONTROL),           KeyCode::PageDown,            b"\x1b[6;5~"),
    k!(Exact(SHIFT),             KeyCode::PageDown,            b"\x1b[6;2~"),
    k!(Any,                      KeyCode::PageDown,            b"\x1b[6~"),
    k!(Empty,                    KeyCode::F1,                  b"\x1bOP"),
    k!(Exact(SHIFT),             KeyCode::F1,                  b"\x1b[1;2P"),
    k!(Exact(CONTROL),           KeyCode::F1,                  b"\x1b[1;5P"),
    k!(Exact(SUPER),             KeyCode::F1,                  b"\x1b[1;6P"),
    k!(Exact(ALT),               KeyCode::F1,                  b"\x1b[1;3P"),
    k!(Empty,                    KeyCode::F2,                  b"\x1bOQ"),
    k!(Exact(SHIFT),             KeyCode::F2,                  b"\x1b[1;2Q"),
    k!(Exact(CONTROL),           KeyCode::F2,                  b"\x1b[1;5Q"),
    k!(Exact(SUPER),             KeyCode::F2,                  b"\x1b[1;6Q"),
    k!(Exact(ALT),               KeyCode::F2,                  b"\x1b[1;3Q"),
    k!(Empty,                    KeyCode::F3,                  b"\x1bOR"),
    k!(Exact(SHIFT),             KeyCode::F3,                  b"\x1b[1;2R"),
    k!(Exact(CONTROL),           KeyCode::F3,                  b"\x1b[1;5R"),
    k!(Exact(SUPER),             KeyCode::F3,                  b"\x1b[1;6R"),
    k!(Exact(ALT),               KeyCode::F3,                  b"\x1b[1;3R"),
    k!(Empty,                    KeyCode::F4,                  b"\x1bOS"),
    k!(Exact(SHIFT),             KeyCode::F4,                  b"\x1b[1;2S"),
    k!(Exact(CONTROL),           KeyCode::F4,                  b"\x1b[1;5S"),
    k!(Exact(SUPER),             KeyCode::F4,                  b"\x1b[1;6S"),
    k!(Exact(ALT),               KeyCode::F4,                  b"\x1b[1;3S"),
    k!(Empty,                    KeyCode::F5,                  b"\x1b[15~"),
    k!(Exact(SHIFT),             KeyCode::F5,                  b"\x1b[15;2~"),
    k!(Exact(CONTROL),           KeyCode::F5,                  b"\x1b[15;5~"),
    k!(Exact(SUPER),             KeyCode::F5,                  b"\x1b[15;6~"),
    k!(Exact(ALT),               KeyCode::F5,                  b"\x1b[15;3~"),
    k!(Empty,                    KeyCode::F6,                  b"\x1b[17~"),
    k!(Exact(SHIFT),             KeyCode::F6,                  b"\x1b[17;2~"),
    k!(Exact(CONTROL),           KeyCode::F6,                  b"\x1b[17;5~"),
    k!(Exact(SUPER),             KeyCode::F6,                  b"\x1b[17;6~"),
    k!(Exact(ALT),               KeyCode::F6,                  b"\x1b[17;3~"),
    k!(Empty,                    KeyCode::F7,                  b"\x1b[18~"),
    k!(Exact(SHIFT),             KeyCode::F7,                  b"\x1b[18;2~"),
    k!(Exact(CONTROL),           KeyCode::F7,                  b"\x1b[18;5~"),
    k!(Exact(SUPER),             KeyCode::F7,                  b"\x1b[18;6~"),
    k!(Exact(ALT),               KeyCode::F7,                  b"\x1b[18;3~"),
    k!(Empty,                    KeyCode::F8,                  b"\x1b[19~"),
    k!(Exact(SHIFT),             KeyCode::F8,                  b"\x1b[19;2~"),
    k!(Exact(CONTROL),           KeyCode::F8,                  b"\x1b[19;5~"),
    k!(Exact(SUPER),             KeyCode::F8,                  b"\x1b[19;6~"),
    k!(Exact(ALT),               KeyCode::F8,                  b"\x1b[19;3~"),
    k!(Empty,                    KeyCode::F9,                  b"\x1b[20~"),
    k!(Exact(SHIFT),             KeyCode::F9,                  b"\x1b[20;2~"),
    k!(Exact(CONTROL),           KeyCode::F9,                  b"\x1b[20;5~"),
    k!(Exact(SUPER),             KeyCode::F9,                  b"\x1b[20;6~"),
    k!(Exact(ALT),               KeyCode::F9,                  b"\x1b[20;3~"),
    k!(Empty,                    KeyCode::F10,                 b"\x1b[21~"),
    k!(Exact(SHIFT),             KeyCode::F10,                 b"\x1b[21;2~"),
    k!(Exact(CONTROL),           KeyCode::F10,                 b"\x1b[21;5~"),
    k!(Exact(SUPER),             KeyCode::F10,                 b"\x1b[21;6~"),
    k!(Exact(ALT),               KeyCode::F10,                 b"\x1b[21;3~"),
    k!(Empty,                    KeyCode::F11,                 b"\x1b[23~"),
    k!(Exact(SHIFT),             KeyCode::F11,                 b"\x1b[23;2~"),
    k!(Exact(CONTROL),           KeyCode::F11,                 b"\x1b[23;5~"),
    k!(Exact(SUPER),             KeyCode::F11,                 b"\x1b[23;6~"),
    k!(Exact(ALT),               KeyCode::F11,                 b"\x1b[23;3~"),
    k!(Empty,                    KeyCode::F12,                 b"\x1b[24~"),
    k!(Exact(SHIFT),             KeyCode::F12,                 b"\x1b[24;2~"),
    k!(Exact(CONTROL),           KeyCode::F12,                 b"\x1b[24;5~"),
    k!(Exact(SUPER),             KeyCode::F12,                 b"\x1b[24;6~"),
    k!(Exact(ALT),               KeyCode::F12,                 b"\x1b[24;3~"),
    k!(Empty,                    KeyCode::F13,                 b"\x1b[1;2P"),
    k!(Empty,                    KeyCode::F14,                 b"\x1b[1;2Q"),
    k!(Empty,                    KeyCode::F15,                 b"\x1b[1;2R"),
    k!(Empty,                    KeyCode::F16,                 b"\x1b[1;2S"),
    k!(Empty,                    KeyCode::F17,                 b"\x1b[15;2~"),
    k!(Empty,                    KeyCode::F18,                 b"\x1b[17;2~"),
    k!(Empty,                    KeyCode::F19,                 b"\x1b[18;2~"),
    k!(Empty,                    KeyCode::F20,                 b"\x1b[19;2~"),
    k!(Empty,                    KeyCode::F21,                 b"\x1b[20;2~"),
    k!(Empty,                    KeyCode::F22,                 b"\x1b[21;2~"),
    k!(Empty,                    KeyCode::F23,                 b"\x1b[23;2~"),
    k!(Empty,                    KeyCode::F24,                 b"\x1b[24;2~"),
    k!(Empty,                    KeyCode::F25,                 b"\x1b[1;5P"),
    k!(Empty,                    KeyCode::F26,                 b"\x1b[1;5Q"),
    k!(Empty,                    KeyCode::F27,                 b"\x1b[1;5R"),
    k!(Empty,                    KeyCode::F28,                 b"\x1b[1;5S"),
    k!(Empty,                    KeyCode::F29,                 b"\x1b[15;5~"),
    k!(Empty,                    KeyCode::F30,                 b"\x1b[17;5~"),
    k!(Empty,                    KeyCode::F31,                 b"\x1b[18;5~"),
    k!(Empty,                    KeyCode::F32,                 b"\x1b[19;5~"),
    k!(Empty,                    KeyCode::F33,                 b"\x1b[20;5~"),
    k!(Empty,                    KeyCode::F34,                 b"\x1b[21;5~"),
    k!(Empty,                    KeyCode::F35,                 b"\x1b[23;5~"),
];
pub fn kmap(key: Key, modifiers: ModifiersState, mode: WinMode) -> Option<&'static [u8]> {
    KEYMAPS
        .iter()
        .find(|mk| mk.matches(key, modifiers, mode))
        .map(|mk| mk.output)
}
