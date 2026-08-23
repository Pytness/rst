use std::ffi::CStr;

use crate::macros::macs::or_bitflags as or;
use crate::term_state::LOG_ENABLED;
use winit::keyboard::{KeyCode, ModifiersState};

use crate::app::App;

use crate::keymap::{CONTROL, KeyMatch, ModifiersMatch::*, MouseShortcut, SHIFT, Shortcut};

pub const TABSPACES: usize = 8;
pub const HALIGN: f64 = 0.5;
pub const VALIGN: f64 = 0.5;
pub const VTIDEN: &[u8] = b"\x1b[?62;4c";

pub const MINLATENCY: u64 = 2;
pub const MAXLATENCY: u64 = 33;

/*
 * Internal mouse shortcuts.
 * Beware that overloading Button1 will disable the selection.
 */
// static MouseShortcut mshortcuts[] = {
//         /* mask                 button   function        argument       release */
//         {TERMMOD, Button3, previewimage, {.s = "feh"}}, {TERMMOD, Button2, showimageinfo, {}, 1},
//         {XK_ANY_MOD, Button2, selpaste, {.i = 0}, 1},   {ShiftMask, Button4, ttysend, {.s = "\033[5;2~"}},
//         {XK_ANY_MOD, Button4, ttysend, {.s = "\031"}},  {ShiftMask, Button5, ttysend, {.s = "\033[6;2~"}},
//         {XK_ANY_MOD, Button5, ttysend, {.s = "\005"}},
// };
//
// static Shortcut shortcuts[] = {
//         /* mask                 keysym          function        argument */
//         {XK_ANY_MOD, XK_Break, sendbreak, {.i = 0}},  {ControlMask, XK_Print, toggleprinter, {.i = 0}},
//         {ShiftMask, XK_Print, printscreen, {.i = 0}}, {XK_ANY_MOD, XK_Print, printsel, {.i = 0}},
//         {TERMMOD, XK_plus, zoom, {.f = +1}},          {TERMMOD, XK_underscore, zoom, {.f = -1}},
//         {TERMMOD, XK_Home, zoomreset, {.f = 0}},      {TERMMOD, XK_C, clipcopy, {.i = 0}},
//         {TERMMOD, XK_V, clippaste, {.i = 0}},         {TERMMOD, XK_Y, selpaste, {.i = 0}},
//         {ShiftMask, XK_Insert, selpaste, {.i = 0}},   {TERMMOD, XK_Num_Lock, numlock, {.i = 0}},
//         {TERMMOD, XK_F1, togglegrdebug, {.i = 0}},    {TERMMOD, XK_F6, dumpgrstate, {.i = 0}},
//         {TERMMOD, XK_F7, unloadimages, {.i = 0}},     {TERMMOD, XK_F8, toggleimages, {.i = 0}},
// };

macro_rules! s {
    ($modifiers:expr, $key:expr, $callback:expr) => {
        Shortcut {
            key_match: KeyMatch::new($modifiers, $key),
            callback: $callback,
        }
    };
}

pub fn toggle_recording(_app: &mut App) {
    println!("Toggling recording...");
    unsafe {
        LOG_ENABLED = !LOG_ENABLED;
    }
}

pub fn zoom_in(app: &mut App) {
    println!("Zooming in...");
}

pub fn zoom_out(_app: &mut App) {
    println!("Zooming out...");
}

pub fn zoom_reset(_app: &mut App) {
    println!("Zoom reset...");
}

pub const MSHORTCUTS: Vec<MouseShortcut> = vec![];

#[rustfmt::skip]
pub const SHORTCUTS: &[Shortcut] = &[
    s!(Exact(or!(CONTROL | SHIFT)), KeyCode::Minus, zoom_out),
    s!(Exact(or!(CONTROL | SHIFT)), KeyCode::Equal, zoom_in),
    s!(Exact(or!(CONTROL | SHIFT)), KeyCode::Home, zoom_reset),

    s!(Empty, KeyCode::F5, toggle_recording),
];

pub const DEFAULTFG: u32 = 258;
pub const DEFAULTBG: u32 = 259;
pub const DEFAULTCS: u32 = 256;
pub const DEFAULTRCS: u32 = 257;

pub const SHELL: Option<&CStr> = Some(c"/bin/zsh");
pub const TERM: &CStr = c"xterm-256color";
pub const BORDERPX: u32 = 4;
pub const BACKGROUND_ALPHA: f32 = 0.8;
pub const DYNAMIC_CURSOR: bool = false;

pub const BLINK_TIMEOUT: u64 = 500;

pub const FONTS: &[&str] = &[
    "CaskaydiaCove Nerd Font:size=10:antialias=true:autohint=true",
    "Symbols Nerd Font:size=10:antialias=true:autohint=true",
    "Noto Color Emoji:size=10:antialias=true:autohint=true",
];
