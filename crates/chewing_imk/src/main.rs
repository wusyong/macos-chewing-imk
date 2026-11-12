// SPDX-License-Identifier: GPL-3.0-or-later

//! ChewingIMK: the macOS input method process.
//!
//! Run without arguments, it is started by macOS as the input method server.
//! Run with `--register`, it registers and enables its own bundle as an input
//! source (used by `cargo xtask install`).

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
        Some(other) => {
            eprintln!("unknown argument: {other}");
            eprintln!("usage: ChewingIMK [--register | --unregister | --status]");
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
