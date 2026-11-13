// SPDX-License-Identifier: GPL-3.0-or-later

//! The preferences window, opened from the input menu. Every change is saved
//! to the settings file and applied at once. Only checkboxes, pop-up menus
//! and buttons are used: typing into a window of the input method's own
//! process would need the input method itself.
//!
//! Settings without a control here (keybindings, font family, Shift timing)
//! can still be edited in the settings file ("打開設定檔").

use std::cell::RefCell;

use chewing::editor::zhuyin_layout::KeyboardLayoutCompat;
use chewing_imk_core::config::{Config, SEL_KEYS};
use chewing_imk_core::keymap::SimulatedKeyboard;
use log::{error, info};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSApplication, NSBackingStoreType, NSButton, NSControlStateValueOff,
    NSControlStateValueOn, NSFloatingWindowLevel, NSGridCell, NSGridView, NSModalResponseOK,
    NSOpenPanel, NSPopUpButton, NSSavePanel, NSStackView, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString, ns_string};

use crate::state;

/// A checkbox bound to one boolean setting.
struct Check {
    button: Retained<NSButton>,
    get: fn(&Config) -> bool,
    set: fn(&mut Config, bool),
}

/// A pop-up menu bound to one setting; item `i` stands for `values[i]`.
struct Choice {
    popup: Retained<NSPopUpButton>,
    values: Vec<i64>,
    get: fn(&Config) -> i64,
    set: fn(&mut Config, i64),
}

#[derive(Default)]
struct Ivars {
    checks: RefCell<Vec<Check>>,
    choices: RefCell<Vec<Choice>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and Preferences does
    // not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ChewingPreferences"]
    #[ivars = Ivars]
    struct Preferences;

    impl Preferences {
        /// Any control changed: save everything.
        #[unsafe(method(changed:))]
        fn changed(&self, _sender: Option<&AnyObject>) {
            self.save();
        }

        #[unsafe(method(openConfigFile:))]
        fn open_config_file(&self, _sender: Option<&AnyObject>) {
            open_config_file();
        }

        #[unsafe(method(exportPhrases:))]
        fn export_phrases(&self, _sender: Option<&AnyObject>) {
            export_phrases(self.mtm());
        }

        #[unsafe(method(importPhrases:))]
        fn import_phrases(&self, _sender: Option<&AnyObject>) {
            import_phrases(self.mtm());
        }
    }
);

struct Window {
    window: Retained<NSWindow>,
    prefs: Retained<Preferences>,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
}

/// Show the preferences window, creating it on first use.
pub fn show(mtm: MainThreadMarker) {
    // Since macOS 14 activation is cooperative, and an input method is
    // never the app the user is in, so `activate()` alone may leave the
    // window behind the current app. Ask to be activated regardless, and
    // float the window above normal windows so it is seen either way.
    let app = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    app.activate();
    WINDOW.with_borrow_mut(|slot| {
        let w = slot.get_or_insert_with(|| build(mtm));
        w.prefs.load();
        w.window.setLevel(NSFloatingWindowLevel);
        w.window.center();
        w.window.makeKeyAndOrderFront(None);
        w.window.orderFrontRegardless();
    });
}

impl Preferences {
    fn new(mtm: MainThreadMarker) -> Retained<Preferences> {
        let this = Preferences::alloc(mtm).set_ivars(Ivars::default());
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }

    fn current_config() -> Config {
        state::with_core(|core| core.config().clone()).unwrap_or_default()
    }

    /// Show the current settings in the controls.
    fn load(&self) {
        let cfg = Self::current_config();
        for c in self.ivars().checks.borrow().iter() {
            c.button.setState(if (c.get)(&cfg) {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }
        for c in self.ivars().choices.borrow().iter() {
            let value = (c.get)(&cfg);
            let index = c.values.iter().position(|&v| v == value).unwrap_or(0);
            c.popup.selectItemAtIndex(index as isize);
        }
    }

    fn save(&self) {
        let mut cfg = Self::current_config();
        for c in self.ivars().checks.borrow().iter() {
            (c.set)(&mut cfg, c.button.state() == NSControlStateValueOn);
        }
        for c in self.ivars().choices.borrow().iter() {
            let index = c.popup.indexOfSelectedItem();
            if let Some(&value) = usize::try_from(index).ok().and_then(|i| c.values.get(i)) {
                (c.set)(&mut cfg, value);
            }
        }
        match state::save_config(cfg) {
            Ok(()) => info!("settings saved"),
            Err(e) => {
                error!("failed to save settings: {e}");
                alert(self.mtm(), "無法儲存設定", &e);
            }
        }
    }

    fn check(
        &self,
        mtm: MainThreadMarker,
        title: &str,
        get: fn(&Config) -> bool,
        set: fn(&mut Config, bool),
    ) -> Retained<NSView> {
        // SAFETY: `changed:` is implemented above and self outlives the
        // window (both are kept in WINDOW).
        let button = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &NSString::from_str(title),
                Some(self),
                Some(sel!(changed:)),
                mtm,
            )
        };
        let view: Retained<NSView> = button.clone().into_super().into_super();
        self.ivars()
            .checks
            .borrow_mut()
            .push(Check { button, get, set });
        view
    }

