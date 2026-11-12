// SPDX-License-Identifier: GPL-3.0-or-later

//! These tests need the real dictionary. Run `cargo xtask test`, or
//! `cargo xtask fetch-data` once and then plain `cargo test`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::keymap::{Modifiers, kvk};

fn dict_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CHEWING_TEST_DATA_DIR") {
        return dir.into();
    }
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/data");
    std::fs::read_dir(&data)
        .ok()
        .and_then(|entries| {
            entries
                .flatten()
                .map(|e| e.path().join("share/libchewing"))
                .find(|p| p.join("static_dict.bin").is_file())
        })
        .unwrap_or_else(|| {
            panic!(
                "dictionary not found under {}; run `cargo xtask fetch-data` first",
                data.display()
            )
        })
}

/// A fresh, empty user dictionary directory per test.
fn user_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "chewing_imk_core-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn core() -> ChewingCore {
    ChewingCore::new(&dict_dir(), Some(&user_dir()), Config::default()).unwrap()
}

fn kvk_for(c: char) -> u16 {
    match c.to_ascii_lowercase() {
        'a' => kvk::ANSI_A,
        'c' => kvk::ANSI_C,
        'j' => kvk::ANSI_J,
        'l' => kvk::ANSI_L,
        's' => kvk::ANSI_S,
        'u' => kvk::ANSI_U,
        '1' => kvk::ANSI_1,
        '2' => kvk::ANSI_2,
        '3' => kvk::ANSI_3,
        ' ' => kvk::SPACE,
        other => panic!("add {other:?} to kvk_for"),
    }
}

fn key_with(c: char, modifiers: Modifiers) -> KeyInput {
    KeyInput {
        key_code: kvk_for(c),
        text: Some(c),
        modifiers,
    }
}

fn key(c: char) -> KeyInput {
    key_with(c, Modifiers::default())
}

fn special(key_code: u16) -> KeyInput {
    KeyInput {
        key_code,
        text: None,
        modifiers: Modifiers::default(),
    }
}

fn type_str(core: &mut ChewingCore, s: &str) -> Vec<KeyResponse> {
    s.chars().map(|c| core.handle_key(&key(c))).collect()
}

fn last_composition(resp: &KeyResponse) -> Option<&Composition> {
    resp.actions.iter().rev().find_map(|a| match a {
        Action::UpdateComposition(c) => Some(c),
        _ => None,
    })
}

