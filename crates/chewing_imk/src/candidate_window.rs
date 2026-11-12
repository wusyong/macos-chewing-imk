// SPDX-License-Identifier: GPL-3.0-or-later

//! The candidate window: candidates in a grid of `per_row` columns, each with
//! its selection key, the highlighted one inverted, and the page number.
//! Uses system colors so it follows light and dark mode.

use std::cell::RefCell;

use chewing_imk_core::CandidateList;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSBackgroundColorAttributeName, NSColor, NSFont,
    NSFontAttributeName, NSForegroundColorAttributeName, NSMutableParagraphStyle, NSParagraphStyle,
    NSParagraphStyleAttributeName, NSTextAlignment, NSTextTab,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSMutableAttributedString,
    NSRect, NSString,
};

use crate::panel::TextPanel;

/// Space between columns, in points.
const COLUMN_GAP: f64 = 6.0;

/// Display settings from the configuration.
pub struct Style<'a> {
    pub font_size: f64,
    /// Empty means the system font.
    pub font_family: &'a str,
}

thread_local! {
    static PANEL: RefCell<Option<TextPanel>> = const { RefCell::new(None) };
}

/// Show `list` next to `line`, the client's text line in screen coordinates.
/// Without a line rectangle the window keeps its previous position.
pub fn show(mtm: MainThreadMarker, list: &CandidateList, style: &Style, line: Option<NSRect>) {
    PANEL.with_borrow_mut(|slot| {
        let panel = slot.get_or_insert_with(|| TextPanel::new(mtm));
        panel.set_text(&render(list, style));
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

fn font(style: &Style) -> Retained<NSFont> {
    let size = style.font_size;
    if style.font_family.is_empty() {
        return NSFont::systemFontOfSize(size);
    }
    NSFont::fontWithName_size(&NSString::from_str(style.font_family), size)
        .unwrap_or_else(|| NSFont::systemFontOfSize(size))
}

struct Attrs<'a> {
    font: &'a NSFont,
    color: &'a NSColor,
    background: Option<&'a NSColor>,
    paragraph: &'a NSParagraphStyle,
}

impl Attrs<'_> {
    fn dict(&self) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
        // SAFETY: the attribute name constants are valid for the whole process.
        let (font_key, color_key, bg_key, para_key) = unsafe {
            (
                NSFontAttributeName,
                NSForegroundColorAttributeName,
                NSBackgroundColorAttributeName,
                NSParagraphStyleAttributeName,
            )
        };
        let mut keys: Vec<&NSAttributedStringKey> = vec![font_key, color_key, para_key];
        let mut values: Vec<&AnyObject> = vec![self.font, self.color, self.paragraph];
        if let Some(bg) = self.background {
            keys.push(bg_key);
            values.push(bg);
        }
        NSDictionary::from_slices(&keys, &values)
    }
}

fn piece(s: &str, attrs: &Attrs) -> Retained<NSAttributedString> {
    // SAFETY: the dictionary maps attribute names to valid values.
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(s),
            Some(&attrs.dict()),
        )
    }
}

fn render(list: &CandidateList, style: &Style) -> Retained<NSMutableAttributedString> {
    let font = font(style);
    let small = NSFont::systemFontOfSize(style.font_size * 0.75);
    let key_color = NSColor::systemBlueColor();
    let text_color = NSColor::labelColor();
    let dim_color = NSColor::secondaryLabelColor();
    let selected_text = NSColor::alternateSelectedControlTextColor();
    let selected_bg = NSColor::selectedContentBackgroundColor();
    let per_row = list.per_row.max(1);

    // Each cell is " key item ", padded so the highlight has some margin.
    let cells: Vec<(String, String)> = list
        .sel_keys
        .iter()
        .zip(&list.items)
        .map(|(key, item)| (format!(" {key}"), format!(" {item} ")))
        .collect();

    // Column width: the widest cell, measured with the real font.
    let plain = NSParagraphStyle::defaultParagraphStyle();
    let measure_attrs = Attrs {
        font: &font,
        color: &text_color,
        background: None,
        paragraph: &plain,
    };
    let column = cells
        .iter()
        .map(|(k, i)| piece(&format!("{k}{i}"), &measure_attrs).size().width)
        .fold(0.0_f64, f64::max)
        + COLUMN_GAP;

    let paragraph = NSMutableParagraphStyle::new();
    let tabs: Vec<Retained<NSTextTab>> = (1..per_row)
        .map(|n| {
            // SAFETY: an empty options dictionary is valid.
            unsafe {
                NSTextTab::initWithTextAlignment_location_options(
                    NSTextTab::alloc(),
                    NSTextAlignment::Left,
                    column * n as f64,
                    &NSDictionary::new(),
                )
            }
        })
        .collect();
    paragraph.setTabStops(Some(&NSArray::from_retained_slice(&tabs)));
    paragraph.setLineSpacing(2.0);

    let text = NSMutableAttributedString::new();
    let separator = Attrs {
        font: &font,
        color: &text_color,
        background: None,
        paragraph: &paragraph,
    };
    for (i, (key, item)) in cells.iter().enumerate() {
        if i > 0 {
            let sep = if i % per_row == 0 { "\n" } else { "\t" };
            text.appendAttributedString(&piece(sep, &separator));
        }
        let selected = list.highlighted == Some(i);
        let (key_fg, item_fg, bg) = if selected {
            (&*selected_text, &*selected_text, Some(&*selected_bg))
        } else {
            (&*key_color, &*text_color, None)
        };
        let key_attrs = Attrs {
            font: &font,
            color: key_fg,
            background: bg,
            paragraph: &paragraph,
        };
        let item_attrs = Attrs {
            color: item_fg,
            ..key_attrs
        };
        text.appendAttributedString(&piece(key, &key_attrs));
        text.appendAttributedString(&piece(item, &item_attrs));
    }
    if list.total_pages > 1 {
        let page_attrs = Attrs {
            font: &small,
            color: &dim_color,
            background: None,
            paragraph: &paragraph,
        };
        let page = format!("\n {}/{}", list.page, list.total_pages);
        text.appendAttributedString(&piece(&page, &page_attrs));
    }
    text
}
