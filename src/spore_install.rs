//! Spore install discovery (OpenRCT2 model).
//!
//! OpenSpore ships no game assets. It locates a legal Spore install and reads
//! `Data/*.package` read-only at runtime. Priority:
//! 0. The folder chosen in this run's setup dialog (`first_run`)
//! 1. `--spore-path <dir>` CLI arg
//! 2. `OPENSPORE_SPORE_PATH` / `SPORE_PATH` env vars
//! 3. `openspore.json` (`{"spore_path": "..."}`) next to exe or cwd
//! 4. The per-user `settings.json` (`%APPDATA%\OpenSpore\`, see `user_paths`)
//!    written by the installer and by the setup dialog
//! 5. Current dir / parent dir containing `Data/Spore_Game.package`
//! 6. Exe dir and parents
//! 7. Windows registry entries of non-Steam installs: EA / Origin / retail
//!    disc keys (`Electronic Arts\SPORE*`) and GOG (`GOG.com\Games\*`)
//! 8. Steam: default locations, the Steam client folder from the registry,
//!    the Steam uninstall key of app 17390 and `libraryfolders.vdf`
//! 9. Registry EA keys that point into a Steam library. Steam's install
//!    scripts write them for the separate Galactic Adventures app folder,
//!    which holds a duplicate base `Data`; they come after the Steam scan so a
//!    Steam install keeps using its base app folder (the 1-6 behaviour).
//!
//! A folder may be the install root, its `Data` folder or a `Sporebin` /
//! `SporebinEP1` (or expansion data) folder; it is mapped to the root.
//! `OPENSPORE_NO_AUTODETECT=1` skips 5-9 (to try the first-run setup on a
//! machine that has Spore installed).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::user_paths::{read_spore_path, UserPaths};

/// Files that must exist for an install to count as valid.
const REQUIRED: &[&str] = &["Data/Spore_Game.package", "Data/Spore_Graphics.package"];
/// All packages we like to load if present (newer/older installs vary).
const KNOWN_PACKAGES: &[&str] = &[
    "Data/Spore_Game.package",
    "Data/Spore_Graphics.package",
    "Data/Spore_Content.package",
    "Data/Spore_Audio1.package",
    "Data/Spore_Audio2.package",
    "Data/PatchData.package",
    "Data/Spore_Pack_03.package",
];
/// Folders inside an install a player may pick instead of its root.
const ROOT_CHILDREN: &[&str] = &[
    "Data",
    "DataEP1",
    "Sporebin",
    "SporebinEP1",
    "bp1content",
    "BP1Data",
];
pub const NO_AUTODETECT_ENV: &str = "OPENSPORE_NO_AUTODETECT";

#[derive(Debug, Clone)]
pub struct SporeInstall {
    pub root: PathBuf,
    pub data_dir: PathBuf,
    pub packages: Vec<PathBuf>,
}

impl SporeInstall {
    pub fn describe(&self) -> String {
        format!("{} ({} packages)", self.root.display(), self.packages.len())
    }
}

/// Why a folder is not a usable Spore install, in words for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotAnInstall {
    /// The folder as given.
    pub chosen: PathBuf,
    /// The folder whose `Data` was checked (`chosen`, or the install root a
    /// chosen `Data` / `Sporebin` folder maps to).
    pub checked: PathBuf,
    pub exists: bool,
    /// Required files not found in `checked`, e.g. `Data\Spore_Game.package`.
    pub missing: Vec<String>,
    pub hint: Option<String>,
}

impl std::fmt::Display for NotAnInstall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.exists {
            return write!(f, "this folder does not exist: {}", self.chosen.display());
        }
        write!(
            f,
            "this folder is not a Spore install: {} not found in {}",
            self.missing.join(" and "),
            self.checked.display()
        )?;
        if let Some(hint) = &self.hint {
            write!(f, ". {hint}")?;
        }
        Ok(())
    }
}

fn missing_in(root: &Path) -> Vec<String> {
    REQUIRED
        .iter()
        .filter(|r| !root.join(r).is_file())
        .map(|r| r.replace('/', std::path::MAIN_SEPARATOR_STR))
        .collect()
}

/// Trim quotes / whitespace a pasted path or registry value may carry and
/// drop trailing separators and `.` components.
fn clean(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let trimmed = text.trim().trim_matches('"').trim();
    Path::new(trimmed).components().collect()
}

