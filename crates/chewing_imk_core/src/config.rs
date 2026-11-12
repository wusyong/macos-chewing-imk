// SPDX-License-Identifier: GPL-3.0-or-later

//! Input method settings. Field names and defaults follow windows-chewing-tsf's
//! `ChewingTsfConfig` so behavior matches. Display settings (fonts, colors,
//! candidate layout) are added with the UI in Phase 3; loading from disk
//! comes in Phase 4.

use std::time::Duration;

use chewing::editor::zhuyin_layout::{
    DaiChien26, Et, Et26, GinYieh, Hsu, Ibm, KeyboardLayoutCompat, Pinyin, Standard, SyllableEditor,
};
use chewing::editor::{CharacterForm, ConversionEngineKind, EditorOptions, UserPhraseAddDirection};

use crate::keybind::KeybindValue;
use crate::keymap::{KeymapOp, SimulatedKeyboard};

/// Selection key sets, indexed by [`Config::sel_key_type`].
pub const SEL_KEYS: [&str; 6] = [
    "1234567890",
    "asdfghjkl;",
    "asdfzxcv89",
    "asdfjkl789",
    "aoeuhtn789",
    "1234qweras",
];

#[derive(Debug, Clone)]
pub struct Config {
    /// Tap Shift alone to switch between Chinese and English.
    pub switch_lang_with_shift: bool,
    /// A Shift tap must be released within this time to count.
    pub shift_key_sensitivity: Duration,
    pub enable_caps_lock: bool,
    pub lock_chinese_on_caps_lock: bool,
    pub enable_auto_learn: bool,
    pub auto_learn_user_history: bool,
    pub auto_learn_new_words: bool,
    pub esc_clean_all_buf: bool,
    pub full_shape_symbols: bool,
    pub upper_case_with_shift: bool,
    pub add_phrase_forward: bool,
    pub phrase_choice_rearward: bool,
    pub auto_wrap_cursor: bool,
    pub easy_symbols_with_shift: bool,
    pub easy_symbols_with_shift_ctrl: bool,
    pub enable_fullwidth_toggle_key: bool,
    pub sort_candidates_by_frequency: bool,
    pub show_cand_with_space_key: bool,
    pub cycle_first_n_selects: usize,
    pub advance_after_selection: bool,
    pub default_full_space: bool,
    pub default_english: bool,
    pub sel_key_type: usize,
    /// 0 = simple, 1 = chewing, 2 = fuzzy chewing
    pub conv_engine: u8,
    pub cand_per_page: usize,
    /// Move a highlight with the arrow keys and pick it with Enter.
    pub cursor_cand_list: bool,
    /// Candidates per row in the window (1 = a vertical list).
    pub cand_per_row: usize,
    /// Candidate window font size, in points.
    pub font_size: f64,
    /// Candidate window font; empty means the system font.
    pub font_family: String,
    pub keyboard_layout: KeyboardLayoutCompat,
    pub simulate_english_layout: SimulatedKeyboard,
    pub show_notification: bool,
    pub output_simp_chinese: bool,
    pub keybind: Vec<KeybindValue>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            switch_lang_with_shift: true,
            shift_key_sensitivity: Duration::from_millis(200),
            enable_caps_lock: false,
            lock_chinese_on_caps_lock: true,
            enable_auto_learn: true,
            auto_learn_user_history: true,
            auto_learn_new_words: false,
            esc_clean_all_buf: false,
            full_shape_symbols: true,
            upper_case_with_shift: false,
            add_phrase_forward: true,
            phrase_choice_rearward: false,
            auto_wrap_cursor: false,
            easy_symbols_with_shift: true,
            easy_symbols_with_shift_ctrl: false,
            enable_fullwidth_toggle_key: false,
            sort_candidates_by_frequency: false,
            show_cand_with_space_key: false,
            cycle_first_n_selects: 0,
            advance_after_selection: true,
            default_full_space: false,
            default_english: false,
            sel_key_type: 0,
            conv_engine: 1,
            cand_per_page: 9,
            cursor_cand_list: true,
            cand_per_row: 3,
            font_size: 16.0,
            font_family: String::new(),
            keyboard_layout: KeyboardLayoutCompat::Default,
            simulate_english_layout: SimulatedKeyboard::Qwerty,
            show_notification: true,
            output_simp_chinese: false,
            keybind: vec![
                KeybindValue::new("Ctrl+F12", "toggle_simplified_chinese", ""),
                KeybindValue::new("Ctrl+Delete", "selecting_unlearn_phrase", ""),
            ],
        }
    }
}

