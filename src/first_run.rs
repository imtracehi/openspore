//! First-run setup: link the player's own Spore install.
//!
//! OpenSpore ships no game files. When the windowed game starts and no Spore
//! install is found (or with `--choose-spore`, the "OpenSpore - change Spore
//! folder" shortcut), a message box explains that, a folder picker returns
//! the Spore folder, the folder is validated (the install root, its `Data`
//! folder or `SporebinEP1` are accepted and mapped to the root) and stored in
//! `settings.json` (see `user_paths`); then the game starts. A rejected folder
//! is explained ("Data\Spore_Game.package not found") with Retry / Cancel.
//!
//! No extra crate: the message boxes are `MessageBoxW`; the picker is
//! `System.Windows.Forms.FolderBrowserDialog` run through Windows PowerShell
//! (`powershell -NoProfile -STA -Command`), which prints the folder on stdout.
//! Never shown for headless runs (`--smoke-test`, `--quick-capture`,
//! `--render-smoke` return before this; `--no-dialogs` /
//! `OPENSPORE_NO_DIALOGS` also skip it), for an explicit `--spore-path`
//! (its error is kept) or in tests; other systems print the error instead.
//! `--detect-spore` prints what discovery sees and exits.
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

use std::path::{Path, PathBuf};

use crate::spore_install::{self, Probe};
use crate::user_paths::{self, UserPaths};

pub enum Outcome {
    Continue,
    Quit,
}

const HEADLESS_FLAGS: &[&str] = &["--smoke-test", "--quick-capture", "--render-smoke", "--no-dialogs"];

fn dialogs_disabled(args: &[String]) -> bool {
    args.iter().any(|a| HEADLESS_FLAGS.contains(&a.as_str()))
        || std::env::var_os("OPENSPORE_NO_DIALOGS").is_some_and(|v| !v.is_empty() && v != "0")
}

/// Run before the game window opens. `Quit` when the player cancelled the
/// setup without a usable install.
pub fn before_window() -> Outcome {
    let outcome = decide();
    if let Outcome::Continue = outcome {
        // The save folder may be new (`%APPDATA%\OpenSpore\saves`); the
        // settings / profile / Sporepedia files beside the save expect it.
        if let Some(dir) = crate::game::RunProgress::save_path().parent() {
            if !dir.as_os_str().is_empty() {
                if let Err(error) = std::fs::create_dir_all(dir) {
                    eprintln!("cannot create the save folder {}: {error}", dir.display());
                }
            }
        }
    }
    outcome
}

fn decide() -> Outcome {
    let args: Vec<String> = std::env::args().collect();
    let paths = UserPaths::from_process();
    println!("OpenSpore {} | {}", env!("CARGO_PKG_VERSION"), paths.describe());
    let forced = args.iter().any(|a| a == "--choose-spore");
    let found = spore_install::discover_with(&Probe::from_process());
    let current = match &found {
        Ok((install, source)) => {
            println!(
                "Spore install: {} (from {})",
                install.root.display(),
                source.label()
            );
            if !forced {
                return Outcome::Continue;
            }
            Some(install.root.clone())
        }
        Err(error) => {
            eprintln!("{error}");
            if !forced && spore_install::cli_spore_path(&args).is_some() {
                // An explicit --spore-path keeps today's behaviour: the game
                // starts and shows the problem.
                return Outcome::Continue;
            }
            None
        }
    };
    if dialogs_disabled(&args) {
        println!("first-run setup: skipped (no dialogs in this mode)");
        return Outcome::Continue;
    }
    setup(current, forced, &paths)
}

#[cfg(not(windows))]
fn setup(current: Option<PathBuf>, _forced: bool, _paths: &UserPaths) -> Outcome {
    // ADAPTED: no native dialog without a GUI toolkit crate; the game starts
    // and its HUD keeps the "not found" message.
    eprintln!("Pass --spore-path <your Spore folder> or set OPENSPORE_SPORE_PATH.");
    let _ = current;
    Outcome::Continue
}