/// Map a folder the player chose (or a registry value) to the install root:
/// the root itself, or its parent for `Data`, `Sporebin`, `SporebinEP1` and
/// the expansion data folders.
pub fn install_root_for(chosen: &Path) -> Result<PathBuf, NotAnInstall> {
    let chosen = clean(chosen);
    if !chosen.as_os_str().is_empty() && missing_in(&chosen).is_empty() {
        return Ok(chosen);
    }
    let parent = chosen
        .file_name()
        .filter(|name| ROOT_CHILDREN.iter().any(|c| name.eq_ignore_ascii_case(c)))
        .and_then(|_| chosen.parent())
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf);
    if let Some(parent) = &parent {
        if missing_in(parent).is_empty() {
            return Ok(parent.clone());
        }
    }
    let checked = parent.unwrap_or_else(|| chosen.clone());
    let missing = missing_in(&checked);
    let galactic_only = !checked.join("Data").is_dir()
        && (checked.join("DataEP1").is_dir() || checked.join("SporebinEP1").is_dir());
    let hint = if galactic_only {
        Some("This looks like a Galactic Adventures folder; choose the main Spore folder instead (the one whose Data folder holds Spore_Game.package)".into())
    } else if !checked.join("Data").is_dir() {
        Some("Choose the folder that contains Spore's Data folder".into())
    } else {
        None
    };
    Err(NotAnInstall {
        exists: chosen.is_dir(),
        chosen,
        checked,
        missing,
        hint,
    })
}

/// Validate a chosen folder and list its packages.
pub fn open(chosen: &Path) -> Result<SporeInstall, NotAnInstall> {
    let root = install_root_for(chosen)?;
    from_root(root.clone()).ok_or_else(|| NotAnInstall {
        chosen: chosen.to_path_buf(),
        exists: root.is_dir(),
        missing: missing_in(&root),
        hint: None,
        checked: root,
    })
}

fn is_valid_root(dir: &Path) -> bool {
    REQUIRED.iter().all(|r| dir.join(r).is_file())
}

fn collect_packages(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for rel in KNOWN_PACKAGES {
        let p = root.join(rel);
        if p.is_file() {
            out.push(p);
        }
    }
    // Expansions can be in this install, or in separate Steam app folders.
    // Read only their asset directories; never load a sibling's duplicate base Data.
    let mut asset_dirs = vec![
        root.join("Data"),
        root.join("DataEP1"),
        root.join("bp1content"),
        root.join("BP1Data"),
    ];
    for (app, subdirs) in [
        (24720, &["DataEP1"][..]),
        (17440, &["bp1content", "BP1Data"][..]),
    ] {
        if let Some(expansion) = steam_expansion_root(root, app) {
            asset_dirs.extend(subdirs.iter().map(|d| expansion.join(d)));
        }
    }
    for dir in asset_dirs {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_file()
                    && p.extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("package"))
                    && !out.contains(&p)
                {
                    out.push(p);
                }
            }
        }
    }
    // Locale text/name packages: prefer en-us, include all others as fallback.
    let locale = root.join("Data").join("Locale");
    let mut locale_pkgs: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&locale) {
        for lang in rd.flatten() {
            let dir = lang.path();
            if dir.is_dir() {
                if let Ok(rd2) = std::fs::read_dir(&dir) {
                    for e in rd2.flatten() {
                        let p = e.path();
                        if p.extension().map(|x| x == "package").unwrap_or(false) {
                            locale_pkgs.push(p);
                        }
                    }
                }
            }
        }
    }
    locale_pkgs.sort_by_key(|p| {
        let s = p.to_string_lossy().to_lowercase();
        (!s.contains("en-us")) as u8
    });
    for p in locale_pkgs {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out.sort();
    out
}

/// Steam installs expansion apps in separate common folders. Require their
/// manifest in the selected install's library, with the expected app id and a
/// completely installed state. Do not infer an expansion from directory names.
fn steam_expansion_root(root: &Path, app: u32) -> Option<PathBuf> {
    let common = root.parent()?;
    if !common.file_name()?.eq_ignore_ascii_case("common") {
        return None;
    }
    let steamapps = common.parent()?;
    if !steamapps.file_name()?.eq_ignore_ascii_case("steamapps") {
        return None;
    }
    let text = std::fs::read_to_string(steamapps.join(format!("appmanifest_{app}.acf"))).ok()?;
    manifest_install_dir(&text, app).map(|dir| common.join(dir))
}

fn manifest_install_dir(text: &str, app: u32) -> Option<String> {
    // These manifest fields contain ASCII identifiers and a single directory.
    // Reject escaping/path traversal rather than interpreting it as a sibling.
    let field = |name: &str| -> Option<&str> {
        text.lines().find_map(|line| {
            let tokens: Vec<_> = line.split('"').collect();
            (tokens.len() == 5 && tokens[1] == name).then(|| tokens[3])
        })
    };
    if field("appid")?.parse::<u32>().ok()? != app
        || field("StateFlags")?.parse::<u32>().ok()? & 4 == 0
    {
        return None;
    }
    let dir = field("installdir")?;
    if dir.is_empty() || dir == "." || dir == ".." || dir.contains(['/', '\\', ':']) {
        return None;
    }
    Some(dir.to_owned())
}

