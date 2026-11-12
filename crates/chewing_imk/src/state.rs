// SPDX-License-Identifier: GPL-3.0-or-later

//! The single [`ChewingCore`] shared by every input controller in this
//! process. IMK calls controllers on the main thread only, so a thread-local
//! is enough.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use chewing_imk_core::ChewingCore;
use chewing_imk_core::config::Config;
use log::{error, info};
use objc2_foundation::NSBundle;

thread_local! {
    static CORE: RefCell<Option<ChewingCore>> = const { RefCell::new(None) };
    static INIT_FAILED: Cell<bool> = const { Cell::new(false) };
}

/// Run `f` with the shared core, creating it on first use. Returns `None` if
/// the core could not be created (e.g. the dictionary is missing); callers
/// should then let keys through untouched.
///
/// Do not call back into the client from inside `f`: collect the actions and
/// apply them after this returns, so a re-entrant call from the client cannot
/// hit a borrowed `RefCell`.
pub fn with_core<R>(f: impl FnOnce(&mut ChewingCore) -> R) -> Option<R> {
    CORE.with_borrow_mut(|slot| {
        if slot.is_none() {
            if INIT_FAILED.get() {
                return None;
            }
            match create_core() {
                Ok(core) => *slot = Some(core),
                Err(e) => {
                    error!("failed to initialize chewing: {e}");
                    INIT_FAILED.set(true);
                    return None;
                }
            }
        }
        slot.as_mut().map(f)
    })
}

fn create_core() -> Result<ChewingCore, String> {
    let resources = NSBundle::mainBundle()
        .resourcePath()
        .ok_or("bundle has no resource path")?;
    let dict_dir = PathBuf::from(resources.to_string()).join("Dictionary");

    let user_dir = chewing_imk_core::default_user_dir();
    if let Some(dir) = &user_dir {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }

    let core = ChewingCore::new(&dict_dir, user_dir.as_deref(), Config::default())
        .map_err(|e| e.to_string())?;
    info!(
        "chewing initialized: dictionary={} user={}",
        dict_dir.display(),
        user_dir
            .as_deref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(libchewing default)".into())
    );
    Ok(core)
}
