// SPDX-License-Identifier: GPL-3.0-or-later

//! The candidate window, styled after the system input methods: each
//! candidate with its selection key in small grey type, the highlighted one
//! on an accent-colored capsule. One row makes a capsule-shaped window, more
//! rows (`per_row` smaller than the page) a rounded rectangle. The window
//! resizes, and the candidates and highlight slide, with a short animation.

use std::cell::RefCell;

use chewing_imk_core::CandidateList;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSAnimatablePropertyContainer, NSBox, NSColor, NSFont, NSTextField};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::panel::{FloatingPanel, animate, label, rounded_box, set_label};

/// Between the window edge and the cells; the highlight capsule then sits
/// concentric with a capsule window.
const EDGE: f64 = 4.0;
/// Inside a cell, around its text.
const CELL_PAD_X: f64 = 8.0;
const CELL_PAD_Y: f64 = 3.0;
/// Between a selection key and its candidate.
const KEY_GAP: f64 = 4.0;
/// Between cells.
const CELL_GAP: f64 = 2.0;
/// Corner radius of a window with more than one row.
const GRID_RADIUS: f64 = 12.0;
/// Selection keys and the page number, relative to the candidate font.
const SMALL_SCALE: f64 = 0.6;

/// Display settings from the configuration.
pub struct Style<'a> {
    pub font_size: f64,
    /// Empty means the system font.
    pub font_family: &'a str,
}

thread_local! {
    static WINDOW: RefCell<Option<CandidateWindow>> = const { RefCell::new(None) };
}

/// Show `list` next to `line`, the client's text line in screen coordinates.
/// Without a line rectangle the window keeps its previous position.
pub fn show(mtm: MainThreadMarker, list: &CandidateList, style: &Style, line: Option<NSRect>) {
    WINDOW.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| CandidateWindow::new(mtm))
            .show(mtm, list, style, line);
    });
}

