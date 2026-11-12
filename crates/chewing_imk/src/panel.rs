// SPDX-License-Identifier: GPL-3.0-or-later

//! A borderless, non-activating floating panel showing one attributed label,
//! placed next to the client's text line. Used by the candidate window and
//! the notice window.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{AllocAnyThread, ClassType, MainThreadMarker, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSPanel, NSPopUpMenuWindowLevel, NSScreen, NSTextField, NSView, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSAttributedString, NSAttributedStringKey, NSDictionary, NSMutableAttributedString, NSPoint,
    NSRect, NSSize, NSString,
};

const PADDING_X: f64 = 10.0;
const PADDING_Y: f64 = 6.0;
/// Gap between the text line and the panel.
const GAP: f64 = 4.0;

pub struct TextPanel {
    panel: Retained<NSPanel>,
    content: Retained<NSView>,
    label: Retained<NSTextField>,
}

impl TextPanel {
    pub fn new(mtm: MainThreadMarker) -> TextPanel {
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

        TextPanel {
            panel,
            content,
            label,
        }
    }

    /// Replace the text and resize the panel to fit it.
    pub fn set_text(&self, text: &NSAttributedString) {
        self.label.setAttributedStringValue(text);
        let size = self.label.fittingSize();
        self.label
            .setFrameOrigin(NSPoint::new(PADDING_X, PADDING_Y));
        self.label.setFrameSize(size);
        let content_size = NSSize::new(size.width + 2.0 * PADDING_X, size.height + 2.0 * PADDING_Y);
        self.content.setFrameSize(content_size);
        self.panel.setContentSize(content_size);
    }

    /// Place the panel below `line`, or above it if there is no room below,
    /// keeping it on the screen that contains the line.
    pub fn move_near(&self, mtm: MainThreadMarker, line: NSRect) {
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

    pub fn show(&self) {
        self.cancel_hide_after();
        self.panel.orderFrontRegardless();
    }

    pub fn hide(&self) {
        self.cancel_hide_after();
        self.panel.orderOut(None);
    }

    /// Hide the panel after `seconds`, unless shown or hidden again first.
    pub fn hide_after(&self, seconds: f64) {
        self.cancel_hide_after();
        let nil: Option<&AnyObject> = None;
        // SAFETY: orderOut: takes one object argument (nil is fine), and the
        // run loop keeps the panel alive until the call.
        let _: () = unsafe {
            msg_send![
                &*self.panel,
                performSelector: sel!(orderOut:),
                withObject: nil,
                afterDelay: seconds
            ]
        };
    }

    fn cancel_hide_after(&self) {
        let nil: Option<&AnyObject> = None;
        // SAFETY: matches the request made in hide_after.
        let _: () = unsafe {
            msg_send![
                NSObject::class(),
                cancelPreviousPerformRequestsWithTarget: &*self.panel,
                selector: sel!(orderOut:),
                object: nil
            ]
        };
    }
}

fn contains(rect: &NSRect, p: NSPoint) -> bool {
    p.x >= rect.origin.x
        && p.x <= rect.origin.x + rect.size.width
        && p.y >= rect.origin.y
        && p.y <= rect.origin.y + rect.size.height
}

pub fn attributes(
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

pub fn append(
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
