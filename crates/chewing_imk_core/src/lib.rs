// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform-independent state machine for the macOS Chewing input method.
//!
//! This crate must not depend on objc2 or any Apple framework, so that all of
//! the key handling rules can be unit tested with a plain `cargo test`.
//! It is filled in starting from Phase 1 (see ROADMAP.md).