pub fn hide() {
    WINDOW.with_borrow(|slot| {
        if let Some(window) = slot {
            window.base.hide();
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

struct Cell {
    key: Retained<NSTextField>,
    item: Retained<NSTextField>,
}

struct CandidateWindow {
    base: FloatingPanel,
    highlight: Retained<NSBox>,
    cells: Vec<Cell>,
    /// Separates the page number from the candidates in a single row.
    separator: Retained<NSBox>,
    page: Retained<NSTextField>,
}

/// Where everything goes, computed before any view is touched.
struct Layout {
    size: NSSize,
    radius: f64,
    /// Per cell: the cell, then its key and item label frames.
    cells: Vec<(NSRect, NSRect, NSRect)>,
    /// Separator and page number frames, if there is more than one page.
    page: Option<(Option<NSRect>, NSRect)>,
}

impl CandidateWindow {
    fn new(mtm: MainThreadMarker) -> CandidateWindow {
        let base = FloatingPanel::new(mtm);
        let highlight = rounded_box(mtm, &NSColor::controlAccentColor());
        highlight.setHidden(true);
        base.content().addSubview(&highlight);
        let separator = rounded_box(mtm, &NSColor::separatorColor());
        base.content().addSubview(&separator);
        let page = label(mtm);
        base.content().addSubview(&page);
        CandidateWindow {
            base,
            highlight,
            cells: Vec::new(),
            separator,
            page,
        }
    }

    fn show(
        &mut self,
        mtm: MainThreadMarker,
        list: &CandidateList,
        style: &Style,
        line: Option<NSRect>,
    ) {
        let font = font(style);
        let small = NSFont::systemFontOfSize((style.font_size * SMALL_SCALE).round());
        let selected_text = NSColor::alternateSelectedControlTextColor();

        // Fill in the labels first: the layout measures them.
        let count = list.items.len();
        let mut new_cells = Vec::new();
        while self.cells.len() < count {
            let cell = Cell {
                key: label(mtm),
                item: label(mtm),
            };
            self.base.content().addSubview(&cell.key);
            self.base.content().addSubview(&cell.item);
            new_cells.push(self.cells.len());
            self.cells.push(cell);
        }
        for (i, cell) in self.cells.iter().enumerate() {
            let Some(item) = list.items.get(i) else {
                cell.key.setHidden(true);
                cell.item.setHidden(true);
                continue;
            };
            let selected = list.highlighted == Some(i);
            let key = list
                .sel_keys
                .get(i)
                .map(char::to_string)
                .unwrap_or_default();
            let (key_color, item_color) = if selected {
                (selected_text.clone(), selected_text.clone())
            } else {
                (NSColor::secondaryLabelColor(), NSColor::labelColor())
            };
            set_label(&cell.key, &key, &small, &key_color);
            set_label(&cell.item, item, &font, &item_color);
            cell.key.setHidden(false);
            cell.item.setHidden(false);
        }
        let paged = list.total_pages > 1;
        if paged {
            let text = format!("{}/{}", list.page, list.total_pages);
            set_label(&self.page, &text, &small, &NSColor::secondaryLabelColor());
        }
        self.page.setHidden(!paged);

        let layout = self.layout(list, &font, &small);
        let shown = self.base.is_shown();
        let highlight = list.highlighted.and_then(|i| layout.cells.get(i));

        // Views that just appeared take their place without sliding in.
        for &i in &new_cells {
            if let Some(&(_, key, item)) = layout.cells.get(i) {
                self.cells[i].key.setFrame(key);
                self.cells[i].item.setFrame(item);
            }
        }
        if let Some(&(rect, _, _)) = highlight
            && self.highlight.isHidden()
        {
            self.highlight.setFrame(rect);
        }
        if let Some((sep, page)) = layout.page
            && !shown
        {
            if let Some(sep) = sep {
                self.separator.setFrame(sep);
            }
            self.page.setFrame(page);
        }

        animate(shown, || {
            self.base.set_frame(mtm, layout.size, layout.radius, line);
            for (cell, &(_, key, item)) in self.cells.iter().zip(&layout.cells) {
                cell.key.animator().setFrame(key);
                cell.item.animator().setFrame(item);
            }
            match highlight {
                Some(&(rect, _, _)) => {
                    self.highlight.animator().setFrame(rect);
                    self.highlight.setCornerRadius(rect.size.height / 2.0);
                    self.highlight.setHidden(false);
                }
                None => self.highlight.setHidden(true),
            }
            match layout.page {
                Some((sep, page)) => {
                    self.page.animator().setFrame(page);
                    if let Some(sep) = sep {
                        self.separator.animator().setFrame(sep);
                    }
                    self.separator.setHidden(sep.is_none());
                }
                None => self.separator.setHidden(true),
            }
        });
        self.base.show();
    }

    fn layout(&self, list: &CandidateList, font: &NSFont, small: &NSFont) -> Layout {
        let count = list.items.len();
        let per_row = list.per_row.clamp(1, count.max(1));
        let one_row = per_row >= count;

        let item_height = font.ascender() - font.descender() + font.leading();
        let cell_height = (item_height + 2.0 * CELL_PAD_Y).ceil();
        // Align the key's baseline with the candidate's.
        let key_drop = font.ascender() - small.ascender();

        let sizes: Vec<(NSSize, NSSize)> = self.cells[..count]
            .iter()
            .map(|c| (c.key.fittingSize(), c.item.fittingSize()))
            .collect();
        let cell_width = |(key, item): (NSSize, NSSize)| {
            (CELL_PAD_X + key.width + KEY_GAP + item.width + CELL_PAD_X).ceil()
        };
        // Columns share the widest cell's width, unless it is a single row.
        let column = sizes.iter().map(|&s| cell_width(s)).fold(0.0_f64, f64::max);

        let mut cells = Vec::with_capacity(count);
        let mut x = EDGE;
        let mut right = EDGE;
        for (i, &(key, item)) in sizes.iter().enumerate() {
            let (col, row) = (i % per_row, i / per_row);
            let width = if one_row {
                cell_width((key, item))
            } else {
                column
            };
            if !one_row {
                x = EDGE + col as f64 * (column + CELL_GAP);
            }
            let y = EDGE + row as f64 * (cell_height + CELL_GAP);
            let cell = NSRect::new(NSPoint::new(x, y), NSSize::new(width, cell_height));
            let text_top = y + (cell_height - item.height) / 2.0;
            let key_rect = NSRect::new(
                NSPoint::new(x + CELL_PAD_X, (text_top + key_drop).round()),
                key,
            );
            let item_rect = NSRect::new(
                NSPoint::new(x + CELL_PAD_X + key.width + KEY_GAP, text_top.round()),
                item,
            );
            cells.push((cell, key_rect, item_rect));
            right = right.max(x + width);
            if one_row {
                x += width + CELL_GAP;
            }
        }
        let rows = count.div_ceil(per_row).max(1);
        let mut width = right + EDGE;
        let mut height = EDGE + rows as f64 * cell_height + (rows - 1) as f64 * CELL_GAP + EDGE;

        let page = (list.total_pages > 1).then(|| {
            let size = self.page.fittingSize();
            if one_row {
                // After the candidates, past a short vertical line.
                let sep = NSRect::new(
                    NSPoint::new(right + 6.0, EDGE + cell_height * 0.25),
                    NSSize::new(1.0, cell_height * 0.5),
                );
                let page = NSRect::new(
                    NSPoint::new(right + 13.0, ((height - size.height) / 2.0).round()),
                    size,
                );
                width = page.origin.x + size.width + CELL_PAD_X + EDGE;
                (Some(sep), page)
            } else {
                // Below the grid, at the right.
                let page = NSRect::new(
                    NSPoint::new(width - CELL_PAD_X - EDGE - size.width, height - EDGE),
                    size,
                );
                height += size.height;
                (None, page)
            }
        });

        let size = NSSize::new(width.ceil(), height.ceil());
        Layout {
            size,
            radius: if one_row {
                size.height / 2.0
            } else {
                GRID_RADIUS
            },
            cells,
            page,
        }
    }
}
