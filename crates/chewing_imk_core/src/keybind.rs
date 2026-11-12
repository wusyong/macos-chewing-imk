// SPDX-License-Identifier: GPL-3.0-or-later

//! User keybindings such as "Ctrl+F12 → toggle_simplified_chinese".
//! Ported from windows-chewing-tsf's `tip/src/keybind.rs`.

use chewing::input::keysym::*;
use chewing::input::{KeyState, KeyboardEvent};

/// A keybinding as stored in the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindValue {
    pub key: String,
    pub action: String,
    pub param: String,
}

impl KeybindValue {
    pub fn new(key: &str, action: &str, param: &str) -> KeybindValue {
        KeybindValue {
            key: key.to_owned(),
            action: action.to_owned(),
            param: param.to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Keybinding {
    pub(crate) key: KeyboardEvent,
    pub(crate) action: String,
    pub(crate) param: String,
}

impl Keybinding {
    pub(crate) fn parse(value: &KeybindValue) -> Option<Keybinding> {
        Some(Keybinding {
            key: key_from_str(&value.key)?,
            action: value.action.clone(),
            param: value.param.clone(),
        })
    }

    pub(crate) fn matches(&self, evt: &KeyboardEvent) -> bool {
        (self.key.ksym == evt.ksym || self.key.ksym == SYM_NONE)
            && [
                KeyState::Alt,
                KeyState::Control,
                KeyState::Shift,
                KeyState::Super,
            ]
            .iter()
            .all(|&state| self.key.is_state_on(state) == evt.is_state_on(state))
    }
}

fn key_from_str(s: &str) -> Option<KeyboardEvent> {
    let mut builder = KeyboardEvent::builder();
    for key in s.split('+').rev() {
        match key.trim().to_lowercase().as_str() {
            "ctrl" | "control" => {
                builder.control();
            }
            "shift" => {
                builder.shift();
            }
            "alt" | "opt" | "option" => {
                builder.alt_if(true);
            }
            "super" | "cmd" | "command" => {
                builder.super_if(true);
            }
            _ => {
                builder.ksym(keysym_from_str(key)?);
            }
        }
    }
    Some(builder.build())
}

fn keysym_from_str(s: &str) -> Option<Keysym> {
    let s = s.trim();
    let ksym = match s {
        "Esc" => SYM_ESC,
        "F1" => SYM_F1,
        "F2" => SYM_F2,
        "F3" => SYM_F3,
        "F4" => SYM_F4,
        "F5" => SYM_F5,
        "F6" => SYM_F6,
        "F7" => SYM_F7,
        "F8" => SYM_F8,
        "F9" => SYM_F9,
        "F10" => SYM_F10,
        "F11" => SYM_F11,
        "F12" => SYM_F12,
        "Home" => SYM_HOME,
        "End" => SYM_END,
        "Delete" => SYM_DELETE,
        "Tab" => SYM_TAB,
        "Backspace" => SYM_BACKSPACE,
        "CapsLock" => SYM_CAPSLOCK,
        "Enter" | "Return" => SYM_RETURN,
        "Space" => SYM_SPACE,
        _ if s.chars().count() == 1 => Keysym::from_char(s.chars().next()?),
        _ => return None,
    };
    Some(ksym)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(key: &str) -> Keybinding {
        Keybinding::parse(&KeybindValue::new(key, "ok", "")).unwrap()
    }

    #[test]
    fn match_ctrl_f12() {
        let target = KeyboardEvent::builder().ksym(SYM_F12).control().build();
        assert_eq!(Some(target), key_from_str("Ctrl+F12"));
        assert_eq!(Some(target), key_from_str("ctrl+F12"));
        assert_eq!(Some(target), key_from_str("control+F12"));
        let kb = binding("Ctrl+F12");
        assert!(kb.matches(&target));
        assert!(
            !kb.matches(
                &KeyboardEvent::builder()
                    .ksym(SYM_LEFTCTRL)
                    .control()
                    .build()
            )
        );
        // Extra modifiers do not match.
        assert!(
            !kb.matches(
                &KeyboardEvent::builder()
                    .ksym(SYM_F12)
                    .control()
                    .shift()
                    .build()
            )
        );
    }

    #[test]
    fn match_ctrl_shift_letter() {
        let target = KeyboardEvent::builder()
            .ksym(Keysym::from_char('A'))
            .control()
            .shift()
            .build();
        assert_eq!(Some(target), key_from_str("Ctrl+Shift+A"));
        assert!(binding("ctrl+shift+A").matches(&target));
    }

    #[test]
    fn match_command_option() {
        let target = KeyboardEvent::builder()
            .ksym(Keysym::from_char('k'))
            .super_if(true)
            .alt_if(true)
            .build();
        assert_eq!(Some(target), key_from_str("Cmd+Opt+k"));
    }

    #[test]
    fn invalid_keys_are_rejected() {
        assert_eq!(key_from_str("Ctrl+NoSuchKey"), None);
        assert!(Keybinding::parse(&KeybindValue::new("Hyper+F1", "x", "")).is_none());
    }
}
