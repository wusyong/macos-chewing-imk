// SPDX-License-Identifier: GPL-3.0-or-later

//! Text Input Sources bookkeeping, run as `ChewingIMK --<command>`.
//!
//! How macOS (14 and later) treats a third-party input method, as tested on
//! this project (and the same for OpenVanilla McBopomofo's installer):
//!
//! - The user's input menu shows the input modes listed in
//!   `com.apple.inputsources` → `AppleEnabledThirdPartyInputSources`. Only
//!   macOS writes that list; other processes cannot change it.
//! - `TISEnableInputSource` on the input method makes System Settings ask
//!   "要允許「新酷音」啟用「新酷音」嗎？". Allowing adds the input method
//!   ("Keyboard Input Method") to that list, but not its input mode, so it is
//!   not in the menu yet.
//! - The input mode is added only by System Settings: Keyboard > Input
//!   Sources > Edit… > + > 繁體中文 > 新酷音. Enabling the mode through TIS
//!   instead only marks it enabled for the session, which makes System
//!   Settings show it grey and not addable; so we never do that.
//!
//! Commands:
//!
//! - `--register`: make macOS know this bundle and, if the user has not
//!   allowed it yet, ask (one prompt).
//! - `--unregister`: disable everything and remove this input method from the
//!   input source lists macOS lets us change.
//! - `--status`: print what macOS knows, for `cargo xtask` to check.

use std::ffi::c_void;
use std::process::ExitCode;

use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{
    NSArray, NSBundle, NSDictionary, NSMutableArray, NSString, NSUserDefaults, ns_string,
};

type OSStatus = i32;
const NO_ERR: OSStatus = 0;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    /// CFStringRef
    static kTISPropertyBundleID: *const c_void;
    /// CFStringRef
    static kTISPropertyInputSourceID: *const c_void;
    /// location: CFURLRef
    fn TISRegisterInputSource(location: *const c_void) -> OSStatus;
    /// properties: CFDictionaryRef; returns an owned CFArrayRef of TISInputSourceRef
    fn TISCreateInputSourceList(
        properties: *const c_void,
        include_all_installed: u8,
    ) -> *mut c_void;
    /// Returns a borrowed CFTypeRef.
    fn TISGetInputSourceProperty(source: *const c_void, key: *const c_void) -> *const c_void;
    fn TISEnableInputSource(source: *const c_void) -> OSStatus;
    fn TISDisableInputSource(source: *const c_void) -> OSStatus;
}

/// One input source of this bundle: the input method or one of its modes.
struct Source {
    id: String,
    object: Retained<AnyObject>,
}

impl Source {
    fn as_ptr(&self) -> *const c_void {
        Retained::as_ptr(&self.object).cast()
    }
}

fn bundle_id() -> Option<Retained<NSString>> {
    NSBundle::mainBundle().bundleIdentifier()
}

/// The input sources of `bundle_id`: all installed ones, or only the
/// enabled ones.
fn sources(bundle_id: &NSString, include_all: bool) -> Vec<Source> {
    // SAFETY: the property keys are CFStringRef constants, toll-free bridged
    // to NSString and valid for the lifetime of the process.
    let key: &NSString = unsafe { &*kTISPropertyBundleID.cast::<NSString>() };
    let filter = NSDictionary::<NSString, NSString>::from_slices(&[key], &[bundle_id]);
    // SAFETY: NSDictionary is toll-free bridged to CFDictionaryRef. The
    // returned CFArrayRef follows the Create rule, so we take ownership.
    let list: Option<Retained<NSArray>> = unsafe {
        let raw = TISCreateInputSourceList(Retained::as_ptr(&filter).cast(), include_all as u8);
        Retained::from_raw(raw.cast())
    };
    let Some(list) = list else {
        return Vec::new();
    };
    list.iter()
        .map(|object| {
            // SAFETY: each element is a TISInputSourceRef; the ID property is
            // a CFStringRef owned by the source.
            let id = unsafe {
                let raw = TISGetInputSourceProperty(
                    Retained::as_ptr(&object).cast(),
                    kTISPropertyInputSourceID,
                );
                raw.cast::<NSString>().as_ref().map(|s| s.to_string())
            };
            Source {
                id: id.unwrap_or_default(),
                object,
            }
        })
        .collect()
}

pub fn register() -> ExitCode {
    let bundle = NSBundle::mainBundle();
    let Some(bundle_id) = bundle_id() else {
        eprintln!("missing bundle identifier; run this from inside the .app bundle");
        return ExitCode::FAILURE;
    };

    if sources(&bundle_id, true).is_empty() {
        let url = bundle.bundleURL();
        // SAFETY: NSURL is toll-free bridged to CFURLRef.
        let status = unsafe { TISRegisterInputSource(Retained::as_ptr(&url).cast()) };
        if status != NO_ERR {
            eprintln!("TISRegisterInputSource failed: OSStatus {status}");
            return ExitCode::FAILURE;
        }
        println!(
            "registered {}",
            url.path().map(|p| p.to_string()).unwrap_or_default()
        );
    }

    let all = sources(&bundle_id, true);
    if all.is_empty() {
        eprintln!("no input source found for {bundle_id} after registering");
        return ExitCode::FAILURE;
    }
    if consent_record(&bundle_id).allowed {
        println!("allowed");
        return ExitCode::SUCCESS;
    }
    // Ask for the input method itself (its ID is the bundle ID). macOS shows
    // the consent prompt; the input mode is added in System Settings later.
    let Some(method) = all.iter().find(|s| *s.id == *bundle_id.to_string()) else {
        eprintln!("no input method source {bundle_id}");
        return ExitCode::FAILURE;
    };
    // SAFETY: a valid TISInputSourceRef.
    let status = unsafe { TISEnableInputSource(method.as_ptr()) };
    if status != NO_ERR {
        eprintln!(
            "TISEnableInputSource({}) failed: OSStatus {status}",
            method.id
        );
        return ExitCode::FAILURE;
    }
    println!("requested: {}", method.id);
    ExitCode::SUCCESS
}

