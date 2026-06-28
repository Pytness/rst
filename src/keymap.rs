use std::ffi::{CStr, CString};

use winit::event::{KeyEvent, Modifiers};
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
    key: KeyCode,
    modifiers: ModifiersState,
    output: &'static CStr,
}

macro_rules! k {
    ($key:expr, $modifiers:expr, $output:expr) => {
        Key {
            key: $key,
            modifiers: $modifiers,
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
    k!(KeyCode::Enter, SHIFT, c"\x1b[13;2u"),
    k!(KeyCode::Enter, CONTROL, c"\x1b[13;5u"),
    k!(KeyCode::Enter, ALT, c"\x1b[13;3u"),
    k!(KeyCode::Enter, EMPTY, c"\r"),
];

pub fn kmap(code: KeyCode, modifiers: ModifiersState) -> Option<&'static CStr> {
    for key in KEYMAPS {
        if key.key == code && key.modifiers == modifiers {
            return Some(key.output);
        }
    }
    None
}
