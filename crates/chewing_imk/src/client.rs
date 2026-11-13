// SPDX-License-Identifier: GPL-3.0-or-later

//! Messages sent to the text input client (the `sender` IMK passes us, which
//! conforms to the informal `IMKTextInput` protocol).

use chewing_imk_core::Composition;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, msg_send, sel};
use objc2_foundation::{
    NSAttributedString, NSAttributedStringKey, NSDictionary, NSInteger, NSNotFound, NSRange,
    NSRect, NSString, NSUInteger,
};
use objc2_input_method_kit::IMKInputController;

/// `kTSMHiliteConvertedText` from HIToolbox/TextServices.h: the underline
/// style for converted (but not yet committed) text.
const TSM_HILITE_CONVERTED_TEXT: NSInteger = 4;

/// "Replace the marked text, or insert at the selection if there is none."
fn not_found() -> NSRange {
    NSRange::new(NSNotFound as NSUInteger, 0)
}

/// Number of UTF-16 code units in the first `chars` chars of `s`.
pub fn utf16_offset(s: &str, chars: usize) -> usize {
    s.chars().take(chars).map(char::len_utf16).sum()
}

pub fn insert_text(client: &AnyObject, text: &str) {
    let text = NSString::from_str(text);
    // SAFETY: `insertText:replacementRange:` is part of IMKTextInput and
    // takes an NSString (or NSAttributedString) and an NSRange.
    let _: () = unsafe { msg_send![client, insertText: &*text, replacementRange: not_found()] };
}

/// Show `composition` as marked text. An empty composition clears it.
pub fn set_marked_text(
    controller: &IMKInputController,
    client: &AnyObject,
    composition: &Composition,
) {
    let len = composition.text.encode_utf16().count();
    let cursor = utf16_offset(&composition.text, composition.cursor);
    let text = NSString::from_str(&composition.text);

    // Ask IMK for the client's own underline style for converted text.
    // SAFETY: the range lies within the marked text.
    let attrs =
        unsafe { controller.markForStyle_atRange(TSM_HILITE_CONVERTED_TEXT, NSRange::new(0, len)) };
    // SAFETY: markForStyle:atRange: returns a dictionary of attributed string
    // attributes, keyed by NSAttributedStringKey.
    let attrs: Option<Retained<NSDictionary<NSAttributedStringKey, AnyObject>>> =
        attrs.map(|d| unsafe { Retained::cast_unchecked(d) });
    // SAFETY: attrs has the correct generic types (see above).
    let marked = unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &text,
            attrs.as_deref(),
        )
    };

    // SAFETY: part of IMKTextInput; selectionRange is relative to the marked
    // text and lies within it.
    let _: () = unsafe {
        msg_send![
            client,
            setMarkedText: &*marked,
            selectionRange: NSRange::new(cursor, 0),
            replacementRange: not_found()
        ]
    };
}

/// The screen rectangle (bottom-left origin) of the line at `index`, used to
/// place the candidate and notice windows. The index counts from the start
/// of the marked text, or of the selection when there is none.
pub fn line_rect(client: &AnyObject, index: usize) -> Option<NSRect> {
    // SAFETY: respondsToSelector: is safe to send to any object.
    let responds: bool = unsafe {
        msg_send![client, respondsToSelector: sel!(attributesForCharacterIndex:lineHeightRectangle:)]
    };
    if !responds {
        return None;
    }
    let mut rect = NSRect::ZERO;
    // SAFETY: part of IMKTextInput; `rect` is a valid out pointer.
    let _: Option<Retained<NSDictionary>> = unsafe {
        msg_send![
            client,
            attributesForCharacterIndex: index as NSUInteger,
            lineHeightRectangle: &mut rect as *mut NSRect
        ]
    };
    (rect.size.height > 0.0).then_some(rect)
}

/// Make the client deliver key events as if the given keyboard layout were
/// active, so physical keys map to the expected characters.
pub fn override_keyboard(client: &AnyObject, layout: &str) {
    let layout = NSString::from_str(layout);
    // SAFETY: respondsToSelector: is safe to send to any object.
    let responds: bool =
        unsafe { msg_send![client, respondsToSelector: sel!(overrideKeyboardWithKeyboardNamed:)] };
    if responds {
        // SAFETY: part of IMKTextInput, takes an NSString.
        let _: () = unsafe { msg_send![client, overrideKeyboardWithKeyboardNamed: &*layout] };
    }
}

#[cfg(test)]
mod tests {
    use super::utf16_offset;

    #[test]
    fn utf16_offset_counts_surrogate_pairs() {
        assert_eq!(utf16_offset("你好", 1), 1);
        // U+20000 (CJK Ext. B) needs two UTF-16 code units.
        assert_eq!(utf16_offset("\u{20000}好", 1), 2);
        assert_eq!(utf16_offset("\u{20000}好", 2), 3);
        assert_eq!(utf16_offset("ab", 5), 2);
    }
}
