// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask install` and `cargo xtask uninstall`.
//!
//! How it works with macOS (see `crates/chewing_imk/src/register.rs` for
//! the details we tested):
//!
//! - **Fresh install** needs the user twice, and only the first time:
//!   1. allow the input method in the prompt macOS shows
//!      ("要允許「新酷音」啟用「新酷音」嗎？"), and
//!   2. add its input mode in System Settings (+ > 繁體中文 > 新酷音), which
//!      we open for them. Only System Settings can put it in the input menu.
//! - **Update** replaces the bundle and stops the running input method; the
//!   next keystroke starts the new binary. Nothing is asked. Names, icons and
//!   input modes (Info.plist, icon, InfoPlist.strings) are cached by macOS
//!   until the user logs out; we say so when one of them changed.
//! - **Uninstall** first has the user remove 新酷音 in System Settings (only
//!   that clears it from the input menu lists), then disables what is left,
//!   removes the bundle and its LaunchServices record. User data stays unless
//!   `--purge` is given.

use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{APP_NAME, EXECUTABLE, Result, bundle, bundle_dir, run};

/// Files whose content macOS caches until the user logs out.
const CACHED_BY_MACOS: &[&str] = &[
    "Contents/Info.plist",
    "Contents/Resources/MenuIcon.tiff",
    "Contents/Resources/zh_TW.lproj/InfoPlist.strings",
    "Contents/Resources/en.lproj/InfoPlist.strings",
];

pub(crate) const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

/// System Settings > Keyboard, where Input Sources are edited.
const KEYBOARD_SETTINGS: &str = "x-apple.systempreferences:com.apple.Keyboard-Settings.extension";

/// The part of a file macOS caches. For Info.plist the version numbers are
/// left out: a new version alone changes nothing in the input menu.
fn cached_content(path: &Path) -> Option<Vec<u8>> {
    let bytes = fs::read(path).ok()?;
    if path.file_name().is_some_and(|n| n == "Info.plist") {
        let text = String::from_utf8_lossy(&bytes);
        let mut skip_next = false;
        let kept: Vec<&str> = text
            .lines()
            .filter(|line| {
                let skip = skip_next;
                skip_next = line.contains("<key>CFBundleVersion</key>")
                    || line.contains("<key>CFBundleShortVersionString</key>");
                !skip
            })
            .collect();
        return Some(kept.join("\n").into_bytes());
    }
    Some(bytes)
}

fn input_methods_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join("Library/Input Methods"))
}

fn installed_app() -> Result<PathBuf> {
    Ok(input_methods_dir()?.join(APP_NAME))
}

fn executable(app: &Path) -> PathBuf {
    app.join("Contents/MacOS").join(EXECUTABLE)
}

