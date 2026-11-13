//! The settings dialog (`GameSettings`, opened by Settings in the galaxy
//! options panel or the in-stage options menu) with its pages
//! (`GameSettingsGraphics/Audio/Game/Camera/Online/Credits`) placed in the
//! page host `046967C0`. Entries OpenSpore supports are live (audio volumes
//! and Mute All, Sound Quality, full screen, screen size, the quality rows
//! OpenSpore can honour (`graphics`), Defaults, Tutorials / Game Hints / Edge
//! Scroll on the Game page, the Control page in the stages); the others are
//! shown disabled. Accept keeps the edits (saved beside the game saves),
//! Cancel drops them. The Credits page scrolls the installed credits text.
//!
//! VERIFIED (march2017 image, `swarm-volume/target/re/dis.py`): the dialog's
//! open routine (0x603300 ..) always selects the Graphic Settings tab first
//! (0x603540: button 0x473FF78 state 4); it calls the Control page's set-up
//! 0x600960(game mode), which answers false outside the five stage modes and
//! then DISABLES the Control Settings tab (0x6034E2 .. 0x60350F: flag 2 off,
//! flag 0x10 on). So from the galaxy screen the Control tab is greyed out; in
//! a stage it shows that stage's window.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{graphics, FrontEnd, Overlay};
use crate::spui::{
    model::{WindowKind, WinId},
    solve::RootPolicy,
    view::{self, SpuiAssets, SpuiPointer, SpuiState},
};

pub const LAYOUT: &str = "GameSettings";

pub mod id {
    pub const ACCEPT: u32 = 0x043B_7A18;
    pub const CANCEL: u32 = 0x043C_8FD8;
    pub const HOST: u32 = 0x0469_67C0;
    /// Tab buttons and the page layout each shows.
    pub const TABS: [(u32, &str); 6] = [
        (0x0473_FF78, "GameSettingsGraphics"),
        (0x0473_FF60, "GameSettingsAudio"),
        (0x0473_FF88, "GameSettingsGame"),
        (0x0473_FF98, "GameSettingsCamera"),
        (0x0473_FFA8, "GameSettingsOnline"),
        (0x0473_FFB8, "GameSettingsCredits"),
    ];
    pub const TAB_ONLINE: usize = 4;
    pub const TAB_CONTROL: usize = 3;
    // Audio page
    pub const MASTER: u32 = 0xADED_51D5;
    pub const MUSIC: u32 = 0x8BDD_39EC;
    pub const VOICES: u32 = 0x09A0_474A;
    pub const SFX: u32 = 0x3AF2_39C4;
    pub const MUTE_ALL: u32 = 0x899E_B157;
    // Graphics page
    pub const FULL_SCREEN: u32 = 0x05B5_91C0;
    /// Screen Size (OptionScreenSize; caption set by code) and Defaults.
    pub const SCREEN_SIZE: u32 = 0x0461_70A1;
    pub const DEFAULTS: u32 = 0x05CA_CA50;
    // Game page ("Game & Help Options")
    pub const TUTORIALS: u32 = 0x04EA_67B3;
    pub const HINTS: u32 = 0x05B5_91A8;
    pub const EDGE_SCROLL: u32 = 0x05B5_91D8;
    // Credits page: the scroll area (its content window 06036E78 is filled
    // by code in retail).
    pub const CREDITS_VIEW: u32 = 0x0603_6E80;
    // Audio page radios: speaker set-up (2 / 4 / 5.1 speakers, top to
    // bottom) and sound quality (Low / Medium / High).
    pub const SPEAKERS: [u32; 3] = [0xDE52_C409, 0xF24B_36E0, 0xAEFD_81A1];
    pub const SOUND_QUALITY: [u32; 3] = [0x048D_F982, 0x048D_F96C, 0x048D_F95D];
    // Game page capture radios (Small / Medium / Large, 160x120 / 320x240 /
    // 640x480) and the text under them that names the capture folder.
    pub const MOVIE_SIZE: [u32; 3] = [0x0462_C820, 0x0462_CB58, 0x0462_CB78];
    pub const PICTURE_SIZE: [u32; 3] = [0x0462_CBA0, 0x0462_CBB0, 0x0462_CBB8];
    pub const CAPTURE_LOCATION: u32 = 0x0631_7E08;
    // Control page (`GameSettingsCamera`): one window per stage (Cell,
    // Creature, Tribe, Civilization, Space), the 11 "how it works" buttons,
    // the mouse diagram's parts (AB3 .. ABE; ABD is the caption) and each
    // stage window's option check box.
    pub const CONTROL_STAGES: [u32; 5] = [0x0679_BEBD, 0x0679_BED6, 0x0679_BEE7, 0x0679_BEF7, 0x0679_BF0B];
    pub const HELP_BUTTON: u32 = 0x067A_0A6D;
    pub const HELP_BUTTONS: u32 = 11;
    pub const MOUSE_PARTS: [u32; 12] = [
        0x067A_0AB3, 0x067A_0AB4, 0x067A_0AB5, 0x067A_0AB6, 0x067A_0AB7, 0x067A_0AB8, 0x067A_0AB9, 0x067A_0ABA, 0x067A_0ABB,
        0x067A_0ABC, 0x067A_0ABD, 0x067A_0ABE,
    ];
    pub const MOUSE_CAPTION: u32 = 0x067A_0ABD;
    pub const STAGE_OPTION: u32 = 0x0679_B630;
    /// The audio engine's global parameter `audioPerformanceLevel` (FNV of the
    /// name; set at 0xA29E48 / 0xA41598 from the property of `OptionAudioPerformance`).
    pub const AUDIO_PERFORMANCE_LEVEL: u32 = 0x6DF5_822D;
}

/// Index of a radio button in one of the three-button rows above.
fn radio_index(row: &[u32; 3], control: u32) -> Option<u8> {
    row.iter().position(|c| *c == control).map(|i| i as u8)
}

/// The locale table of the settings strings (`AD7B2086`); the mouse help
/// captions are `090A50A1` .. `090A50AC` in it.
const SETTINGS_TABLE: u32 = 0xAD7B_2086;

/// The mouse diagram a Control page button shows when the pointer enters it
/// (VERIFIED, the enter handler 0x600370: it hides the twelve diagram windows,
/// shows these and sets the caption). `button` is the button's index from
/// [`id::HELP_BUTTON`]; `option` is the stage's check box (Cell / Creature:
/// follow the cursor; Tribe / Civilization / Space: rotate with the right mouse
/// button). Returns the diagram windows' control ids and the string id of the
/// caption.
pub fn help_view(button: usize, option: bool) -> (&'static [u32], u32) {
    const AB3: u32 = 0x067A_0AB3;
    const AB4: u32 = 0x067A_0AB4;
    const AB6: u32 = 0x067A_0AB6;
    const AB7: u32 = 0x067A_0AB7;
    const AB8: u32 = 0x067A_0AB8;
    const AB9: u32 = 0x067A_0AB9;
    const ABA: u32 = 0x067A_0ABA;
    const ABB: u32 = 0x067A_0ABB;
    const ABC: u32 = 0x067A_0ABC;
    const ABE: u32 = 0x067A_0ABE;
    const TEXT: u32 = 0x090A_50A0;
    match (button, option) {
        (0, _) => (&[AB4, ABE, AB6, ABB], TEXT + 0x0A),
        (1, _) => (&[AB3, AB9], TEXT + 0x06),
        (2, _) => (&[AB3, AB8], TEXT + 0x05),
        (3, _) => (&[AB3, AB7], TEXT + 0x02),
        (4, _) | (8, _) => (&[AB3, AB7], TEXT + 0x03),
        (5, _) => (&[AB3, AB8], TEXT + 0x04),
        (6 | 9, true) => (&[ABC, ABA], TEXT + 0x01),
        (6 | 9, false) => (&[ABB, ABC, AB6], TEXT + 0x08),
        (7 | 10, true) => (&[AB3, AB8], TEXT + 0x05),
        (7, false) => (&[AB3, AB7, AB8], TEXT + 0x07),
        _ => (&[AB3, AB9], TEXT + 0x0C),
    }
}