    fn choice(
        &self,
        mtm: MainThreadMarker,
        items: &[(&str, i64)],
        get: fn(&Config) -> i64,
        set: fn(&mut Config, i64),
    ) -> Retained<NSView> {
        let popup =
            NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), NSRect::ZERO, false);
        for (title, _) in items {
            popup.addItemWithTitle(&NSString::from_str(title));
        }
        // SAFETY: see `check`.
        unsafe {
            popup.setTarget(Some(self));
            popup.setAction(Some(sel!(changed:)));
        }
        let view: Retained<NSView> = popup.clone().into_super().into_super().into_super();
        self.ivars().choices.borrow_mut().push(Choice {
            popup,
            values: items.iter().map(|&(_, v)| v).collect(),
            get,
            set,
        });
        view
    }

    fn button(
        &self,
        mtm: MainThreadMarker,
        title: &str,
        action: objc2::runtime::Sel,
    ) -> Retained<NSView> {
        // SAFETY: the actions are implemented above.
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(title),
                Some(self),
                Some(action),
                mtm,
            )
        };
        button.into_super().into_super()
    }
}

fn label(mtm: MainThreadMarker, text: &str) -> Retained<NSView> {
    NSTextField::labelWithString(&NSString::from_str(text), mtm)
        .into_super()
        .into_super()
}

const KEYBOARD_LAYOUTS: &[(&str, i64)] = &[
    ("標準（大千）", 0),
    ("許氏", 1),
    ("IBM", 2),
    ("精業", 3),
    ("倚天", 4),
    ("倚天 26 鍵", 5),
    ("Dvorak", 6),
    ("Dvorak 許氏", 7),
    ("大千 26 鍵", 8),
    ("漢語拼音", 9),
    ("台灣華語羅馬拼音", 10),
    ("注音二式", 11),
    ("Carpalx", 12),
    ("Colemak-DH ANSI", 13),
    ("Colemak-DH Ortholinear", 14),
    ("Workman", 15),
    ("Colemak", 16),
];

const SIMULATED_LAYOUTS: &[(&str, i64)] = &[
    ("不模擬（QWERTY）", 0),
    ("Dvorak", 1),
    ("Carpalx（QGMLWY）", 2),
    ("Colemak", 3),
    ("Colemak-DH ANSI", 4),
    ("Colemak-DH Ortholinear", 5),
    ("Workman", 6),
];

const CONVERSION_ENGINES: &[(&str, i64)] = &[("簡易", 0), ("智慧（預設）", 1), ("模糊智慧", 2)];

const FONT_SIZES: &[(&str, i64)] = &[
    ("12", 12),
    ("14", 14),
    ("16", 16),
    ("18", 18),
    ("20", 20),
    ("24", 24),
    ("28", 28),
    ("32", 32),
];

const COUNTS: &[(&str, i64)] = &[
    ("1", 1),
    ("2", 2),
    ("3", 3),
    ("4", 4),
    ("5", 5),
    ("6", 6),
    ("7", 7),
    ("8", 8),
    ("9", 9),
    ("10", 10),
];