fn commits(resp: &KeyResponse) -> Vec<&str> {
    resp.actions
        .iter()
        .filter_map(|a| match a {
            Action::Commit(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn bopomofo_shows_as_marked_text() {
    let mut core = core();
    let resp = core.handle_key(&key('s'));
    assert!(resp.handled);
    assert_eq!(
        last_composition(&resp),
        Some(&Composition {
            text: "ㄋ".into(),
            cursor: 1
        })
    );
    let resp = core.handle_key(&key('u'));
    assert_eq!(last_composition(&resp).unwrap().text, "ㄋㄧ");
    assert!(core.is_composing());
}

#[test]
fn tone_key_converts_syllables() {
    let mut core = core();
    let resps = type_str(&mut core, "su3cl3");
    assert!(resps.iter().all(|r| r.handled));
    assert_eq!(
        last_composition(resps.last().unwrap()),
        Some(&Composition {
            text: "你好".into(),
            cursor: 2
        })
    );
    // Bopomofo is inserted at the cursor, after the converted text.
    let resp = core.handle_key(&key('c'));
    assert_eq!(
        last_composition(&resp),
        Some(&Composition {
            text: "你好ㄏ".into(),
            cursor: 3
        })
    );
}

#[test]
fn return_commits_and_clears_marked_text() {
    let mut core = core();
    type_str(&mut core, "su3cl3");
    let resp = core.handle_key(&special(kvk::RETURN));
    assert!(resp.handled);
    assert_eq!(commits(&resp), ["你好"]);
    // The commit replaced the marked text, so no extra empty update.
    assert_eq!(last_composition(&resp), None);
    assert!(!core.is_composing());
}

#[test]
fn backspace_removes_marked_text() {
    let mut core = core();
    core.handle_key(&key('s'));
    let resp = core.handle_key(&special(kvk::DELETE));
    assert!(resp.handled);
    assert_eq!(last_composition(&resp), Some(&Composition::default()));
    assert!(!core.is_composing());
}

#[test]
fn down_arrow_opens_candidates_and_digit_selects() {
    let mut core = core();
    type_str(&mut core, "cl3");
    let resp = core.handle_key(&special(kvk::DOWN_ARROW));
    assert!(resp.handled);
    let list = resp
        .actions
        .iter()
        .find_map(|a| match a {
            Action::ShowCandidates(l) => Some(l.clone()),
            _ => None,
        })
        .expect("candidates shown");
    assert_eq!(list.items.first().map(String::as_str), Some("好"));
    assert_eq!(list.sel_keys.len(), list.items.len());
    assert!(list.items.len() <= 9);
    assert_eq!(list.page, 1);
    assert!(core.is_composing());

    let second = list.items[1].clone();
    let resp = core.handle_key(&key('2'));
    assert!(resp.handled);
    assert!(resp.actions.contains(&Action::HideCandidates));
    assert_eq!(last_composition(&resp).unwrap().text, second);
}

#[test]
fn escape_closes_candidates() {
    let mut core = core();
    type_str(&mut core, "cl3");
    core.handle_key(&special(kvk::DOWN_ARROW));
    let resp = core.handle_key(&special(kvk::ESCAPE));
    assert!(resp.handled);
    assert!(resp.actions.contains(&Action::HideCandidates));
    assert_eq!(last_composition(&resp).unwrap().text, "好");
}

#[test]
fn keys_pass_through_when_idle() {
    let mut core = core();
    for k in [
        kvk::RETURN,
        kvk::DELETE,
        kvk::LEFT_ARROW,
        kvk::ESCAPE,
        kvk::TAB,
    ] {
        assert_eq!(
            core.handle_key(&special(k)),
            KeyResponse::default(),
            "{k:#x}"
        );
    }
    // Space is left to the application in half width mode.
    assert_eq!(core.handle_key(&key(' ')), KeyResponse::default());
}

#[test]
fn shortcuts_pass_through() {
    let mut core = core();
    let command = Modifiers {
        command: true,
        ..Default::default()
    };
    let option = Modifiers {
        option: true,
        ..Default::default()
    };
    let control = Modifiers {
        control: true,
        ..Default::default()
    };
    for m in [command, option, control] {
        assert_eq!(core.handle_key(&key_with('a', m)), KeyResponse::default());
    }
    // Command passes through even while composing.
    core.handle_key(&key('s'));
    assert_eq!(
        core.handle_key(&key_with('a', command)),
        KeyResponse::default()
    );
}

#[test]
fn control_digit_is_handled_while_composing() {
    let mut core = core();
    type_str(&mut core, "su3cl3");
    let control = Modifiers {
        control: true,
        ..Default::default()
    };
    let resp = core.handle_key(&key_with('2', control));
    assert!(resp.handled);
}

#[test]
fn shift_letter_is_easy_symbol_by_default() {
    // easy_symbols_with_shift is on by default, so Shift+letter goes to
    // libchewing in Chinese mode instead of typing English.
    let mut core = core();
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    let resp = core.handle_key(&key_with('A', shift));
    assert!(resp.handled);
}

#[test]
fn commit_composition_commits_converted_text_only() {
    let mut core = core();
    type_str(&mut core, "su3c");
    let actions = core.commit_composition();
    assert_eq!(actions, [Action::Commit("你".into())]);
    assert!(!core.is_composing());
    // Nothing left: committing again does nothing.
    assert!(core.commit_composition().is_empty());
}

#[test]
fn commit_composition_with_only_bopomofo_clears_marked_text() {
    let mut core = core();
    core.handle_key(&key('s'));
    let actions = core.commit_composition();
    assert_eq!(actions, [Action::UpdateComposition(Composition::default())]);
}
