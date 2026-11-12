// SPDX-License-Identifier: GPL-3.0-or-later

//! Convert macOS key events into libchewing [`KeyboardEvent`]s.
//!
//! libchewing's zhuyin layouts look at the physical key (`code`, an X11 /
//! xkbcommon keycode), while symbol and English input look at the produced
//! character (`ksym`). macOS gives us both: `NSEvent.keyCode` is a virtual
//! keycode (`kVK_*` from HIToolbox/Events.h) that identifies the physical key,
//! and `charactersIgnoringModifiers` is the character with only Shift applied.

use chewing::input::KeyboardEvent;
use chewing::input::keycode::{self, Keycode};
use chewing::input::keysym::{self, Keysym};

/// Modifier state of a key event, independent of AppKit.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    /// The Option key; plays the role of Alt.
    pub option: bool,
    pub command: bool,
    pub caps_lock: bool,
}

/// A key press as delivered by macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInput {
    /// `NSEvent.keyCode` (a `kVK_*` value).
    pub key_code: u16,
    /// First character of `NSEvent.charactersIgnoringModifiers`, if any.
    pub text: Option<char>,
    pub modifiers: Modifiers,
}

/// macOS virtual keycodes (HIToolbox `kVK_*`) used in this module.
#[allow(dead_code)]
pub mod kvk {
    pub const ANSI_A: u16 = 0x00;
    pub const ANSI_S: u16 = 0x01;
    pub const ANSI_D: u16 = 0x02;
    pub const ANSI_F: u16 = 0x03;
    pub const ANSI_H: u16 = 0x04;
    pub const ANSI_G: u16 = 0x05;
    pub const ANSI_Z: u16 = 0x06;
    pub const ANSI_X: u16 = 0x07;
    pub const ANSI_C: u16 = 0x08;
    pub const ANSI_V: u16 = 0x09;
    pub const ANSI_B: u16 = 0x0B;
    pub const ANSI_Q: u16 = 0x0C;
    pub const ANSI_W: u16 = 0x0D;
    pub const ANSI_E: u16 = 0x0E;
    pub const ANSI_R: u16 = 0x0F;
    pub const ANSI_Y: u16 = 0x10;
    pub const ANSI_T: u16 = 0x11;
    pub const ANSI_1: u16 = 0x12;
    pub const ANSI_2: u16 = 0x13;
    pub const ANSI_3: u16 = 0x14;
    pub const ANSI_4: u16 = 0x15;
    pub const ANSI_6: u16 = 0x16;
    pub const ANSI_5: u16 = 0x17;
    pub const ANSI_EQUAL: u16 = 0x18;
    pub const ANSI_9: u16 = 0x19;
    pub const ANSI_7: u16 = 0x1A;
    pub const ANSI_MINUS: u16 = 0x1B;
    pub const ANSI_8: u16 = 0x1C;
    pub const ANSI_0: u16 = 0x1D;
    pub const ANSI_RIGHT_BRACKET: u16 = 0x1E;
    pub const ANSI_O: u16 = 0x1F;
    pub const ANSI_U: u16 = 0x20;
    pub const ANSI_LEFT_BRACKET: u16 = 0x21;
    pub const ANSI_I: u16 = 0x22;
    pub const ANSI_P: u16 = 0x23;
    pub const RETURN: u16 = 0x24;
    pub const ANSI_L: u16 = 0x25;
    pub const ANSI_J: u16 = 0x26;
    pub const ANSI_QUOTE: u16 = 0x27;
    pub const ANSI_K: u16 = 0x28;
    pub const ANSI_SEMICOLON: u16 = 0x29;
    pub const ANSI_BACKSLASH: u16 = 0x2A;
    pub const ANSI_COMMA: u16 = 0x2B;
    pub const ANSI_SLASH: u16 = 0x2C;
    pub const ANSI_N: u16 = 0x2D;
    pub const ANSI_M: u16 = 0x2E;
    pub const ANSI_PERIOD: u16 = 0x2F;
    pub const TAB: u16 = 0x30;
    pub const SPACE: u16 = 0x31;
    pub const ANSI_GRAVE: u16 = 0x32;
    /// The key labelled "delete" on Mac keyboards (Backspace).
    pub const DELETE: u16 = 0x33;
    pub const ESCAPE: u16 = 0x35;
    pub const RIGHT_COMMAND: u16 = 0x36;
    pub const COMMAND: u16 = 0x37;
    pub const SHIFT: u16 = 0x38;
    pub const CAPS_LOCK: u16 = 0x39;
    pub const OPTION: u16 = 0x3A;
    pub const CONTROL: u16 = 0x3B;
    pub const RIGHT_SHIFT: u16 = 0x3C;
    pub const RIGHT_OPTION: u16 = 0x3D;
    pub const RIGHT_CONTROL: u16 = 0x3E;
    pub const FUNCTION: u16 = 0x3F;
    pub const KEYPAD_DECIMAL: u16 = 0x41;
    pub const KEYPAD_MULTIPLY: u16 = 0x43;
    pub const KEYPAD_PLUS: u16 = 0x45;
    pub const KEYPAD_CLEAR: u16 = 0x47;
    pub const KEYPAD_DIVIDE: u16 = 0x4B;
    pub const KEYPAD_ENTER: u16 = 0x4C;
    pub const KEYPAD_MINUS: u16 = 0x4E;
    pub const KEYPAD_EQUALS: u16 = 0x51;
    pub const KEYPAD_0: u16 = 0x52;
    pub const KEYPAD_1: u16 = 0x53;
    pub const KEYPAD_2: u16 = 0x54;
    pub const KEYPAD_3: u16 = 0x55;
    pub const KEYPAD_4: u16 = 0x56;
    pub const KEYPAD_5: u16 = 0x57;
    pub const KEYPAD_6: u16 = 0x58;
    pub const KEYPAD_7: u16 = 0x59;
    pub const KEYPAD_8: u16 = 0x5B;
    pub const KEYPAD_9: u16 = 0x5C;
    pub const F5: u16 = 0x60;
    pub const F6: u16 = 0x61;
    pub const F7: u16 = 0x62;
    pub const F3: u16 = 0x63;
    pub const F8: u16 = 0x64;
    pub const F9: u16 = 0x65;
    pub const F11: u16 = 0x67;
    pub const F10: u16 = 0x6D;
    pub const F12: u16 = 0x6F;
    pub const HELP: u16 = 0x72;
    pub const HOME: u16 = 0x73;
    pub const PAGE_UP: u16 = 0x74;
    pub const FORWARD_DELETE: u16 = 0x75;
    pub const F4: u16 = 0x76;
    pub const END: u16 = 0x77;
    pub const F2: u16 = 0x78;
    pub const PAGE_DOWN: u16 = 0x79;
    pub const F1: u16 = 0x7A;
    pub const LEFT_ARROW: u16 = 0x7B;
    pub const RIGHT_ARROW: u16 = 0x7C;
    pub const DOWN_ARROW: u16 = 0x7D;
    pub const UP_ARROW: u16 = 0x7E;
}