/// The stage window (index into [`id::CONTROL_STAGES`]) a window of the Control page lies in.
fn stage_window_of(ui: &SpuiState, mut w: WinId) -> Option<usize> {
    loop {
        let win = ui.layout.window(w);
        if let Some(i) = id::CONTROL_STAGES.iter().position(|c| *c == win.control_id) {
            return Some(i);
        }
        w = win.parent?;
    }
}

/// Index of a game stage in the Control page's stage windows (Cell .. Space).
fn control_stage_index(stage: &crate::stages::GameStage) -> Option<usize> {
    use crate::stages::GameStage::*;
    match stage {
        MainMenu => None,
        Cell => Some(0),
        Creature => Some(1),
        Tribal => Some(2),
        Civilization => Some(3),
        Space => Some(4),
    }
}

/// The player's settings (stored as `<save stem>-settings.json`).
#[derive(Resource, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UserSettings {
    pub master: f32,
    pub music: f32,
    pub voices: f32,
    pub sfx: f32,
    pub mute_all: bool,
    pub fullscreen: bool,
    /// Game page: tutorial hints / game hints (read by the stage guides),
    /// edge scrolling (Tribal and Civ cameras).
    pub tutorials: bool,
    pub hints: bool,
    pub edge_scroll: bool,
    /// Graphics quality levels, 0 low, 1 medium, 2 high (`graphics`):
    /// Shadows (low = off, medium = one cascade), Texture Detail, Graphics
    /// Effects, Player Model Quality, Lighting Quality, Planet Quality,
    /// Depth of Field.
    pub shadows: u8,
    pub texture_detail: u8,
    pub effects: u8,
    pub model_quality: u8,
    pub lighting: u8,
    pub planet_quality: u8,
    pub depth_of_field: u8,
    /// Screen Size (windowed); `None` keeps the window's size.
    pub resolution: Option<[u32; 2]>,
    /// Audio page speaker set-up (0 stereo, 1 quad, 2 surround) and sound
    /// quality (0 low .. 2 high); Game page movie size (0 .. 2) and picture size
    /// (0 .. 2). Sound Quality is live: `OptionAudioPerformance` sets the audio
    /// engine's `audioPerformanceLevel` 1 / 2 / 3 (`audio_performance_level`,
    /// fed to the PD engine by [`apply`]). The other three are kept in the file
    /// as the retail option values ([`speaker_mode`], [`movie_resolution`],
    /// [`picture_resolution`]) but nothing in OpenSpore reads them: the output
    /// device's channel layout is the host's, and it has no movie / picture
    /// capture (retail: the C and V keys, `playmodePhotoRes`, `recordMovieWidth`).
    /// Defaults (`ConfigManager.txt`): stereo, sound quality = the CPU level
    /// (High), Medium movie and picture size.
    pub speakers: u8,
    pub sound_quality: u8,
    pub movie_size: u8,
    pub picture_size: u8,
    /// The Control page's check boxes, the config options `OptionCellControls`
    /// .. `OptionSpaceControls` (0x0679B833, 0x0679B85E, 0x0679B868, 0x0679B873,
    /// 0x0679B880; `ConfigManager.txt` sets all of them off). Every stage reads its
    /// own (`crate::control_options`): Cell `cell_follows_cursor`, Creature the
    /// two-button mouse map, Tribe / Civ the rotate-on-right-drag camera, Space
    /// the Flight trigger file.
    pub stage_controls: [bool; 5],
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            master: 1.0,
            music: 1.0,
            voices: 1.0,
            sfx: 1.0,
            mute_all: false,
            fullscreen: false,
            tutorials: true,
            hints: true,
            edge_scroll: false,
            shadows: super::graphics::DEFAULTS[0],
            texture_detail: super::graphics::DEFAULTS[1],
            effects: super::graphics::DEFAULTS[2],
            model_quality: super::graphics::DEFAULTS[3],
            lighting: super::graphics::DEFAULTS[4],
            planet_quality: super::graphics::DEFAULTS[5],
            depth_of_field: super::graphics::DEFAULTS[6],
            resolution: None,
            speakers: 0,
            // `setOption OptionAudioPerformance $cpuLevel` (ConfigManager.txt
            // 595; the machine rates high, `graphics::DEFAULTS`).
            sound_quality: super::graphics::AUDIO_PERFORMANCE_DEFAULT,
            movie_size: 1,
            picture_size: 1,
            stage_controls: [false; 5],
            path: None,
        }
    }
}

/// Live gains read by the audio code (`game_audio::Bus::volume`): master,
/// music, voices, sfx as f32 bits.
static GAINS: [AtomicU32; 4] = [
    AtomicU32::new(0x3F80_0000),
    AtomicU32::new(0x3F80_0000),
    AtomicU32::new(0x3F80_0000),
    AtomicU32::new(0x3F80_0000),
];

fn gain(i: usize) -> f32 {
    f32::from_bits(GAINS[i].load(Ordering::Relaxed))
}

/// Player volume of a bus (the settings' Music / SFX sliders; ambience and UI
/// follow SFX). Multiplied into `game_audio::Bus::volume`.
pub fn bus_gain(bus: crate::game_audio::Bus) -> f32 {
    use crate::game_audio::Bus;
    match bus {
        Bus::Music => gain(1),
        Bus::Voice => gain(2),
        Bus::Ambience | Bus::Sfx | Bus::Ui => gain(3),
    }
}

/// Master volume (0 with Mute All), applied to every playing voice.
pub fn master_gain() -> f32 {
    gain(0)
}

static TUTORIALS: AtomicBool = AtomicBool::new(true);
static HINTS: AtomicBool = AtomicBool::new(true);
/// The five Control page options (`UserSettings::stage_controls`): Cell,
/// Creature, Tribe, Civilization, Space.
static STAGE_CONTROLS: [AtomicBool; 5] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];