#[cfg(windows)]
fn setup(current: Option<PathBuf>, forced: bool, paths: &UserPaths) -> Outcome {
    use windows::Win32::UI::WindowsAndMessaging::{
        IDOK, IDRETRY, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_OKCANCEL,
        MB_RETRYCANCEL,
    };
    let keep = current.is_some();
    let cancel_word = if keep { "Cancel keeps the current folder." } else { "Cancel quits." };
    let intro = match &current {
        Some(root) => format!(
            "Choose the Spore installation OpenSpore should use.\n\nCurrent folder: {}\n\n\
             OpenSpore reads the game files of your own Spore install (Steam, EA app / Origin, \
             GOG or disc) and never changes them.\n\nClick OK to choose a folder. {cancel_word}",
            root.display()
        ),
        None => format!(
            "OpenSpore needs your own installed copy of Spore.\n\n\
             OpenSpore includes no game files. It reads the game files of your Spore install \
             (Steam, EA app / Origin, GOG or disc) and never changes them, but it could not \
             find Spore on this computer.\n\n\
             Click OK and choose your Spore folder: the one that contains the Data folder, \
             for example ...\\steamapps\\common\\Spore or \
             C:\\Program Files (x86)\\Electronic Arts\\SPORE. {cancel_word}"
        ),
    };
    println!(
        "first-run setup: {}; showing the setup message box",
        if forced { "--choose-spore" } else { "no Spore install found" }
    );
    if message_box(&intro, MB_OKCANCEL | MB_ICONINFORMATION) != IDOK {
        return cancelled(keep);
    }
    let mut start = current.clone().or_else(default_start_folder);
    loop {
        println!("first-run setup: opening the folder picker");
        let picked = match pick_folder(start.as_deref()) {
            Ok(Some(folder)) => folder,
            Ok(None) => return cancelled(keep),
            Err(error) => {
                eprintln!("first-run setup: folder picker failed: {error}");
                message_box(
                    &format!(
                        "The folder picker could not be opened ({error}).\n\n\
                         Start OpenSpore with --spore-path \"<your Spore folder>\" instead."
                    ),
                    MB_OK | MB_ICONERROR,
                );
                return cancelled(keep);
            }
        };
        println!("first-run setup: picked {}", picked.display());
        match spore_install::install_root_for(&picked) {
            Ok(root) => {
                spore_install::set_session_root(root.clone());
                let settings = paths.settings_path();
                match user_paths::write_spore_path(&settings, &root) {
                    Ok(()) => println!(
                        "first-run setup: using {} (saved to {})",
                        root.display(),
                        settings.display()
                    ),
                    Err(error) => {
                        eprintln!("first-run setup: cannot write {}: {error}", settings.display());
                        message_box(
                            &format!(
                                "OpenSpore will use {} now, but could not remember it for next time:\n{}: {error}",
                                root.display(),
                                settings.display()
                            ),
                            MB_OK | MB_ICONWARNING,
                        );
                    }
                }
                return Outcome::Continue;
            }
            Err(problem) => {
                println!("first-run setup: rejected {}: {problem}", picked.display());
                let mut text = problem.to_string();
                if let Some(first) = text.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                let text = format!(
                    "{text}.\n\nOpenSpore needs the Spore folder whose Data folder holds \
                     Spore_Game.package and Spore_Graphics.package.\n\n\
                     Retry to choose another folder. {cancel_word}"
                );
                if message_box(&text, MB_RETRYCANCEL | MB_ICONWARNING) != IDRETRY {
                    return cancelled(keep);
                }
                start = Some(picked);
            }
        }
    }
}

fn cancelled(keep_current: bool) -> Outcome {
    if keep_current {
        println!("first-run setup: cancelled; keeping the current Spore folder");
        Outcome::Continue
    } else {
        println!("first-run setup: cancelled without a Spore folder; quitting");
        Outcome::Quit
    }
}

#[cfg(windows)]
fn default_start_folder() -> Option<PathBuf> {
    let x86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from)?;
    [
        x86.join("Steam").join("steamapps").join("common"),
        x86.join("Electronic Arts"),
        x86.clone(),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

#[cfg(windows)]
fn message_box(
    text: &str,
    style: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE,
) -> windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_RESULT {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_SETFOREGROUND, MB_TOPMOST};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let text = wide(text);
    let caption = wide("OpenSpore setup");
    // SAFETY: both buffers are NUL-terminated UTF-16 and outlive the call.
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(caption.as_ptr()),
            style | MB_SETFOREGROUND | MB_TOPMOST,
        )
    }
}