fn from_root(root: PathBuf) -> Option<SporeInstall> {
    if !is_valid_root(&root) {
        return None;
    }
    let packages = collect_packages(&root);
    Some(SporeInstall {
        data_dir: root.join("Data"),
        root,
        packages,
    })
}

// ---------------------------------------------------------------------------
// Discovery sources
// ---------------------------------------------------------------------------

/// Where an install was found (logged, and shown by `--detect-spore`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    SetupDialog,
    CommandLine,
    Environment,
    ConfigFile,
    Settings,
    WorkingDir,
    ExeDir,
    Registry,
    Steam,
    SteamRegistry,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::SetupDialog => "folder chosen in the setup dialog",
            Source::CommandLine => "--spore-path",
            Source::Environment => "OPENSPORE_SPORE_PATH / SPORE_PATH",
            Source::ConfigFile => "openspore.json",
            Source::Settings => "settings.json",
            Source::WorkingDir => "current folder",
            Source::ExeDir => "folder of the OpenSpore exe",
            Source::Registry => "registry (EA / Origin / disc / GOG)",
            Source::Steam => "Steam library",
            Source::SteamRegistry => "registry entry inside a Steam library",
        }
    }
}

static SESSION_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// The install root the setup dialog accepted; used by every later
/// `discover()` of this process even if `settings.json` could not be written.
pub fn set_session_root(root: PathBuf) {
    let _ = SESSION_ROOT.set(root);
}

/// Everything discovery reads, in priority order. `from_process` reads this
/// process and machine; tests fill it with temporary folders.
#[derive(Debug, Clone, Default)]
pub struct Probe {
    pub session: Option<PathBuf>,
    pub cli: Option<PathBuf>,
    pub env: Vec<PathBuf>,
    pub config_files: Vec<PathBuf>,
    pub settings_file: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub exe: Option<PathBuf>,
    /// Steps 5-9 (folder heuristics, registry, Steam).
    pub autodetect: bool,
    /// Registry folders; `None` queries this machine (once per process).
    pub registry: Option<Vec<PathBuf>>,
    /// Steam candidate roots; `None` scans this machine's Steam libraries.
    pub steam: Option<Vec<PathBuf>>,
}

pub fn cli_spore_path(args: &[String]) -> Option<PathBuf> {
    for i in 0..args.len() {
        if (args[i] == "--spore-path" || args[i] == "--spore_path") && i + 1 < args.len() {
            return Some(PathBuf::from(&args[i + 1]));
        }
        if let Some(v) = args[i].strip_prefix("--spore-path=") {
            return Some(PathBuf::from(v));
        }
    }
    None
}

impl Probe {
    pub fn from_process() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let exe = std::env::current_exe().ok();
        let cwd = std::env::current_dir().ok();
        let config_files = [
            exe.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)),
            cwd.clone(),
        ]
        .into_iter()
        .flatten()
        .map(|dir| dir.join("openspore.json"))
        .collect();
        Self {
            session: SESSION_ROOT.get().cloned(),
            cli: cli_spore_path(&args),
            env: ["OPENSPORE_SPORE_PATH", "SPORE_PATH"]
                .into_iter()
                .filter_map(std::env::var_os)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .collect(),
            config_files,
            settings_file: Some(UserPaths::from_process().settings_path()),
            cwd,
            exe,
            autodetect: std::env::var_os(NO_AUTODETECT_ENV)
                .is_none_or(|v| v.is_empty() || v == "0"),
            registry: None,
            steam: None,
        }
    }
}

/// `<library>` when `dir` lies in `<library>\steamapps\common\...`.
fn steam_library_of(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find_map(|a| {
        let steamapps = a.parent()?;
        (a.file_name()?.eq_ignore_ascii_case("common")
            && steamapps.file_name()?.eq_ignore_ascii_case("steamapps"))
        .then(|| steamapps.parent().map(Path::to_path_buf))
        .flatten()
    })
}