/// The live value of a stage's Control page option (`OptionCellControls` ..
/// `OptionSpaceControls`, index 0 Cell .. 4 Space): what the retail stages read
/// with `IConfigManager::Get(0x0679B833 + ...)`. `crate::control_options` names
/// what each one changes.
pub fn stage_control_option(stage: usize) -> bool {
    STAGE_CONTROLS.get(stage).is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// Tests: set a stage's option without a settings dialog
/// (`control_options::hold_option`).
#[cfg(test)]
pub fn set_stage_control_option(stage: usize, on: bool) {
    STAGE_CONTROLS[stage].store(on, Ordering::Relaxed);
}

/// `OptionCellControls` (Settings > Control > Cell): the cell steers toward the
/// cursor without a mouse button held (VERIFIED, `cell-re-notes.md`: `steer =
/// opt || lmb || rmb` in `MovePlayerToMousePosition` 0xE5B790).
pub fn cell_follows_cursor() -> bool {
    stage_control_option(0)
}

/// Settings > Game > Tutorials (the stage guides' tutorial hints).
pub fn tutorials_enabled() -> bool {
    TUTORIALS.load(Ordering::Relaxed)
}

/// Settings > Game > Game Hints (the stage guides' hint line).
pub fn hints_enabled() -> bool {
    HINTS.load(Ordering::Relaxed)
}

/// Creature voices slider (`Bus::Voice`: the clip voices of `anim_audio`).
pub fn voice_gain() -> f32 {
    gain(2)
}

static SOUND_QUALITY: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(graphics::AUDIO_PERFORMANCE_DEFAULT);

/// Settings > Audio > Sound Quality, 0 Low .. 2 High (published by `publish`).
#[allow(dead_code)]
pub fn sound_quality_level() -> u8 {
    SOUND_QUALITY.load(Ordering::Relaxed)
}

/// `OptionAudioPerformance` (Options.txt): Low / Medium / High set the
/// property `audioPerformanceLevel` to 1 / 2 / 3. VERIFIED: the audio engine
/// takes it as its global parameter 0x6DF5822D (set at 0xA29E48 / 0xA41598 from
/// the property, stored as an integer at +0x200 by the parameter handler
/// 0xA3058B), which is what the PD patches see as `audioPerformanceLevel`.
pub fn audio_performance_level(sound_quality: u8) -> u32 {
    sound_quality.min(2) as u32 + 1
}

/// `AudioSpeakerMode` of a Speaker Configuration radio (stereo / quad /
/// surround): the audio engine's speaker-mode setter (0xA41439 ..) accepts only
/// 2, 4 and 5 (VERIFIED, any other value is dropped).
#[allow(dead_code)] // kept for the capture / multichannel features that would read it
pub fn speaker_mode(radio: u8) -> u32 {
    [2, 4, 5][radio.min(2) as usize]
}

/// `OptionPhotoRes` (Options.txt): `playmodePhotoRes` 800 / 1280 / 2400 pixels
/// wide for Low / Medium / High (the file's comments: 800x600, 1280x960,
/// 2400x1800), the size of the C key's screenshot (manual: "Take screenshot C",
/// small / medium / large).
#[allow(dead_code)] // kept for the capture / multichannel features that would read it
pub fn picture_resolution(level: u8) -> [u32; 2] {
    [[800, 600], [1280, 960], [2400, 1800]][level.min(2) as usize]
}

/// `OptionVideoRes` (Options.txt): `recordMovieWidth` x `recordMovieHeight`
/// 160x120 / 320x240 / 639x480 (sic) for Low / Medium / High, the size of the V
/// key's movie capture.
#[allow(dead_code)] // kept for the capture / multichannel features that would read it
pub fn movie_resolution(level: u8) -> [u32; 2] {
    [[160, 120], [320, 240], [639, 480]][level.min(2) as usize]
}

impl UserSettings {
    pub fn path_for(save: &Path) -> PathBuf {
        let stem = save.file_stem().and_then(|s| s.to_str()).unwrap_or("openspore_save");
        save.with_file_name(format!("{stem}-settings.json"))
    }

    /// Load the settings stored beside `save` (defaults when missing).
    pub fn load_beside(&mut self, save: &Path) {
        let path = Self::path_for(save);
        let mut loaded = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<UserSettings>(&b).ok())
            .unwrap_or_default();
        loaded.path = Some(path);
        *self = loaded;
        self.publish();
    }

    pub fn store(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        std::fs::write(path, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)
    }

    /// Master (0 with Mute All), music, voices, sfx.
    pub fn gains(&self) -> [f32; 4] {
        let master = if self.mute_all { 0.0 } else { self.master };
        [master, self.music, self.voices, self.sfx].map(|v| v.clamp(0.0, 1.0))
    }

    /// The quality levels in `graphics::ROWS` order.
    pub fn levels(&self) -> [u8; 7] {
        [self.shadows, self.texture_detail, self.effects, self.model_quality, self.lighting, self.planet_quality, self.depth_of_field]
    }

    pub fn set_level(&mut self, row: usize, level: u8) {
        let level = level.min(2);
        match row {
            0 => self.shadows = level,
            1 => self.texture_detail = level,
            2 => self.effects = level,
            3 => self.model_quality = level,
            4 => self.lighting = level,
            5 => self.planet_quality = level,
            _ => self.depth_of_field = level,
        }
    }

    /// Make the volumes and the graphics levels live.
    pub fn publish(&self) {
        super::graphics::publish(self.levels());
        for (i, v) in self.gains().into_iter().enumerate() {
            GAINS[i].store(v.to_bits(), Ordering::Relaxed);
        }
        SOUND_QUALITY.store(self.sound_quality.min(2), Ordering::Relaxed);
        TUTORIALS.store(self.tutorials, Ordering::Relaxed);
        HINTS.store(self.hints, Ordering::Relaxed);
        // A test holds an option (`control_options::hold_option`): another test app's first
        // frame must not switch it off under it.
        #[cfg(test)]
        if crate::control_options::option_held() {
            return;
        }
        for (flag, value) in STAGE_CONTROLS.iter().zip(self.stage_controls) {
            flag.store(value, Ordering::Relaxed);
        }
    }
}

/// Dialog state: the open tab and the edits not yet accepted.
#[derive(Resource, Default)]
pub struct SettingsUi {
    pub tab: usize,
    pub draft: Option<UserSettings>,
    /// Slider being dragged.
    pub dragging: Option<u32>,
    /// Control page: the stage whose window shows (the stage being played,
    /// `0x600960`; from the galaxy screen the tab is disabled, so this only
    /// matters to the capture demos), and the help button the pointer entered
    /// last (the diagram stays until another is entered, as in 0x600370).
    pub control_stage: usize,
    pub help: Option<usize>,
    /// `--front-end settings:controls` / `stage<k>`: the capture asked for the
    /// Control tab from the galaxy, where retail greys it out.
    pub demo_controls: bool,
}

/// Tab 3 (Control Settings) is the one retail disables outside the stages
/// (VERIFIED, 0x600960 / 0x6034E2); Online (4) is disabled by design.
fn tab_enabled(tab: usize, in_stage: bool, demo_controls: bool) -> bool {
    match tab {
        id::TAB_ONLINE => false,
        id::TAB_CONTROL => in_stage || demo_controls,
        _ => true,
    }
}

/// `--front-end settings:<tab>` (captures).
static DEMO_TAB: AtomicU32 = AtomicU32::new(u32::MAX);

/// `--front-end settings:controls-<n>`: the Control page with help button `n`
/// entered (captures; the pointer cannot be moved offscreen).
static DEMO_HELP: AtomicU32 = AtomicU32::new(u32::MAX);