fn build(mtm: MainThreadMarker) -> Window {
    let prefs = Preferences::new(mtm);
    let p = &prefs;
    let empty = || NSGridCell::emptyContentView(mtm);

    let sel_keys: Vec<(&str, i64)> = SEL_KEYS
        .iter()
        .enumerate()
        .map(|(i, k)| (*k, i as i64))
        .collect();

    let rows: Vec<[Retained<NSView>; 2]> = vec![
        [
            label(mtm, "注音鍵盤配置："),
            p.choice(
                mtm,
                KEYBOARD_LAYOUTS,
                |c| c.keyboard_layout as i64,
                |c, v| {
                    c.keyboard_layout = KeyboardLayoutCompat::try_from(v as u8)
                        .unwrap_or(KeyboardLayoutCompat::Default)
                },
            ),
        ],
        [
            label(mtm, "模擬英文鍵盤："),
            p.choice(
                mtm,
                SIMULATED_LAYOUTS,
                |c| i32::from(c.simulate_english_layout) as i64,
                |c, v| c.simulate_english_layout = SimulatedKeyboard::from(v as i32),
            ),
        ],
        [
            label(mtm, "選字鍵："),
            p.choice(
                mtm,
                &sel_keys,
                |c| c.sel_key_type as i64,
                |c, v| c.sel_key_type = v as usize,
            ),
        ],
        [
            label(mtm, "每頁候選字數："),
            p.choice(
                mtm,
                COUNTS,
                |c| c.cand_per_page as i64,
                |c, v| c.cand_per_page = v as usize,
            ),
        ],
        [
            label(mtm, "每列候選字數："),
            p.choice(
                mtm,
                COUNTS,
                |c| c.cand_per_row as i64,
                |c, v| c.cand_per_row = v as usize,
            ),
        ],
        [
            label(mtm, "候選字大小："),
            p.choice(
                mtm,
                FONT_SIZES,
                |c| c.font_size.round() as i64,
                |c, v| c.font_size = v as f64,
            ),
        ],
        [
            label(mtm, "選詞方式："),
            p.choice(
                mtm,
                CONVERSION_ENGINES,
                |c| c.conv_engine as i64,
                |c, v| c.conv_engine = v as u8,
            ),
        ],
        [
            label(mtm, "中英切換："),
            p.check(
                mtm,
                "單按 Shift 切換中英",
                |c| c.switch_lang_with_shift,
                |c, v| c.switch_lang_with_shift = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "以 Caps Lock 切換中英",
                |c| c.enable_caps_lock,
                |c, v| c.enable_caps_lock = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "Caps Lock 開啟時為中文模式",
                |c| c.lock_chinese_on_caps_lock,
                |c, v| c.lock_chinese_on_caps_lock = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "切換時顯示提示",
                |c| c.show_notification,
                |c, v| c.show_notification = v,
            ),
        ],
        [
            label(mtm, "預設："),
            p.check(
                mtm,
                "英數模式",
                |c| c.default_english,
                |c, v| c.default_english = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "全形",
                |c| c.default_full_space,
                |c, v| c.default_full_space = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "輸出簡體中文",
                |c| c.output_simp_chinese,
                |c, v| c.output_simp_chinese = v,
            ),
        ],
        [
            label(mtm, "選字："),
            p.check(
                mtm,
                "用方向鍵選字",
                |c| c.cursor_cand_list,
                |c, v| c.cursor_cand_list = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "空白鍵開啟候選窗",
                |c| c.show_cand_with_space_key,
                |c, v| c.show_cand_with_space_key = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "選字後游標自動後移",
                |c| c.advance_after_selection,
                |c, v| c.advance_after_selection = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "從游標後方選詞",
                |c| c.phrase_choice_rearward,
                |c, v| c.phrase_choice_rearward = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "依使用頻率排列候選字",
                |c| c.sort_candidates_by_frequency,
                |c, v| c.sort_candidates_by_frequency = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "Esc 清除整個組字區",
                |c| c.esc_clean_all_buf,
                |c, v| c.esc_clean_all_buf = v,
            ),
        ],
        [
            label(mtm, "詞彙："),
            p.check(
                mtm,
                "自動學習詞彙",
                |c| c.enable_auto_learn,
                |c, v| c.enable_auto_learn = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "自動學習常用詞",
                |c| c.auto_learn_user_history,
                |c, v| c.auto_learn_user_history = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "自動學習新詞",
                |c| c.auto_learn_new_words,
                |c, v| c.auto_learn_new_words = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "Ctrl + 數字從游標往前加詞",
                |c| c.add_phrase_forward,
                |c, v| c.add_phrase_forward = v,
            ),
        ],
        [
            label(mtm, "Shift 組合："),
            p.check(
                mtm,
                "Shift + 字母輸入符號",
                |c| c.easy_symbols_with_shift,
                |c, v| c.easy_symbols_with_shift = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "Shift + 符號輸出全形",
                |c| c.full_shape_symbols,
                |c, v| c.full_shape_symbols = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "Shift + 字母輸出大寫",
                |c| c.upper_case_with_shift,
                |c, v| c.upper_case_with_shift = v,
            ),
        ],
        [
            empty(),
            p.check(
                mtm,
                "Shift + Space 切換全半形",
                |c| c.enable_fullwidth_toggle_key,
                |c, v| c.enable_fullwidth_toggle_key = v,
            ),
        ],
    ];

    let buttons = NSStackView::stackViewWithViews(
        &NSArray::from_retained_slice(&[
            p.button(mtm, "匯入使用者詞庫…", sel!(importPhrases:)),
            p.button(mtm, "匯出使用者詞庫…", sel!(exportPhrases:)),
            p.button(mtm, "打開設定檔", sel!(openConfigFile:)),
        ]),
        mtm,
    );
    buttons.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    buttons.setSpacing(8.0);

    let mut grid_rows: Vec<Retained<NSArray<NSView>>> = rows
        .into_iter()
        .map(|row| NSArray::from_retained_slice(&row))
        .collect();
    grid_rows.push(NSArray::from_retained_slice(&[
        empty(),
        buttons.into_super(),
    ]));
    let grid = NSGridView::gridViewWithViews(&NSArray::from_retained_slice(&grid_rows), mtm);
    grid.setRowSpacing(6.0);
    grid.setColumnSpacing(8.0);

    const MARGIN: f64 = 20.0;
    let size = grid.fittingSize();
    grid.setFrameOrigin(NSPoint::new(MARGIN, MARGIN));
    grid.setFrameSize(size);
    let content_size = NSSize::new(size.width + 2.0 * MARGIN, size.height + 2.0 * MARGIN);
    let content =
        NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::ZERO, content_size));
    content.addSubview(&grid);

    // SAFETY: a standard titled window; we keep a strong reference.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::ZERO, content_size),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: we keep our own strong reference and reuse the window.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(ns_string!("新酷音設定"));
    window.setContentView(Some(&content));
    Window { window, prefs }
}