/// Library paths in a `libraryfolders.vdf`: every quoted absolute path, with
/// the VDF backslash escaping undone.
fn vdf_library_paths(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in text.lines() {
        for token in line.split('"') {
            let t = token.trim();
            if t.len() > 3 && t.chars().nth(1) == Some(':') {
                let p = PathBuf::from(t.replace("\\\\", "\\"));
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Spore roots to try in one Steam library (or Steam client) folder.
fn steam_library_candidates(lib: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string(lib.join("steamapps").join("appmanifest_17390.acf")) {
        if let Some(dir) = manifest_install_dir(&text, 17390) {
            out.push(lib.join("steamapps").join("common").join(dir));
        }
    }
    for cand in [
        lib.join("steamapps").join("common").join("Spore"),
        lib.join("common").join("Spore"),
        lib.to_path_buf(),
    ] {
        if !out.contains(&cand) {
            out.push(cand);
        }
    }
    out
}

fn steam_candidates(extra_libraries: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    // Explicit env / default Steam locations on Windows.
    let mut libs: Vec<PathBuf> = extra_libraries.to_vec();
    if let Ok(p) = std::env::var("ProgramFiles(x86)") {
        libs.push(PathBuf::from(p).join("Steam"));
    }
    if let Ok(p) = std::env::var("ProgramFiles") {
        libs.push(PathBuf::from(p).join("Steam"));
    }
    for drive in ["C:", "D:", "E:", "F:", "G:"] {
        libs.push(PathBuf::from(format!(r"{drive}\Program Files (x86)\Steam")));
        libs.push(PathBuf::from(format!(r"{drive}\SteamLibrary")));
        libs.push(PathBuf::from(format!(
            r"{drive}\SteamLibrary\steamapps\common\Spore"
        )));
    }
    // Parse libraryfolders.vdf for extra libraries.
    for steam in libs.clone() {
        let vdf = steam.join("steamapps").join("libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            for p in vdf_library_paths(&text) {
                if !libs.contains(&p) {
                    libs.push(p);
                }
            }
        }
    }
    for lib in libs {
        for cand in steam_library_candidates(&lib) {
            if !out.contains(&cand) {
                out.push(cand);
            }
        }
    }
    out
}

/// Locate the Spore install or return a human-readable error.
pub fn discover() -> Result<SporeInstall, String> {
    discover_with(&Probe::from_process()).map(|(install, _)| install)
}

pub const NOT_FOUND: &str = "Spore install not found. Run OpenSpore with --choose-spore to pick your Spore folder, pass --spore-path <dir>, or set OPENSPORE_SPORE_PATH. Expected <install>/Data/Spore_Game.package to exist.";

/// Discovery over explicit inputs; also says where the install was found.
pub fn discover_with(probe: &Probe) -> Result<(SporeInstall, Source), String> {
    // 0-4: explicit choices.
    if let Some(found) = probe.session.clone().and_then(from_root) {
        return Ok((found, Source::SetupDialog));
    }
    if let Some(p) = &probe.cli {
        return open(p)
            .map(|found| (found, Source::CommandLine))
            .map_err(|e| format!("--spore-path is not a valid Spore install ({e})"));
    }
    for p in &probe.env {
        if let Ok(found) = open(p) {
            return Ok((found, Source::Environment));
        }
    }
    for cfg in &probe.config_files {
        if let Some(found) = read_spore_path(cfg).and_then(|p| open(&p).ok()) {
            return Ok((found, Source::ConfigFile));
        }
    }
    if let Some(found) = probe
        .settings_file
        .as_deref()
        .and_then(read_spore_path)
        .and_then(|p| open(&p).ok())
    {
        return Ok((found, Source::Settings));
    }
    if !probe.autodetect {
        return Err(NOT_FOUND.to_string());
    }
    // 5: cwd and parents (covers running from inside the Spore folder during dev).
    if let Some(cwd) = &probe.cwd {
        let mut cur: Option<&Path> = Some(cwd);
        while let Some(dir) = cur {
            if let Some(found) = from_root(dir.to_path_buf()) {
                return Ok((found, Source::WorkingDir));
            }
            // Also handle cwd == Data/ itself.
            if dir.file_name().map(|n| n == "Data").unwrap_or(false) {
                if let Some(parent) = dir.parent() {
                    if let Some(found) = from_root(parent.to_path_buf()) {
                        return Ok((found, Source::WorkingDir));
                    }
                }
            }
            cur = dir.parent();
        }
    }
    // 6: exe dir and parents.
    if let Some(mut exe) = probe.exe.clone() {
        for _ in 0..4 {
            if let Some(parent) = exe.parent() {
                if let Some(found) = from_root(parent.to_path_buf()) {
                    return Ok((found, Source::ExeDir));
                }
                exe = parent.to_path_buf();
            } else {
                break;
            }
        }
    }
    // 7: registry entries of non-Steam installs.
    let registry = probe
        .registry
        .clone()
        .unwrap_or_else(|| machine_registry().spore_dirs.clone());
    let (in_steam, outside): (Vec<PathBuf>, Vec<PathBuf>) = registry
        .into_iter()
        .partition(|d| steam_library_of(d).is_some());
    for dir in &outside {
        if let Ok(found) = open(dir) {
            return Ok((found, Source::Registry));
        }
    }
    // 8: Steam scan.
    let steam = probe.steam.clone().unwrap_or_else(|| {
        let mut libraries = machine_registry().steam_dirs.clone();
        libraries.extend(in_steam.iter().filter_map(|d| steam_library_of(d)));
        steam_candidates(&libraries)
    });
    for cand in steam {
        if let Some(found) = from_root(cand) {
            return Ok((found, Source::Steam));
        }
    }
    // 9: registry folders inside a Steam library.
    for dir in &in_steam {
        if let Ok(found) = open(dir) {
            return Ok((found, Source::SteamRegistry));
        }
    }
    Err(NOT_FOUND.to_string())
}

// ---------------------------------------------------------------------------
// Windows registry (read with `reg query`; no extra crate)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryFindings {
    /// Candidate Spore folders (install root, Data or expansion folders).
    pub spore_dirs: Vec<PathBuf>,
    /// Steam client folders (their `libraryfolders.vdf` lists the libraries).
    pub steam_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RegKey {
    path: String,
    /// (name, data) of the string values.
    values: Vec<(String, String)>,
}

impl RegKey {
    fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.trim().is_empty())
    }
    fn leaf(&self) -> &str {
        self.path.rsplit('\\').next().unwrap_or(&self.path)
    }
}

/// Parse `reg query` output: key header lines (`HKEY_...`) followed by
/// indented `name    REG_SZ    data` value lines. Only string values are kept.
fn parse_reg_query(text: &str) -> Vec<RegKey> {
    let mut keys: Vec<RegKey> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with("HKEY_") {
            keys.push(RegKey {
                path: line.trim().to_string(),
                values: Vec::new(),
            });
            continue;
        }
        let Some(key) = keys.last_mut() else { continue };
        let body = line.trim_start();
        let Some(at) = body.find("    REG_") else { continue };
        let name = body[..at].trim().to_string();
        let rest = &body[at + 4..];
        let (kind, data) = match rest.find("    ") {
            Some(i) => (&rest[..i], &rest[i + 4..]),
            None => (rest.trim(), ""),
        };
        let data = match kind.trim() {
            "REG_SZ" => data.to_string(),
            "REG_EXPAND_SZ" => expand_env(data),
            _ => continue,
        };
        key.values.push((name, data));
    }
    keys
}

