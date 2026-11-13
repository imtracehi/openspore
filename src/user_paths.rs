//! Where OpenSpore keeps the player's own files.
//!
//! OpenSpore may be installed under a folder the player cannot write (for
//! example Program Files), so nothing is written next to the exe unless the
//! player asked for that:
//! - settings (`settings.json`: the linked Spore folder): `%APPDATA%\OpenSpore\`
//! - saves: `%APPDATA%\OpenSpore\saves\openspore_save.json`, unless
//!   `--save-path` is given (handled by `RunProgress::save_path`), a save
//!   already exists next to the exe (back-compat: it stays where it is and
//!   keeps being used; nothing is moved, renamed or deleted), or portable mode
//! - runtime files (the extracted icon, caches): `%LOCALAPPDATA%\OpenSpore\`
//!
//! Portable mode (a `portable.txt` file next to the exe): settings and saves
//! next to the exe, runtime files in `<exe dir>\cache\`.
//! `OPENSPORE_USER_DIR=<dir>` stands in for `%APPDATA%\OpenSpore` (settings,
//! `saves\`) and `%LOCALAPPDATA%\OpenSpore` (`<dir>\local\`); manual test runs
//! use it so they never touch the machine's real user folders, and test builds
//! default it to `%TEMP%\openspore-test-user`.
//! On other systems the XDG config / cache folders take the place of
//! `%APPDATA%` / `%LOCALAPPDATA%`.
//! None of these is ever inside the Spore install.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::game::SAVE_FILE;

pub const SETTINGS_FILE: &str = "settings.json";
pub const PORTABLE_MARKER: &str = "portable.txt";
pub const USER_DIR_ENV: &str = "OPENSPORE_USER_DIR";
const APP_DIR: &str = "OpenSpore";

/// The inputs that decide every location; `from_process` reads them from the
/// running process, tests build them from temporary folders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserPaths {
    /// Folder holding the OpenSpore executable.
    pub exe_dir: Option<PathBuf>,
    /// `OPENSPORE_USER_DIR`.
    pub user_dir_override: Option<PathBuf>,
    /// `%APPDATA%` (XDG config home elsewhere).
    pub roaming: Option<PathBuf>,
    /// `%LOCALAPPDATA%` (XDG cache home elsewhere).
    pub local: Option<PathBuf>,
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl UserPaths {
    pub fn from_process() -> Self {
        let home = || env_dir("HOME");
        let (roaming, local) = if cfg!(windows) {
            (env_dir("APPDATA"), env_dir("LOCALAPPDATA"))
        } else {
            (
                env_dir("XDG_CONFIG_HOME").or_else(|| home().map(|h| h.join(".config"))),
                env_dir("XDG_CACHE_HOME").or_else(|| home().map(|h| h.join(".cache"))),
            )
        };
        Self {
            exe_dir: std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf)),
            // Test builds never touch the machine's real user folders.
            user_dir_override: env_dir(USER_DIR_ENV).or_else(|| {
                cfg!(test).then(|| std::env::temp_dir().join("openspore-test-user"))
            }),
            roaming,
            local,
        }
    }

    /// `portable.txt` next to the exe: everything stays next to the exe.
    pub fn portable(&self) -> bool {
        self.exe_dir
            .as_ref()
            .is_some_and(|d| d.join(PORTABLE_MARKER).is_file())
    }

    fn exe_dir_or_cwd(&self) -> PathBuf {
        self.exe_dir.clone().unwrap_or_else(|| PathBuf::from("."))
    }

    /// Folder of `settings.json` (and, outside portable mode, `saves\`).
    pub fn config_dir(&self) -> PathBuf {
        if self.portable() {
            return self.exe_dir_or_cwd();
        }
        if let Some(dir) = &self.user_dir_override {
            return dir.clone();
        }
        match &self.roaming {
            Some(roaming) => roaming.join(APP_DIR),
            None => self.exe_dir_or_cwd(),
        }
    }

    pub fn settings_path(&self) -> PathBuf {
        self.config_dir().join(SETTINGS_FILE)
    }

    /// Folder for files OpenSpore writes while running (extracted icon,
    /// caches). Never the exe folder itself outside portable mode.
    pub fn runtime_dir(&self) -> PathBuf {
        if self.portable() {
            return self.exe_dir_or_cwd().join("cache");
        }
        if let Some(dir) = &self.user_dir_override {
            return dir.join("local");
        }
        match &self.local {
            Some(local) => local.join(APP_DIR),
            None => self.config_dir().join("cache"),
        }
    }

    /// The save used when `--save-path` is not given.
    pub fn default_save_path(&self) -> PathBuf {
        if let Some(exe_dir) = &self.exe_dir {
            let beside = exe_dir.join(SAVE_FILE);
            // Portable mode, or a save (or its backup) an earlier build wrote
            // next to the exe: keep using it in place.
            if self.portable()
                || beside.is_file()
                || crate::persistence::backup_path(&beside).is_file()
            {
                return beside;
            }
        }
        if self.exe_dir.is_none() && self.user_dir_override.is_none() && self.roaming.is_none() {
            return PathBuf::from(SAVE_FILE);
        }
        self.config_dir().join("saves").join(SAVE_FILE)
    }

    /// One line for the log / bug reports.
    pub fn describe(&self) -> String {
        format!(
            "user files: settings {} | default save {} | runtime files {}{}",
            self.settings_path().display(),
            self.default_save_path().display(),
            self.runtime_dir().display(),
            if self.portable() { " (portable mode)" } else { "" }
        )
    }
}

