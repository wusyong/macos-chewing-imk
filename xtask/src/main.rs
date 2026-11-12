// SPDX-License-Identifier: GPL-3.0-or-later

//! Development tasks. Run `cargo xtask help`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const APP_NAME: &str = "ChewingIMK.app";
const EXECUTABLE: &str = "ChewingIMK";
const SUBSYSTEM: &str = "im.chewing.inputmethod.Chewing";

const USAGE: &str = "\
usage: cargo xtask <command> [--release]

commands:
  bundle     build ChewingIMK and assemble target/<profile>/ChewingIMK.app
  install    bundle, copy to ~/Library/Input Methods, restart and register it
  uninstall  stop ChewingIMK and remove it from ~/Library/Input Methods
  log        stream the input method's os_log output (debug level)
";

type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let release = args.iter().any(|a| a == "--release");
    let result = match args.first().map(String::as_str) {
        Some("bundle") => bundle(release).map(|app| println!("bundled {}", app.display())),
        Some("install") => install(release),
        Some("uninstall") => uninstall(),
        Some("log") => stream_log(),
        Some("help" | "-h" | "--help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command: {other}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace root")
        .to_path_buf()
}

fn run(cmd: &mut Command) -> Result {
    let status = cmd
        .status()
        .map_err(|e| format!("failed to run {cmd:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} exited with {status}"))
    }
}

/// Build the binary and assemble the .app bundle. Returns the bundle path.
fn bundle(release: bool) -> Result<PathBuf> {
    let root = workspace_root();
    let profile = if release { "release" } else { "debug" };

    let mut build = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    build
        .current_dir(&root)
        .args(["build", "-p", "chewing_imk"]);
    if release {
        build.arg("--release");
    }
    run(&mut build)?;

    let target_dir = root.join("target").join(profile);
    let app = target_dir.join(APP_NAME);
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    let src_resources = root.join("crates/chewing_imk/resources");

    if app.exists() {
        fs::remove_dir_all(&app).map_err(|e| format!("remove {}: {e}", app.display()))?;
    }
    for dir in [&macos, &resources] {
        fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }

    let plist = fs::read_to_string(src_resources.join("Info.plist"))
        .map_err(|e| format!("read Info.plist: {e}"))?
        .replace("@VERSION@", env!("CARGO_PKG_VERSION"));
    fs::write(contents.join("Info.plist"), plist).map_err(|e| format!("write Info.plist: {e}"))?;

    copy(&target_dir.join(EXECUTABLE), &macos.join(EXECUTABLE))?;
    copy(
        &src_resources.join("MenuIcon.tiff"),
        &resources.join("MenuIcon.tiff"),
    )?;

    // Ad-hoc signature so the bundle's resources are sealed; real Developer ID
    // signing comes in Phase 5.
    run(Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(&app))?;
    Ok(app)
}

fn copy(from: &Path, to: &Path) -> Result {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("copy {} -> {}: {e}", from.display(), to.display()))
}

fn input_methods_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join("Library/Input Methods"))
}

/// Stop the running input method, if any. macOS restarts it on demand.
fn stop_running() {
    let _ = Command::new("pkill").args(["-x", EXECUTABLE]).status();
}

fn install(release: bool) -> Result {
    let app = bundle(release)?;
    let dest_dir = input_methods_dir()?;
    let dest = dest_dir.join(APP_NAME);

    stop_running();
    fs::create_dir_all(&dest_dir).map_err(|e| format!("create {}: {e}", dest_dir.display()))?;
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| format!("remove {}: {e}", dest.display()))?;
    }
    run(Command::new("ditto").arg(&app).arg(&dest))?;
    println!("installed {}", dest.display());

    run(Command::new(dest.join("Contents/MacOS").join(EXECUTABLE)).arg("--register"))?;
    println!(
        "done. Pick 新酷音 from the input menu; if it does not show up yet, log out and back in."
    );
    Ok(())
}

fn uninstall() -> Result {
    let dest = input_methods_dir()?.join(APP_NAME);
    stop_running();
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| format!("remove {}: {e}", dest.display()))?;
        println!("removed {}", dest.display());
    } else {
        println!("not installed: {}", dest.display());
    }
    println!("the input menu entry disappears after logging out and back in.");
    Ok(())
}

fn stream_log() -> Result {
    let predicate = format!("subsystem == \"{SUBSYSTEM}\"");
    run(Command::new("/usr/bin/log").args([
        "stream",
        "--style",
        "compact",
        "--level",
        "debug",
        "--predicate",
        &predicate,
    ]))
}