/// The folder picker script: no double quotes (it travels as one argument);
/// the start folder comes in through an environment variable, never pasted
/// into the script; the result is written to stdout as UTF-8 bytes.
const PICKER_SCRIPT: &str = "Add-Type -AssemblyName System.Windows.Forms; \
[System.Windows.Forms.Application]::EnableVisualStyles(); \
$d = New-Object System.Windows.Forms.FolderBrowserDialog; \
$d.Description = 'Choose your Spore folder (the one that contains the Data folder)'; \
$d.ShowNewFolderButton = $false; \
$s = $env:OPENSPORE_PICKER_START; \
if ($s -and (Test-Path -LiteralPath $s -PathType Container)) { $d.SelectedPath = $s }; \
$owner = New-Object System.Windows.Forms.Form; $owner.TopMost = $true; \
$r = $d.ShowDialog($owner); $owner.Dispose(); \
if ($r -eq [System.Windows.Forms.DialogResult]::OK) { \
$b = [System.Text.Encoding]::UTF8.GetBytes($d.SelectedPath); \
$o = [Console]::OpenStandardOutput(); $o.Write($b, 0, $b.Length); $o.Flush() }";

#[cfg(windows)]
fn pick_folder(start: Option<&Path>) -> std::io::Result<Option<PathBuf>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let powershell = std::env::var_os("SystemRoot")
        .map(|root| {
            PathBuf::from(root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe")
        })
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    let mut cmd = std::process::Command::new(powershell);
    cmd.args(["-NoProfile", "-NonInteractive", "-STA", "-Command", PICKER_SCRIPT])
        .env("OPENSPORE_PICKER_START", start.map(Path::as_os_str).unwrap_or_default())
        .stdin(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "powershell exited with {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(picker_result(&out.stdout))
}

/// The folder printed by the picker; nothing printed = cancelled.
fn picker_result(stdout: &[u8]) -> Option<PathBuf> {
    let text = String::from_utf8_lossy(stdout);
    let text = text.trim_start_matches('\u{feff}').trim();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

/// `--detect-spore`: print the user folders, the registry findings and the
/// discovery result. Exit code 0 when an install was found.
pub fn detect() -> i32 {
    let paths = UserPaths::from_process();
    println!("OpenSpore {}", env!("CARGO_PKG_VERSION"));
    println!("{}", paths.describe());
    let registry = spore_install::registry_findings();
    for dir in &registry.spore_dirs {
        println!("registry Spore folder: {}", dir.display());
    }
    for dir in &registry.steam_dirs {
        println!("registry Steam folder: {}", dir.display());
    }
    match spore_install::discover_with(&Probe::from_process()) {
        Ok((install, source)) => {
            println!(
                "Spore install: {} (from {})",
                install.describe(),
                source.label()
            );
            0
        }
        Err(error) => {
            println!("{error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_output_is_trimmed_and_empty_means_cancelled() {
        assert_eq!(picker_result(b""), None);
        assert_eq!(picker_result(b"\r\n"), None);
        assert_eq!(
            picker_result("\u{feff}D:\\Spiele\\Spore Ä\r\n".as_bytes()),
            Some(PathBuf::from("D:\\Spiele\\Spore Ä"))
        );
    }

    #[test]
    fn the_picker_script_travels_as_one_argument() {
        assert!(!PICKER_SCRIPT.contains('"'));
        assert!(PICKER_SCRIPT.contains("FolderBrowserDialog"));
    }

    #[test]
    fn capture_and_smoke_runs_never_open_dialogs() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(dialogs_disabled(&args(&["openspore", "--smoke-test"])));
        assert!(dialogs_disabled(&args(&["openspore", "--quick-capture", "out", "cell"])));
        assert!(dialogs_disabled(&args(&["openspore", "--render-smoke", "out"])));
        assert!(dialogs_disabled(&args(&["openspore", "--no-dialogs"])));
    }
}