/// The Spore folder stored in a settings file, if any.
pub fn read_spore_path(settings: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(settings).ok()?;
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let path = value.get("spore_path")?.as_str()?.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Store the Spore folder in a settings file, keeping any other keys, by
/// writing a sibling temporary file and renaming it over the old one.
pub fn write_spore_path(settings: &Path, spore_root: &Path) -> std::io::Result<()> {
    let mut value = std::fs::read_to_string(settings)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t.trim_start_matches('\u{feff}')).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    value["spore_path"] = serde_json::Value::String(spore_root.to_string_lossy().into_owned());
    if let Some(parent) = settings.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(&value).map_err(std::io::Error::other)?;
    let mut temp = settings.as_os_str().to_os_string();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, settings)
}

/// Write a runtime file (see [`UserPaths::runtime_dir`]); returns its path.
pub fn write_runtime_file(name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let dir = UserPaths::from_process().runtime_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fresh folder under the system temp dir, removed on drop.
    pub(crate) struct TempDir(pub PathBuf);
    impl TempDir {
        pub fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "openspore-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn layout(tmp: &TempDir) -> UserPaths {
        let exe_dir = tmp.0.join("Programs").join("OpenSpore");
        std::fs::create_dir_all(&exe_dir).unwrap();
        UserPaths {
            exe_dir: Some(exe_dir),
            user_dir_override: None,
            roaming: Some(tmp.0.join("Roaming")),
            local: Some(tmp.0.join("Local")),
        }
    }

    #[test]
    fn installed_build_keeps_user_files_out_of_the_program_folder() {
        let tmp = TempDir::new("paths-installed");
        let paths = layout(&tmp);
        assert_eq!(paths.settings_path(), tmp.0.join("Roaming/OpenSpore/settings.json"));
        assert_eq!(
            paths.default_save_path(),
            tmp.0.join("Roaming/OpenSpore/saves").join(SAVE_FILE)
        );
        assert_eq!(paths.runtime_dir(), tmp.0.join("Local/OpenSpore"));
        assert!(!paths.portable());
    }

    #[test]
    fn an_existing_save_next_to_the_exe_is_kept_in_place() {
        let tmp = TempDir::new("paths-legacy");
        let paths = layout(&tmp);
        let exe_dir = paths.exe_dir.clone().unwrap();
        let legacy = exe_dir.join(SAVE_FILE);
        std::fs::write(&legacy, b"{}").unwrap();
        assert_eq!(paths.default_save_path(), legacy);
        // Only its backup survived an interrupted save: still the old place.
        std::fs::remove_file(&legacy).unwrap();
        std::fs::write(crate::persistence::backup_path(&legacy), b"{}").unwrap();
        assert_eq!(paths.default_save_path(), legacy);
        // Settings and runtime files still go to the user folders.
        assert_eq!(paths.settings_path(), tmp.0.join("Roaming/OpenSpore/settings.json"));
        assert_eq!(paths.runtime_dir(), tmp.0.join("Local/OpenSpore"));
        // Nothing was moved or deleted.
        assert!(crate::persistence::backup_path(&legacy).is_file());
    }

    #[test]
    fn portable_txt_keeps_everything_next_to_the_exe() {
        let tmp = TempDir::new("paths-portable");
        let mut paths = layout(&tmp);
        let exe_dir = paths.exe_dir.clone().unwrap();
        std::fs::write(exe_dir.join(PORTABLE_MARKER), b"").unwrap();
        paths.user_dir_override = Some(tmp.0.join("Override"));
        assert!(paths.portable());
        assert_eq!(paths.settings_path(), exe_dir.join(SETTINGS_FILE));
        assert_eq!(paths.default_save_path(), exe_dir.join(SAVE_FILE));
        assert_eq!(paths.runtime_dir(), exe_dir.join("cache"));
    }

    #[test]
    fn user_dir_override_replaces_appdata_and_localappdata() {
        let tmp = TempDir::new("paths-override");
        let mut paths = layout(&tmp);
        let user = tmp.0.join("User");
        paths.user_dir_override = Some(user.clone());
        assert_eq!(paths.settings_path(), user.join(SETTINGS_FILE));
        assert_eq!(paths.default_save_path(), user.join("saves").join(SAVE_FILE));
        assert_eq!(paths.runtime_dir(), user.join("local"));
        // Without APPDATA either, the override still decides.
        paths.roaming = None;
        paths.local = None;
        assert_eq!(paths.settings_path(), user.join(SETTINGS_FILE));
    }

    #[test]
    fn settings_round_trip_keeps_other_keys() {
        let tmp = TempDir::new("paths-settings");
        let settings = tmp.0.join("nested").join(SETTINGS_FILE);
        assert_eq!(read_spore_path(&settings), None);
        let root = tmp.0.join("Games").join("Spore \"quoted\" ünïcode");
        write_spore_path(&settings, &root).unwrap();
        assert_eq!(read_spore_path(&settings), Some(root.clone()));
        // A key written by a later build (or by hand) survives an update.
        std::fs::write(
            &settings,
            r#"{"spore_path": "C:\\Old", "window": {"width": 1280}}"#,
        )
        .unwrap();
        assert_eq!(read_spore_path(&settings), Some(PathBuf::from(r"C:\Old")));
        write_spore_path(&settings, &root).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(value["window"]["width"], 1280);
        assert_eq!(read_spore_path(&settings), Some(root));
        // A settings file written with a byte-order mark (Inno Setup / Notepad).
        std::fs::write(&settings, "\u{feff}{\"spore_path\": \"D:\\\\Spore\"}").unwrap();
        assert_eq!(read_spore_path(&settings), Some(PathBuf::from(r"D:\Spore")));
        // Unreadable or empty values are ignored rather than trusted.
        std::fs::write(&settings, r#"{"spore_path": "  "}"#).unwrap();
        assert_eq!(read_spore_path(&settings), None);
        std::fs::write(&settings, "not json").unwrap();
        assert_eq!(read_spore_path(&settings), None);
        assert!(!settings.with_extension("json.tmp").exists());
    }
}
