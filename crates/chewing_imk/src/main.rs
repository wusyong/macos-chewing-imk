// SPDX-License-Identifier: GPL-3.0-or-later

//! ChewingIMK: the macOS input method process.
//!
//! Run without arguments, it is started by macOS as the input method server.
//! Run with `--register`, it registers and enables its own bundle as an input
//! source (used by `cargo xtask install`). `--demo-candidates` shows the
//! candidate window with sample candidates, for working on its look.

mod candidate_window;
mod client;
mod controller;
mod menu;
mod notice_window;
mod panel;
mod prefs;
mod register;
mod state;

use std::process::ExitCode;

use log::{LevelFilter, error, info};
use objc2::rc::Retained;
use objc2::{AllocAnyThread, ClassType, MainThreadMarker};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSBundle, NSString, ns_string};
use objc2_input_method_kit::IMKServer;

use crate::controller::ChewingController;

/// os_log subsystem; matches the bundle identifier.
pub const LOG_SUBSYSTEM: &str = "im.chewing.inputmethod.Chewing";

fn main() -> ExitCode {
    init_logging();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => run_server(),
        Some("--register") => register::register(),
        Some("--unregister") => register::unregister(),
        Some("--status") => register::status(),
        Some("--demo-candidates") => demo_candidates(),
        Some(other) => {
            eprintln!("unknown argument: {other}");
            eprintln!(
                "usage: ChewingIMK [--register | --unregister | --status | --demo-candidates]"
            );
            ExitCode::FAILURE
        }
    }
}

fn init_logging() {
    let level = if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    // `level_filter` also sets log's global max level.
    let inner = oslog::OsLogger::new(LOG_SUBSYSTEM).level_filter(level);
    if let Err(e) = log::set_boxed_logger(Box::new(PrivateLogger(inner))) {
        eprintln!("failed to initialize logging: {e}");
    }
}

/// Keeps what the user types out of the system log.
///
/// libchewing logs every key with its character (info) and every conversion
/// with the resulting phrases (debug). oslog maps info to os_log's Default
/// level, which is persisted to disk. So everything from libchewing below
/// Warn is dropped.
struct PrivateLogger(oslog::OsLogger);

impl PrivateLogger {
    fn is_libchewing(target: &str) -> bool {
        target == "chewing" || target.starts_with("chewing::")
    }
}

impl log::Log for PrivateLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        if Self::is_libchewing(metadata.target()) && metadata.level() > log::Level::Warn {
            return false;
        }
        self.0.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.0.log(record);
        }
    }

    fn flush(&self) {
        self.0.flush();
    }
}

fn run_server() -> ExitCode {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("ChewingIMK must run on the main thread");
        return ExitCode::FAILURE;
    };

    // IMKServer looks the controller class up by the name in Info.plist
    // (InputMethodServerControllerClass), so it must be registered with the
    // Objective-C runtime before the server is created.
    let _ = ChewingController::class();

    let bundle = NSBundle::mainBundle();
    let Some(bundle_id) = bundle.bundleIdentifier() else {
        error!("missing bundle identifier; ChewingIMK must run from its .app bundle");
        return ExitCode::FAILURE;
    };
    let Some(connection_name) = info_string(&bundle, ns_string!("InputMethodConnectionName"))
    else {
        error!("missing InputMethodConnectionName in Info.plist");
        return ExitCode::FAILURE;
    };

    // SAFETY: both arguments are valid, non-nil NSStrings.
    let server = unsafe {
        IMKServer::initWithName_bundleIdentifier(
            IMKServer::alloc(),
            Some(&connection_name),
            Some(&bundle_id),
        )
    };
    let Some(server) = server else {
        error!("failed to create IMKServer for connection {connection_name}");
        return ExitCode::FAILURE;
    };
    info!(
        "IMKServer started: bundle={bundle_id} connection={connection_name} version={}",
        env!("CARGO_PKG_VERSION")
    );

    let app = NSApplication::sharedApplication(mtm);
    app.run();

    // Not reached in practice: the process is terminated by the system.
    drop(server);
    ExitCode::SUCCESS
}

/// Show the candidate window near the top of the screen and step the
/// highlight through sample pages, until the process is killed.
fn demo_candidates() -> ExitCode {
    use std::cell::Cell;

    use chewing_imk_core::CandidateList;
    use objc2_app_kit::{NSApplicationActivationPolicy, NSScreen};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSTimer};

    let Some(mtm) = MainThreadMarker::new() else {
        return ExitCode::FAILURE;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let pages: [&[&str]; 2] = [
        &["哈囉", "👋", "囉", "捋", "剁", "哥", "攞", "犸", "囉嗦"],
        &["你", "妳", "擬", "尼", "泥", "逆", "膩", "倪", "匿"],
    ];
    let per_row: usize = std::env::var("DEMO_PER_ROW")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(9);
    let line = NSScreen::mainScreen(mtm).map(|s| {
        let f = s.visibleFrame();
        NSRect::new(
            NSPoint::new(f.origin.x + 200.0, f.origin.y + f.size.height - 200.0),
            NSSize::new(2.0, 20.0),
        )
    });
    let step = Cell::new(0_usize);
    let tick = move || {
        let n = step.get();
        step.set(n + 1);
        let page = (n / 4) % pages.len();
        let items = pages[page];
        let list = CandidateList {
            items: items.iter().map(|s| s.to_string()).collect(),
            sel_keys: "123456789".chars().take(items.len()).collect(),
            page: page + 1,
            total_pages: pages.len(),
            per_row,
            highlighted: Some(n % 4),
        };
        let style = candidate_window::Style {
            font_size: 20.0,
            font_family: "",
        };
        if let Some(mtm) = MainThreadMarker::new() {
            candidate_window::show(mtm, &list, &style, line);
        }
    };
    tick();
    let block = block2::RcBlock::new(move |_: std::ptr::NonNull<NSTimer>| tick());
    // SAFETY: the block only runs on the main thread's run loop.
    let _timer =
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.8, true, &block) };
    app.run();
    ExitCode::SUCCESS
}

fn info_string(bundle: &NSBundle, key: &NSString) -> Option<Retained<NSString>> {
    bundle
        .objectForInfoDictionaryKey(key)?
        .downcast::<NSString>()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::PrivateLogger;

    #[test]
    fn only_libchewing_targets_are_filtered() {
        assert!(PrivateLogger::is_libchewing("chewing"));
        assert!(PrivateLogger::is_libchewing("chewing::editor"));
        assert!(PrivateLogger::is_libchewing("chewing::conversion::chewing"));
        assert!(!PrivateLogger::is_libchewing("chewing_imk_core::engine"));
        assert!(!PrivateLogger::is_libchewing("ChewingIMK::controller"));
    }
}
