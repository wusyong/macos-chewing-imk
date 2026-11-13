// SPDX-License-Identifier: GPL-3.0-or-later

//! Borderless, non-activating floating panels placed next to the client's
//! text line, drawn like the system input method windows: a translucent
//! material with continuous rounded corners, a hairline border and a shadow.
//! Showing and hiding fade; while shown, changes of size and position
//! animate. Used by the candidate window and the notice window.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSBezierPath, NSBox, NSBoxType, NSColor, NSFont, NSImage,
    NSImageResizingMode, NSPanel, NSPopUpMenuWindowLevel, NSScreen, NSTextField, NSTitlePosition,
    NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{CAMediaTimingFunction, kCAMediaTimingFunctionEaseOut};

/// Length of every animation, in seconds.
const ANIMATION_SECONDS: f64 = 0.15;
/// Gap between the text line and the panel.
const GAP: f64 = 4.0;

define_class!(
    /// A view with its origin at the top left, so its subviews stay in place
    /// while the panel grows or shrinks.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ChewingFlippedView"]
    pub struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<FlippedView> {
        // SAFETY: initWithFrame: is NSView's designated initializer.
        unsafe { msg_send![FlippedView::alloc(mtm), initWithFrame: frame] }
    }
}

/// Run `changes` as one animation; inside it, use `view.animator()` for the
/// properties that should animate. Without `animate`, or when the user asked
/// for reduced motion, the changes apply at once.
pub fn animate(animate: bool, changes: impl FnOnce()) {
    let reduce_motion = NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
    NSAnimationContext::beginGrouping();
    let context = NSAnimationContext::currentContext();
    context.setDuration(if animate && !reduce_motion {
        ANIMATION_SECONDS
    } else {
        0.0
    });
    context.setAllowsImplicitAnimation(true);
    // SAFETY: the timing function name constant is valid for the whole process.
    let ease_out =
        CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionEaseOut });
    context.setTimingFunction(Some(&ease_out));
    changes();
    NSAnimationContext::endGrouping();
}

/// A text label with the given font and color.
pub fn label(mtm: MainThreadMarker) -> Retained<NSTextField> {
    NSTextField::labelWithString(&NSString::new(), mtm)
}

pub fn set_label(label: &NSTextField, text: &str, font: &NSFont, color: &NSColor) {
    label.setStringValue(&NSString::from_str(text));
    label.setFont(Some(font));
    label.setTextColor(Some(color));
}

/// A stretchable rounded rectangle, the shape of the panel's material. A
/// mask image, not a layer's corner radius: only the mask also shapes the
/// blur behind the window and the window's shadow.
fn mask_image(radius: f64) -> Retained<NSImage> {
    let side = 2.0 * radius + 1.0;
    let draw = block2::RcBlock::new(move |rect: NSRect| -> Bool {
        NSColor::blackColor().set();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius).fill();
        Bool::YES
    });
    let image =
        NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(side, side), false, &draw);
    image.setCapInsets(NSEdgeInsets {
        top: radius,
        left: radius,
        bottom: radius,
        right: radius,
    });
    // NSImageResizingModeStretch is 1 on arm64 (AppKit's NSImage.h);
    // objc2-app-kit 0.3.2 swaps Stretch and Tile there, so not ::Stretch.
    image.setResizingMode(NSImageResizingMode(1));
    image
}

/// A rounded rectangle filled with `color`, for highlights and separators.
pub fn rounded_box(mtm: MainThreadMarker, color: &NSColor) -> Retained<NSBox> {
    let b = NSBox::new(mtm);
    b.setBoxType(NSBoxType::Custom);
    b.setTitlePosition(NSTitlePosition::NoTitle);
    b.setContentViewMargins(NSSize::ZERO);
    b.setBorderWidth(0.0);
    b.setBorderColor(&NSColor::clearColor());
    b.setFillColor(color);
    b
}

pub struct FloatingPanel {
    panel: Retained<NSPanel>,
    effect: Retained<NSVisualEffectView>,
    border: Retained<NSBox>,
    content: Retained<FlippedView>,
    radius: Cell<f64>,
    /// Shown, or fading in; false once hiding started.
    shown: Cell<bool>,
}

impl FloatingPanel {
    pub fn new(mtm: MainThreadMarker) -> FloatingPanel {
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
        // Transparent outside the rounded material, so the shadow follows it.
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(true);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );

        let fill = NSAutoresizingMaskOptions::ViewWidthSizable
            | NSAutoresizingMaskOptions::ViewHeightSizable;
        let effect = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
        effect.setMaterial(NSVisualEffectMaterial::Popover);
        effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        effect.setState(NSVisualEffectState::Active);
        // Layer-backed subviews animate smoothly.
        effect.setWantsLayer(true);