/// `--front-end settings:stage<k>[-<n>]`: the Control page showing stage `k`'s
/// window (0 Cell .. 4 Space), optionally with help button `n` entered.
static DEMO_STAGE: AtomicU32 = AtomicU32::new(u32::MAX);

pub fn demo_tab(name: &str) {
    if let Some(rest) = name.strip_prefix("stage") {
        let mut parts = rest.splitn(2, '-');
        if let Some(k) = parts.next().and_then(|k| k.parse::<u32>().ok()).filter(|k| *k < 5) {
            DEMO_STAGE.store(k, Ordering::Relaxed);
            if let Some(n) = parts.next().and_then(|n| n.parse::<u32>().ok()) {
                DEMO_HELP.store(n, Ordering::Relaxed);
            }
            DEMO_TAB.store(3, Ordering::Relaxed);
        }
        return;
    }
    if let Some(n) = name.strip_prefix("controls-").and_then(|n| n.parse::<u32>().ok()) {
        DEMO_HELP.store(n, Ordering::Relaxed);
        DEMO_TAB.store(3, Ordering::Relaxed);
        return;
    }
    let tab = match name {
        "graphics" => 0,
        "audio" => 1,
        "game" => 2,
        "controls" => 3,
        "online" => 4,
        "credits" => 5,
        _ => return,
    };
    DEMO_TAB.store(tab, Ordering::Relaxed);
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsRoot {
    Frame,
    Page(usize),
}

#[allow(clippy::too_many_arguments)]
pub fn manage(
    mut commands: Commands,
    fe: Res<FrontEnd>,
    user: Res<UserSettings>,
    mut dialog: ResMut<SettingsUi>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &SettingsRoot)>,
) {
    let dialog = &mut *dialog;
    let open = fe.overlay == Some(Overlay::Settings);
    if !open {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        dialog.draft = None;
        dialog.dragging = None;
        dialog.help = None;
        dialog.demo_controls = false;
        return;
    }
    if dialog.draft.is_none() {
        dialog.draft = Some(user.clone());
        let help = DEMO_HELP.swap(u32::MAX, Ordering::Relaxed);
        if help != u32::MAX {
            dialog.help = Some(help as usize);
        }
        let stage = DEMO_STAGE.swap(u32::MAX, Ordering::Relaxed);
        if stage != u32::MAX {
            dialog.control_stage = stage as usize;
        }
        // Retail builds the dialog anew on every open and selects the Graphic
        // Settings tab (0x603540), whatever page was open last.
        let demo = DEMO_TAB.swap(u32::MAX, Ordering::Relaxed);
        dialog.tab = if demo != u32::MAX { demo as usize } else { 0 };
        dialog.demo_controls = demo == id::TAB_CONTROL as u32;
    }
    if originals.resource_index.records.is_empty() {
        return;
    }
    let Ok(camera) = camera.single() else { return };
    let index = &originals.resource_index;
    if !roots.iter().any(|(_, r)| *r == SettingsRoot::Frame) {
        if let Some(layout) = assets.layout(index, LAYOUT) {
            let root = view::spawn(&mut commands, layout, RootPolicy::Centered, Some(camera), 200);
            commands.entity(root).insert(SettingsRoot::Frame);
        }
    }
    let want = SettingsRoot::Page(dialog.tab);
    let mut have = false;
    for (e, r) in &roots {
        if let SettingsRoot::Page(_) = r {
            if *r == want {
                have = true;
            } else {
                commands.entity(e).despawn();
            }
        }
    }
    if !have {
        if let Some(layout) = assets.layout(index, id::TABS[dialog.tab].1) {
            let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 201);
            commands.entity(root).insert(want);
        }
    }
}

fn find(ui: &SpuiState, control: u32) -> Option<WinId> {
    ui.find(control)
}

