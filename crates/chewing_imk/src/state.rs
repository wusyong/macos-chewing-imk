// SPDX-License-Identifier: GPL-3.0-or-later

//! The single [`ChewingCore`] shared by every input controller in this
//! process, and the settings file it follows. IMK calls controllers on the
//! main thread only, so a thread-local is enough.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use chewing_imk_core::ChewingCore;
use chewing_imk_core::config::Config;
use chewing_imk_core::config_file::{CONFIG_FILE_NAME, ConfigWatcher};
use log::{error, info, warn};
use objc2_foundation::NSBundle;

struct State {
    core: ChewingCore,
    watcher: Option<ConfigWatcher>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
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
    with_state(|state| f(&mut state.core))
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with_borrow_mut(|slot| {
        if slot.is_none() {
            if INIT_FAILED.get() {
                return None;
            }
            match create_state() {
                Ok(state) => *slot = Some(state),
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

/// The settings file, or `None` if there is no user data directory.
pub fn config_path() -> Option<PathBuf> {
    chewing_imk_core::default_user_dir().map(|dir| dir.join(CONFIG_FILE_NAME))
}

/// Apply the settings file if it changed since we last looked, unless
/// something is being composed (recreating the editor would drop it).
pub fn reload_config_if_changed() {
    with_state(|state| {
        if state.core.is_composing() {
            return;
        }
        let Some(watcher) = state.watcher.as_mut() else {
            return;
        };
        match watcher.reload_if_changed() {
            None => {}
            Some(Ok(cfg)) => match state.core.apply_config(cfg) {
                Ok(()) => info!("settings reloaded from {}", watcher.path().display()),
                Err(e) => error!("failed to apply settings: {e}"),
            },
            // Keep the current settings; the file may be mid-edit.
            Some(Err(e)) => warn!("ignoring {}: {e}", watcher.path().display()),
        }
    });
}

/// Save `cfg` to the settings file and apply it right away.
pub fn save_config(cfg: Config) -> Result<(), String> {
    with_state(|state| {
        let watcher = state.watcher.as_mut().ok_or("no user data directory")?;
        cfg.save(watcher.path()).map_err(|e| e.to_string())?;
        watcher.mark_current();
        state.core.apply_config(cfg).map_err(|e| e.to_string())
    })
    .unwrap_or_else(|| Err("chewing is not available".into()))
}

fn create_state() -> Result<State, String> {
    let resources = NSBundle::mainBundle()
        .resourcePath()
        .ok_or("bundle has no resource path")?;
    let dict_dir = PathBuf::from(resources.to_string()).join("Dictionary");

    let user_dir = chewing_imk_core::default_user_dir();
    if let Some(dir) = &user_dir {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }

    let mut watcher = config_path().map(ConfigWatcher::new);
    let cfg = match watcher.as_mut().and_then(|w| w.reload_if_changed()) {
        Some(Ok(cfg)) => cfg,
        Some(Err(e)) => {
            warn!("ignoring settings file: {e}");
            Config::default()
        }
        None => Config::default(),
    };

    let core = ChewingCore::new(&dict_dir, user_dir.as_deref(), cfg).map_err(|e| e.to_string())?;
    info!(
        "chewing initialized: dictionary={} user={}",
        dict_dir.display(),
        user_dir
            .as_deref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(libchewing default)".into())
    );
    Ok(State { core, watcher })
}
