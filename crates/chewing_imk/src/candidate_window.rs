// SPDX-License-Identifier: GPL-3.0-or-later

//! A minimal candidate window: one candidate per line with its selection
//! key, plus the page number. Styling and settings come in Phase 3.

use std::cell::RefCell;

use chewing_imk_core::CandidateList;
use objc2::MainThreadMarker;
use objc2_app_kit::NSColor;
use objc2_foundation::{NSMutableAttributedString, NSRect};

use crate::panel::{TextPanel, append, attributes};

const FONT_SIZE: f64 = 18.0;

thread_local! {
    static PANEL: RefCell<Option<TextPanel>> = const { RefCell::new(None) };
}

/// Show `list` next to `line`, the client's text line in screen coordinates.
/// Without a line rectangle the window keeps its previous position.
pub fn show(mtm: MainThreadMarker, list: &CandidateList, line: Option<NSRect>) {
    PANEL.with_borrow_mut(|slot| {
        let panel = slot.get_or_insert_with(|| TextPanel::new(mtm));
        panel.set_text(&render(list));
        if let Some(line) = line {
            panel.move_near(mtm, line);
        }
        panel.show();
    });
}

pub fn hide() {
    PANEL.with_borrow(|slot| {
        if let Some(panel) = slot {
            panel.hide();
        }
    });
}

fn render(list: &CandidateList) -> objc2::rc::Retained<NSMutableAttributedString> {
    let text = NSMutableAttributedString::new();
    let key_attrs = attributes(&NSColor::systemBlueColor(), FONT_SIZE);
    let item_attrs = attributes(&NSColor::labelColor(), FONT_SIZE);
    let page_attrs = attributes(&NSColor::secondaryLabelColor(), FONT_SIZE * 0.7);

    for (i, (key, item)) in list.sel_keys.iter().zip(&list.items).enumerate() {
        if i > 0 {
            append(&text, "\n", &item_attrs);
        }
        append(&text, &format!("{key} "), &key_attrs);
        append(&text, item, &item_attrs);
    }
    if list.total_pages > 1 {
        append(
            &text,
            &format!("\n{}/{}", list.page, list.total_pages),
            &page_attrs,
        );
    }
    text
}
