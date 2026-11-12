// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform-independent state machine for the macOS Chewing input method.
//!
//! This crate must not depend on objc2 or any Apple framework, so that all of
//! the key handling rules can be unit tested with a plain `cargo test`.

pub mod config;
pub mod config_file;
mod engine;
pub mod keybind;
pub mod keymap;

pub use engine::{
    Action, CandidateList, ChewingCore, Composition, ImportReport, KeyResponse, LangMode,
};

/// libchewing's default user data directory, shared with `chewing-cli`:
/// `~/Library/Application Support/im.chewing.Chewing` (or `$CHEWING_USER_PATH`).
pub fn default_user_dir() -> Option<std::path::PathBuf> {
    chewing::path::data_dir()
}
