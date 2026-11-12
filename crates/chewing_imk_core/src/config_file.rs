// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading and writing [`Config`] as TOML.
//!
//! Keys and value encodings follow windows-chewing-tsf's `ChewingTsfConfig`
//! (for example `keyboard_layout = 0`, `shift_key_sensitivity = 200` in
//! milliseconds), so the meaning of every setting is the same on both
//! platforms. Missing keys take their default; unknown keys are ignored.

use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use chewing::editor::zhuyin_layout::KeyboardLayoutCompat;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::keybind::KeybindValue;
use crate::keymap::SimulatedKeyboard;

/// File name inside libchewing's user data directory.
pub const CONFIG_FILE_NAME: &str = "macos-config.toml";

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct ConfigFile {
    switch_lang_with_shift: bool,
    shift_key_sensitivity: u64,
    enable_fullwidth_toggle_key: bool,
    enable_caps_lock: bool,
    lock_chinese_on_caps_lock: bool,
    show_notification: bool,
    enable_auto_learn: bool,
    auto_learn_user_history: bool,
    auto_learn_new_words: bool,
    esc_clean_all_buf: bool,
    full_shape_symbols: bool,
    upper_case_with_shift: bool,
    add_phrase_forward: bool,
    phrase_choice_rearward: bool,
    easy_symbols_with_shift: bool,
    easy_symbols_with_shift_ctrl: bool,
    auto_wrap_cursor: bool,
    cursor_cand_list: bool,
    sort_candidates_by_frequency: bool,
    show_cand_with_space_key: bool,
    cycle_first_n_selects: usize,
    advance_after_selection: bool,
    default_full_space: bool,
    default_english: bool,
    output_simp_chinese: bool,
    sel_key_type: usize,
    conv_engine: u8,
    cand_per_row: usize,
    cand_per_page: usize,
    font_size: f64,
    font_family: String,
    keyboard_layout: u8,
    simulate_english_layout: i32,
    keybind: Vec<KeybindEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct KeybindEntry {
    key: String,
    action: String,
    #[serde(default)]
    param: String,
}

impl Default for ConfigFile {
    fn default() -> Self {
        ConfigFile::from(&Config::default())
    }
}

impl From<&Config> for ConfigFile {
    fn from(c: &Config) -> Self {
        ConfigFile {
            switch_lang_with_shift: c.switch_lang_with_shift,
            shift_key_sensitivity: c.shift_key_sensitivity.as_millis() as u64,
            enable_fullwidth_toggle_key: c.enable_fullwidth_toggle_key,
            enable_caps_lock: c.enable_caps_lock,
            lock_chinese_on_caps_lock: c.lock_chinese_on_caps_lock,
            show_notification: c.show_notification,
            enable_auto_learn: c.enable_auto_learn,
            auto_learn_user_history: c.auto_learn_user_history,
            auto_learn_new_words: c.auto_learn_new_words,
            esc_clean_all_buf: c.esc_clean_all_buf,
            full_shape_symbols: c.full_shape_symbols,
            upper_case_with_shift: c.upper_case_with_shift,
            add_phrase_forward: c.add_phrase_forward,
            phrase_choice_rearward: c.phrase_choice_rearward,
            easy_symbols_with_shift: c.easy_symbols_with_shift,
            easy_symbols_with_shift_ctrl: c.easy_symbols_with_shift_ctrl,
            auto_wrap_cursor: c.auto_wrap_cursor,
            cursor_cand_list: c.cursor_cand_list,
            sort_candidates_by_frequency: c.sort_candidates_by_frequency,
            show_cand_with_space_key: c.show_cand_with_space_key,
            cycle_first_n_selects: c.cycle_first_n_selects,
            advance_after_selection: c.advance_after_selection,
            default_full_space: c.default_full_space,
            default_english: c.default_english,
            output_simp_chinese: c.output_simp_chinese,
            sel_key_type: c.sel_key_type,
            conv_engine: c.conv_engine,
            cand_per_row: c.cand_per_row,
            cand_per_page: c.cand_per_page,
            font_size: c.font_size,
            font_family: c.font_family.clone(),
            keyboard_layout: c.keyboard_layout as u8,
            simulate_english_layout: c.simulate_english_layout.into(),
            keybind: c
                .keybind
                .iter()
                .map(|k| KeybindEntry {
                    key: k.key.clone(),
                    action: k.action.clone(),
                    param: k.param.clone(),
                })
                .collect(),
        }
    }
}

impl From<ConfigFile> for Config {
    fn from(f: ConfigFile) -> Self {
        let defaults = Config::default();
        Config {
            switch_lang_with_shift: f.switch_lang_with_shift,
            shift_key_sensitivity: Duration::from_millis(f.shift_key_sensitivity),
            enable_fullwidth_toggle_key: f.enable_fullwidth_toggle_key,
            enable_caps_lock: f.enable_caps_lock,
            lock_chinese_on_caps_lock: f.lock_chinese_on_caps_lock,
            show_notification: f.show_notification,
            enable_auto_learn: f.enable_auto_learn,
            auto_learn_user_history: f.auto_learn_user_history,
            auto_learn_new_words: f.auto_learn_new_words,
            esc_clean_all_buf: f.esc_clean_all_buf,
            full_shape_symbols: f.full_shape_symbols,
            upper_case_with_shift: f.upper_case_with_shift,
            add_phrase_forward: f.add_phrase_forward,
            phrase_choice_rearward: f.phrase_choice_rearward,
            easy_symbols_with_shift: f.easy_symbols_with_shift,
            easy_symbols_with_shift_ctrl: f.easy_symbols_with_shift_ctrl,
            auto_wrap_cursor: f.auto_wrap_cursor,
            cursor_cand_list: f.cursor_cand_list,
            sort_candidates_by_frequency: f.sort_candidates_by_frequency,
            show_cand_with_space_key: f.show_cand_with_space_key,
            cycle_first_n_selects: f.cycle_first_n_selects,
            advance_after_selection: f.advance_after_selection,
            default_full_space: f.default_full_space,
            default_english: f.default_english,
            output_simp_chinese: f.output_simp_chinese,
            // Out-of-range values fall back to the defaults.
            sel_key_type: if f.sel_key_type < crate::config::SEL_KEYS.len() {
                f.sel_key_type
            } else {
                defaults.sel_key_type
            },
            conv_engine: f.conv_engine.min(2),
            cand_per_row: f.cand_per_row.clamp(1, 10),
            cand_per_page: f.cand_per_page.clamp(1, 10),
            font_size: if (8.0..=72.0).contains(&f.font_size) {
                f.font_size
            } else {
                defaults.font_size
            },
            font_family: f.font_family,
            keyboard_layout: KeyboardLayoutCompat::try_from(f.keyboard_layout)
                .unwrap_or(KeyboardLayoutCompat::Default),
            simulate_english_layout: SimulatedKeyboard::from(f.simulate_english_layout),
            keybind: f
                .keybind
                .into_iter()
                .map(|k| KeybindValue {
                    key: k.key,
                    action: k.action,
                    param: k.param,
                })
                .collect(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "{e}"),
            ConfigError::Parse(e) => write!(f, "invalid settings: {e}"),
            ConfigError::Serialize(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Parse settings from TOML text.
    pub fn from_toml(text: &str) -> Result<Config, ConfigError> {
        toml::from_str::<ConfigFile>(text)
            .map(Config::from)
            .map_err(ConfigError::Parse)
    }

    pub fn to_toml(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(&ConfigFile::from(self)).map_err(ConfigError::Serialize)
    }

    /// Load settings from `path`. A missing file means all defaults.
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        match fs::read_to_string(path) {
            Ok(text) => Config::from_toml(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(ConfigError::Io(e)),
        }
    }

    /// Save settings to `path`, replacing it atomically.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = format!(
            "# 新酷音 macOS 版設定。欄位與 windows-chewing-tsf 相同。\n\n{}",
            self.to_toml()?
        );
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text).map_err(ConfigError::Io)?;
        fs::rename(&tmp, path).map_err(ConfigError::Io)
    }
}

/// Reloads the settings file when it changes, like windows-chewing-tsf's
/// `reload_if_needed`.
#[derive(Debug)]
pub struct ConfigWatcher {
    path: std::path::PathBuf,
    modified: Option<SystemTime>,
}

impl ConfigWatcher {
    /// Starts out as "changed", so the first `reload_if_changed` loads.
    pub fn new(path: std::path::PathBuf) -> ConfigWatcher {
        ConfigWatcher {
            path,
            modified: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the new settings if the file changed since the last call.
    pub fn reload_if_changed(&mut self) -> Option<Result<Config, ConfigError>> {
        let modified = fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok()
            .or(Some(SystemTime::UNIX_EPOCH));
        if modified == self.modified {
            return None;
        }
        self.modified = modified;
        Some(Config::load(&self.path))
    }

    /// Remember the file as current after we wrote it ourselves.
    pub fn mark_current(&mut self) {
        self.modified = fs::metadata(&self.path).and_then(|m| m.modified()).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_all_defaults() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.to_toml().unwrap(), Config::default().to_toml().unwrap());
    }

    #[test]
    fn round_trip_keeps_every_setting() {
        let cfg = Config {
            switch_lang_with_shift: false,
            shift_key_sensitivity: Duration::from_millis(350),
            keyboard_layout: KeyboardLayoutCompat::Hsu,
            simulate_english_layout: SimulatedKeyboard::Dvorak,
            cand_per_row: 1,
            font_family: "PingFang TC".into(),
            keybind: vec![KeybindValue::new("Ctrl+Shift+A", "text", "!?")],
            ..Config::default()
        };
        let text = cfg.to_toml().unwrap();
        let back = Config::from_toml(&text).unwrap();
        assert_eq!(back.to_toml().unwrap(), text);
        assert!(!back.switch_lang_with_shift);
        assert_eq!(back.shift_key_sensitivity, Duration::from_millis(350));
        assert_eq!(back.keyboard_layout, KeyboardLayoutCompat::Hsu);
        assert_eq!(back.simulate_english_layout, SimulatedKeyboard::Dvorak);
    }

    #[test]
    fn partial_file_uses_windows_names_and_defaults() {
        let cfg = Config::from_toml(
            "keyboard_layout = 1\nshift_key_sensitivity = 300\ncand_per_row = 1\n",
        )
        .unwrap();
        assert_eq!(cfg.keyboard_layout, KeyboardLayoutCompat::Hsu);
        assert_eq!(cfg.shift_key_sensitivity, Duration::from_millis(300));
        assert_eq!(cfg.cand_per_row, 1);
        // Untouched settings keep their defaults.
        assert!(cfg.cursor_cand_list);
        assert_eq!(cfg.keybind.len(), 2);
    }

    #[test]
    fn out_of_range_values_fall_back() {
        let cfg = Config::from_toml(
            "keyboard_layout = 200\nsel_key_type = 99\ncand_per_page = 0\nfont_size = 1000.0\n",
        )
        .unwrap();
        assert_eq!(cfg.keyboard_layout, KeyboardLayoutCompat::Default);
        assert_eq!(cfg.sel_key_type, 0);
        assert_eq!(cfg.cand_per_page, 1);
        assert_eq!(cfg.font_size, 16.0);
    }

    #[test]
    fn invalid_toml_is_an_error() {
        assert!(matches!(
            Config::from_toml("keyboard_layout = \"oops\""),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn save_load_and_watch() {
        let dir = std::env::temp_dir().join(format!("chewing_imk_config-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CONFIG_FILE_NAME);

        let mut watcher = ConfigWatcher::new(path.clone());
        // No file yet: loads defaults once, then reports no change.
        assert!(watcher.reload_if_changed().unwrap().is_ok());
        assert!(watcher.reload_if_changed().is_none());

        let cfg = Config {
            cand_per_row: 2,
            ..Config::default()
        };
        cfg.save(&path).unwrap();
        let loaded = watcher.reload_if_changed().unwrap().unwrap();
        assert_eq!(loaded.cand_per_row, 2);
        assert!(watcher.reload_if_changed().is_none());

        // Our own writes can be marked as already applied.
        cfg.save(&path).unwrap();
        watcher.mark_current();
        assert!(watcher.reload_if_changed().is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