impl Config {
    pub fn sel_keys(&self) -> &'static str {
        SEL_KEYS
            .get(self.sel_key_type)
            .copied()
            .unwrap_or(SEL_KEYS[0])
    }

    /// The macOS keyboard layout to ask the client for while we are active.
    ///
    /// Most zhuyin layouts are defined on physical keys, so we ask for ABC
    /// (QWERTY) whatever the user picked last. The Dvorak and Colemak
    /// variants are defined on the characters those layouts produce
    /// (`KeymapOp::Ksym`), so they need the matching system layout. Layouts
    /// macOS does not ship (Carpalx, Colemak-DH, Workman) are left to the
    /// user's own installed layout.
    pub fn base_keyboard_layout(&self) -> Option<&'static str> {
        use KeyboardLayoutCompat as K;
        match self.keyboard_layout {
            K::Dvorak | K::DvorakHsu => Some("com.apple.keylayout.Dvorak"),
            K::Colemak => Some("com.apple.keylayout.Colemak"),
            K::Carpalx | K::ColemakDhAnsi | K::ColemakDhOrth | K::Workman => None,
            _ => Some("com.apple.keylayout.ABC"),
        }
    }

    /// How keys are reinterpreted for the configured layouts. Ported from
    /// `keymap_from_kbtype` and `apply_runtime_config`.
    pub(crate) fn keymap(&self) -> KeymapOp {
        if self.simulate_english_layout != SimulatedKeyboard::Qwerty {
            return self.simulate_english_layout.into();
        }
        keymap_from_kbtype(self.keyboard_layout)
    }

    /// Apply the settings to libchewing's editor options. Ported from
    /// windows-chewing-tsf's `build_editor_from_cfg`.
    pub(crate) fn apply_to(&self, opt: &mut EditorOptions) {
        opt.easy_symbol_input = self.easy_symbols_with_shift || self.easy_symbols_with_shift_ctrl;
        // NB: Historically the config was inverted.
        opt.user_phrase_add_dir = if self.add_phrase_forward {
            UserPhraseAddDirection::Backward
        } else {
            UserPhraseAddDirection::Forward
        };
        opt.phrase_choice_rearward = self.phrase_choice_rearward;
        opt.auto_shift_cursor = self.advance_after_selection;
        opt.auto_wrap_cursor = self.auto_wrap_cursor;
        opt.candidates_per_page = self.cand_per_page;
        opt.esc_clear_all_buffer = self.esc_clean_all_buf;
        opt.space_is_select_key = self.show_cand_with_space_key;
        opt.cycle_first_n_selects = self.cycle_first_n_selects;
        opt.disable_auto_learn_phrase = !self.enable_auto_learn;
        opt.auto_learn_user_history = self.auto_learn_user_history;
        opt.auto_learn_new_words = self.auto_learn_new_words;
        opt.enable_fullwidth_toggle_key = self.enable_fullwidth_toggle_key;
        opt.sort_candidates_by_frequency = self.sort_candidates_by_frequency;
        opt.conversion_engine = match self.conv_engine {
            0 => ConversionEngineKind::SimpleEngine,
            2 => ConversionEngineKind::FuzzyChewingEngine,
            _ => ConversionEngineKind::ChewingEngine,
        };
        // From `apply_init_config`.
        opt.character_form = if self.default_full_space {
            CharacterForm::Fullwidth
        } else {
            CharacterForm::Halfwidth
        };
        opt.auto_commit_threshold = 50;
    }

    /// Ported from windows-chewing-tsf's `syl_editor_from_kbtype`.
    pub(crate) fn syllable_editor(&self) -> Box<dyn SyllableEditor> {
        use KeyboardLayoutCompat as K;
        match self.keyboard_layout {
            K::Default
            | K::Dvorak
            | K::Carpalx
            | K::ColemakDhAnsi
            | K::ColemakDhOrth
            | K::Workman
            | K::Colemak => Box::new(Standard::new()),
            K::Hsu | K::DvorakHsu => Box::new(Hsu::new()),
            K::Ibm => Box::new(Ibm::new()),
            K::GinYieh => Box::new(GinYieh::new()),
            K::Et => Box::new(Et::new()),
            K::Et26 => Box::new(Et26::new()),
            K::DachenCp26 => Box::new(DaiChien26::new()),
            K::HanyuPinyin => Box::new(Pinyin::hanyu()),
            K::ThlPinyin => Box::new(Pinyin::thl()),
            K::Mps2Pinyin => Box::new(Pinyin::mps2()),
        }
    }
}

/// Ported from windows-chewing-tsf's `keymap_from_kbtype`. These layouts are
/// defined relative to the characters a non-QWERTY system layout produces.
pub(crate) fn keymap_from_kbtype(kbtype: KeyboardLayoutCompat) -> KeymapOp {
    use KeyboardLayoutCompat as K;
    use chewing::input::keymap::*;
    match kbtype {
        K::Dvorak => KeymapOp::Ksym(&INVERTED_DVORAK_MAP),
        K::DvorakHsu => KeymapOp::Ksym(&DVORAK_MAP),
        K::Carpalx => KeymapOp::Ksym(&INVERTED_QGMLWY_MAP),
        K::ColemakDhAnsi => KeymapOp::Ksym(&INVERTED_COLEMAK_DH_ANSI_MAP),
        K::ColemakDhOrth => KeymapOp::Ksym(&INVERTED_COLEMAK_DH_ORTH_MAP),
        K::Workman => KeymapOp::Ksym(&INVERTED_WORKMAN_MAP),
        K::Colemak => KeymapOp::Ksym(&INVERTED_COLEMAK_MAP),
        _ => KeymapOp::None,
    }
}
