// SPDX-License-Identifier: GPL-3.0-or-later

//! The key handling state machine.
//!
//! The rules in [`ChewingCore::handle_key`] are ported from windows-chewing-tsf
//! (`on_test_keydown` and `on_keydown` in `tip/src/text_service/chewing.rs`).
//! The TSF-specific structure (two-phase test/keydown, edit sessions) is gone:
//! macOS asks once per key, and we answer with "handled or not" plus a list of
//! [`Action`]s for the platform layer to carry out, in order.

use std::error::Error;
use std::path::Path;
use std::time::Duration;

use chewing::editor::zhuyin_layout::KeyboardLayoutCompat;
use chewing::editor::{BasicEditor, CharacterForm, Editor, EditorKeyBehavior, LanguageMode};
use chewing::input::keysym::{self, Keysym, SYM_SPACE};
use chewing::input::{KeyState, KeyboardEvent, keycode};
use zhconv::{Variant, zhconv};

use crate::config::Config;
use crate::keybind::Keybinding;
use crate::keymap::{KeyInput, KeymapOp, ModifierInput, kvk};

/// Text being composed, shown inline at the insertion point (marked text).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Composition {
    pub text: String,
    /// Caret position, counted in `char`s of `text`.
    pub cursor: usize,
}

impl Composition {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// One page of candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateList {
    pub items: Vec<String>,
    /// Selection key for each item, same length as `items`.
    pub sel_keys: Vec<char>,
    /// 1-based.
    pub page: usize,
    pub total_pages: usize,
}

/// What the platform layer must do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Insert text into the client, replacing any marked text.
    Commit(String),
    /// Replace the marked text. An empty composition removes it.
    UpdateComposition(Composition),
    ShowCandidates(CandidateList),
    HideCandidates,
    /// Briefly show a short message near the insertion point.
    ShowNotice(String),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct KeyResponse {
    /// Whether the key was consumed. If false the client receives it.
    pub handled: bool,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LangMode {
    Chinese,
    English,
}

impl From<LangMode> for LanguageMode {
    fn from(value: LangMode) -> Self {
        match value {
            LangMode::Chinese => LanguageMode::Chinese,
            LangMode::English => LanguageMode::English,
        }
    }
}

/// Tracks whether Shift is being tapped on its own. Ported from
/// windows-chewing-tsf's `ShiftKeyState`; times come from the key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShiftKeyState {
    Up,
    /// Pressed at this event timestamp, nothing else pressed since.
    Down(Duration),
    /// Used as a modifier for another key.
    Consumed,
}

impl ShiftKeyState {
    /// Returns how long Shift was held if it was tapped alone.
    fn release(&mut self, now: Duration) -> Option<Duration> {
        let held = match *self {
            ShiftKeyState::Down(since) => Some(now.saturating_sub(since)),
            ShiftKeyState::Consumed | ShiftKeyState::Up => None,
        };
        *self = ShiftKeyState::Up;
        held
    }
}

pub struct ChewingCore {
    editor: Editor,
    cfg: Config,
    lang_mode: LangMode,
    kbtype: KeyboardLayoutCompat,
    keymap: KeymapOp,
    keybindings: Vec<Keybinding>,
    shift_state: ShiftKeyState,
    output_simp_chinese: bool,
    /// Marked text is currently shown in the client.
    has_marked_text: bool,
    /// The candidate window is currently shown.
    candidates_shown: bool,
}

impl ChewingCore {
    /// `dict_dir` holds the system dictionary (static_dict.bin, ...).
    /// `user_dir` holds the user dictionary; `None` uses libchewing's default
    /// (`~/Library/Application Support/im.chewing.Chewing`).
    pub fn new(
        dict_dir: &Path,
        user_dir: Option<&Path>,
        cfg: Config,
    ) -> Result<ChewingCore, Box<dyn Error + Send + Sync>> {
        let mut editor = Editor::chewing(
            Some(dict_dir.to_string_lossy().into_owned()),
            user_dir.map(|p| p.to_string_lossy().into_owned()),
        )?;
        editor.set_editor_options(|opt| cfg.apply_to(opt));
        editor.set_syllable_editor(cfg.syllable_editor());
        let lang_mode = if cfg.default_english {
            LangMode::English
        } else {
            LangMode::Chinese
        };
        let keybindings = cfg.keybind.iter().filter_map(Keybinding::parse).collect();
        Ok(ChewingCore {
            editor,
            kbtype: cfg.keyboard_layout,
            keymap: cfg.keymap(),
            keybindings,
            shift_state: ShiftKeyState::Up,
            output_simp_chinese: cfg.output_simp_chinese,
            cfg,
            lang_mode,
            has_marked_text: false,
            candidates_shown: false,
        })
    }

