// SPDX-License-Identifier: GPL-3.0-or-later

//! Development tasks. Run `cargo xtask help`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod install;

pub(crate) const APP_NAME: &str = "ChewingIMK.app";
pub(crate) const EXECUTABLE: &str = "ChewingIMK";
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
usage: cargo xtask <command> [--release] [--purge]

commands:
  fetch-data download and verify the pinned libchewing-data dictionary
  test       fetch-data, then run the workspace tests against it
  bundle     build ChewingIMK and assemble target/<profile>/bundle.noindex/ChewingIMK.app
  install    bundle, install or update in ~/Library/Input Methods, enable it
  uninstall  disable and remove the input method (--purge: also settings and
             user dictionary)
  log        stream the input method's os_log output (debug level)
";

pub(crate) type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let release = args.iter().any(|a| a == "--release");
    let result = match args.first().map(String::as_str) {
        Some("fetch-data") => fetch_data().map(|dir| println!("dictionary: {}", dir.display())),
        Some("test") => test(),
        Some("bundle") => bundle(release).map(|app| println!("bundled {}", app.display())),
        Some("install") => install::install(release),
        Some("uninstall") => install::uninstall(args.iter().any(|a| a == "--purge")),
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

pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace root")
        .to_path_buf()
}

pub(crate) fn run(cmd: &mut Command) -> Result {
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

/// Where `bundle` assembles the app. The `.noindex` suffix keeps Spotlight
/// out, so LaunchServices does not register this copy as a second app with
/// our bundle identifier; only the installed one should be known.
pub(crate) fn bundle_dir(profile: &str) -> PathBuf {
    workspace_root()
        .join("target")
        .join(profile)
        .join("bundle.noindex")
}

/// Build the binary and assemble the .app bundle. Returns the bundle path.
pub(crate) fn bundle(release: bool) -> Result<PathBuf> {
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
    let app = bundle_dir(profile).join(APP_NAME);
    // Older builds put the bundle next to the binary, where macOS indexes
    // it and registers a second app with our bundle ID.
    let legacy = target_dir.join(APP_NAME);
    if legacy.exists() {
        let _ = Command::new(install::LSREGISTER)
            .arg("-u")
            .arg(&legacy)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        fs::remove_dir_all(&legacy).map_err(|e| format!("remove {}: {e}", legacy.display()))?;
    }
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
    // Localized names of the bundle and its input mode.
    for lang in ["zh_TW.lproj", "en.lproj"] {
        let dir = resources.join(lang);
        fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        copy(
            &src_resources.join(lang).join("InfoPlist.strings"),
            &dir.join("InfoPlist.strings"),
        )?;
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
