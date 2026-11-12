// SPDX-License-Identifier: GPL-3.0-or-later

//! The input menu (the menu bar's input source menu while 新酷音 is active).
//! Mirrors windows-chewing-tsf's popup menu. Item actions are sent by IMK to
//! the input controller, with a dictionary holding the menu item
//! (`kIMKCommandMenuItemName`) as the sender.

use chewing_imk_core::LangMode;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSMenu, NSMenuItem, NSWorkspace,
};
use objc2_foundation::{NSDictionary, NSString, NSURL, ns_string};
use objc2_input_method_kit::kIMKCommandMenuItemName;

/// State shown with check marks.
pub struct MenuState {
    pub lang_mode: LangMode,
    pub fullwidth: bool,
    pub simplified: bool,
}

/// Web links in the menu, as in windows-chewing-tsf.
const HELP: &str = "https://chewing.im/features.html";
const WEBSITE: &str = "https://chewing.im/";
const DICT_BUG_REPORT: &str = "https://codeberg.org/chewing/libchewing-data/issues";
const DICTIONARIES: &[(&str, &str)] = &[
    ("萌典 (moedict)", "https://www.moedict.tw/"),
    ("教育部國語辭典", "https://dict.revised.moe.edu.tw/"),
    ("教育部國語辭典簡編本", "https://dict.concised.moe.edu.tw/"),
    ("教育部國語小字典", "https://dict.mini.moe.edu.tw/"),
    ("教育部成語典", "https://dict.idioms.moe.edu.tw/"),
];

pub fn build(mtm: MainThreadMarker, state: &MenuState) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("新酷音"));
    menu.setAutoenablesItems(false);

    menu.addItem(&item(
        mtm,
        "英數模式",
        sel!(toggleLanguageMode:),
        Some(state.lang_mode == LangMode::English),
    ));
    menu.addItem(&item(
        mtm,
        "輸出全形英數字母",
        sel!(toggleShapeMode:),
        Some(state.fullwidth),
    ));
    menu.addItem(&item(
        mtm,
        "輸出簡體中文",
        sel!(toggleSimplifiedChinese:),
        Some(state.simplified),
    ));

    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&item(
        mtm,
        "設定新酷音輸入法…",
        sel!(openPreferences:),
        None,
    ));
    menu.addItem(&link(mtm, "新酷音使用說明", HELP));
    menu.addItem(&NSMenuItem::sectionHeaderWithTitle(
        ns_string!("網路辭典"),
        mtm,
    ));
    for (title, url) in DICTIONARIES {
        menu.addItem(&link(mtm, title, url));
    }

    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&link(mtm, "參觀新酷音官方網站", WEBSITE));
    menu.addItem(&link(mtm, "注音及選字選詞錯誤回報", DICT_BUG_REPORT));
    menu
}

fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    checked: Option<bool>,
) -> Retained<NSMenuItem> {
    // SAFETY: the action is implemented by ChewingController, which IMK
    // makes the target of input menu items.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            ns_string!(""),
        )
    };
    if let Some(checked) = checked {
        // SAFETY: plain integer constants.
        item.setState(if checked {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
    }
    item
}

fn link(mtm: MainThreadMarker, title: &str, url: &str) -> Retained<NSMenuItem> {
    let item = item(mtm, title, sel!(openLink:), None);
    // SAFETY: an NSString is a valid represented object.
    unsafe { item.setRepresentedObject(Some(&NSString::from_str(url))) };
    item
}

/// The menu item that sent an action, from IMK's sender dictionary.
fn sender_item(sender: Option<&AnyObject>) -> Option<Retained<NSMenuItem>> {
    let dict = sender?.downcast_ref::<NSDictionary>()?;
    // SAFETY: kIMKCommandMenuItemName is a valid NSString constant.
    let key = unsafe { kIMKCommandMenuItemName };
    let obj = dict.objectForKey(key)?;
    obj.downcast::<NSMenuItem>().ok()
}

/// Handle `openLink:`: open the URL stored on the menu item.
pub fn open_link(sender: Option<&AnyObject>) {
    let Some(url) = sender_item(sender)
        .and_then(|item| item.representedObject())
        .and_then(|obj| obj.downcast::<NSString>().ok())
        .and_then(|s| NSURL::URLWithString(&s))
    else {
        log::warn!("openLink: without a URL");
        return;
    };
    NSWorkspace::sharedWorkspace().openURL(&url);
}
