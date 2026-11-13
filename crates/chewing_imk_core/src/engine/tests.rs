// SPDX-License-Identifier: GPL-3.0-or-later

//! These tests need the real dictionary. Run `cargo xtask test`, or
//! `cargo xtask fetch-data` once and then plain `cargo test`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::keybind::KeybindValue;
use crate::keymap::{ModifierInput, Modifiers, kvk};

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
    core_with(Config::default())
}

fn core_with(cfg: Config) -> ChewingCore {
    ChewingCore::new(&dict_dir(), Some(&user_dir()), cfg).unwrap()
}

fn kvk_for(c: char) -> u16 {
    match c.to_ascii_lowercase() {
        'a' => kvk::ANSI_A,
        'b' => kvk::ANSI_B,
        'c' => kvk::ANSI_C,
        'g' => kvk::ANSI_G,
        'z' => kvk::ANSI_Z,
        'j' => kvk::ANSI_J,
        'l' => kvk::ANSI_L,
        's' => kvk::ANSI_S,
        'u' => kvk::ANSI_U,
        '1' => kvk::ANSI_1,
        '2' => kvk::ANSI_2,
        '3' => kvk::ANSI_3,
        '4' => kvk::ANSI_4,
        '8' => kvk::ANSI_8,
        '/' | '?' => kvk::ANSI_SLASH,
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

// ---- Phase 2: Shift tap, Caps Lock, keybindings, simplified output ----

fn modifier(key_code: u16, modifiers: Modifiers, ms: u64) -> ModifierInput {
    ModifierInput {
        key_code,
        modifiers,
        timestamp: Duration::from_millis(ms),
    }
}

const SHIFT: Modifiers = Modifiers {
    shift: true,
    control: false,
    option: false,
    command: false,
    caps_lock: false,
};

/// Press Shift at `down_ms` and release it at `up_ms`.
fn tap_shift(core: &mut ChewingCore, down_ms: u64, up_ms: u64) -> KeyResponse {
    let down = core.handle_modifiers(&modifier(kvk::SHIFT, SHIFT, down_ms));
    assert_eq!(down, KeyResponse::default());
    core.handle_modifiers(&modifier(kvk::SHIFT, Modifiers::default(), up_ms))
}

fn notices(actions: &[Action]) -> Vec<&str> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::ShowNotice(n) => Some(n.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn shift_tap_switches_language() {
    let mut core = core();
    let resp = tap_shift(&mut core, 1000, 1100);
    assert!(!resp.handled, "modifier changes are never consumed");
    assert_eq!(notices(&resp.actions), ["英數模式"]);
    assert_eq!(core.lang_mode(), LangMode::English);
    // English + half width: letters go straight to the application.
    assert_eq!(core.handle_key(&key('a')), KeyResponse::default());

    let resp = tap_shift(&mut core, 2000, 2050);
    assert_eq!(notices(&resp.actions), ["中文模式"]);
    assert_eq!(core.lang_mode(), LangMode::Chinese);
}

#[test]
fn slow_shift_press_does_not_switch() {
    let mut core = core();
    let resp = tap_shift(&mut core, 1000, 1300);
    assert!(resp.actions.is_empty());
    assert_eq!(core.lang_mode(), LangMode::Chinese);
}

#[test]
fn shift_used_as_modifier_does_not_switch() {
    let mut core = core();
    core.handle_modifiers(&modifier(kvk::SHIFT, SHIFT, 1000));
    core.handle_key(&key_with('A', SHIFT));
    let resp = core.handle_modifiers(&modifier(kvk::SHIFT, Modifiers::default(), 1050));
    assert!(resp.actions.is_empty());
    assert_eq!(core.lang_mode(), LangMode::Chinese);

    // Shift pressed together with Command (e.g. Cmd+Shift+4).
    let cmd_shift = Modifiers {
        command: true,
        ..SHIFT
    };
    core.handle_modifiers(&modifier(kvk::SHIFT, cmd_shift, 2000));
    let resp = core.handle_modifiers(&modifier(
        kvk::SHIFT,
        Modifiers {
            command: true,
            ..Default::default()
        },
        2050,
    ));
    assert!(resp.actions.is_empty());

    // Command pressed while Shift is held.
    core.handle_modifiers(&modifier(kvk::SHIFT, SHIFT, 3000));
    core.handle_modifiers(&modifier(kvk::COMMAND, cmd_shift, 3010));
    core.handle_modifiers(&modifier(kvk::COMMAND, SHIFT, 3020));
    let resp = core.handle_modifiers(&modifier(kvk::SHIFT, Modifiers::default(), 3050));
    assert!(resp.actions.is_empty());
    assert_eq!(core.lang_mode(), LangMode::Chinese);
}

#[test]
fn shift_tap_can_be_disabled() {
    let mut core = core_with(Config {
        switch_lang_with_shift: false,
        ..Config::default()
    });
    assert!(tap_shift(&mut core, 1000, 1050).actions.is_empty());
    assert_eq!(core.lang_mode(), LangMode::Chinese);
}

#[test]
fn switching_language_drops_bopomofo_but_keeps_text() {
    let mut core = core();
    type_str(&mut core, "su3c");
    let resp = tap_shift(&mut core, 1000, 1050);
    assert_eq!(last_composition(&resp).unwrap().text, "你");
    // Typing English continues the same composition.
    let resp = core.handle_key(&key('a'));
    assert!(resp.handled);
    assert_eq!(last_composition(&resp).unwrap().text, "你a");
}

#[test]
fn notices_can_be_turned_off() {
    let mut core = core_with(Config {
        show_notification: false,
        ..Config::default()
    });
    assert!(tap_shift(&mut core, 1000, 1050).actions.is_empty());
    assert_eq!(core.lang_mode(), LangMode::English);
}

#[test]
fn caps_lock_controls_language_when_enabled() {
    let mut core = core_with(Config {
        enable_caps_lock: true,
        ..Config::default()
    });
    // lock_chinese_on_caps_lock (default): Caps Lock on = Chinese.
    core.sync_caps_lock(false);
    assert_eq!(core.lang_mode(), LangMode::English);

    let caps = Modifiers {
        caps_lock: true,
        ..Default::default()
    };
    let resp = core.handle_modifiers(&modifier(kvk::CAPS_LOCK, caps, 1000));
    assert_eq!(core.lang_mode(), LangMode::Chinese);
    assert_eq!(notices(&resp.actions), ["中文模式"]);

    // A Shift tap only explains why nothing changes.
    let resp = tap_shift(&mut core, 2000, 2050);
    assert_eq!(notices(&resp.actions), ["CapsLock 鎖定中文模式"]);
    assert_eq!(core.lang_mode(), LangMode::Chinese);
}

#[test]
fn caps_lock_is_ignored_by_default() {
    let mut core = core();
    let caps = Modifiers {
        caps_lock: true,
        ..Default::default()
    };
    assert!(core.sync_caps_lock(false).is_empty());
    let resp = core.handle_modifiers(&modifier(kvk::CAPS_LOCK, caps, 1000));
    assert!(resp.actions.is_empty());
    assert_eq!(core.lang_mode(), LangMode::Chinese);
}

#[test]
fn ctrl_f12_toggles_simplified_output() {
    let mut core = core();
    let ctrl_f12 = KeyInput {
        key_code: kvk::F12,
        text: None,
        modifiers: Modifiers {
            control: true,
            ..Default::default()
        },
    };
    let resp = core.handle_key(&ctrl_f12);
    assert!(resp.handled);
    assert_eq!(notices(&resp.actions), ["簡體中文輸出"]);
    assert!(core.output_simp_chinese());

    // ㄕㄨ ㄖㄨˋ ㄈㄚˇ = 輸入法
    let resps = type_str(&mut core, "gj bj4z83");
    assert_eq!(
        last_composition(resps.last().unwrap()).unwrap().text,
        "输入法"
    );
    let resp = core.handle_key(&special(kvk::RETURN));
    assert_eq!(commits(&resp), ["输入法"]);

    let resp = core.handle_key(&ctrl_f12);
    assert_eq!(notices(&resp.actions), ["繁體中文輸出"]);
    assert!(!core.output_simp_chinese());
}

#[test]
fn text_keybinding_commits_then_types() {
    let mut core = core_with(Config {
        keybind: vec![KeybindValue::new("Ctrl+Shift+A", "text", "!?")],
        ..Config::default()
    });
    type_str(&mut core, "su3");
    let ctrl_shift = Modifiers {
        control: true,
        ..SHIFT
    };
    let resp = core.handle_key(&key_with('A', ctrl_shift));
    assert!(resp.handled);
    assert_eq!(commits(&resp), ["你!?"]);
    assert!(!core.is_composing());
}

#[test]
fn hsu_keybinding_switches_layout() {
    let mut core = core_with(Config {
        keybind: vec![KeybindValue::new("Ctrl+F11", "toggle_hsu_keyboard", "")],
        ..Config::default()
    });
    let ctrl_f11 = KeyInput {
        key_code: kvk::F11,
        text: None,
        modifiers: Modifiers {
            control: true,
            ..Default::default()
        },
    };
    let resp = core.handle_key(&ctrl_f11);
    assert_eq!(notices(&resp.actions), ["許氏鍵盤"]);
    // In the Hsu layout "n" is ㄋ.
    let resp = core.handle_key(&KeyInput {
        key_code: kvk::ANSI_N,
        text: Some('n'),
        modifiers: Modifiers::default(),
    });
    assert_eq!(last_composition(&resp).unwrap().text, "ㄋ");

    let resp = core.handle_key(&ctrl_f11);
    assert_eq!(notices(&resp.actions), ["標準鍵盤"]);
}

#[test]
fn libchewing_notifications_are_shown() {
    // Ctrl+2 after two syllables adds them as a user phrase.
    let mut core = core();
    type_str(&mut core, "su3cl3");
    let control = Modifiers {
        control: true,
        ..Default::default()
    };
    let resp = core.handle_key(&key_with('2', control));
    let shown = notices(&resp.actions);
    assert_eq!(shown.len(), 1, "{:?}", resp.actions);
    assert!(shown[0].contains("你好"), "{shown:?}");
}

#[test]
fn duplicated_modifier_events_still_toggle_once() {
    // Chromium-based apps (Claude, VS Code) deliver each modifier change
    // twice, a few milliseconds apart.
    let mut core = core();
    let up = Modifiers::default();
    core.handle_modifiers(&modifier(kvk::SHIFT, SHIFT, 6580));
    core.handle_modifiers(&modifier(kvk::SHIFT, SHIFT, 6584));
    let first = core.handle_modifiers(&modifier(kvk::SHIFT, up, 6747));
    let second = core.handle_modifiers(&modifier(kvk::SHIFT, up, 6749));
    assert_eq!(notices(&first.actions), ["英數模式"]);
    assert!(second.actions.is_empty());
    assert_eq!(core.lang_mode(), LangMode::English);

    // A duplicated slow press still does not count.
    core.handle_modifiers(&modifier(kvk::SHIFT, SHIFT, 10_000));
    core.handle_modifiers(&modifier(kvk::SHIFT, SHIFT, 10_150));
    let resp = core.handle_modifiers(&modifier(kvk::SHIFT, up, 10_250));
    assert!(resp.actions.is_empty());
    assert_eq!(core.lang_mode(), LangMode::English);
}

// ---- Phase 3: candidate cursor ----

fn shown_list(resp: &KeyResponse) -> Option<&CandidateList> {
    resp.actions.iter().rev().find_map(|a| match a {
        Action::ShowCandidates(l) => Some(l),
        _ => None,
    })
}

#[test]
fn arrow_keys_move_the_candidate_highlight() {
    let mut core = core();
    type_str(&mut core, "su3");
    let resp = core.handle_key(&special(kvk::DOWN_ARROW));
    let list = shown_list(&resp).unwrap();
    assert_eq!(list.per_row, 3);
    assert_eq!(list.highlighted, Some(0));
    assert!(list.items.len() > 4, "need two rows: {:?}", list.items);

    let steps = [
        (kvk::RIGHT_ARROW, 1),
        (kvk::DOWN_ARROW, 4),
        (kvk::UP_ARROW, 1),
        (kvk::LEFT_ARROW, 0),
    ];
    for (k, expected) in steps {
        let resp = core.handle_key(&special(k));
        assert!(resp.handled);
        // Only the candidate window changes.
        assert_eq!(resp.actions.len(), 1, "{:?}", resp.actions);
        assert_eq!(shown_list(&resp).unwrap().highlighted, Some(expected));
    }
}

#[test]
fn return_picks_the_highlighted_candidate() {
    let mut core = core();
    type_str(&mut core, "cl3");
    let resp = core.handle_key(&special(kvk::DOWN_ARROW));
    let third = shown_list(&resp).unwrap().items[2].clone();
    core.handle_key(&special(kvk::RIGHT_ARROW));
    core.handle_key(&special(kvk::RIGHT_ARROW));
    let resp = core.handle_key(&special(kvk::RETURN));
    assert!(resp.handled);
    assert!(resp.actions.contains(&Action::HideCandidates));
    assert!(commits(&resp).is_empty(), "picking does not commit yet");
    assert_eq!(last_composition(&resp).unwrap().text, third);
}

#[test]
fn without_cursor_mode_arrows_go_to_libchewing() {
    let mut core = core_with(Config {
        cursor_cand_list: false,
        ..Config::default()
    });
    type_str(&mut core, "cl3");
    let resp = core.handle_key(&special(kvk::DOWN_ARROW));
    assert_eq!(shown_list(&resp).unwrap().highlighted, None);
    // Right is handled by libchewing (not a highlight move).
    let resp = core.handle_key(&special(kvk::RIGHT_ARROW));
    assert!(shown_list(&resp).is_none_or(|l| l.highlighted.is_none()));
}

#[test]
fn ctrl_delete_unlearns_the_highlighted_phrase() {
    let mut core = core();
    let control = Modifiers {
        control: true,
        ..Default::default()
    };
    // Teach "你好" as a user phrase, then look at the candidates for it.
    type_str(&mut core, "su3cl3");
    core.handle_key(&key_with('2', control));
    core.handle_key(&special(kvk::HOME));
    let resp = core.handle_key(&special(kvk::DOWN_ARROW));
    let list = shown_list(&resp).unwrap();
    let idx = list
        .items
        .iter()
        .position(|c| c == "你好")
        .unwrap_or_else(|| panic!("你好 not offered: {:?}", list.items));
    for _ in 0..idx {
        core.handle_key(&special(kvk::RIGHT_ARROW));
    }
    let resp = core.handle_key(&KeyInput {
        key_code: kvk::FORWARD_DELETE,
        text: None,
        modifiers: control,
    });
    assert!(resp.handled);
    assert_eq!(notices(&resp.actions), ["刪除：你好"]);
}

// ---- Phase 4: applying new settings ----

#[test]
fn apply_config_switches_layout_and_keeps_modes() {
    let mut core = core();
    tap_shift(&mut core, 1000, 1050); // English
    core.toggle_shape_mode(); // full width
    let cfg = Config {
        keyboard_layout: chewing::editor::zhuyin_layout::KeyboardLayoutCompat::Hsu,
        cand_per_row: 1,
        ..Config::default()
    };
    core.apply_config(cfg).unwrap();
    assert_eq!(core.lang_mode(), LangMode::English);
    assert_eq!(core.character_form(), CharacterForm::Fullwidth);
    assert_eq!(core.config().cand_per_row, 1);

    // Back to Chinese: "n" is ㄋ in the Hsu layout.
    tap_shift(&mut core, 2000, 2050);
    let resp = core.handle_key(&KeyInput {
        key_code: kvk::ANSI_N,
        text: Some('n'),
        modifiers: Modifiers::default(),
    });
    assert_eq!(last_composition(&resp).unwrap().text, "ㄋ");
}

#[test]
fn base_keyboard_layout_follows_zhuyin_layout() {
    use chewing::editor::zhuyin_layout::KeyboardLayoutCompat as K;
    let layout = |k| {
        Config {
            keyboard_layout: k,
            ..Config::default()
        }
        .base_keyboard_layout()
    };
    assert_eq!(layout(K::Default), Some("com.apple.keylayout.ABC"));
    assert_eq!(layout(K::Hsu), Some("com.apple.keylayout.ABC"));
    assert_eq!(layout(K::Dvorak), Some("com.apple.keylayout.Dvorak"));
    assert_eq!(layout(K::Colemak), Some("com.apple.keylayout.Colemak"));
    assert_eq!(layout(K::Workman), None);
}

#[test]
fn user_phrases_export_and_import() {
    let mut core = core();
    assert_eq!(core.export_user_phrases(), "phrase,bopomofo\n");

    let report = core.import_user_phrases(
        "phrase,bopomofo\n# comment\n\n你好,ㄋㄧˇ ㄏㄠˇ\n輸入法,ㄕㄨ ㄖㄨˋ ㄈㄚˇ\n壞的,ㄏㄨㄞˋ\nnot csv\n",
    );
    assert_eq!(report.added, 2);
    assert_eq!(report.skipped, vec![6, 7]);

    let exported = core.export_user_phrases();
    assert!(exported.contains("你好,ㄋㄧˇ ㄏㄠˇ\n"), "{exported}");
    assert!(exported.contains("輸入法,ㄕㄨ ㄖㄨˋ ㄈㄚˇ\n"), "{exported}");

    // What we export, a fresh dictionary imports in full.
    let mut other = self::core();
    let report = other.import_user_phrases(&exported);
    assert_eq!(report.added, 2);
    assert!(report.skipped.is_empty());
    assert_eq!(other.export_user_phrases(), exported);
}

#[test]
fn csv_quoting_round_trips() {
    assert_eq!(super::csv_field("a,b"), "\"a,b\"");
    assert_eq!(super::csv_unquote("\"a,\"\"b\"\"\""), "a,\"b\"");
    assert_eq!(super::csv_unquote("plain"), "plain");
}

#[test]
fn shift_slash_types_fullwidth_question_mark() {
    // macOS reports Shift+/ as "?" with Shift held; it must type "？".
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut core = core();
    let resp = core.handle_key(&key_with('?', shift));
    assert!(resp.handled);
    assert_eq!(
        resp.actions,
        [Action::UpdateComposition(Composition {
            text: "？".into(),
            cursor: 1,
        })]
    );
}