    pub fn lang_mode(&self) -> LangMode {
        self.lang_mode
    }

    pub fn output_simp_chinese(&self) -> bool {
        self.output_simp_chinese
    }

    pub fn character_form(&self) -> CharacterForm {
        self.editor.editor_options().character_form
    }

    /// Whether there is anything on screen that belongs to the input method.
    pub fn is_composing(&self) -> bool {
        self.has_marked_text || self.candidates_shown
    }

    pub fn handle_key(&mut self, input: &KeyInput) -> KeyResponse {
        let lang_mode = self.lang_mode;
        self.editor
            .set_editor_options(|opt| opt.language_mode = lang_mode.into());

        let evt = input.to_keyboard_event(self.keymap);
        // A key pressed while Shift is held: Shift is a modifier this time,
        // not a tap. (On macOS Shift itself only arrives as a modifier change.)
        if input.modifiers.shift {
            self.shift_state = ShiftKeyState::Consumed;
        }

        let keybinding = self.keybindings.iter().find(|kb| kb.matches(&evt)).cloned();
        if let Some(kb) = &keybinding
            && !kb.action.starts_with("selecting_")
        {
            return self.run_keybinding(kb);
        }
        // Keybindings always belong to us; `selecting_*` ones are handled
        // with the candidate window (Phase 3) and otherwise act as a key.
        if keybinding.is_none() && !self.should_handle(&evt) {
            return KeyResponse::default();
        }
        self.process(evt)
    }

    /// A modifier key changed (`NSEventTypeFlagsChanged`). Implements the
    /// lone Shift tap and Caps Lock, ported from `on_keyup`. The event is
    /// never consumed.
    pub fn handle_modifiers(&mut self, input: &ModifierInput) -> KeyResponse {
        let mut actions = vec![];
        let m = input.modifiers;
        match input.key_code {
            kvk::SHIFT | kvk::RIGHT_SHIFT if m.shift => {
                // Shift went down. Only a plain Shift press can become a tap.
                let plain =
                    self.cfg.switch_lang_with_shift && !(m.control || m.option || m.command);
                self.shift_state = match self.shift_state {
                    ShiftKeyState::Up if plain => ShiftKeyState::Down(input.timestamp),
                    // Chromium-based clients (Electron apps such as Claude,
                    // VS Code) deliver every modifier change twice. A repeated
                    // press is the same press; keep timing from the first.
                    ShiftKeyState::Down(since) if plain => ShiftKeyState::Down(since),
                    _ => ShiftKeyState::Consumed,
                };
            }
            kvk::SHIFT | kvk::RIGHT_SHIFT => {
                let tapped = self
                    .shift_state
                    .release(input.timestamp)
                    .is_some_and(|held| held < self.cfg.shift_key_sensitivity);
                if tapped && self.cfg.switch_lang_with_shift {
                    if self.cfg.enable_caps_lock {
                        // The language mode follows Caps Lock instead.
                        let msg = match self.lang_mode {
                            LangMode::English => "CapsLock 鎖定英數模式",
                            LangMode::Chinese => "CapsLock 鎖定中文模式",
                        };
                        self.push_notice(&mut actions, msg);
                    } else {
                        self.toggle_lang_mode_into(&mut actions);
                    }
                }
            }
            kvk::CAPS_LOCK if self.cfg.enable_caps_lock => {
                actions.extend(self.sync_caps_lock(m.caps_lock));
                let msg = self.lang_mode_name();
                self.push_notice(&mut actions, msg);
            }
            _ => {
                // Another modifier while Shift is down: not a lone tap.
                if m.shift {
                    self.shift_state = ShiftKeyState::Consumed;
                }
            }
        }
        KeyResponse {
            handled: false,
            actions,
        }
    }

    /// With `enable_caps_lock`, the language mode follows the Caps Lock
    /// state. Call when activated and when Caps Lock changes. Ported from
    /// `sync_lang_mode`.
    pub fn sync_caps_lock(&mut self, caps_lock_on: bool) -> Vec<Action> {
        let mut actions = vec![];
        if !self.cfg.enable_caps_lock {
            return actions;
        }
        let (locked, unlocked) = if self.cfg.lock_chinese_on_caps_lock {
            (LangMode::Chinese, LangMode::English)
        } else {
            (LangMode::English, LangMode::Chinese)
        };
        let mode = if caps_lock_on { locked } else { unlocked };
        if mode != self.lang_mode {
            self.lang_mode = mode;
            self.editor.clear_syllable_editor();
            self.push_composition(&mut actions);
        }
        actions
    }