/// macOS virtual keycode -> X11 keycode, sorted by the macOS keycode.
const KEYCODE_MAP: &[(u16, Keycode)] = &[
    (kvk::ANSI_A, keycode::KEY_A),
    (kvk::ANSI_S, keycode::KEY_S),
    (kvk::ANSI_D, keycode::KEY_D),
    (kvk::ANSI_F, keycode::KEY_F),
    (kvk::ANSI_H, keycode::KEY_H),
    (kvk::ANSI_G, keycode::KEY_G),
    (kvk::ANSI_Z, keycode::KEY_Z),
    (kvk::ANSI_X, keycode::KEY_X),
    (kvk::ANSI_C, keycode::KEY_C),
    (kvk::ANSI_V, keycode::KEY_V),
    (kvk::ANSI_B, keycode::KEY_B),
    (kvk::ANSI_Q, keycode::KEY_Q),
    (kvk::ANSI_W, keycode::KEY_W),
    (kvk::ANSI_E, keycode::KEY_E),
    (kvk::ANSI_R, keycode::KEY_R),
    (kvk::ANSI_Y, keycode::KEY_Y),
    (kvk::ANSI_T, keycode::KEY_T),
    (kvk::ANSI_1, keycode::KEY_1),
    (kvk::ANSI_2, keycode::KEY_2),
    (kvk::ANSI_3, keycode::KEY_3),
    (kvk::ANSI_4, keycode::KEY_4),
    (kvk::ANSI_6, keycode::KEY_6),
    (kvk::ANSI_5, keycode::KEY_5),
    (kvk::ANSI_EQUAL, keycode::KEY_EQUAL),
    (kvk::ANSI_9, keycode::KEY_9),
    (kvk::ANSI_7, keycode::KEY_7),
    (kvk::ANSI_MINUS, keycode::KEY_MINUS),
    (kvk::ANSI_8, keycode::KEY_8),
    (kvk::ANSI_0, keycode::KEY_0),
    (kvk::ANSI_RIGHT_BRACKET, keycode::KEY_RIGHTBRACE),
    (kvk::ANSI_O, keycode::KEY_O),
    (kvk::ANSI_U, keycode::KEY_U),
    (kvk::ANSI_LEFT_BRACKET, keycode::KEY_LEFTBRACE),
    (kvk::ANSI_I, keycode::KEY_I),
    (kvk::ANSI_P, keycode::KEY_P),
    (kvk::RETURN, keycode::KEY_ENTER),
    (kvk::ANSI_L, keycode::KEY_L),
    (kvk::ANSI_J, keycode::KEY_J),
    (kvk::ANSI_QUOTE, keycode::KEY_APOSTROPHE),
    (kvk::ANSI_K, keycode::KEY_K),
    (kvk::ANSI_SEMICOLON, keycode::KEY_SEMICOLON),
    (kvk::ANSI_BACKSLASH, keycode::KEY_BACKSLASH),
    (kvk::ANSI_COMMA, keycode::KEY_COMMA),
    (kvk::ANSI_SLASH, keycode::KEY_SLASH),
    (kvk::ANSI_N, keycode::KEY_N),
    (kvk::ANSI_M, keycode::KEY_M),
    (kvk::ANSI_PERIOD, keycode::KEY_DOT),
    (kvk::TAB, keycode::KEY_TAB),
    (kvk::SPACE, keycode::KEY_SPACE),
    (kvk::ANSI_GRAVE, keycode::KEY_GRAVE),
    (kvk::DELETE, keycode::KEY_BACKSPACE),
    (kvk::ESCAPE, keycode::KEY_ESC),
    (kvk::RIGHT_COMMAND, keycode::KEY_RIGHTMETA),
    (kvk::COMMAND, keycode::KEY_LEFTMETA),
    (kvk::SHIFT, keycode::KEY_LEFTSHIFT),
    (kvk::CAPS_LOCK, keycode::KEY_CAPSLOCK),
    (kvk::OPTION, keycode::KEY_LEFTALT),
    (kvk::CONTROL, keycode::KEY_LEFTCTRL),
    (kvk::RIGHT_SHIFT, keycode::KEY_RIGHTSHIFT),
    (kvk::RIGHT_OPTION, keycode::KEY_RIGHTALT),
    (kvk::RIGHT_CONTROL, keycode::KEY_RIGHTCTRL),
    (kvk::KEYPAD_DECIMAL, keycode::KEY_KPDOT),
    (kvk::KEYPAD_MULTIPLY, keycode::KEY_KPASTERISK),
    (kvk::KEYPAD_PLUS, keycode::KEY_KPPLUS),
    (kvk::KEYPAD_CLEAR, keycode::KEY_NUMLOCK),
    (kvk::KEYPAD_DIVIDE, keycode::KEY_KPSLASH),
    (kvk::KEYPAD_ENTER, keycode::KEY_KPENTER),
    (kvk::KEYPAD_MINUS, keycode::KEY_KPMINUS),
    (kvk::KEYPAD_EQUALS, keycode::KEY_KPEQUAL),
    (kvk::KEYPAD_0, keycode::KEY_KP0),
    (kvk::KEYPAD_1, keycode::KEY_KP1),
    (kvk::KEYPAD_2, keycode::KEY_KP2),
    (kvk::KEYPAD_3, keycode::KEY_KP3),
    (kvk::KEYPAD_4, keycode::KEY_KP4),
    (kvk::KEYPAD_5, keycode::KEY_KP5),
    (kvk::KEYPAD_6, keycode::KEY_KP6),
    (kvk::KEYPAD_7, keycode::KEY_KP7),
    (kvk::KEYPAD_8, keycode::KEY_KP8),
    (kvk::KEYPAD_9, keycode::KEY_KP9),
    (kvk::F5, keycode::KEY_F5),
    (kvk::F6, keycode::KEY_F6),
    (kvk::F7, keycode::KEY_F7),
    (kvk::F3, keycode::KEY_F3),
    (kvk::F8, keycode::KEY_F8),
    (kvk::F9, keycode::KEY_F9),
    (kvk::F11, keycode::KEY_F11),
    (kvk::F10, keycode::KEY_F10),
    (kvk::F12, keycode::KEY_F12),
    (kvk::HELP, keycode::KEY_INSERT),
    (kvk::HOME, keycode::KEY_HOME),
    (kvk::PAGE_UP, keycode::KEY_PAGEUP),
    (kvk::FORWARD_DELETE, keycode::KEY_DELETE),
    (kvk::F4, keycode::KEY_F4),
    (kvk::END, keycode::KEY_END),
    (kvk::F2, keycode::KEY_F2),
    (kvk::PAGE_DOWN, keycode::KEY_PAGEDOWN),
    (kvk::F1, keycode::KEY_F1),
    (kvk::LEFT_ARROW, keycode::KEY_LEFT),
    (kvk::RIGHT_ARROW, keycode::KEY_RIGHT),
    (kvk::DOWN_ARROW, keycode::KEY_DOWN),
    (kvk::UP_ARROW, keycode::KEY_UP),
];