/// Update (`-f`) or remove (`-u`) a LaunchServices record. Its output is
/// noise (e.g. Spotlight scan errors for build folders), so it is hidden.
fn lsregister(flag: &str, app: &Path) {
    let _ = Command::new(LSREGISTER)
        .arg(flag)
        .arg(app)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Forget the build copies (target/*/bundle.noindex/ChewingIMK.app). They
/// carry the same bundle identifier, and macOS must only ever find the
/// installed one.
fn forget_build_copies() {
    for profile in ["debug", "release"] {
        let copy = bundle_dir(profile).join(APP_NAME);
        if copy.exists() {
            lsregister("-u", &copy);
        }
    }
}

/// Stop the running input method, if any. macOS starts it again on demand.
fn stop_running() {
    let _ = Command::new("/usr/bin/pkill")
        .args(["-x", EXECUTABLE])
        .status();
}

/// What `ChewingIMK --status` reports.
struct Status {
    /// The user allowed the input method.
    allowed: bool,
    /// The input mode is in the input menu.
    added: bool,
}

fn status(app: &Path) -> Result<Status> {
    let out = Command::new(executable(app))
        .arg("--status")
        .output()
        .map_err(|e| format!("run --status: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(Status {
        allowed: text.lines().any(|l| l == "allowed"),
        added: text.lines().any(|l| l.starts_with("added ")),
    })
}

fn open_keyboard_settings() {
    let _ = Command::new("/usr/bin/open")
        .arg(KEYBOARD_SETTINGS)
        .status();
}

/// Wait until `done()` is true, asking the user to press Return after each
/// try; `not_yet` is shown when they pressed Return too early. Returns false
/// if the user typed `s` to skip.
fn wait_until(not_yet: &str, done: impl Fn() -> Result<bool>) -> Result<bool> {
    loop {
        print!("Press Return when done (s + Return to skip)… ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if io::stdin().lock().read_line(&mut line).unwrap_or(0) == 0 {
            return Ok(false); // stdin closed
        }
        if line.trim().eq_ignore_ascii_case("s") {
            return Ok(false);
        }
        if done()? {
            return Ok(true);
        }
        println!("{not_yet}");
    }
}

pub fn install(release: bool) -> Result {
    let built = bundle(release)?;
    let dest_dir = input_methods_dir()?;
    let dest = installed_app()?;

    let updating = dest.exists();
    let cache_changed: Vec<&str> = if updating {
        CACHED_BY_MACOS
            .iter()
            .copied()
            .filter(|f| cached_content(&dest.join(f)) != cached_content(&built.join(f)))
            .collect()
    } else {
        Vec::new()
    };

    // Replace the bundle. The running copy keeps its open files until it
    // exits; the next use of the input method starts the new binary.
    stop_running();
    fs::create_dir_all(&dest_dir).map_err(|e| format!("create {}: {e}", dest_dir.display()))?;
    if updating {
        fs::remove_dir_all(&dest).map_err(|e| format!("remove {}: {e}", dest.display()))?;
    }
    run(Command::new("/usr/bin/ditto").arg(&built).arg(&dest))?;
    // Make LaunchServices read the new bundle now rather than lazily, and
    // forget the copies left in target/.
    lsregister("-f", &dest);
    forget_build_copies();
    println!(
        "{} {}",
        if updating { "updated" } else { "installed" },
        dest.display()
    );

    let interactive = io::stdin().is_terminal();

    // 1. Let macOS know the bundle, and ask for the user's permission once.
    let out = Command::new(executable(&dest))
        .arg("--register")
        .output()
        .map_err(|e| format!("run --register: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "--register failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    if String::from_utf8_lossy(&out.stdout).contains("requested: ") {
        println!("\nStep 1/2 — macOS asks 「要允許「新酷音」啟用「新酷音」嗎？」: click 允許.");
        if !interactive {
            println!("Then run `cargo xtask install` again.");
            return Ok(());
        }
        wait_until("Not allowed yet: click 允許 in System Settings.", || {
            Ok(status(&dest)?.allowed)
        })?;
    }

    // 2. Have the user add the input mode; only System Settings can.
    if !status(&dest)?.added {
        println!(
            "\nStep 2/2 — add 新酷音 to your input sources. Opening System Settings > Keyboard:\n  \
             輸入方式「編輯…」> + > 繁體中文 > 新酷音 > 加入 > 完成\n  \
             (If 新酷音 is grey in the list, select it, click −, then add it again.)"
        );
        open_keyboard_settings();
        if !interactive {
            println!("Then run `cargo xtask install` again to check.");
            return Ok(());
        }
        wait_until("新酷音 is not added yet.", || Ok(status(&dest)?.added))?;
    }

    let st = status(&dest)?;
    if st.added {
        println!("\n新酷音 is ready: pick it in the input menu.");
    } else if !st.allowed {
        println!("\n新酷音 was not allowed. Run `cargo xtask install` again to be asked again.");
    } else {
        println!(
            "\n新酷音 is not in your input sources yet. Add it in System Settings > Keyboard >\n\
             輸入方式「編輯…」> + > 繁體中文 > 新酷音, or run `cargo xtask install` again."
        );
    }
    if !cache_changed.is_empty() {
        println!(
            "\nmacOS caches these until you log out: {}\n\
             The new code is already in use, but the input menu shows the old name, icon or\n\
             input modes until you log out and back in.",
            cache_changed.join(", ")
        );
    }
    Ok(())
}

pub fn uninstall(purge: bool) -> Result {
    let dest = installed_app()?;
    if dest.exists() {
        // Only System Settings can take 新酷音 out of the input menu lists,
        // and only while the bundle still exists.
        if status(&dest)?.added {
            println!(
                "Remove 新酷音 from your input sources first. Opening System Settings > Keyboard:\n  \
                 輸入方式「編輯…」> select 新酷音 > − > 完成"
            );
            open_keyboard_settings();
            if !io::stdin().is_terminal() {
                println!("Then run `cargo xtask uninstall` again.");
                return Ok(());
            }
            let removed = wait_until(
                "新酷音 is still in your input sources: select it and click −.",
                || Ok(!status(&dest)?.added),
            )?;
            if !removed {
                println!(
                    "note: skipped; 新酷音 stays listed until you remove it in System Settings."
                );
            }
        }
        // Disable what is left while the bundle (and so its sources) exists.
        run(Command::new(executable(&dest)).arg("--unregister"))?;
        stop_running();
        lsregister("-u", &dest);
        forget_build_copies();
        fs::remove_dir_all(&dest).map_err(|e| format!("remove {}: {e}", dest.display()))?;
        println!("removed {}", dest.display());
    } else {
        println!("not installed: {}", dest.display());
        // Still clean up preference entries left by an earlier install,
        // using a build copy (same bundle identifier) to run the cleanup.
        let copy = ["debug", "release"]
            .iter()
            .map(|p| bundle_dir(p).join(APP_NAME))
            .find(|p| executable(p).exists());
        if let Some(copy) = copy {
            run(Command::new(executable(&copy)).arg("--unregister"))?;
            forget_build_copies();
        }
    }

    if purge {
        let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
        let data = PathBuf::from(home).join("Library/Application Support/im.chewing.Chewing");
        if data.exists() {
            fs::remove_dir_all(&data).map_err(|e| format!("remove {}: {e}", data.display()))?;
            println!("removed settings and user dictionary: {}", data.display());
        }
    } else {
        println!(
            "kept settings and user dictionary in ~/Library/Application Support/im.chewing.Chewing\n\
             (use `cargo xtask uninstall --purge` to remove them too)"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::cached_content;

    #[test]
    fn version_bump_is_not_a_cached_change() {
        let dir = std::env::temp_dir().join(format!("xtask-plist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let template = std::fs::read_to_string(
            crate::workspace_root().join("crates/chewing_imk/resources/Info.plist"),
        )
        .unwrap();
        let (a, b) = (dir.join("a/Info.plist"), dir.join("b/Info.plist"));
        for (path, version) in [(&a, "0.1.0"), (&b, "0.1.1")] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, template.replace("@VERSION@", version)).unwrap();
        }
        assert_eq!(cached_content(&a), cached_content(&b));
        // A real change is still detected.
        std::fs::write(&b, template.replace("新酷音", "新酷音2")).unwrap();
        assert_ne!(cached_content(&a), cached_content(&b));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