    /// Switch between Chinese and English (Shift tap, or the menu).
    pub fn toggle_lang_mode(&mut self) -> Vec<Action> {
        let mut actions = vec![];
        self.toggle_lang_mode_into(&mut actions);
        actions
    }

    /// Switch between half and full width (the menu; Shift+Space is handled
    /// by libchewing when `enable_fullwidth_toggle_key` is set).
    pub fn toggle_shape_mode(&mut self) {
        self.editor.set_editor_options(|opt| {
            opt.character_form = match opt.character_form {
                CharacterForm::Fullwidth => CharacterForm::Halfwidth,
                CharacterForm::Halfwidth => CharacterForm::Fullwidth,
            }
        });
    }

    /// Switch simplified Chinese output on or off.
    pub fn toggle_simplified_chinese(&mut self) -> Vec<Action> {
        self.output_simp_chinese = !self.output_simp_chinese;
        let mut actions = vec![];
        // Re-render what is being composed in the new script.
        self.push_composition(&mut actions);
        self.push_candidates(&mut actions);
        let msg = if self.output_simp_chinese {
            "簡體中文輸出"
        } else {
            "繁體中文輸出"
        };
        self.push_notice(&mut actions, msg);
        actions
    }

    fn toggle_lang_mode_into(&mut self, actions: &mut Vec<Action>) {
        self.lang_mode = match self.lang_mode {
            LangMode::Chinese => LangMode::English,
            LangMode::English => LangMode::Chinese,
        };
        // Drop half-typed bopomofo; converted text stays.
        self.editor.clear_syllable_editor();
        self.push_composition(actions);
        let msg = self.lang_mode_name();
        self.push_notice(actions, msg);
    }