/// `%NAME%` expansion for `REG_EXPAND_SZ` values.
fn expand_env(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) if !name.is_empty() => out.push_str(&v),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn reg_path(value: &str) -> PathBuf {
    PathBuf::from(value.trim().trim_matches('"').replace('/', "\\"))
}

fn push_reg_path(list: &mut Vec<PathBuf>, value: Option<&str>) {
    if let Some(v) = value {
        let p = reg_path(v);
        if !list.contains(&p) {
            list.push(p);
        }
    }
}

/// Pick the Spore / Steam folders out of parsed registry keys.
fn findings_from(keys: &[RegKey]) -> RegistryFindings {
    let mut found = RegistryFindings::default();
    for key in keys {
        let path = key.path.to_ascii_lowercase();
        let leaf = key.leaf().to_ascii_lowercase();
        if path.contains("\\electronic arts\\") && leaf.starts_with("spore") {
            // Retail disc / Origin / EA app: `InstallLoc` / `Install Dir` hold
            // the install root; `DataDir` the Data folder (all that Steam and
            // some EA installs write).
            for name in ["InstallLoc", "Install Dir", "DataDir"] {
                push_reg_path(&mut found.spore_dirs, key.value(name));
            }
        } else if path.contains("\\gog.com\\games\\")
            && key
                .value("gameName")
                .is_some_and(|n| n.to_ascii_lowercase().contains("spore"))
        {
            for name in ["path", "workingDir"] {
                push_reg_path(&mut found.spore_dirs, key.value(name));
            }
        } else if path.ends_with("\\uninstall\\steam app 17390") {
            push_reg_path(&mut found.spore_dirs, key.value("InstallLocation"));
        } else if path.ends_with("\\valve\\steam") {
            for name in ["SteamPath", "InstallPath"] {
                push_reg_path(&mut found.steam_dirs, key.value(name));
            }
        }
    }
    found
}

fn machine_registry() -> &'static RegistryFindings {
    static FINDINGS: OnceLock<RegistryFindings> = OnceLock::new();
    FINDINGS.get_or_init(query_registry)
}

