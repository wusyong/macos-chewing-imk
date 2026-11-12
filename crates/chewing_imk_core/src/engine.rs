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

use chewing::editor::{BasicEditor, CharacterForm, Editor, EditorKeyBehavior, LanguageMode};
use chewing::input::keysym::{self, Keysym, SYM_SPACE};
use chewing::input::{KeyState, KeyboardEvent, keycode};

use crate::config::Config;
use crate::keymap::KeyInput;

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

pub struct ChewingCore {
    editor: Editor,
    cfg: Config,
    lang_mode: LangMode,
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
        Ok(ChewingCore {
            editor,
            cfg,
            lang_mode,
            has_marked_text: false,
            candidates_shown: false,
        })
    }

    pub fn lang_mode(&self) -> LangMode {
        self.lang_mode
    }

    /// Whether there is anything on screen that belongs to the input method.
    pub fn is_composing(&self) -> bool {
        self.has_marked_text || self.candidates_shown
    }

    pub fn handle_key(&mut self, input: &KeyInput) -> KeyResponse {
        let lang_mode = self.lang_mode;
        self.editor
            .set_editor_options(|opt| opt.language_mode = lang_mode.into());

        let evt = input.to_keyboard_event();
        if !self.should_handle(&evt) {
            return KeyResponse::default();
        }
        self.process(evt)
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
            actions.push(Action::Commit(text));
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
    /// `on_test_keydown`; the Shift toggle and keybindings come in Phase 2.
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
                actions.push(Action::Commit(text));
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
        let composition = self.composition();
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
