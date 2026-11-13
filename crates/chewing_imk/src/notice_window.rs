// SPDX-License-Identifier: GPL-3.0-or-later

//! A short message shown near the insertion point for half a second, e.g.
//! "英數模式" after a Shift tap. Matches windows-chewing-tsf's notification.

use std::cell::RefCell;

use objc2::MainThreadMarker;
use objc2_foundation::NSRect;

use crate::panel::TextPanel;

const FONT_SIZE: f64 = 15.0;
/// windows-chewing-tsf shows notifications for 500 ms.
const SHOW_SECONDS: f64 = 0.5;

thread_local! {
    static PANEL: RefCell<Option<TextPanel>> = const { RefCell::new(None) };
}

pub fn show(mtm: MainThreadMarker, message: &str, line: Option<NSRect>) {
    PANEL.with_borrow_mut(|slot| {
        let panel = slot.get_or_insert_with(|| TextPanel::new(mtm));
        panel.show(mtm, message, FONT_SIZE, line);
        panel.hide_after(SHOW_SECONDS);
    });
}
