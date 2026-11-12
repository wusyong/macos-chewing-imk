// SPDX-License-Identifier: GPL-3.0-or-later

//! `ChewingIMK --register`: register this bundle with Text Input Sources and
//! enable it, so it shows up in the input menu without visiting System
//! Settings. Used by `cargo xtask install` and, later, the installer.

use std::ffi::c_void;
use std::process::ExitCode;

use objc2::rc::Retained;
use objc2_foundation::{NSArray, NSBundle, NSDictionary, NSString};

type OSStatus = i32;
const NO_ERR: OSStatus = 0;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    /// CFStringRef
    static kTISPropertyBundleID: *const c_void;
    /// location: CFURLRef
    fn TISRegisterInputSource(location: *const c_void) -> OSStatus;
    /// properties: CFDictionaryRef; returns an owned CFArrayRef of TISInputSourceRef
    fn TISCreateInputSourceList(
        properties: *const c_void,
        include_all_installed: u8,
    ) -> *mut c_void;
    /// source: TISInputSourceRef
    fn TISEnableInputSource(source: *const c_void) -> OSStatus;
}

pub fn register_and_enable() -> ExitCode {
    let bundle = NSBundle::mainBundle();
    let url = bundle.bundleURL();
    let Some(bundle_id) = bundle.bundleIdentifier() else {
        eprintln!("missing bundle identifier; run this from inside the .app bundle");
        return ExitCode::FAILURE;
    };

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

    // SAFETY: kTISPropertyBundleID is a CFStringRef constant, toll-free
    // bridged to NSString, and valid for the lifetime of the process.
    let key: &NSString = unsafe { &*kTISPropertyBundleID.cast::<NSString>() };
    let filter = NSDictionary::<NSString, NSString>::from_slices(&[key], &[&*bundle_id]);

    // SAFETY: NSDictionary is toll-free bridged to CFDictionaryRef. The
    // returned CFArrayRef follows the Create rule, so we take ownership.
    let sources: Option<Retained<NSArray>> = unsafe {
        let raw = TISCreateInputSourceList(Retained::as_ptr(&filter).cast(), 1);
        Retained::from_raw(raw.cast())
    };
    let Some(sources) = sources.filter(|s| s.count() > 0) else {
        eprintln!("no input source found for {bundle_id} after registering");
        return ExitCode::FAILURE;
    };

    let mut ok = true;
    for source in sources.iter() {
        // SAFETY: each element is a TISInputSourceRef.
        let status = unsafe { TISEnableInputSource(Retained::as_ptr(&source).cast()) };
        if status != NO_ERR {
            eprintln!("TISEnableInputSource failed: OSStatus {status}");
            ok = false;
        }
    }
    if !ok {
        return ExitCode::FAILURE;
    }
    println!("enabled {bundle_id} ({} input source(s))", sources.count());
    ExitCode::SUCCESS
}