pub fn unregister() -> ExitCode {
    let Some(bundle_id) = bundle_id() else {
        eprintln!("missing bundle identifier; run this from inside the .app bundle");
        return ExitCode::FAILURE;
    };
    let all = sources(&bundle_id, true);
    if all.is_empty() {
        // Not known to Text Input Sources (any more), but the preferences
        // may still name us.
        println!("{bundle_id} is not registered");
        forget_preferences(&bundle_id);
        return ExitCode::SUCCESS;
    }
    // Modes before the input method itself.
    for source in all.iter().rev() {
        // SAFETY: a valid TISInputSourceRef.
        let status = unsafe { TISDisableInputSource(source.as_ptr()) };
        if status != NO_ERR {
            eprintln!(
                "TISDisableInputSource({}) failed: OSStatus {status}",
                source.id
            );
        }
    }
    println!(
        "disabled: {}",
        all.iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    forget_preferences(&bundle_id);
    ExitCode::SUCCESS
}

/// Input source lists in the user's preferences that may name us.
const PREFERENCE_LISTS: &[(&str, &str)] = &[
    (
        "com.apple.inputsources",
        "AppleEnabledThirdPartyInputSources",
    ),
    ("com.apple.HIToolbox", "AppleEnabledInputSources"),
    ("com.apple.HIToolbox", "AppleSelectedInputSources"),
    ("com.apple.HIToolbox", "AppleInputSourceHistory"),
];

/// Remove the entries whose "Bundle ID" is ours from the input source lists.
/// Entries of other input sources are left exactly as they are.
fn forget_preferences(bundle_id: &NSString) {
    let key = ns_string!("Bundle ID");
    let mut removed = 0;
    for (domain, list) in PREFERENCE_LISTS {
        let Some(defaults) = NSUserDefaults::initWithSuiteName(
            NSUserDefaults::alloc(),
            Some(&NSString::from_str(domain)),
        ) else {
            continue;
        };
        let list = NSString::from_str(list);
        let Some(entries) = defaults.arrayForKey(&list) else {
            continue;
        };
        let kept = NSMutableArray::<AnyObject>::new();
        for entry in entries.iter() {
            let ours = entry
                .downcast_ref::<NSDictionary>()
                .and_then(|d| d.objectForKey(key))
                .and_then(|v| v.downcast::<NSString>().ok())
                .is_some_and(|v| &*v == bundle_id);
            if ours {
                removed += 1;
            } else {
                kept.addObject(&entry);
            }
        }
        let ours = entries.count() - kept.count();
        if ours == 0 {
            continue;
        }
        // SAFETY: an array of the original property list values.
        unsafe { defaults.setObject_forKey(Some(&kept), &list) };
        let still_there = defaults.arrayForKey(&list).map(|a| a.count()) != Some(kept.count());
        if !defaults.synchronize() || still_there {
            // com.apple.inputsources records the user's consent; macOS does
            // not let other processes change it.
            println!(
                "{domain} is managed by macOS; {ours} entr{} left there",
                if ours == 1 { "y" } else { "ies" }
            );
            removed -= ours;
        }
    }
    println!(
        "removed {removed} preference entr{}",
        if removed == 1 { "y" } else { "ies" }
    );
}

/// Prints `installed <id>` for every known source and `enabled <id>` for the
/// enabled ones.
pub fn status() -> ExitCode {
    let Some(bundle_id) = bundle_id() else {
        eprintln!("missing bundle identifier; run this from inside the .app bundle");
        return ExitCode::FAILURE;
    };
    for s in sources(&bundle_id, true) {
        println!("installed {}", s.id);
    }
    for s in sources(&bundle_id, false) {
        println!("enabled {}", s.id);
    }
    let record = consent_record(&bundle_id);
    if record.allowed {
        println!("allowed");
    }
    for mode in record.added_modes {
        println!("added {mode}");
    }
    ExitCode::SUCCESS
}

/// What `com.apple.inputsources` says about us: whether the user allowed the
/// input method, and which of its modes are in the input menu.
struct ConsentRecord {
    allowed: bool,
    added_modes: Vec<String>,
}

fn consent_record(bundle_id: &NSString) -> ConsentRecord {
    let mut record = ConsentRecord {
        allowed: false,
        added_modes: Vec::new(),
    };
    let Some(defaults) = NSUserDefaults::initWithSuiteName(
        NSUserDefaults::alloc(),
        Some(ns_string!("com.apple.inputsources")),
    ) else {
        return record;
    };
    let Some(entries) = defaults.arrayForKey(ns_string!("AppleEnabledThirdPartyInputSources"))
    else {
        return record;
    };
    let text = |d: &NSDictionary, key: &NSString| {
        d.objectForKey(key)
            .and_then(|v| v.downcast::<NSString>().ok())
            .map(|v| v.to_string())
    };
    for entry in entries.iter() {
        let Some(d) = entry.downcast_ref::<NSDictionary>() else {
            continue;
        };
        if text(d, ns_string!("Bundle ID")).as_deref() != Some(&*bundle_id.to_string()) {
            continue;
        }
        match text(d, ns_string!("Input Mode")) {
            Some(mode) => record.added_modes.push(mode),
            None => record.allowed = true,
        }
    }
    record
}