/// Keys whose keysym does not come from the produced character, sorted by
/// the macOS keycode. Mirrors windows-chewing-tsf's `VKEY_MAP`.
const KEYSYM_MAP: &[(u16, Keysym)] = &[
    (kvk::RETURN, keysym::SYM_RETURN),
    (kvk::TAB, keysym::SYM_TAB),
    (kvk::SPACE, keysym::SYM_SPACE),
    (kvk::DELETE, keysym::SYM_BACKSPACE),
    (kvk::ESCAPE, keysym::SYM_ESC),
    (kvk::RIGHT_COMMAND, keysym::SYM_RIGHTMETA),
    (kvk::COMMAND, keysym::SYM_LEFTMETA),
    (kvk::SHIFT, keysym::SYM_LEFTSHIFT),
    (kvk::CAPS_LOCK, keysym::SYM_CAPSLOCK),
    (kvk::OPTION, keysym::SYM_LEFTALT),
    (kvk::CONTROL, keysym::SYM_LEFTCTRL),
    (kvk::RIGHT_SHIFT, keysym::SYM_RIGHTSHIFT),
    (kvk::RIGHT_OPTION, keysym::SYM_RIGHTALT),
    (kvk::RIGHT_CONTROL, keysym::SYM_RIGHTCTRL),
    (kvk::KEYPAD_DECIMAL, keysym::SYM_KPDECIMAL),
    (kvk::KEYPAD_MULTIPLY, keysym::SYM_KPMULTIPLY),
    (kvk::KEYPAD_PLUS, keysym::SYM_KPADD),
    (kvk::KEYPAD_CLEAR, keysym::SYM_NUMLOCK),
    (kvk::KEYPAD_DIVIDE, keysym::SYM_KPDIVIDE),
    (kvk::KEYPAD_ENTER, keysym::SYM_KPENTER),
    (kvk::KEYPAD_MINUS, keysym::SYM_KPSUBTRACT),
    (kvk::KEYPAD_EQUALS, keysym::SYM_KPEQUAL),
    (kvk::KEYPAD_0, keysym::SYM_KP0),
    (kvk::KEYPAD_1, keysym::SYM_KP1),
    (kvk::KEYPAD_2, keysym::SYM_KP2),
    (kvk::KEYPAD_3, keysym::SYM_KP3),
    (kvk::KEYPAD_4, keysym::SYM_KP4),
    (kvk::KEYPAD_5, keysym::SYM_KP5),
    (kvk::KEYPAD_6, keysym::SYM_KP6),
    (kvk::KEYPAD_7, keysym::SYM_KP7),
    (kvk::KEYPAD_8, keysym::SYM_KP8),
    (kvk::KEYPAD_9, keysym::SYM_KP9),
    (kvk::F5, keysym::SYM_F5),
    (kvk::F6, keysym::SYM_F6),
    (kvk::F7, keysym::SYM_F7),
    (kvk::F3, keysym::SYM_F3),
    (kvk::F8, keysym::SYM_F8),
    (kvk::F9, keysym::SYM_F9),
    (kvk::F11, keysym::SYM_F11),
    (kvk::F10, keysym::SYM_F10),
    (kvk::F12, keysym::SYM_F12),
    (kvk::HOME, keysym::SYM_HOME),
    (kvk::PAGE_UP, keysym::SYM_PAGEUP),
    (kvk::FORWARD_DELETE, keysym::SYM_DELETE),
    (kvk::F4, keysym::SYM_F4),
    (kvk::END, keysym::SYM_END),
    (kvk::F2, keysym::SYM_F2),
    (kvk::PAGE_DOWN, keysym::SYM_PAGEDOWN),
    (kvk::F1, keysym::SYM_F1),
    (kvk::LEFT_ARROW, keysym::SYM_LEFT),
    (kvk::RIGHT_ARROW, keysym::SYM_RIGHT),
    (kvk::DOWN_ARROW, keysym::SYM_DOWN),
    (kvk::UP_ARROW, keysym::SYM_UP),
];

