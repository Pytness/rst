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
            (Exact(m1), Exact(m2)) => *m1 == m2,
            (Any, _) | (_, Any) => true,
            _ => false,
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
}

impl PartialEq for KeyMatch {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.modifiers.matches(other.modifiers)
    }
}

pub struct MappedKey {
    pub key_match: KeyMatch,
    pub output: &'static CStr,
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

macro_rules! k {
    ($modifiers:expr, $key:expr, $output:expr) => {
        MappedKey {
            key_match: KeyMatch::new($modifiers, $key),
            output: &$output,
        }
    };
}

pub const SHIFT: ModifiersState = ModifiersState::SHIFT;
pub const CONTROL: ModifiersState = ModifiersState::CONTROL;
pub const ALT: ModifiersState = ModifiersState::ALT;
pub const SUPER: ModifiersState = ModifiersState::SUPER;

use ModifiersMatch::*;

use crate::app::App;

const KEYMAPS: &[MappedKey] = &[
    k!(Empty, KeyCode::Enter, c"\r"),
    k!(Exact(SHIFT), KeyCode::Enter, c"\x1b[13;2u"),
    k!(Exact(CONTROL), KeyCode::Enter, c"\x1b[13;5u"),
    k!(Exact(ALT), KeyCode::Enter, c"\x1b[13;3u"),
];

pub fn kmap(code: KeyCode, modifiers: ModifiersState) -> Option<&'static CStr> {
    let key_match = KeyMatch::new(Exact(modifiers), code);

    for mapped_key in KEYMAPS {
        if mapped_key.key_match == key_match {
            return Some(mapped_key.output);
        }
    }

    None
}