fn alert(mtm: MainThreadMarker, message: &str, info: &str) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(message));
    alert.setInformativeText(&NSString::from_str(info));
    alert.runModal();
}

/// Open the settings file in the default text editor, creating it first.
fn open_config_file() {
    let Some(path) = state::config_path() else {
        error!("no user data directory");
        return;
    };
    if !path.exists() {
        let cfg = state::with_core(|core| core.config().clone()).unwrap_or_default();
        if let Err(e) = state::save_config(cfg) {
            error!("failed to create {}: {e}", path.display());
            return;
        }
    }
    if let Err(e) = std::process::Command::new("/usr/bin/open")
        .arg("-t")
        .arg(&path)
        .status()
    {
        error!("failed to open {}: {e}", path.display());
    }
}

fn export_phrases(mtm: MainThreadMarker) {
    let Some(csv) = state::with_core(|core| core.export_user_phrases()) else {
        return;
    };
    let panel = NSSavePanel::savePanel(mtm);
    panel.setNameFieldStringValue(ns_string!("新酷音使用者詞庫.csv"));
    if panel.runModal() != NSModalResponseOK {
        return;
    }
    let Some(path) = panel.URL().and_then(|u| u.path()) else {
        return;
    };
    let path = path.to_string();
    let count = csv.lines().count().saturating_sub(1);
    match std::fs::write(&path, csv) {
        Ok(()) => alert(mtm, "匯出完成", &format!("已匯出 {count} 個詞到 {path}")),
        Err(e) => alert(mtm, "無法匯出", &format!("{path}：{e}")),
    }
}

fn import_phrases(mtm: MainThreadMarker) {
    let panel = NSOpenPanel::openPanel(mtm);
    if panel.runModal() != NSModalResponseOK {
        return;
    }
    let Some(path) = panel.URLs().firstObject().and_then(|u| u.path()) else {
        return;
    };
    let path = path.to_string();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            alert(mtm, "無法匯入", &format!("{path}：{e}"));
            return;
        }
    };
    let Some(report) = state::with_core(|core| core.import_user_phrases(&text)) else {
        return;
    };
    let mut info = format!("已匯入 {} 個詞。", report.added);
    if !report.skipped.is_empty() {
        let lines: Vec<String> = report
            .skipped
            .iter()
            .take(10)
            .map(|n| n.to_string())
            .collect();
        info.push_str(&format!(
            "\n有 {} 行無法讀取（第 {} 行{}）。格式為「詞,注音」，注音之間以空白分隔。",
            report.skipped.len(),
            lines.join("、"),
            if report.skipped.len() > 10 { "等" } else { "" }
        ));
    }
    alert(mtm, "匯入完成", &info);
}
