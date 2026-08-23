use std::ffi::CStr;

use winit::keyboard::{KeyCode, ModifiersState};

// typedef struct {
// 	KeySym k;
// 	uint mask;
// 	char *s;
// 	/* three-valued logic variables: 0 indifferent, 1 on, -1 off */
// 	signed char appkey;    /* application keypad */
// 	signed char appcursor; /* application cursor */
// } Key;

pub struct Key {
    modifiers: ModifiersState,
    key: KeyCode,
    output: &'static CStr,
}

macro_rules! k {
    ($modifiers:expr, $key:expr, $output:expr) => {
        Key {
            modifiers: $modifiers,
            key: $key,
            output: &$output,
        }
    };
}

const EMPTY: ModifiersState = ModifiersState::empty();
const SHIFT: ModifiersState = ModifiersState::SHIFT;
const CONTROL: ModifiersState = ModifiersState::CONTROL;
const ALT: ModifiersState = ModifiersState::ALT;
const SUPER: ModifiersState = ModifiersState::SUPER;

// {XK_Return, ShiftMask, "\033[13;2u", 0, 0},
// {XK_Return, ControlMask, "\033[13;5u", 0, 0},
// {XK_Return, Mod1Mask, "\033[13;3u", 0, 0},
// {XK_Return, XK_ANY_MOD, "\r", 0, 0},

const KEYMAPS: &[Key] = &[
    k!(SHIFT, KeyCode::Enter, c"\x1b[13;2u"),
    k!(CONTROL, KeyCode::Enter, c"\x1b[13;5u"),
    k!(ALT, KeyCode::Enter, c"\x1b[13;3u"),
    k!(EMPTY, KeyCode::Enter, c"\r"),
];

pub fn kmap(code: KeyCode, modifiers: ModifiersState) -> Option<&'static CStr> {
    for key in KEYMAPS {
        if key.key == code && key.modifiers == modifiers {
            return Some(key.output);
        }
    }
    None
}
