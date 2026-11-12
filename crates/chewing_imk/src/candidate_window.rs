// SPDX-License-Identifier: GPL-3.0-or-later

//! A minimal candidate window: a borderless, non-activating panel showing one
//! candidate per line with its selection key, plus the page number. Styling
//! and settings come in Phase 3.

use std::cell::RefCell;

use chewing_imk_core::CandidateList;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSPanel, NSPopUpMenuWindowLevel, NSScreen, NSTextField, NSView, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSAttributedString, NSAttributedStringKey, NSDictionary, NSMutableAttributedString, NSPoint,
    NSRect, NSSize, NSString,
};

const FONT_SIZE: f64 = 18.0;
const PADDING_X: f64 = 10.0;
const PADDING_Y: f64 = 6.0;
/// Gap between the text line and the window.
const GAP: f64 = 4.0;

struct CandidateWindow {
    panel: Retained<NSPanel>,
    content: Retained<NSView>,
    label: Retained<NSTextField>,
}

thread_local! {
    static WINDOW: RefCell<Option<CandidateWindow>> = const { RefCell::new(None) };
}

/// Show `list` next to `line`, the client's text line in screen coordinates.
/// Without a line rectangle the window keeps its previous position.
pub fn show(mtm: MainThreadMarker, list: &CandidateList, line: Option<NSRect>) {
    WINDOW.with_borrow_mut(|slot| {
        let window = slot.get_or_insert_with(|| CandidateWindow::new(mtm));
        window.update(list);
        if let Some(line) = line {
            window.move_near(mtm, line);
        }
        window.panel.orderFrontRegardless();
    });
}

pub fn hide() {
    WINDOW.with_borrow(|slot| {
        if let Some(window) = slot {
            window.panel.orderOut(None);
        }
    });
}

impl CandidateWindow {
    fn new(mtm: MainThreadMarker) -> CandidateWindow {
        let frame = NSRect::new(NSPoint::ZERO, NSSize::new(100.0, 40.0));
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            frame,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        // SAFETY: we keep our own strong reference and never call close.
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setLevel(NSPopUpMenuWindowLevel);
        panel.setHidesOnDeactivate(false);
        panel.setIgnoresMouseEvents(true);
        panel.setHasShadow(true);
        panel.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );

        let content = NSView::initWithFrame(NSView::alloc(mtm), frame);
        let label = NSTextField::labelWithAttributedString(&NSAttributedString::new(), mtm);
        content.addSubview(&label);
        panel.setContentView(Some(&content));

        CandidateWindow {
            panel,
            content,
            label,
        }
    }

    fn update(&self, list: &CandidateList) {
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

        self.label.setAttributedStringValue(&text);
        let size = self.label.fittingSize();
        self.label
            .setFrameOrigin(NSPoint::new(PADDING_X, PADDING_Y));
        self.label.setFrameSize(size);
        let content_size = NSSize::new(size.width + 2.0 * PADDING_X, size.height + 2.0 * PADDING_Y);
        self.content.setFrameSize(content_size);
        self.panel.setContentSize(content_size);
    }

    /// Place the window below `line`, or above it if there is no room below,
    /// keeping it on the screen that contains the line.
    fn move_near(&self, mtm: MainThreadMarker, line: NSRect) {
        let size = self.panel.frame().size;
        let mut x = line.origin.x;
        let mut top = line.origin.y - GAP;

        let screens = NSScreen::screens(mtm);
        let visible = screens
            .iter()
            .map(|s| s.visibleFrame())
            .find(|f| contains(f, line.origin))
            .or_else(|| screens.firstObject().map(|s| s.visibleFrame()));
        if let Some(v) = visible {
            if top - size.height < v.origin.y {
                // Not enough room below the line: go above it.
                top = line.origin.y + line.size.height + GAP + size.height;
            }
            let max_x = v.origin.x + v.size.width - size.width;
            x = x.min(max_x).max(v.origin.x);
            top = top.min(v.origin.y + v.size.height);
        }
        self.panel.setFrameTopLeftPoint(NSPoint::new(x, top));
    }
}

fn contains(rect: &NSRect, p: NSPoint) -> bool {
    p.x >= rect.origin.x
        && p.x <= rect.origin.x + rect.size.width
        && p.y >= rect.origin.y
        && p.y <= rect.origin.y + rect.size.height
}

fn attributes(
    color: &NSColor,
    font_size: f64,
) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    let font = NSFont::systemFontOfSize(font_size);
    // SAFETY: the attribute name constants are valid for the whole process.
    let keys: [&NSAttributedStringKey; 2] =
        unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let font: &AnyObject = &font;
    let color: &AnyObject = color;
    NSDictionary::from_slices(&keys, &[font, color])
}

fn append(
    text: &NSMutableAttributedString,
    s: &str,
    attrs: &NSDictionary<NSAttributedStringKey, AnyObject>,
) {
    // SAFETY: attrs maps attribute names to valid attribute values.
    let part = unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(s),
            Some(attrs),
        )
    };
    text.appendAttributedString(&part);
}