    fn lang_mode_name(&self) -> &'static str {
        match self.lang_mode {
            LangMode::English => "英數模式",
            LangMode::Chinese => "中文模式",
        }
    }

    fn push_notice(&self, actions: &mut Vec<Action>, msg: &str) {
        if self.cfg.show_notification {
            actions.push(Action::ShowNotice(msg.to_owned()));
        }
    }

    /// Ported from the keybinding part of `on_keydown`.
    fn run_keybinding(&mut self, kb: &Keybinding) -> KeyResponse {
        let mut actions = vec![];
        match kb.action.as_str() {
            "toggle_simplified_chinese" => actions = self.toggle_simplified_chinese(),
            "toggle_hsu_keyboard" => {
                let (kbtype, msg) = if self.kbtype == KeyboardLayoutCompat::Hsu {
                    (KeyboardLayoutCompat::Default, "標準鍵盤")
                } else {
                    (KeyboardLayoutCompat::Hsu, "許氏鍵盤")
                };
                self.kbtype = kbtype;
                self.keymap = crate::config::keymap_from_kbtype(kbtype);
                let cfg = Config {
                    keyboard_layout: kbtype,
                    ..Config::default()
                };
                self.editor.set_syllable_editor(cfg.syllable_editor());
                self.push_composition(&mut actions);
                self.push_notice(&mut actions, msg);
            }
            "text" => {
                // Commit what is being composed, then type the text.
                let mut text = String::new();
                if !self.editor.is_empty() && self.editor.commit().is_ok() {
                    text.push_str(self.editor.display_commit());
                    self.editor.ack();
                }
                text.push_str(&kb.param);
                self.push_candidates(&mut actions);
                if !text.is_empty() {
                    actions.push(Action::Commit(self.output(&text)));
                    self.has_marked_text = false;
                }
                self.push_composition(&mut actions);
            }
            // Unknown actions swallow the key, like windows-chewing-tsf.
            _ => {}
        }
        KeyResponse {
            handled: true,
            actions,
        }
    }

    /// Convert output to simplified Chinese if enabled.
    fn output(&self, text: &str) -> String {
        if self.output_simp_chinese {
            zhconv(text, Variant::ZhHans)
        } else {
            text.to_owned()
        }
    }

    /// Commit whatever has been converted so far and reset. Called when the
    /// client asks us to finish (`commitComposition:`) or we lose the client
    /// (`deactivateServer:`). Bopomofo that has not formed a syllable yet is
    /// dropped.
    pub fn commit_composition(&mut self) -> Vec<Action> {
        let mut actions = vec![];
        if self.editor.is_selecting() {
            let _ = self.editor.cancel_selecting();
        }
        self.editor.clear_syllable_editor();
        let mut text = String::new();
        if !self.editor.is_empty() && self.editor.commit().is_ok() {
            text = self.editor.display_commit().to_owned();
            self.editor.ack();
        }
        self.editor.clear();

        if !text.is_empty() {
            actions.push(Action::Commit(self.output(&text)));
            self.has_marked_text = false;
        } else if self.has_marked_text {
            actions.push(Action::UpdateComposition(Composition::default()));
            self.has_marked_text = false;
        }
        if self.candidates_shown {
            actions.push(Action::HideCandidates);
            self.candidates_shown = false;
        }
        actions
    }

    /// Throw away everything without committing.
    pub fn reset(&mut self) {
        self.editor.clear();
        self.has_marked_text = false;
        self.candidates_shown = false;
    }

    /// Decide whether the key belongs to us at all. Ported from
    /// `on_test_keydown`. The Shift toggle lives in `handle_modifiers`, and
    /// keybindings are checked before this.
    fn should_handle(&self, evt: &KeyboardEvent) -> bool {
        // macOS: Command shortcuts always belong to the application.
        if evt.is_state_on(KeyState::Super) {
            return false;
        }
        // Option (Alt) may be a shortcut used in the application.
        if evt.is_state_on(KeyState::Alt) {
            return false;
        }
        if evt.is_state_on(KeyState::Control) {
            // Ctrl + digit while composing adds a user phrase.
            if self.is_composing() && evt.ksym.is_digit() {
                return true;
            }
            // Ctrl + Shift + key for easy symbol input.
            return evt.is_state_on(KeyState::Shift) && self.cfg.easy_symbols_with_shift_ctrl;
        }
        if self.cfg.enable_caps_lock && !self.cfg.lock_chinese_on_caps_lock && evt.ksym.is_unicode()
        {
            // Need to handle case conversion.
            return true;
        }
        if !self.is_composing() {
            let shape_mode = self.editor.editor_options().character_form;
            // Nothing to do in pure English + half width mode...
            if self.lang_mode == LangMode::English && shape_mode == CharacterForm::Halfwidth {
                // ...except the full width toggle.
                return evt.ksym == SYM_SPACE
                    && evt.is_state_on(KeyState::Shift)
                    && self.cfg.enable_fullwidth_toggle_key;
            }
            // Leave Space to the application unless it produces a full width
            // space, so it stays available for shortcuts.
            if evt.ksym == SYM_SPACE
                && shape_mode != CharacterForm::Fullwidth
                && !evt.is_state_on(KeyState::Shift)
            {
                return false;
            }
            if !evt.ksym.is_unicode() {
                return false;
            }
        }
        true
    }

    /// Feed the key to libchewing and report the results. Ported from
    /// `on_keydown`.
    fn process(&mut self, mut evt: KeyboardEvent) -> KeyResponse {
        if evt.ksym.is_unicode() {
            let shift = evt.is_state_on(KeyState::Shift);
            let mut momentary_english_mode = false;
            let mut upper_case = shift;
            // Shift is pressed, but we don't want full width symbols, or easy
            // symbol input is not enabled: type English for this key.
            if shift
                && self.lang_mode == LangMode::Chinese
                && (!self.cfg.full_shape_symbols || evt.ksym.is_atoz())
                && !self.cfg.easy_symbols_with_shift
                && !(evt.is_state_on(KeyState::Control) && self.cfg.easy_symbols_with_shift_ctrl)
            {
                momentary_english_mode = true;
                // First determine the case by Shift...
                if !self.cfg.upper_case_with_shift {
                    upper_case = false;
                }
                // ...then invert it with the Caps Lock state.
                let caps_lock = evt.is_state_on(KeyState::CapsLock);
                if self.cfg.enable_caps_lock && self.cfg.lock_chinese_on_caps_lock {
                    if !caps_lock {
                        upper_case = !upper_case;
                    }
                } else if caps_lock {
                    upper_case = !upper_case;
                }
            }
            if evt.ksym.is_ascii() {
                let c = evt.ksym.to_unicode();
                evt.ksym = Keysym::from(if upper_case {
                    c.to_ascii_uppercase()
                } else {
                    c.to_ascii_lowercase()
                });
            }
            if self.editor.is_selecting() {
                evt = self.map_sel_key(evt);
            }
            if evt.ksym == SYM_SPACE && shift {
                self.editor.process_keyevent(evt);
            } else if self.lang_mode == LangMode::English || momentary_english_mode {
                let old = self.editor.editor_options().language_mode;
                self.editor
                    .set_editor_options(|opt| opt.language_mode = LanguageMode::English);
                self.editor.process_keyevent(evt);
                self.editor
                    .set_editor_options(|opt| opt.language_mode = old);
            } else {
                self.editor.process_keyevent(evt);
            }
        } else {
            self.editor.process_keyevent(evt);
        }

        let behavior = self.editor.last_key_behavior();
        let mut actions = vec![];
        self.push_candidates(&mut actions);
        // Messages from libchewing (e.g. "加入：…" after Ctrl+digit) are
        // always shown, like windows-chewing-tsf.
        let notice = self.editor.notification();
        if !notice.is_empty() {
            actions.push(Action::ShowNotice(notice.to_owned()));
        }

        if behavior == EditorKeyBehavior::Ignore {
            return KeyResponse {
                handled: false,
                actions,
            };
        }

        if behavior == EditorKeyBehavior::Commit {
            let text = self.editor.display_commit().to_owned();
            self.editor.ack();
            if !text.is_empty() {
                // Inserting text also replaces the marked text.
                actions.push(Action::Commit(self.output(&text)));
                self.has_marked_text = false;
            }
        }
        self.push_composition(&mut actions);

        KeyResponse {
            handled: true,
            actions,
        }
    }

    /// Map the configured selection keys to the digit keys libchewing expects.
    fn map_sel_key(&self, mut evt: KeyboardEvent) -> KeyboardEvent {
        if let Some(idx) = self
            .cfg
            .sel_keys()
            .chars()
            .position(|k| k == evt.ksym.to_unicode())
        {
            if idx < 9 {
                evt.code = keycode::Keycode(keycode::KEY_1.0 + idx as u8);
                evt.ksym = Keysym(keysym::SYM_1.0 + idx as u32);
            } else {
                evt.code = keycode::KEY_0;
                evt.ksym = keysym::SYM_0;
            }
        }
        evt
    }

    fn push_candidates(&mut self, actions: &mut Vec<Action>) {
        if self.editor.is_selecting() {
            if let Some(list) = self.candidate_list() {
                actions.push(Action::ShowCandidates(list));
                self.candidates_shown = true;
                return;
            }
            // Nothing to choose from.
            let _ = self.editor.cancel_selecting();
        }
        if self.candidates_shown {
            actions.push(Action::HideCandidates);
            self.candidates_shown = false;
        }
    }

    fn candidate_list(&self) -> Option<CandidateList> {
        let total_pages = self.editor.total_page().ok()?;
        if total_pages == 0 {
            return None;
        }
        let page = self.editor.current_page_no().ok()? + 1;
        let per_page = self.editor.editor_options().candidates_per_page;
        let mut items = self.editor.paginated_candidates().ok()?;
        items.truncate(per_page);
        let sel_keys: Vec<char> = self.cfg.sel_keys().chars().take(items.len()).collect();
        items.truncate(sel_keys.len());
        Some(CandidateList {
            items,
            sel_keys,
            page,
            total_pages,
        })
    }

    fn push_composition(&mut self, actions: &mut Vec<Action>) {
        let mut composition = self.composition();
        if self.output_simp_chinese {
            composition.text = self.output(&composition.text);
            composition.cursor = composition.cursor.min(composition.text.chars().count());
        }
        if !composition.is_empty() {
            actions.push(Action::UpdateComposition(composition));
            self.has_marked_text = true;
        } else if self.has_marked_text {
            actions.push(Action::UpdateComposition(composition));
            self.has_marked_text = false;
        }
    }

    /// The converted text with the bopomofo being typed inserted at the
    /// cursor. Ported from `update_preedit`.
    fn composition(&self) -> Composition {
        let cursor = self.editor.cursor();
        let bopomofo = self.editor.syllable_buffer_display();
        let bopomofo_len = bopomofo.chars().count();
        let mut text = String::new();
        let mut inserted = bopomofo.is_empty();
        for it in self.editor.intervals() {
            if !inserted && it.start <= cursor && cursor <= it.end {
                let head = cursor - it.start;
                text.extend(it.text.chars().take(head));
                text.push_str(&bopomofo);
                text.extend(it.text.chars().skip(head));
                inserted = true;
            } else {
                text.push_str(&it.text);
            }
        }
        if !inserted {
            text.push_str(&bopomofo);
        }
        Composition {
            text,
            cursor: cursor + bopomofo_len,
        }
    }
}

#[cfg(test)]
mod tests;
