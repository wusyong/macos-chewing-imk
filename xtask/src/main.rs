// SPDX-License-Identifier: GPL-3.0-or-later

//! Development tasks. Run `cargo xtask help`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const APP_NAME: &str = "ChewingIMK.app";
const EXECUTABLE: &str = "ChewingIMK";
const SUBSYSTEM: &str = "im.chewing.inputmethod.Chewing";

/// Pinned dictionary release, same as windows-chewing-tsf's
/// nix/libchewing-data.nix. Bump the version and hash together.
const DATA_NAME: &str = "libchewing-data-2026.11.1-Generic";
const DATA_URL: &str = "https://codeberg.org/chewing/libchewing-data/releases/download/v2026.11.1/libchewing-data-2026.11.1-Generic.zip";
const DATA_SHA256: &str = "ee78b22bcaab85d29004e8aa33c25e9e885b01e0f48573b017566b6168803e91";
const DATA_FILES: &[&str] = &[
    "rare_dict.bin",
    "static_dict.bin",
    "static_lm.bin",
    "static_words.bin",
    "swkb.dat",
    "symbols.dat",
];

const USAGE: &str = "\
usage: cargo xtask <command> [--release]

commands:
  fetch-data download and verify the pinned libchewing-data dictionary
  test       fetch-data, then run the workspace tests against it
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
        Some("fetch-data") => fetch_data().map(|dir| println!("dictionary: {}", dir.display())),
        Some("test") => test(),
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

fn cargo() -> Command {
    Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
}

/// Download, verify and unpack the pinned dictionary into target/data, unless
/// it is already there. Returns the directory holding the dictionary files.
fn fetch_data() -> Result<PathBuf> {
    let data_root = workspace_root().join("target/data");
    let dict_dir = data_root.join(DATA_NAME).join("share/libchewing");
    if DATA_FILES.iter().all(|f| dict_dir.join(f).is_file()) {
        return Ok(dict_dir);
    }

    fs::create_dir_all(&data_root).map_err(|e| format!("create {}: {e}", data_root.display()))?;
    let zip = data_root.join(format!("{DATA_NAME}.zip"));
    println!("downloading {DATA_URL}");
    run(Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&zip)
        .arg(DATA_URL))?;

    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(&zip)
        .output()
        .map_err(|e| format!("failed to run shasum: {e}"))?;
    let actual = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned();
    if actual != DATA_SHA256 {
        let _ = fs::remove_file(&zip);
        return Err(format!(
            "sha256 mismatch for {DATA_URL}\n  expected {DATA_SHA256}\n  actual   {actual}"
        ));
    }

    run(Command::new("ditto")
        .args(["-x", "-k"])
        .arg(&zip)
        .arg(&data_root))?;
    let _ = fs::remove_file(&zip);
    for f in DATA_FILES {
        if !dict_dir.join(f).is_file() {
            return Err(format!("{f} missing from {DATA_NAME}"));
        }
    }
    Ok(dict_dir)
}

fn test() -> Result {
    let dict_dir = fetch_data()?;
    run(cargo()
        .current_dir(workspace_root())
        .args(["test", "--workspace"])
        .env("CHEWING_TEST_DATA_DIR", dict_dir))
}

/// Build the binary and assemble the .app bundle. Returns the bundle path.
fn bundle(release: bool) -> Result<PathBuf> {
    let dict_dir = fetch_data()?;
    let root = workspace_root();
    let profile = if release { "release" } else { "debug" };

    let mut build = cargo();
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
    let dictionary = resources.join("Dictionary");
    let src_resources = root.join("crates/chewing_imk/resources");

    if app.exists() {
        fs::remove_dir_all(&app).map_err(|e| format!("remove {}: {e}", app.display()))?;
    }
    for dir in [&macos, &resources, &dictionary] {
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
    for f in DATA_FILES {
        copy(&dict_dir.join(f), &dictionary.join(f))?;
    }

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