/// AppKit reports function keys (arrows, F13, Help, ...) as characters in
/// this private-use range (`NSUpArrowFunctionKey` and friends).
const APPKIT_FUNCTION_KEYS: std::ops::RangeInclusive<char> = '\u{F700}'..='\u{F8FF}';

fn is_printable(c: char) -> bool {
    !c.is_control() && !APPKIT_FUNCTION_KEYS.contains(&c)
}

fn lookup<T: Copy>(table: &[(u16, T)], key_code: u16) -> Option<T> {
    table
        .binary_search_by_key(&key_code, |&(k, _)| k)
        .ok()
        .map(|i| table[i].1)
}

impl KeyInput {
    /// Convert to a libchewing keyboard event.
    pub fn to_keyboard_event(&self) -> KeyboardEvent {
        let code = lookup(KEYCODE_MAP, self.key_code).unwrap_or_default();
        let ksym = lookup(KEYSYM_MAP, self.key_code)
            .or_else(|| self.text.filter(|&c| is_printable(c)).map(Keysym::from))
            .unwrap_or(keysym::SYM_NONE);
        let m = self.modifiers;
        KeyboardEvent::builder()
            .code(code)
            .ksym(ksym)
            .shift_if(m.shift)
            .control_if(m.control)
            .alt_if(m.option)
            .super_if(m.command)
            .caps_lock_if(m.caps_lock)
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chewing::input::KeyState;

    #[test]
    fn tables_are_sorted_for_binary_search() {
        assert!(KEYCODE_MAP.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(KEYSYM_MAP.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn letter_uses_physical_code_and_character() {
        let evt = KeyInput {
            key_code: kvk::ANSI_J,
            text: Some('j'),
            modifiers: Modifiers::default(),
        }
        .to_keyboard_event();
        assert_eq!(evt.code, keycode::KEY_J);
        assert_eq!(evt.ksym, keysym::SYM_LOWER_J);
    }

    #[test]
    fn shifted_symbol_keeps_physical_key() {
        let evt = KeyInput {
            key_code: kvk::ANSI_1,
            text: Some('!'),
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
        }
        .to_keyboard_event();
        assert_eq!(evt.code, keycode::KEY_1);
        assert_eq!(evt.ksym, Keysym::from('!'));
        assert!(evt.is_state_on(KeyState::Shift));
    }

    #[test]
    fn special_keys_ignore_their_control_characters() {
        let evt = KeyInput {
            key_code: kvk::DELETE,
            text: Some('\u{7f}'),
            modifiers: Modifiers::default(),
        }
        .to_keyboard_event();
        assert_eq!(evt.code, keycode::KEY_BACKSPACE);
        assert_eq!(evt.ksym, keysym::SYM_BACKSPACE);

        let evt = KeyInput {
            key_code: kvk::KEYPAD_7,
            text: Some('7'),
            modifiers: Modifiers::default(),
        }
        .to_keyboard_event();
        assert_eq!(evt.code, keycode::KEY_KP7);
        assert_eq!(evt.ksym, keysym::SYM_KP7);
    }

    #[test]
    fn unknown_key_has_no_symbol() {
        let evt = KeyInput {
            key_code: 0x40, // F17
            text: Some('\u{f714}'),
            modifiers: Modifiers::default(),
        }
        .to_keyboard_event();
        assert_eq!(evt.code, Keycode::default());
        assert_eq!(evt.ksym, keysym::SYM_NONE);
        assert!(!evt.ksym.is_unicode());
    }
}