pub fn bind(
    fe: Res<FrontEnd>,
    mut dialog: ResMut<SettingsUi>,
    pointer: Res<SpuiPointer>,
    mut roots: Query<(&mut SpuiState, &SettingsRoot)>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<&Camera, With<crate::GameCamera>>,
    stage: Res<State<crate::stages::GameStage>>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
) {
    if fe.overlay != Some(Overlay::Settings) {
        return;
    }
    let dialog = &mut *dialog;
    let in_stage = control_stage_index(stage.get()).is_some();
    if let Some(i) = control_stage_index(stage.get()) {
        dialog.control_stage = i;
    }
    let Some(draft) = dialog.draft.as_mut() else { return };
    // Page host rectangle of the frame (surface pixels).
    let mut host = None;
    for (mut ui, kind) in &mut roots {
        if *kind != SettingsRoot::Frame || !ui.ready {
            continue;
        }
        for (i, (tab, _)) in id::TABS.iter().enumerate() {
            for w in ui.find_all(*tab) {
                ui.set_selected(w, i == dialog.tab);
                // ADAPTED: OpenSpore has no online services (by design). The
                // Control tab is greyed out in the galaxy as in retail.
                ui.set_enabled(w, tab_enabled(i, in_stage, dialog.demo_controls));
            }
        }
        host = find(&ui, id::HOST).map(|w| ui.rect(w));
    }
    for (mut ui, kind) in &mut roots {
        let SettingsRoot::Page(tab) = *kind else { continue };
        if !ui.ready {
            continue;
        }
        if let (Some(h), Some(&root)) = (host, ui.layout.roots.first()) {
            let area = [h.x1, h.y1, h.x2, h.y2];
            if ui.area(root) != area {
                ui.set_area(root, area);
            }
            // Pages are hidden in their files until the dialog shows one.
            if !ui.visible(root) {
                ui.set_visible(root, true);
            }
        }
        let n = ui.layout.windows.len();
        match tab {
            // Audio: the four volume sliders and Mute All are live; speaker
            // set-up and sound quality are not OpenSpore options.
            1 => {
                let sliders = [(id::MASTER, draft.master), (id::MUSIC, draft.music), (id::VOICES, draft.voices), (id::SFX, draft.sfx)];
                for (control, value) in sliders {
                    if let Some(w) = find(&ui, control) {
                        ui.set_range(w, Some(value));
                    }
                }
                // Drag a slider: the pointer sets its value (top = loud).
                let p = pointer.position;
                if !pointer.pressed {
                    dialog.dragging = None;
                } else if dialog.dragging.is_none() {
                    dialog.dragging = p.and_then(|p| {
                        sliders.iter().map(|(c, _)| *c).find(|c| find(&ui, *c).is_some_and(|w| ui.rect(w).contains(p.x, p.y)))
                    });
                }
                if let (Some(control), Some(p)) = (dialog.dragging, p) {
                    if let Some(w) = find(&ui, control) {
                        let r = ui.rect(w);
                        let v = (1.0 - (p.y - r.y1) / r.height().max(1.0)).clamp(0.0, 1.0);
                        match control {
                            id::MASTER => draft.master = v,
                            id::MUSIC => draft.music = v,
                            id::VOICES => draft.voices = v,
                            _ => draft.sfx = v,
                        }
                    }
                }
                for w in 0..n {
                    let c = ui.layout.windows[w].control_id;
                    if ui.layout.windows[w].kind == WindowKind::Button {
                        if c == id::MUTE_ALL {
                            ui.set_selected(w, draft.mute_all);
                        } else {
                            if let Some(i) = radio_index(&id::SPEAKERS, c) {
                                ui.set_selected(w, i == draft.speakers);
                                // "Player must be in the Spore Galaxy Screen to
                                // change Speaker Configuration" (the page's own note).
                                ui.set_enabled(w, *stage.get() == crate::stages::GameStage::MainMenu);
                            } else if let Some(i) = radio_index(&id::SOUND_QUALITY, c) {
                                ui.set_selected(w, i == draft.sound_quality);
                                ui.set_enabled(w, true);
                            } else {
                                ui.set_enabled(w, false);
                            }
                        }
                    }
                }
            }
            // Graphics: Full Screen, Screen Size, Defaults and the quality
            // rows OpenSpore honours are live; the others show their value.
            0 => {
                let levels = draft.levels();
                // The window's size (the frame size when rendering offscreen).
                let size = draft
                    .resolution
                    .or_else(|| window_size(&windows))
                    .or_else(|| cameras.iter().find_map(|c| c.physical_target_size()).map(|s| [s.x, s.y]));
                for w in 0..n {
                    let win = &ui.layout.windows[w];
                    if matches!(win.kind, WindowKind::Button | WindowKind::Spinner) {
                        let control = win.control_id;
                        if control == id::FULL_SCREEN {
                            ui.set_selected(w, draft.fullscreen);
                        } else if control == id::SCREEN_SIZE {
                            ui.set_enabled(w, !draft.fullscreen);
                            if let Some([sw, sh]) = size {
                                ui.set_text(w, format!("{sw} x {sh}"));
                            }
                        } else if control == id::DEFAULTS {
                            ui.set_enabled(w, true);
                        } else if let Some((row, level)) = quality_level(&ui, w) {
                            ui.set_selected(w, level == levels[row]);
                            ui.set_enabled(w, super::graphics::HONOURED[row]);
                        } else {
                            ui.set_enabled(w, false);
                        }
                    }
                }
            }
            // Game & capture: the three game options are live; OpenSpore has
            // no movie / picture capture.
            2 => {
                for w in 0..n {
                    let win = &ui.layout.windows[w];
                    if win.kind == WindowKind::Button {
                        match win.control_id {
                            id::TUTORIALS => ui.set_selected(w, draft.tutorials),
                            id::HINTS => ui.set_selected(w, draft.hints),
                            id::EDGE_SCROLL => ui.set_selected(w, draft.edge_scroll),
                            c => {
                                if let Some(i) = radio_index(&id::MOVIE_SIZE, c) {
                                    ui.set_selected(w, i == draft.movie_size);
                                } else if let Some(i) = radio_index(&id::PICTURE_SIZE, c) {
                                    ui.set_selected(w, i == draft.picture_size);
                                } else {
                                    ui.set_enabled(w, false);
                                }
                            }
                        }
                    }
                }
                // The authored placeholder "Movie and Pic Location Text" names
                // the capture folder in retail; OpenSpore writes no captures.
                if let Some(w) = find(&ui, id::CAPTURE_LOCATION) {
                    ui.set_visible(w, false);
                }
            }
            // Controls: the window of the stage being played, and the mouse
            // diagram of the help button the pointer entered (0x600370).
            3 => {
                for (i, control) in id::CONTROL_STAGES.iter().enumerate() {
                    if let Some(w) = find(&ui, *control) {
                        if ui.visible(w) != (i == dialog.control_stage) {
                            ui.set_visible(w, i == dialog.control_stage);
                        }
                    }
                }
                // The stage option (Cell, Creature: follow the cursor; Tribe, Civ,
                // Space: rotate with the right mouse button). Every stage reads its
                // own (`crate::control_options`), so all five boxes are live.
                let option = draft.stage_controls[dialog.control_stage.min(4)];
                for w in 0..n {
                    let win = &ui.layout.windows[w];
                    if win.kind != WindowKind::Button {
                        continue;
                    }
                    let c = win.control_id;
                    if c == id::STAGE_OPTION {
                        let stage = stage_window_of(&ui, w).unwrap_or(0);
                        ui.set_selected(w, draft.stage_controls[stage.min(4)]);
                        ui.set_enabled(w, true);
                    } else if (id::HELP_BUTTON..id::HELP_BUTTON + id::HELP_BUTTONS).contains(&c) {
                        // Hover buttons: live while their stage window shows.
                        ui.set_enabled(w, true);
                        if ui.states[w] & crate::spui::ids::state::HOVER != 0 {
                            dialog.help = Some((c - id::HELP_BUTTON) as usize);
                        }
                    } else {
                        ui.set_enabled(w, false);
                    }
                }
                let shown = dialog.help.map(|b| help_view(b, option));
                for part in id::MOUSE_PARTS {
                    if let Some(w) = find(&ui, part) {
                        let on = shown.is_some_and(|(parts, _)| parts.contains(&part))
                            || (part == id::MOUSE_CAPTION && shown.is_some());
                        if ui.visible(w) != on {
                            ui.set_visible(w, on);
                        }
                    }
                }
                if let (Some((_, text)), Some(w)) = (shown, find(&ui, id::MOUSE_CAPTION)) {
                    if let Some(caption) = assets.localized(&originals.resource_index, SETTINGS_TABLE, text) {
                        ui.set_text(w, caption);
                    }
                }
            }
            // Online, credits: informational or not available.
            _ => {
                for w in 0..n {
                    if ui.layout.windows[w].kind == WindowKind::Button {
                        ui.set_enabled(w, false);
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn actions(
    mut fe: ResMut<FrontEnd>,
    mut dialog: ResMut<SettingsUi>,
    mut user: ResMut<UserSettings>,
    mut roots: Query<(&mut SpuiState, &SettingsRoot)>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut sounds: MessageWriter<crate::game_audio::PlayCue>,
    stage: Res<State<crate::stages::GameStage>>,
    monitors: Query<&bevy::window::Monitor>,
) {
    if fe.overlay != Some(Overlay::Settings) {
        return;
    }
    let dialog = &mut *dialog;
    let mut clicks = Vec::new();
    for (mut ui, kind) in &mut roots {
        let quality = *kind == SettingsRoot::Page(0);
        for w in ui.take_clicks() {
            let level = if quality { quality_level(&ui, w) } else { None };
            clicks.push((*kind, ui.layout.window(w).control_id, level));
        }
    }
    for (kind, control, level) in clicks {
        sounds.write(crate::game_audio::PlayCue(crate::game_audio::Cue::UiClick));
        let Some(draft) = dialog.draft.as_mut() else { return };
        match (kind, control) {
            (SettingsRoot::Frame, id::ACCEPT) => {
                let mut accepted = draft.clone();
                accepted.path = user.path.clone();
                let fullscreen_changed = accepted.fullscreen != user.fullscreen;
                let size_changed = accepted.resolution != user.resolution;
                *user = accepted;
                user.publish();
                if let Err(error) = user.store() {
                    fe.status = format!("Cannot save settings: {error}");
                }
                if fullscreen_changed || size_changed {
                    for mut window in &mut windows {
                        apply_window(&user, &mut window);
                    }
                }
                fe.overlay = None;
                return;
            }
            (SettingsRoot::Frame, id::CANCEL) => {
                fe.overlay = None;
                return;
            }
            (SettingsRoot::Frame, c) => {
                if let Some(i) = id::TABS.iter().position(|(t, _)| *t == c) {
                    if tab_enabled(i, *stage.get() != crate::stages::GameStage::MainMenu, dialog.demo_controls) {
                        dialog.tab = i;
                    }
                }
            }
            (SettingsRoot::Page(_), id::MUTE_ALL) => draft.mute_all = !draft.mute_all,
            (SettingsRoot::Page(_), id::FULL_SCREEN) => draft.fullscreen = !draft.fullscreen,
            (SettingsRoot::Page(_), id::TUTORIALS) => draft.tutorials = !draft.tutorials,
            (SettingsRoot::Page(_), id::HINTS) => draft.hints = !draft.hints,
            (SettingsRoot::Page(_), id::EDGE_SCROLL) => draft.edge_scroll = !draft.edge_scroll,
            // The Control page's check box of the stage window that shows
            // (0x0679B630: `IConfigManager::Set(option, state)` of that stage).
            (SettingsRoot::Page(_), id::STAGE_OPTION) => {
                let stage = dialog.control_stage.min(4);
                draft.stage_controls[stage] = !draft.stage_controls[stage];
            }
            (SettingsRoot::Page(_), c) if radio_index(&id::SPEAKERS, c).is_some() => {
                if *stage.get() == crate::stages::GameStage::MainMenu {
                    draft.speakers = radio_index(&id::SPEAKERS, c).unwrap_or(0);
                }
            }
            (SettingsRoot::Page(_), c) if radio_index(&id::SOUND_QUALITY, c).is_some() => {
                draft.sound_quality = radio_index(&id::SOUND_QUALITY, c).unwrap_or(2);
            }
            (SettingsRoot::Page(_), c) if radio_index(&id::MOVIE_SIZE, c).is_some() => {
                draft.movie_size = radio_index(&id::MOVIE_SIZE, c).unwrap_or(1);
            }
            (SettingsRoot::Page(_), c) if radio_index(&id::PICTURE_SIZE, c).is_some() => {
                draft.picture_size = radio_index(&id::PICTURE_SIZE, c).unwrap_or(1);
            }
            (SettingsRoot::Page(0), id::SCREEN_SIZE) => {
                let current = draft.resolution.or_else(|| windows.single().ok().map(|w| [w.resolution.width() as u32, w.resolution.height() as u32]));
                // The video modes of the monitors (retail lists the adapter's
                // modes); the built-in list when none reports any.
                let modes = super::graphics::screen_sizes(monitors.iter().flat_map(|m| m.video_modes.iter().map(|v| [v.physical_size.x, v.physical_size.y])));
                draft.resolution = Some(super::graphics::next_screen_size_in(&modes, current.unwrap_or([1280, 900])));
            }
            // Defaults: the configured level of every quality row.
            (SettingsRoot::Page(0), id::DEFAULTS) => {
                for (row, level) in super::graphics::DEFAULTS.iter().enumerate() {
                    draft.set_level(row, *level);
                }
            }
            (SettingsRoot::Page(0), _) => {
                if let Some((row, level)) = level.filter(|(row, _)| super::graphics::HONOURED[*row]) {
                    draft.set_level(row, level);
                }
            }
            _ => {}
        }
    }
}

/// Row and level of a quality radio button (`graphics::button_level`: its
/// row window is the option, its command the level).
fn quality_level(ui: &SpuiState, w: WinId) -> Option<(usize, u8)> {
    let win = ui.layout.window(w);
    if win.kind != WindowKind::Button || win.control_id != 0 {
        return None;
    }
    let parent = ui.layout.window(win.parent?).control_id;
    super::graphics::button_level(parent, win.command_id)
}

fn window_size(windows: &Query<&Window, With<bevy::window::PrimaryWindow>>) -> Option<[u32; 2]> {
    windows.single().ok().map(|w| [w.resolution.width() as u32, w.resolution.height() as u32])
}

/// Full screen or the chosen windowed size.
pub fn apply_window(user: &UserSettings, window: &mut Window) {
    window.mode = if user.fullscreen {
        bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
    } else {
        bevy::window::WindowMode::Windowed
    };
    if let (false, Some([w, h])) = (user.fullscreen, user.resolution) {
        window.resolution.set(w as f32, h as f32);
    }
}

/// Apply the accepted settings the stages read: edge scrolling (Tribal and
/// Civ camera controls, again when a stage starts), the Shadows level
/// (`graphics::apply_shadows`) and Sound Quality (the PD engine's
/// `audioPerformanceLevel`, set when it changes and before the engine's first
/// tick); the other levels are read where they apply.
#[allow(clippy::type_complexity)]
pub fn apply(
    mut commands: Commands,
    user: Res<UserSettings>,
    stage: Res<State<crate::stages::GameStage>>,
    tribal: Option<ResMut<crate::tribal_controls::TribalControls>>,
    civ: Option<ResMut<crate::civ_controls::CivControls>>,
    mut suns: Query<
        (Entity, &mut DirectionalLight, Option<&mut bevy::light::CascadeShadowConfig>, Option<&super::graphics::CascadesBackup>),
        With<crate::GameSun>,
    >,
    mut last_shadows: Local<Option<u8>>,
    pd: Option<ResMut<crate::pd_audio::PdMusic>>,
    mut last_performance: Local<Option<u32>>,
) {
    let performance = audio_performance_level(user.sound_quality);
    if let Some(mut pd) = pd {
        if *last_performance != Some(performance) {
            *last_performance = Some(performance);
            pd.set_audio_param_hash(id::AUDIO_PERFORMANCE_LEVEL, performance as f32);
        }
    }
    if user.is_changed() || stage.is_changed() {
        if let Some(mut tribal) = tribal {
            tribal.edge_scroll = user.edge_scroll;
        }
        if let Some(mut civ) = civ {
            civ.edge_scroll = user.edge_scroll;
        }
    }
    let changed = *last_shadows != Some(user.shadows);
    *last_shadows = Some(user.shadows);
    super::graphics::apply_shadows(&mut commands, user.shadows, changed, &mut suns);
}

/// The scrolling credits on the Credits page: a retail
/// `UI::ScrollFrameVertical` page (set-up 0x600CA0, message handler 0x601380)
/// that starts at the top, begins to scroll by itself after a pause and also
/// answers the mouse wheel.
///
/// VERIFIED (march2017 image):
/// * the page keeps a seconds clock (+0x80) and a milliseconds clock (+0x60);
///   once the seconds clock reads more than 3 (`GetElapsedTime` = ticks + 0.5,
///   truncated: 3.5 s, 0x6013E5) the auto-scroll flag (+0x98) is set;
/// * every update message (0xC) adds [`CREDITS_STEP`] = 0.1 (0x14887D4) to the
///   target scroll position (+0x38) while the flag is set and the page is
///   visible (0x601447); the update message is sent once per frame, so retail's
///   rate follows the frame rate (6 px/s at 60 frames per second);
/// * the shown position (+0x3C) chases the target by
///   `clamp(milliseconds since the last update * 0.008, 0, 1)` of the gap per
///   update (0x601471: `0x5A6DF0` is the 0..1 clamp, 0x13F6A50 = 0.008) and
///   snaps to it within half a unit (0x6014CD);
/// * the mouse wheel (message 9) moves the target by `delta * 0.8` against the
///   wheel (0x601619: 0x1485484 = 0.8), clamped to `0 ..= content - view`
///   (0x601625 .. 0x601656); the wheel delta of Windows is 120 per notch;
/// * nothing wraps: the list stops at its end.
#[derive(Component)]
pub struct CreditsRoll {
    /// Seconds since the page opened (the retail seconds clock).
    age: f32,
    /// The target position (+0x38) and the shown one (+0x3C), in pixels.
    target: f32,
    shown: f32,
}

/// Seconds before the credits start to scroll (the clock must read > 3 after
/// its rounding, 0x6013DC .. 0x6013EA).
pub const CREDITS_DELAY: f32 = 3.5;
/// Pixels the target moves per update message (0x14887D4).
pub const CREDITS_STEP: f32 = 0.1;
/// Update messages per second OpenSpore assumes (ADAPTED: retail sends one per
/// rendered frame, so its speed follows the frame rate; this fixes it at 60).
pub const CREDITS_UPDATES_PER_SECOND: f32 = 60.0;
/// Pixels per wheel unit (0x1485484); a wheel notch is 120 units.
pub const CREDITS_WHEEL: f32 = 0.8;

impl CreditsRoll {
    fn new() -> Self {
        Self { age: 0.0, target: 0.0, shown: 0.0 }
    }

    /// One update: `dt` seconds since the last, `wheel` the notches turned
    /// (positive = wheel up = towards the top), `max` the scroll range.
    pub fn step(&mut self, dt: f32, wheel: f32, max: f32) {
        self.age += dt;
        if self.age > CREDITS_DELAY {
            self.target += CREDITS_STEP * CREDITS_UPDATES_PER_SECOND * dt;
        }
        if wheel != 0.0 {
            self.target = (self.target - wheel * 120.0 * CREDITS_WHEEL).clamp(0.0, max.max(0.0));
        }
        // The list ends where the content does.
        self.target = self.target.min(max.max(0.0));
        if (self.target - self.shown).abs() > 0.0 {
            let k = (dt * 1000.0 * 0.008).clamp(0.0, 1.0);
            self.shown += (self.target - self.shown) * k;
            if (self.target - self.shown).abs() < 0.5 {
                self.shown = self.target;
            }
        }
    }
}

#[derive(Component)]
pub struct CreditsText;

/// The installed credits: the core team table (`B4212552`, sections as
/// `# TITLE` comments) and the extended team / thanks table (`D4041E7E`, the
/// table CoreCredits names), in file order. ADAPTED: retail lays out the
/// `CreditsNames` list (keys with title / name styles); its keys do not
/// resolve to installed strings, so the tables are shown as written.
pub fn credits_text(index: &crate::asset_catalog::ResourceIndex) -> String {
    let mut out = String::new();
    for table in [0xB421_2552u32, 0xD404_1E7E] {
        let key = index
            .records
            .keys()
            .filter(|k| k.type_id == view::LOCALE_TABLE_TYPE && k.instance_id == table)
            .min_by_key(|k| k.group_id)
            .copied();
        let Some(bytes) = key.and_then(|k| index.read(k).ok()) else { continue };
        let text = String::from_utf8_lossy(&bytes);
        out.push_str(&credits_lines(&text));
        out.push('\n');
    }
    out
}

/// One credits table as display lines.
pub fn credits_lines(text: &str) -> String {
    let mut out = String::new();
    for raw in text.lines() {
        let line = raw.trim_start_matches('\u{feff}').trim();
        if let Some(title) = line.strip_prefix('#') {
            let title = title.trim();
            if title.starts_with("--") || title.to_uppercase().contains("WARNING") || title.is_empty() {
                continue;
            }
            out.push_str(&format!("\n{title}\n\n"));
        } else if line.is_empty() {
            out.push('\n');
        } else if let Some((id, rest)) = line.split_once(char::is_whitespace) {
            if id.starts_with("0x") || id.starts_with("0X") {
                out.push_str(rest.trim());
                out.push('\n');
            }
        }
    }
    out
}

/// Show and scroll the credits while the Credits page is open.
#[allow(clippy::too_many_arguments)]
pub fn credits_roll(
    mut commands: Commands,
    time: Res<Time<Real>>,
    fe: Res<FrontEnd>,
    dialog: Res<SettingsUi>,
    originals: Res<crate::Originals>,
    assets: Res<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(&SpuiState, &SettingsRoot)>,
    mut rolls: Query<(Entity, &mut Node, &mut CreditsRoll), Without<CreditsText>>,
    mut texts: Query<(&mut Node, &ComputedNode), With<CreditsText>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut cache: Local<Option<String>>,
) {
    let open = fe.overlay == Some(Overlay::Settings) && dialog.tab == 5;
    let page = roots.iter().find(|(ui, k)| **k == SettingsRoot::Page(5) && ui.ready).map(|(ui, _)| ui);
    let area = page.and_then(|ui| ui.find(id::CREDITS_VIEW).map(|w| ui.rect(w)));
    let notches: f32 = wheel
        .read()
        .map(|e| match e.unit {
            bevy::input::mouse::MouseScrollUnit::Line => e.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => e.y / 120.0,
        })
        .sum();
    let (Some(area), true) = (area, open) else {
        for (e, _, _) in &rolls {
            commands.entity(e).despawn();
        }
        return;
    };
    if rolls.is_empty() {
        let Ok(camera) = camera.single() else { return };
        let text = cache.get_or_insert_with(|| credits_text(&originals.resource_index).trim_start().to_string()).clone();
        let font = assets.font_source("Trebuchet EA");
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(area.x1),
                    top: Val::Px(area.y1),
                    width: Val::Px(area.width()),
                    height: Val::Px(area.height()),
                    overflow: Overflow::clip(),
                    ..default()
                },
                GlobalZIndex(205),
                UiTargetCamera(camera),
                Pickable::IGNORE,
                CreditsRoll::new(),
                Name::new("credits roll"),
            ))
            .with_children(|p| {
                p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        // The list starts at the top of the frame (position 0).
                        top: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        ..default()
                    },
                    Text::new(text),
                    TextFont { font, font_size: bevy::text::FontSize::Px(13.0), ..default() },
                    TextColor(Color::srgb(0.85, 0.9, 1.0)),
                    TextLayout::justify(Justify::Center),
                    Pickable::IGNORE,
                    CreditsText,
                ));
            });
        return;
    }
    for (_, mut node, mut roll) in &mut rolls {
        node.left = Val::Px(area.x1);
        node.top = Val::Px(area.y1);
        for (mut text, computed) in &mut texts {
            // Scroll range: the laid-out text height beyond the frame
            // (+0x40 of the page, from the scroll bar's range).
            let height = computed.size().y * computed.inverse_scale_factor();
            roll.step(time.delta_secs(), notches, height - area.height());
            text.top = Val::Px(-roll.shown);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_tables_read_as_titled_lines() {
        let table = "\u{feff}# WARNING: generated\n\n# DEVELOPMENT TEAM\n\n0xf8994976 Rajesh Ajjanagadde\n0x1cdb1702 Brodie Andersen\n#--disabled--0x2780f32e Bill Willis\n";
        let text = credits_lines(table);
        assert!(text.contains("DEVELOPMENT TEAM\n"));
        assert!(text.contains("Rajesh Ajjanagadde\nBrodie Andersen\n"));
        assert!(!text.contains("WARNING") && !text.contains("Bill Willis"));
    }

    #[test]
    fn control_page_help_follows_the_enter_handler() {
        // 0x600370: Move (button 0) shows four diagram parts, Zoom (1) the body
        // and the wheel; the option changes the caption of the camera buttons.
        let (parts, text) = help_view(0, false);
        assert_eq!((parts.len(), text), (4, 0x090A_50AA));
        assert_eq!(help_view(1, true), help_view(1, false));
        assert_eq!(help_view(6, true).1, 0x090A_50A1);
        assert_eq!(help_view(6, false).1, 0x090A_50A8);
        assert_eq!(help_view(10, true).1, 0x090A_50A5);
        assert_eq!(help_view(10, false).1, 0x090A_50AC);
        // Every caption of the table is reachable and every part id is a diagram window.
        for b in 0..id::HELP_BUTTONS as usize {
            for option in [false, true] {
                let (parts, text) = help_view(b, option);
                assert!((0x090A_50A1..=0x090A_50AC).contains(&text));
                assert!(parts.iter().all(|p| id::MOUSE_PARTS.contains(p)));
            }
        }
        assert_eq!(control_stage_index(&crate::stages::GameStage::MainMenu), None);
        assert_eq!(control_stage_index(&crate::stages::GameStage::Space), Some(4));
    }

    #[test]
    fn capture_and_speaker_choices_are_kept_in_the_file() {
        let s: UserSettings = serde_json::from_str("{}").unwrap();
        assert_eq!((s.speakers, s.sound_quality, s.movie_size, s.picture_size), (0, 2, 1, 1));
        // ConfigManager.txt: every OptionXControls starts off.
        assert_eq!(s.stage_controls, [false; 5]);
        let c: UserSettings = serde_json::from_str(r#"{"stage_controls":[true,false,false,false,false]}"#).unwrap();
        assert!(c.stage_controls[0] && !c.stage_controls[1]);
        let t: UserSettings = serde_json::from_str(r#"{"sound_quality":0,"picture_size":2}"#).unwrap();
        assert_eq!((t.sound_quality, t.picture_size, t.movie_size), (0, 2, 1));
        assert_eq!(radio_index(&id::SOUND_QUALITY, 0x048D_F95D), Some(2));
        assert_eq!(radio_index(&id::SPEAKERS, 1), None);
    }

    #[test]
    fn stored_options_map_to_the_retail_option_values() {
        // Options.txt: Low / Medium / High audio performance 1 / 2 / 3.
        assert_eq!([0, 1, 2].map(audio_performance_level), [1, 2, 3]);
        assert_eq!(id::AUDIO_PERFORMANCE_LEVEL, 0x6DF5_822D);
        // The speaker mode setter accepts 2, 4 and 5 only.
        assert_eq!([0, 1, 2].map(speaker_mode), [2, 4, 5]);
        // OptionPhotoRes (width 800 / 1280 / 2400, 4:3) and OptionVideoRes.
        assert_eq!([0, 1, 2].map(picture_resolution), [[800, 600], [1280, 960], [2400, 1800]]);
        assert_eq!([0, 1, 2].map(movie_resolution), [[160, 120], [320, 240], [639, 480]]);
        assert_eq!(picture_resolution(9), [2400, 1800]);
    }

    #[test]
    fn the_control_tab_is_greyed_out_in_the_galaxy_only() {
        // 0x600960 answers false outside the five stage modes, and the open
        // routine then disables the tab (0x6034E2); Online is off by design.
        assert!(!tab_enabled(id::TAB_CONTROL, false, false));
        assert!(tab_enabled(id::TAB_CONTROL, true, false));
        assert!(tab_enabled(id::TAB_CONTROL, false, true), "the capture hook");
        assert!(!tab_enabled(id::TAB_ONLINE, true, false));
        for tab in [0, 1, 2, 5] {
            assert!(tab_enabled(tab, false, false));
        }
    }

    #[test]
    fn credits_wait_then_creep_and_stop_at_the_end() {
        // 0x601380: nothing moves for 3.5 s, then 0.1 px per update (6 px/s at
        // 60 updates a second), the shown position chases the target, nothing
        // wraps, the wheel moves it 0.8 x 120 = 96 px per notch.
        let mut roll = CreditsRoll::new();
        let frame = 1.0 / 60.0;
        for _ in 0..(3.4 / frame) as usize {
            roll.step(frame, 0.0, 1000.0);
        }
        assert_eq!((roll.target, roll.shown), (0.0, 0.0), "the pause");
        for _ in 0..(10.0 / frame) as usize {
            roll.step(frame, 0.0, 1000.0);
        }
        // About 6.6 s after the pause, 6 px/s.
        assert!((roll.target - 6.0 * (13.4 - 3.5)).abs() < 1.5, "{}", roll.target);
        assert!(roll.shown > 0.0 && roll.shown <= roll.target);
        // The end of the content stops it.
        for _ in 0..(400.0 / frame) as usize {
            roll.step(frame, 0.0, 100.0);
        }
        assert_eq!((roll.target, roll.shown), (100.0, 100.0));
        // The wheel: one notch up moves 96 px towards the top (the creep adds
        // its 0.1 px first), never past it.
        roll.step(frame, 1.0, 100.0);
        assert!((roll.target - 4.1).abs() < 0.05, "{}", roll.target);
        roll.step(frame, 3.0, 100.0);
        assert_eq!(roll.target, 0.0);
        // A turn down is clamped to the range.
        roll.step(frame, -1.0, 50.0);
        assert!(roll.target <= 50.0);
    }

    #[test]
    fn old_settings_files_keep_the_new_defaults() {
        let s: UserSettings = serde_json::from_str(r#"{"master":0.5}"#).unwrap();
        assert!(s.tutorials && s.hints && !s.edge_scroll);
        assert_eq!(s.shadows, 2);
        // ConfigManager.txt defaults: Texture Detail Medium, the rest High.
        assert_eq!(s.levels(), [2, 1, 2, 2, 2, 2, 2]);
        assert_eq!(s.resolution, None);
        assert_eq!(s.master, 0.5);
    }

    #[test]
    fn settings_file_sits_beside_the_save_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("openspore-fe-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let save = dir.join("qc_save.json");
        assert_eq!(UserSettings::path_for(&save), dir.join("qc_save-settings.json"));
        // `load_beside` publishes the gains: keep them at the defaults so
        // parallel audio tests see unit gains.
        let mut s: UserSettings = serde_json::from_str("{}").unwrap();
        s.path = Some(UserSettings::path_for(&save));
        assert_eq!(s.master, 1.0);
        s.music = 0.25;
        s.mute_all = true;
        s.store().unwrap();
        let t: UserSettings = serde_json::from_slice(&std::fs::read(UserSettings::path_for(&save)).unwrap()).unwrap();
        assert_eq!((t.music, t.mute_all), (0.25, true));
        assert_eq!(t.gains(), [0.0, 0.25, 1.0, 1.0], "Mute All silences the master");
        assert_eq!(bus_gain(crate::game_audio::Bus::Music), 1.0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