#[cfg(windows)]
fn query_registry() -> RegistryFindings {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let reg = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("reg.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("reg.exe"));
    let query = |key: &str, recursive: bool| -> Vec<RegKey> {
        let mut cmd = std::process::Command::new(&reg);
        cmd.arg("query").arg(key);
        if recursive {
            cmd.arg("/s");
        }
        cmd.stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        match cmd.output() {
            // `reg` prints in the console code page; non-ASCII folder names
            // may not survive and then fail validation (the picker remains).
            Ok(out) if out.status.success() => parse_reg_query(&String::from_utf8_lossy(&out.stdout)),
            _ => Vec::new(),
        }
    };
    let mut keys = Vec::new();
    for (key, recursive) in [
        (r"HKLM\SOFTWARE\WOW6432Node\Electronic Arts", true),
        (r"HKLM\SOFTWARE\Electronic Arts", true),
        (r"HKLM\SOFTWARE\WOW6432Node\GOG.com\Games", true),
        (r"HKLM\SOFTWARE\GOG.com\Games", true),
        (r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Steam App 17390", false),
        (r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Steam App 17390", false),
        (r"HKCU\Software\Valve\Steam", false),
        (r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam", false),
    ] {
        keys.extend(query(key, recursive));
    }
    findings_from(&keys)
}

#[cfg(not(windows))]
fn query_registry() -> RegistryFindings {
    RegistryFindings::default()
}

/// Registry findings of this machine (for `--detect-spore`).
pub fn registry_findings() -> RegistryFindings {
    machine_registry().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user_paths::tests::TempDir;

    #[test]
    fn steam_manifest_requires_installed_expected_app_and_safe_sibling() {
        let fixture = |app: &str, state: &str, dir: &str| {
            format!(
            "\"AppState\"\n{{\n\"appid\" \"{app}\"\n\"StateFlags\" \"{state}\"\n\"installdir\" \"{dir}\"\n}}"
        )
        };
        assert_eq!(
            manifest_install_dir(&fixture("24720", "4", "spore 24720"), 24720),
            Some("spore 24720".into())
        );
        for text in [
            fixture("17440", "4", "Spore"),
            fixture("24720", "2", "Spore"),
            fixture("24720", "4", "../Spore"),
            fixture("24720", "4", "D:\\Spore"),
            fixture("24720", "4", ".."),
            fixture("24720", "4", ""),
        ] {
            assert_eq!(manifest_install_dir(&text, 24720), None);
        }
    }

    /// A fake install: the two required (empty) packages plus the usual folders.
    fn fake_install(root: &Path) -> PathBuf {
        for dir in ["Data", "DataEP1", "Sporebin", "SporebinEP1"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for rel in REQUIRED {
            std::fs::write(root.join(rel), b"").unwrap();
        }
        root.to_path_buf()
    }

    fn sep(s: &str) -> String {
        s.replace('/', std::path::MAIN_SEPARATOR_STR)
    }

    #[test]
    fn a_chosen_root_data_or_sporebin_folder_maps_to_the_install_root() {
        let tmp = TempDir::new("install-map");
        let root = fake_install(&tmp.0.join("Spore Games").join("SPORE"));
        assert_eq!(install_root_for(&root), Ok(root.clone()));
        for child in ["Data", "SporebinEP1", "Sporebin", "DataEP1"] {
            assert_eq!(install_root_for(&root.join(child)), Ok(root.clone()), "{child}");
        }
        // Case, quotes and a trailing separator as pasted or read from the registry.
        let pasted = format!("  \"{}{}\" ", root.join("data").display(), std::path::MAIN_SEPARATOR);
        assert_eq!(install_root_for(Path::new(&pasted)), Ok(root.clone()));
        let install = open(&root.join("Data")).unwrap();
        assert_eq!(install.root, root);
        assert_eq!(install.packages.len(), 2);
    }

    #[test]
    fn a_folder_without_the_packages_says_what_is_missing() {
        let tmp = TempDir::new("install-missing");
        let root = tmp.0.join("NotSpore");
        std::fs::create_dir_all(root.join("Data")).unwrap();
        std::fs::write(root.join("Data/Spore_Graphics.package"), b"").unwrap();
        let err = install_root_for(&root).unwrap_err();
        assert_eq!(err.missing, vec![sep("Data/Spore_Game.package")]);
        assert_eq!(err.checked, root);
        assert!(err.exists);
        let text = err.to_string();
        assert!(
            text.starts_with(&format!(
                "this folder is not a Spore install: {} not found",
                sep("Data/Spore_Game.package")
            )),
            "{text}"
        );
        // Picking its Data folder reports the same root and the same file.
        let err = install_root_for(&root.join("Data")).unwrap_err();
        assert_eq!(err.checked, root);
        assert_eq!(err.missing, vec![sep("Data/Spore_Game.package")]);
        // An empty folder misses both and gets a hint.
        let empty = tmp.0.join("Empty");
        std::fs::create_dir_all(&empty).unwrap();
        let err = install_root_for(&empty).unwrap_err();
        assert_eq!(err.missing.len(), 2);
        assert!(err.hint.is_some());
        // A Galactic Adventures-only folder (retail discs install it apart).
        let ga = tmp.0.join("SPORE_EP1");
        std::fs::create_dir_all(ga.join("SporebinEP1")).unwrap();
        std::fs::create_dir_all(ga.join("DataEP1")).unwrap();
        let err = install_root_for(&ga.join("SporebinEP1")).unwrap_err();
        assert_eq!(err.checked, ga);
        assert!(err.to_string().contains("Galactic Adventures"), "{err}");
        // A folder that does not exist.
        let err = install_root_for(&tmp.0.join("Gone")).unwrap_err();
        assert!(!err.exists);
        assert!(err.to_string().starts_with("this folder does not exist"));
    }

    fn quiet_probe() -> Probe {
        Probe {
            registry: Some(Vec::new()),
            steam: Some(Vec::new()),
            autodetect: true,
            ..Default::default()
        }
    }

    #[test]
    fn discovery_order_explicit_choices_then_registry_then_steam() {
        let tmp = TempDir::new("install-order");
        let settings_install = fake_install(&tmp.0.join("FromSettings"));
        let registry_install = fake_install(&tmp.0.join("Electronic Arts").join("SPORE"));
        let steam_lib = tmp.0.join("SteamLibrary");
        let steam_install = fake_install(&steam_lib.join("steamapps/common/Spore"));
        let steam_ga = fake_install(&steam_lib.join("steamapps/common/spore 24720"));
        let settings = tmp.0.join("User").join("settings.json");
        crate::user_paths::write_spore_path(&settings, &settings_install.join("Data")).unwrap();

        // Nothing configured or detected.
        let err = discover_with(&quiet_probe()).unwrap_err();
        assert!(err.contains("--choose-spore"), "{err}");

        // Settings file (it may store the Data folder; mapped to the root).
        let mut probe = quiet_probe();
        probe.settings_file = Some(settings.clone());
        probe.registry = Some(vec![registry_install.join("Data")]);
        probe.steam = Some(vec![steam_install.clone()]);
        let (found, source) = discover_with(&probe).unwrap();
        assert_eq!((found.root, source), (settings_install.clone(), Source::Settings));

        // A stale settings entry falls through to the registry, before Steam.
        crate::user_paths::write_spore_path(&settings, &tmp.0.join("Moved")).unwrap();
        let (found, source) = discover_with(&probe).unwrap();
        assert_eq!((found.root, source), (registry_install.clone(), Source::Registry));

        // An EA key inside a Steam library (the Galactic Adventures app folder
        // with a duplicate base Data) loses to the Steam base app folder.
        probe.registry = Some(vec![steam_ga.join("Data")]);
        let (found, source) = discover_with(&probe).unwrap();
        assert_eq!((found.root, source), (steam_install.clone(), Source::Steam));
        probe.steam = Some(Vec::new());
        let (found, source) = discover_with(&probe).unwrap();
        assert_eq!((found.root, source), (steam_ga.clone(), Source::SteamRegistry));

        // Explicit overrides beat everything; an invalid --spore-path is an error.
        probe.env = vec![tmp.0.join("Nope"), registry_install.join("Sporebin")];
        let (found, source) = discover_with(&probe).unwrap();
        assert_eq!((found.root, source), (registry_install.clone(), Source::Environment));
        probe.cli = Some(tmp.0.join("Nope"));
        let err = discover_with(&probe).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
        probe.cli = Some(settings_install.join("SporebinEP1"));
        let (found, source) = discover_with(&probe).unwrap();
        assert_eq!((found.root, source), (settings_install.clone(), Source::CommandLine));
        probe.session = Some(steam_install.clone());
        let (found, source) = discover_with(&probe).unwrap();
        assert_eq!((found.root, source), (steam_install.clone(), Source::SetupDialog));

        // No autodetection: only explicit choices count, even from inside an install.
        let probe = Probe {
            cwd: Some(steam_install.join("Data")),
            registry: Some(vec![registry_install.clone()]),
            steam: Some(vec![steam_install.clone()]),
            autodetect: false,
            ..Default::default()
        };
        assert!(discover_with(&probe).is_err());
        let probe = Probe { autodetect: true, ..probe };
        let (found, source) = discover_with(&probe).unwrap();
        assert_eq!((found.root, source), (steam_install, Source::WorkingDir));
    }

    #[test]
    fn steam_library_uses_the_base_app_manifest_and_unescaped_vdf_paths() {
        let tmp = TempDir::new("install-steam");
        let lib = tmp.0.join("Lib");
        let custom = fake_install(&lib.join("steamapps/common/Spore Custom"));
        std::fs::write(
            lib.join("steamapps/appmanifest_17390.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"17390\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"installdir\"\t\t\"Spore Custom\"\n}\n",
        )
        .unwrap();
        let cands = steam_library_candidates(&lib);
        assert_eq!(cands[0], custom);
        assert!(cands.contains(&lib.join("steamapps/common/Spore")));
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}\n";
        assert_eq!(
            vdf_library_paths(vdf),
            vec![PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\SteamLibrary")]
        );
        assert_eq!(
            steam_library_of(&lib.join("steamapps/common/spore 24720/Data")),
            Some(lib.clone())
        );
        assert_eq!(steam_library_of(&tmp.0.join("Electronic Arts/SPORE")), None);
    }

    /// `reg query` output as printed on Windows 11 (values from a Steam
    /// install, a retail disc install and a GOG entry; spacing preserved).
    const REG_FIXTURE: &str = "\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Electronic Arts\\EA Desktop\r
    InstallLocation    REG_SZ    C:\\Program Files\\Electronic Arts\\EA Desktop\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Electronic Arts\\ea games\\spore(tm)\\ergc\r
    (Default)    REG_SZ    %CDKEY%\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Electronic Arts\\spore\r
    appdir    REG_SZ    Spore\r
    datadir    REG_SZ    D:\\SteamLibrary\\steamapps\\common\\spore 24720\\Data\r
    locale    REG_SZ    en-us\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Electronic Arts\\SPORE Creepy and Cute Parts Pack\r
    AddOnID    REG_DWORD    0x1\r
    datadir    REG_SZ    D:\\SteamLibrary\\steamapps\\common\\spore 24720\\bp1content\\\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Electronic Arts\\SPORE_EP1\r
    Install Dir    REG_SZ    \"C:\\Program Files (x86)\\Electronic Arts\\SPORE_EP1\"\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Electronic Arts\\SPORE(TM)\r
    InstallLoc    REG_SZ    C:\\Program Files (x86)\\Electronic Arts\\SPORE\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\GOG.com\\Games\\1207658959\r
    gameName    REG_SZ    Spore Collection\r
    path    REG_SZ    C:\\GOG Games\\Spore\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\GOG.com\\Games\\1207658930\r
    gameName    REG_SZ    Some Other Game\r
    path    REG_SZ    C:\\GOG Games\\Other\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Steam App 17390\r
    DisplayName    REG_SZ    Spore\r
    InstallLocation    REG_SZ    \r
    NoRepair    REG_DWORD    0x1\r
\r
HKEY_CURRENT_USER\\Software\\Valve\\Steam\r
    SteamPath    REG_SZ    c:/program files (x86)/steam\r
";

    #[test]
    fn registry_values_of_ea_gog_and_steam_installs_are_found() {
        let keys = parse_reg_query(REG_FIXTURE);
        assert_eq!(keys.len(), 10);
        assert_eq!(keys[2].value("DataDir"), Some(r"D:\SteamLibrary\steamapps\common\spore 24720\Data"));
        assert_eq!(keys[8].value("InstallLocation"), None);
        let found = findings_from(&keys);
        assert_eq!(
            found.spore_dirs,
            vec![
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\spore 24720\Data"),
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\spore 24720\bp1content\"),
                PathBuf::from(r"C:\Program Files (x86)\Electronic Arts\SPORE_EP1"),
                PathBuf::from(r"C:\Program Files (x86)\Electronic Arts\SPORE"),
                PathBuf::from(r"C:\GOG Games\Spore"),
            ]
        );
        assert_eq!(found.steam_dirs, vec![PathBuf::from(r"c:\program files (x86)\steam")]);
    }

    #[test]
    fn expand_sz_values_expand_known_variables_only() {
        let path = std::env::var("PATH").expect("PATH is set");
        assert_eq!(expand_env("%PATH%\\x"), format!("{path}\\x"));
        assert_eq!(expand_env("a%OPENSPORE_UNSET_VAR_X%b"), "a%OPENSPORE_UNSET_VAR_X%b");
        assert_eq!(expand_env("50% off"), "50% off");
    }
}