        let content = FlippedView::new(mtm, frame);
        content.setAutoresizingMask(fill);
        effect.addSubview(&content);

        // The hairline border, drawn above the content.
        let border = rounded_box(mtm, &NSColor::clearColor());
        border.setBorderWidth(1.0);
        border.setBorderColor(&NSColor::separatorColor());
        border.setFrame(frame);
        border.setAutoresizingMask(fill);
        effect.addSubview(&border);

        panel.setContentView(Some(&effect));
        FloatingPanel {
            panel,
            effect,
            border,
            content,
            radius: Cell::new(-1.0),
            shown: Cell::new(false),
        }
    }

    /// Where subviews go; its origin is the top left corner.
    pub fn content(&self) -> &NSView {
        &self.content
    }

    pub fn is_shown(&self) -> bool {
        self.shown.get()
    }

    /// Resize to `size` with rounded corners of `radius`, below (or above)
    /// `line` if given, else keeping the top left corner. Animates while
    /// shown; call it inside [`animate`] together with the content changes.
    pub fn set_frame(
        &self,
        mtm: MainThreadMarker,
        size: NSSize,
        radius: f64,
        line: Option<NSRect>,
    ) {
        let current = self.panel.frame();
        let top_left = match line {
            Some(line) => top_left_near(mtm, line, size),
            None => NSPoint::new(current.origin.x, current.origin.y + current.size.height),
        };
        let frame = NSRect::new(NSPoint::new(top_left.x, top_left.y - size.height), size);
        if self.radius.replace(radius) != radius {
            self.effect.setMaskImage(Some(&mask_image(radius)));
            self.border.setCornerRadius(radius);
        }
        if self.shown.get() {
            self.panel.animator().setFrame_display(frame, true);
        } else {
            self.panel.setFrame_display(frame, false);
        }
        self.panel.invalidateShadow();
    }

    /// Fade in, unless already shown.
    pub fn show(&self) {
        self.cancel_order_out();
        if self.shown.replace(true) && self.panel.isVisible() {
            return;
        }
        if !self.panel.isVisible() {
            self.panel.setAlphaValue(0.0);
        }
        self.panel.orderFrontRegardless();
        animate(true, || self.panel.animator().setAlphaValue(1.0));
    }

    /// Fade out, then remove the panel from the screen.
    pub fn hide(&self) {
        if !self.shown.replace(false) {
            return;
        }
        animate(true, || self.panel.animator().setAlphaValue(0.0));
        self.order_out_after(ANIMATION_SECONDS);
    }

    /// Hide the panel after `seconds` (without fading), unless shown or
    /// hidden again first.
    pub fn hide_after(&self, seconds: f64) {
        self.shown.set(false);
        self.order_out_after(seconds);
    }

    fn order_out_after(&self, seconds: f64) {
        self.cancel_order_out();
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

    fn cancel_order_out(&self) {
        let nil: Option<&AnyObject> = None;
        // SAFETY: matches the request made in order_out_after.
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

/// The top left corner for a panel of `size` below `line`, or above it if
/// there is no room below, kept on the screen that contains the line.
fn top_left_near(mtm: MainThreadMarker, line: NSRect, size: NSSize) -> NSPoint {
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
    NSPoint::new(x, top)
}

fn contains(rect: &NSRect, p: NSPoint) -> bool {
    p.x >= rect.origin.x
        && p.x <= rect.origin.x + rect.size.width
        && p.y >= rect.origin.y
        && p.y <= rect.origin.y + rect.size.height
}

/// A capsule showing one line of text: the notice window.
pub struct TextPanel {
    base: FloatingPanel,
    label: Retained<NSTextField>,
}

impl TextPanel {
    const PADDING_X: f64 = 14.0;
    const PADDING_Y: f64 = 6.0;

    pub fn new(mtm: MainThreadMarker) -> TextPanel {
        let base = FloatingPanel::new(mtm);
        let label = label(mtm);
        base.content().addSubview(&label);
        TextPanel { base, label }
    }

    pub fn show(&self, mtm: MainThreadMarker, text: &str, font_size: f64, line: Option<NSRect>) {
        let font = NSFont::systemFontOfSize(font_size);
        set_label(&self.label, text, &font, &NSColor::labelColor());
        let size = self.label.fittingSize();
        self.label.setFrame(NSRect::new(
            NSPoint::new(Self::PADDING_X, Self::PADDING_Y),
            size,
        ));
        let panel_size = NSSize::new(
            size.width + 2.0 * Self::PADDING_X,
            size.height + 2.0 * Self::PADDING_Y,
        );
        animate(self.base.is_shown(), || {
            self.base
                .set_frame(mtm, panel_size, panel_size.height / 2.0, line)
        });
        self.base.show();
    }

    pub fn hide_after(&self, seconds: f64) {
        self.base.hide_after(seconds);
    }
}
