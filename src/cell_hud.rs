//! The retail Cell-stage HUD: spawns the SPUI layouts the original game loads
//! for the Cell stage and binds them to [`CellHudState`].
//!
//! * `GlobalUICell-3`: bottom bar (DNA readout, five-segment progress bar with
//!   level knobs, history/sporepedia/collection/pause buttons), zoom buttons,
//!   Mating Call button and tray, health meter and portrait frame.
//! * `GlobalUIOptions-2`: the swirl orb (options/menu button) at the bottom left.
//! * `CLGMissionCard`: the goal card at the top left.
//! * `CellHealth_New`: the name/health rollover shown over the cell under the
//!   cursor.
//!
//! The layouts carry all geometry and art. This module only decides which
//! windows are shown, fills their text and meters, and turns button clicks into
//! the existing keyboard actions so the engine needs no change.
//!
//! Control ids and roles come from the layouts themselves (tooltip strings of
//! the buttons, window names). Where the retail code's exact behaviour is not
//! yet documented by `cell-re` the binding is marked `ADAPTED`.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

pub mod dialogs;
pub mod portrait;
pub mod timeline;

use crate::{
    asset_catalog::ResourceIndex,
    cell_contract::{CellGoalCard, CellHudState, CellRollover},
    spui::{
        model::WinId,
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

pub const MAIN_LAYOUT: &str = "GlobalUICell-3";
pub const OPTIONS_LAYOUT: &str = "GlobalUIOptions-2";
pub const CARD_LAYOUT: &str = "CLGMissionCard";
pub const ROLLOVER_LAYOUT: &str = "CellHealth_New";

/// Control ids (roles from the layouts and their tooltips).
pub mod id {
    // GlobalUICell-3
    pub const DNA_TEXT: u32 = 0xABCD_1001;
    pub const MATE_BUTTON: u32 = 0x0448_37C0;
    pub const MATE_TRAY: u32 = 0x0400_1120;
    pub const PAUSE_TOGGLE: u32 = 0xD305_CA84;
    pub const ZOOM_IN: u32 = 0x05CA_B570;
    pub const ZOOM_OUT: u32 = 0x05CA_B571;
    pub const HEALTH_FILL: u32 = 0x0665_FE90;
    pub const BAR: u32 = 0x051B_5478;
    pub const SEGMENTS: [u32; 5] = [0x051B_54C0, 0x051B_54C1, 0x051B_54C2, 0x051B_54C3, 0x051B_54C4];
    pub const MARKER: u32 = 0x051B_5168;
    /// Advance to Creature (hidden until the ending), its glow frame and the
    /// History button it replaces (RE 10C step 4).
    pub const ADVANCE: u32 = 0x0645_5C5E;
    pub const ADVANCE_GLOW: u32 = 0x0652_A628;
    pub const HISTORY: u32 = 0x0448_27B8;
    /// Speech bubbles of the layout (invisible until shown, each fades and
    /// inflates in): call mate introduction, call mate reminder, stage end
    /// ("go to creature") reminder.
    pub const BUBBLE_MATE_INTRO: u32 = 0x05AF_4AE0;
    pub const BUBBLE_MATE_REMINDER: u32 = 0x05AF_4AE1;
    pub const BUBBLE_STAGE_END: u32 = 0x05AF_4AE2;
    /// Text and frame windows of those bubbles.
    pub const BUBBLE_TEXT: [u32; 3] = [0x05AF_4AF0, 0x05AF_4AF1, 0x05AF_4AF2];
    pub const BUBBLE_FRAME: [u32; 3] = [0x05AF_4BF0, 0x05AF_4BF1, 0x05AF_4BF2];
    // GlobalUIOptions-2: swirl orb button and its menu panel
    pub const OPTIONS_ORB: u32 = 0x010E_D852;
    pub const OPTIONS_PANEL: u32 = 0x0615_B50D;
    pub const MENU_FRAME: u32 = 0x064C_23F8;
    /// Resume Game (also the panel's close button).
    pub const MENU_RESUME: u32 = 0x0595_E0A8;
    pub const MENU_GUIDE: u32 = 0x0447_060A;
    pub const MENU_SPYGLASS: u32 = 0x0620_25F9;
    pub const MENU_BAN: u32 = 0x044E_A79E;
    pub const MENU_SETTINGS: u32 = 0x043B_72D0;
    pub const MENU_SAVE: u32 = 0x010F_366E;
    pub const MENU_EXIT: u32 = 0x0226_8BA4;
    pub const MENU_QUIT: u32 = 0x010F_366D;
    // CLGMissionCard
    pub const CARD_ROOT: u32 = 0x055E_BC68;
    pub const CARD_TITLE: u32 = 0x0523_50C8;
    pub const CARD_COUNT: u32 = 0x740E_A83E;
    pub const CARD_BODY: u32 = 0x0523_5140;
    pub const CARD_OVERFLOW: u32 = 0x74CB_0C14;
    /// Dark text panel under the header and the clipping group holding the
    /// description text.
    pub const CARD_PANEL: u32 = 0x54BF_5692;
    pub const CARD_GROUP: u32 = 0x74BF_56AE;
    // CellHealth_New
    pub const ROLL_ROOT: u32 = 0x9331_0E4A;
    pub const ROLL_NAME: u32 = 0x031E_94D0;
    pub const ROLL_GREEN: u32 = 0x026E_0DF0;
    pub const ROLL_YELLOW: u32 = 0x026E_0DF1;
    pub const ROLL_RED: u32 = 0x026E_0DF2;
    pub const ROLL_SILVER: u32 = 0x026E_0DF4;
    /// Bar frame (full size), the name plate (name + part icons), the two
    /// part icon slots (`ROLL_PART_SLOT + n`) and the icon windows
    /// (`ROLL_PART_ICON + unlock slot`).
    pub const ROLL_FRAME: u32 = 0x026E_0DF3;
    pub const ROLL_PLATE: u32 = 0x9331_03EA;
    pub const ROLL_PART_SLOT: u32 = 0x9331_0400;
    pub const ROLL_PART_ICON: u32 = 0x9331_0500;
}

/// Material colours of the progress bar segments. The retail code swaps the
/// segment materials by the tuning properties `ProgressBarGameLevelMaterial*`
/// (installed property file `0x5C770DB7`); the defaults are the installed
/// values, used when the file cannot be read.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ProgressColors {
    /// Filled part.
    pub left: u32,
    /// Filled part while health/DNA fails (not shown yet).
    pub left_fail: u32,
    /// Part just gained (flash).
    pub delta: u32,
    pub delta_fail: u32,
    pub middle: u32,
    /// Unfilled part.
    pub right: u32,
    loaded: bool,
}

impl Default for ProgressColors {
    fn default() -> Self {
        let rgb = |r: f32, g: f32, b: f32| -> u32 {
            let q = |v: f32| ((v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32) & 0xFF;
            0xFF00_0000 | q(r) << 16 | q(g) << 8 | q(b)
        };
        Self {
            left: rgb(0.0, 1.0, 0.0),
            left_fail: rgb(1.0, 0.0, 0.0),
            delta: rgb(1.0, 1.0, 0.0),
            delta_fail: rgb(1.0, 1.0, 0.0),
            middle: rgb(0.2, 0.25, 0.6),
            right: rgb(0.2, 0.25, 0.6),
            loaded: false,
        }
    }
}

impl ProgressColors {
    fn load(&mut self, index: &ResourceIndex) {
        self.loaded = true;
        let key = crate::dbpf::ResourceKey {
            group_id: 0,
            instance_id: 0x5C77_0DB7,
            type_id: crate::asset_catalog::PROP,
        };
        let Ok(props) = index.properties(key) else {
            return;
        };
        let read = |name: &str, into: &mut u32| {
            if let Some([r, g, b]) = props
                .0
                .get(&crate::world_objects::name_id(name))
                .and_then(|p| p.color_rgb())
            {
                let q = |v: f32| ((v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32) & 0xFF;
                *into = 0xFF00_0000 | q(r) << 16 | q(g) << 8 | q(b);
            }
        };
        read("ProgressBarGameLevelMaterialLeft", &mut self.left);
        read("ProgressBarGameLevelMaterialLeftFail", &mut self.left_fail);
        read("ProgressBarGameLevelMaterialDelta", &mut self.delta);
        read("ProgressBarGameLevelMaterialDeltaFail", &mut self.delta_fail);
        read("ProgressBarGameLevelMaterialMiddle", &mut self.middle);
        read("ProgressBarGameLevelMaterialRight", &mut self.right);
    }
}

/// The DNA caption is the number preceded by the DNA symbol glyph (U+268B of
/// `SporeCurrencyEA`; retail formats it as `%-F%-p`, glyph 0x268B in Cell mode).
const DNA_GLYPH: char = '\u{268B}';
/// ADAPTED: em size of the DNA caption measured from the retail frames (the
/// style says 14 pt = 19 px, but the 13 px high caption window shows ~16 px
/// digits).
const DNA_TEXT_PX: f32 = 16.0;

/// Card root origin: the mission log manager's base position, 5.0 / 5.0
/// (constructor `0xE17419` writes `[mgr+0x5C]` and `[mgr+0x60]`); the first
/// card's root window is moved there (`0xE14B50`, `SetLocation`).
const CARD_X: f32 = 5.0;
const CARD_Y: f32 = 5.0;

/// Width of the health fill window at full health (authored area).
const HEALTH_FULL: f32 = 60.0;

/// Which of the Cell layouts a spawned root is.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellHudRoot {
    Main,
    Orb,
    Card,
    Rollover,
    /// Hint bar (`hint` layout) and callout dialog (`dialogs`).
    Hint,
    Callout,
}

/// Retail timings of the HUD speech bubbles (RE `notes\cell-re-notes.md` 8):
/// the mate introduction shows during the 6 s after the first meal past 149
/// food; the reminders show 10 s of every 30 s.
pub const BUBBLE_INTRO_SECONDS: f32 = 6.0;
pub const BUBBLE_ON_SECONDS: f32 = 10.0;
pub const BUBBLE_PERIOD_SECONDS: f32 = 30.0;
/// The mate reminder needs more than this much DNA earned since the last
/// creator visit (RE 8: `DNAearned - serializable.field_30 > 14`).
pub const BUBBLE_REMINDER_DNA: i32 = 14;

/// Seconds (of running game) the mate button and the Advance button have been
/// shown, which drive the speech bubbles.
#[derive(Resource, Default, Debug)]
pub struct BubbleClock {
    pub mate: f32,
    pub advance: f32,
    /// The mate introduction has been shown once (the retail timer is armed
    /// by the first meal / part pickup only): later appearances of the button
    /// start in the reminder cycle.
    pub intro_done: bool,
}

/// Advance the bubble timers by `dt`: they run with the game (not while a
/// callout or the menu pauses it) and restart whenever their button goes away.
pub fn step_clock(clock: &mut BubbleClock, hud: &CellHudState, dt: f32) {
    if !hud.callouts.is_empty() || hud.paused {
        return;
    }
    if hud.show_mate_button {
        let start = if clock.intro_done { BUBBLE_INTRO_SECONDS } else { 0.0 };
        clock.mate = clock.mate.max(start) + dt;
        clock.intro_done |= clock.mate >= BUBBLE_INTRO_SECONDS;
    } else {
        clock.mate = 0.0;
    }
    clock.advance = if hud.advance_visible { clock.advance + dt } else { 0.0 };
}

/// Which speech bubbles show: (mate introduction, mate reminder, stage end).
pub fn bubbles(clock: &BubbleClock, hud: &CellHudState) -> (bool, bool, bool) {
    let free = hud.callouts.is_empty() && !hud.paused && !hud.hud_hidden && hud.summary.is_none();
    let cycle = |t: f32, start: f32| ((t - start) % BUBBLE_PERIOD_SECONDS) < BUBBLE_ON_SECONDS;
    let mate = free && hud.show_mate_button;
    let intro = mate && clock.mate < BUBBLE_INTRO_SECONDS;
    let reminder = mate
        && clock.mate >= BUBBLE_INTRO_SECONDS
        && hud.dna_since_creator > BUBBLE_REMINDER_DNA
        && cycle(clock.mate, BUBBLE_INTRO_SECONDS);
    let end = free && hud.advance_visible && cycle(clock.advance, 0.0);
    (intro, reminder, end)
}

/// True while the SPUI Cell HUD is shown; the legacy Cell HUD steps aside.
#[derive(Resource, Default)]
pub struct CellHudActive(pub bool);

pub struct CellHudPlugin;

impl Plugin for CellHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CellHudState>()
            .init_resource::<CellHudActive>()
            .init_resource::<ProgressColors>()
            .init_resource::<dialogs::DialogCache>()
            .init_resource::<portrait::PortraitState>()
            .init_resource::<BubbleClock>()
            .init_resource::<RolloverState>()
            .init_resource::<timeline::TimelineCache>()
            .init_resource::<timeline::thumbs::GenerationThumbs>()
            .add_systems(
                Update,
                (manage, demo_state, bind, dialogs::bind, timeline::manage, timeline::bind, actions, overlays, letterbox)
                    .chain()
                    .before(view::sync)
                    .after(view::input)
                    .after(crate::cell_flow::publish_hud),
            )
            // The portrait scene follows the cell; its node sits over the
            // portrait window once the HUD has been solved.
            .add_systems(
                Update,
                (
                    portrait::demo_parts,
                    portrait::update,
                    portrait::place,
                    timeline::thumbs::record,
                    timeline::thumbs::update,
                    timeline::thumbnail,
                )
                    .chain()
                    .after(view::sync)
                    .after(manage),
            );
    }
}

/// Spawn or despawn the HUD layouts depending on the stage.
#[allow(clippy::too_many_arguments)]
fn manage(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    editor: Res<crate::editor::EditorState>,
    options: Res<crate::HudOptions>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut active: ResMut<CellHudActive>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &CellHudRoot)>,
) {
    let want = *stage.get() == GameStage::Cell && !editor.open && !options.diagnostics;
    if !want {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        active.0 = false;
        return;
    }
    if roots.iter().any(|(_, r)| *r == CellHudRoot::Main) {
        return;
    }
    if originals.resource_index.records.is_empty() {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let index = &originals.resource_index;
    let Some(main) = assets.layout(index, MAIN_LAYOUT) else {
        return;
    };
    let spawn = |commands: &mut Commands, assets: &mut SpuiAssets, name: &str, kind, policy, z| {
        if let Some(layout) = assets.layout(index, name) {
            let root = view::spawn(commands, layout, policy, Some(camera), z);
            commands.entity(root).insert(kind);
        }
    };
    // Rollover and card first so the bottom bar stays above them.
    if let Some(layout) = assets.layout(index, ROLLOVER_LAYOUT) {
        for slot in 0..ROLLOVER_POOL {
            let root = view::spawn(&mut commands, layout.clone(), RootPolicy::Authored, Some(camera), 98);
            commands.entity(root).insert((CellHudRoot::Rollover, RolloverSlot(slot)));
        }
    }
    spawn(&mut commands, &mut assets, CARD_LAYOUT, CellHudRoot::Card, RootPolicy::Authored, 99);
    let root = view::spawn(&mut commands, main, RootPolicy::Fit, Some(camera), 100);
    commands.entity(root).insert(CellHudRoot::Main);
    spawn(&mut commands, &mut assets, OPTIONS_LAYOUT, CellHudRoot::Orb, RootPolicy::Fit, 101);
    // The hint bar and the callout dialog stay spawned (hidden) so that their
    // pop-in effects run when they are shown.
    spawn(&mut commands, &mut assets, dialogs::HINT_LAYOUT, CellHudRoot::Hint, RootPolicy::Fit, 102);
    spawn(&mut commands, &mut assets, dialogs::CALLOUT_LAYOUT, CellHudRoot::Callout, RootPolicy::Authored, 103);
    active.0 = true;
}

/// Retail goal strings of the Cell "Collect" goal (locale table `CreatureGame`):
/// title, headline and description.
const GOAL_TITLE: &str = "loc:CreatureGame:f4daf5c9";
const GOAL_HEADLINE: &str = "loc:CreatureGame:f4daf5cb";
const GOAL_DESCRIPTION: &str = "loc:CreatureGame:f4daf5ca";
const GOAL_BODY: &str = "loc:CreatureGame:f4daf5cb\nloc:CreatureGame:f4daf5ca";

/// The cell of the capture demo's rollover.
fn demo_rollover() -> CellRollover {
    CellRollover {
        entity: Entity::PLACEHOLDER,
        name: "Grypher".into(),
        health: 5,
        max_health: 6,
        carries_part: true,
        part_slots: vec![7, 5],
        giant: false,
        screen: Vec2::new(640.0, 470.0),
        hovered: true,
        trigger: None,
        avatar: false,
    }
}

/// Flags of the capture demo state.
struct DemoFlags {
    on: bool,
    paused: bool,
    callout: Option<String>,
    hint: Option<String>,
    summary: bool,
    letterbox: bool,
    /// `--cell-hud-damage`: the avatar takes a hit (rollover record created by
    /// a health trigger, not by the cursor).
    damage: bool,
}

impl DemoFlags {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned());
        Self {
            on: has("--cell-hud-demo"),
            paused: has("--cell-hud-paused"),
            callout: value("--cell-hud-callout"),
            hint: value("--cell-hud-hint"),
            summary: has("--cell-hud-summary"),
            letterbox: has("--cell-hud-letterbox"),
            damage: has("--cell-hud-damage"),
        }
    }
}

/// `--cell-hud-demo` (captures): replace what the engine published with a
/// mid-game HUD state like the retail frames. `--cell-hud-paused` also opens
/// the options menu; `--cell-hud-callout <Name>` (for example
/// `Callout_CLG_ProgressBar`), `--cell-hud-hint <id>` (`clg-carnivore`) and
/// `--cell-hud-summary` show the callout dialog, the hint bar or the
/// end-of-stage summary window on top of it; `--cell-hud-letterbox` is the
/// cinematic mode (letterbox bars, HUD hidden).
fn demo_state(
    stage: Res<State<GameStage>>,
    mut demo: Local<Option<DemoFlags>>,
    mut frame: Local<u32>,
    mut hud: ResMut<CellHudState>,
) {
    let flags = demo.get_or_insert_with(DemoFlags::from_args);
    if !flags.on || *stage.get() != GameStage::Cell {
        return;
    }
    *frame += 1;
    *hud = CellHudState {
        dna_points: 18,
        level_progress: 0.15,
        level: 1,
        level_count: 5,
        health: 5,
        max_health: 6,
        show_mate_button: true,
        goals: vec![(GOAL_TITLE.into(), GOAL_BODY.into())],
        rollover: Some(demo_rollover()),
        rollovers: {
            let mut cells = vec![demo_rollover()];
            if flags.damage {
                // The avatar's health fell from 6 to 2 on the 5th frame
                // (`ShowHealthRollover(avatar, 6)`), without the cursor.
                let mut world = World::new();
                let avatar = world.spawn_empty().id();
                cells.push(CellRollover {
                    entity: avatar,
                    name: String::new(),
                    health: 2,
                    max_health: 6,
                    carries_part: false,
                    part_slots: Vec::new(),
                    giant: false,
                    screen: Vec2::new(640.0, 560.0),
                    hovered: false,
                    trigger: (*frame == 5).then_some(6),
                    avatar: true,
                });
            }
            cells
        },
        zoom: 1.0,
        paused: flags.paused,
        stage_progress: 0.23,
        biome: 0,
        food_progression: 230,
        goal_cards: vec![CellGoalCard {
            kind: "Collect".into(),
            progress: Some((1, 6)),
            title: GOAL_HEADLINE.into(),
            body: GOAL_DESCRIPTION.into(),
        }],
        callouts: flags.callout.iter().cloned().collect(),
        hint: flags.hint.clone(),
        summary: flags.summary.then(|| crate::cell_contract::CellSummary {
            game_time_seconds: 25.0 * 60.0,
            kills: 12,
            deaths: 1,
            diet: "clg_meat".into(),
            dna: 232,
        }),
        letterbox: flags.letterbox,
        hud_hidden: flags.summary || flags.letterbox,
        advance_visible: flags.callout.as_deref() == Some("Callout_CLG_TransitionButton"),
        ..default()
    };
}

/// Resolve `loc:<Table>:<hex id>` references (one per line) to locale text;
/// anything else is literal text.
fn resolve(assets: &mut SpuiAssets, index: &ResourceIndex, text: &str) -> String {
    text.lines()
        .map(|line| match line.strip_prefix("loc:").and_then(|r| r.split_once(':')) {
            Some((table, hex)) => u32::from_str_radix(hex.trim_start_matches("0x"), 16)
                .ok()
                .and_then(|id| assets.localized(index, crate::world_objects::name_id(table), id))
                .unwrap_or_default(),
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn window(state: &SpuiState, control: u32) -> Option<WinId> {
    state.find(control)
}

/// Width of the text window while a bubble's text is being measured.
const BUBBLE_MEASURE_WIDTH: f32 = 700.0;

/// Size a speech bubble's frame to its text (the retail code does this with
/// the text bounds; the margins are the authored ones): the text window takes
/// the measured width and the frame grows around it, symmetrically about its
/// authored centre or, with `keep_right`, to the left of its right edge.
fn fit_bubble(ui: &mut SpuiState, text: u32, frame: u32, keep_right: bool) {
    let (Some(text), Some(frame)) = (window(ui, text), window(ui, frame)) else {
        return;
    };
    let (a_text, a_frame) = (ui.layout.window(text).area, ui.layout.window(frame).area);
    let (pad_left, pad_right) = (a_text[0] - a_frame[0], a_frame[2] - a_text[2]);
    // A measurement narrower than a few pixels is a layout that has not seen
    // the text yet: keep the window wide until a real one arrives.
    let width = ui
        .text_extent(text)
        .filter(|e| e.x >= 4.0)
        .map_or(BUBBLE_MEASURE_WIDTH, |e| e.x.ceil() + 2.0);
    let (frame_left, frame_right) = if keep_right {
        (a_frame[2] - (width + pad_left + pad_right), a_frame[2])
    } else {
        let centre = (a_frame[0] + a_frame[2]) * 0.5;
        (centre - (width + pad_left + pad_right) * 0.5, centre + (width + pad_left + pad_right) * 0.5)
    };
    ui.set_area(frame, [frame_left, a_frame[1], frame_right, a_frame[3]]);
    ui.set_area(text, [frame_left + pad_left, a_text[1], frame_left + pad_left + width, a_text[3]]);
}

fn bind_main(ui: &mut SpuiState, hud: &CellHudState, colors: &ProgressColors, clock: &BubbleClock) {
    let (intro, reminder, end) = bubbles(clock, hud);
    for (i, (control, on)) in [
        (id::BUBBLE_MATE_INTRO, intro),
        (id::BUBBLE_MATE_REMINDER, reminder),
        (id::BUBBLE_STAGE_END, end),
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(w) = window(ui, control) {
            ui.set_visible(w, on);
        }
        if on {
            // The two mate bubbles hang centred over the mate button; the stage
            // end bubble keeps its right edge (its arrow points at Advance).
            fit_bubble(ui, id::BUBBLE_TEXT[i], id::BUBBLE_FRAME[i], i == 2);
        }
    }
    if let Some(w) = window(ui, id::DNA_TEXT) {
        ui.set_text(w, format!("{DNA_GLYPH}{}", hud.dna_points));
        ui.set_text_size(w, Some(DNA_TEXT_PX));
    }
    for control in [id::MATE_BUTTON, id::MATE_TRAY] {
        if let Some(w) = window(ui, control) {
            ui.set_visible(w, hud.show_mate_button);
        }
    }
    if let Some(w) = window(ui, id::PAUSE_TOGGLE) {
        ui.set_selected(w, hud.paused);
    }
    for control in [id::ADVANCE, id::ADVANCE_GLOW] {
        if let Some(w) = window(ui, control) {
            ui.set_visible(w, hud.advance_visible);
        }
    }
    if let Some(w) = window(ui, id::HISTORY) {
        ui.set_visible(w, !hud.advance_visible);
    }
    // Health meter: the bright fill window shrinks with health.
    if let Some(w) = window(ui, id::HEALTH_FILL) {
        let frac = if hud.max_health > 0 {
            (hud.health as f32 / hud.max_health as f32).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let mut area = ui.layout.window(w).area;
        area[2] = area[0] + HEALTH_FULL * frac;
        ui.set_area(w, area);
    }
    // Progress: five segments, one per size level, plus the marker.
    let levels = hud.level_count.max(1) as f32;
    let overall = ((hud.level as f32 + hud.level_progress.clamp(0.0, 1.0)) / levels).clamp(0.0, 1.0);
    let segment_count = id::SEGMENTS.len() as f32;
    for (i, control) in id::SEGMENTS.iter().enumerate() {
        let Some(w) = window(ui, *control) else {
            continue;
        };
        let fill = (overall * segment_count - i as f32).clamp(0.0, 1.0);
        // The unfilled material for the whole segment, the filled one over the
        // part the DNA has reached (both recolour the segment's children too).
        ui.set_shade(w, colors.right);
        ui.set_overlay(w, (fill > 0.0).then_some((fill, colors.left)));
    }
    let first = window(ui, id::SEGMENTS[0]);
    let last = window(ui, id::SEGMENTS[4]);
    let bar = window(ui, id::BAR);
    let marker = window(ui, id::MARKER);
    if let (Some(first), Some(last), Some(bar), Some(marker)) = (first, last, bar, marker) {
        let (bar_rect, a, b) = (ui.rect(bar), ui.rect(first), ui.rect(last));
        let x = (a.x1 + overall * (b.x2 - a.x1)) - bar_rect.x1;
        let mut area = ui.layout.window(marker).area;
        area[0] = x;
        area[2] = x;
        ui.set_area(marker, area);
    }
}

fn bind_card(ui: &mut SpuiState, hud: &CellHudState, assets: &mut SpuiAssets, index: &ResourceIndex) {
    let Some(root) = window(ui, id::CARD_ROOT) else {
        return;
    };
    ui.set_visible(root, !hud.goal_cards.is_empty());
    // The card root moves to the manager's base (the authored origin is
    // (17, 16)).
    let authored = ui.layout.window(root).area;
    ui.set_area(
        root,
        [CARD_X, CARD_Y, CARD_X + authored[2] - authored[0], CARD_Y + authored[3] - authored[1]],
    );
    let Some(card) = hud.goal_cards.first() else {
        return;
    };
    // Header (the goal kind), bold headline, then the description lines.
    let header = match card.kind.as_str() {
        "Collect" => resolve(assets, index, GOAL_TITLE),
        other => other.to_string(),
    };
    let headline = resolve(assets, index, &card.title);
    let description = resolve(assets, index, &card.body);
    if let Some(w) = window(ui, id::CARD_TITLE) {
        ui.set_text(w, header);
    }
    if let Some(w) = window(ui, id::CARD_BODY) {
        ui.set_text(w, headline);
    }
    if let Some(w) = window(ui, id::CARD_OVERFLOW) {
        ui.set_text(w, description);
    }
    fit_card(ui);
    if let Some(w) = window(ui, id::CARD_COUNT) {
        let count = card.progress.map(|(a, b)| format!("{a}/{b}")).unwrap_or_default();
        ui.set_text(w, count);
    }
}

/// Stack the headline and the description inside the panel and size the panel
/// to them. ADAPTED: the retail code sizes these windows to their text
/// (`IText::GetTextBounds`); the offsets are matched to the retail frames.
fn fit_card(ui: &mut SpuiState) {
    let (Some(panel), Some(group), Some(head), Some(desc)) = (
        window(ui, id::CARD_PANEL),
        window(ui, id::CARD_GROUP),
        window(ui, id::CARD_BODY),
        window(ui, id::CARD_OVERFLOW),
    ) else {
        return;
    };
    let area = |ui: &SpuiState, w: WinId| ui.layout.window(w).area;
    let (a_panel, a_group, a_head, a_desc) = (area(ui, panel), area(ui, group), area(ui, head), area(ui, desc));
    let height = |ui: &SpuiState, w: WinId| ui.text_extent(w).map_or(0.0, |e| e.y);
    // One 12 px headline line is 15 px high; the description wraps freely.
    let head_h = height(ui, head).max(15.0);
    let desc_h = height(ui, desc);
    const PAD_TOP: f32 = 6.0;
    const PAD_BOTTOM: f32 = 8.0;
    // The headline's parent and the card root share their origin.
    let head_top = a_panel[1] + PAD_TOP;
    let desc_top = head_top + head_h;
    ui.set_area(head, [a_head[0], head_top, a_head[2], head_top + head_h]);
    ui.set_area(group, [a_group[0], a_group[1], a_group[2], desc_top + desc_h + 2.0]);
    let rel = desc_top - a_group[1];
    ui.set_area(desc, [a_desc[0], rel, a_desc[2], rel + desc_h]);
    ui.set_area(panel, [a_panel[0], a_panel[1], a_panel[2], desc_top + desc_h + PAD_BOTTOM]);
}

/// Height of one menu button row in the authored options panel.
const MENU_ROW: f32 = 41.0;

fn by_element(ui: &SpuiState, element: usize) -> Option<WinId> {
    ui.layout.windows.iter().find(|w| w.element == element).map(|w| w.id)
}

/// The Cell stage's options menu behind the orb (`0xE01960`): shown while the
/// game is paused through the menu. In the Cell game mode (`0x1654C00`) the
/// Spy Glass and Ban rows are disabled, not hidden, and carry the tooltips
/// "... mode not available in Cell Stage" (locale `tooltips` `0x0678C04A` /
/// `0x0678C04B`); the panel keeps its authored size.
pub(crate) fn bind_orb_cell(ui: &mut SpuiState, hud: &CellHudState, tips: [Option<String>; 2]) {
    let Some(panel) = window(ui, id::OPTIONS_PANEL) else {
        return;
    };
    ui.set_visible(panel, hud.paused);
    for (control, tip) in [id::MENU_SPYGLASS, id::MENU_BAN].into_iter().zip(tips) {
        if let Some(w) = window(ui, control) {
            ui.set_enabled(w, false);
            ui.set_tooltip(w, tip);
        }
    }
}

/// The options menu of the other stages' HUDs (they share the Cell stage's
/// orb layout): shown while the game is paused through the menu. ADAPTED: the
/// Spy Glass and Ban rows belong to other stages and online features, so they
/// are hidden and the rows below move up with the frame (the retail code
/// enables them outside the Cell stage; OpenSpore has neither mode).
///
/// Every row is anchored to the bottom of the panel (`SimpleLayout` bottom),
/// so shrinking the panel moves all of them up; the two rows above the gap
/// are put back with an equal offset.
pub(crate) fn bind_orb(ui: &mut SpuiState, hud: &CellHudState) {
    let Some(panel) = window(ui, id::OPTIONS_PANEL) else {
        return;
    };
    ui.set_visible(panel, hud.paused);
    for control in [id::MENU_SPYGLASS, id::MENU_BAN] {
        if let Some(w) = window(ui, control) {
            ui.set_visible(w, false);
        }
    }
    let gap = 2.0 * MENU_ROW;
    let offset = |ui: &mut SpuiState, w: Option<WinId>, dy1: f32, dy2: f32| {
        if let Some(w) = w {
            let mut a = ui.layout.window(w).area;
            a[1] += dy1;
            a[3] += dy2;
            ui.set_area(w, a);
        }
    };
    // Frame: the panel and its content group lose the height of the gap.
    let content = window(ui, id::MENU_FRAME);
    offset(ui, Some(panel), 0.0, -gap);
    offset(ui, content, 0.0, -gap);
    // Rows above the gap (Resume Game, Spore Guide) stay where they were.
    let (resume, guide) = (by_element(ui, 252), by_element(ui, 140));
    offset(ui, resume, gap, gap);
    offset(ui, guide, gap, gap);
    // Bottom decoration is not anchored.
    let decor = by_element(ui, 255);
    offset(ui, decor, -gap, -gap);
}

/// One health rollover record: the pool object `0xE62200` creates, `0xE582C0`
/// steps every frame and `0xE57FD0` shows (`CellHealth_New`). The comments
/// give the record offsets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RolloverBars {
    /// +0x08: the fraction the main (green or silver) bar shows.
    pub shown: f32,
    /// +0x10 / +0x14: alpha and fraction of the red bar (health just lost).
    pub red_alpha: f32,
    pub red: f32,
    /// +0x18 / +0x1C: alpha and fraction of the yellow bar (health just gained).
    pub yellow_alpha: f32,
    pub yellow: f32,
    /// +0x20: alpha of the whole rollover.
    pub alpha: f32,
    /// +0x24: seconds the rollover stays once its bars are still (0.5 after
    /// every `ShowHealthRollover`, `0xE62340`).
    pub hold: f32,
    /// +0x28: seconds before the bars start to change (0.25 at creation).
    pub linger: f32,
    /// +0x2C: alpha of the name plate window `0x933103EA`.
    pub plate: f32,
}

impl RolloverBars {
    /// `0xE62200(cell, fraction)`: a new record shows `fraction` everywhere.
    pub fn new(fraction: f32) -> Self {
        Self {
            shown: fraction,
            red_alpha: 0.0,
            red: fraction,
            yellow_alpha: 0.0,
            yellow: fraction,
            alpha: 0.0,
            hold: 0.5,
            linger: 0.25,
            plate: 0.0,
        }
    }

    /// `ShowHealthRollover` (`0xE62340`): the returned record's `+0x24` is
    /// set to 0.5, so the record stays 0.5 s longer.
    pub fn touch(&mut self) {
        self.hold = 0.5;
    }

    /// The cursor resting on the cell: as `touch`, and the name plate bright.
    pub fn refresh(&mut self) {
        self.touch();
        self.plate = 1.0;
    }

    /// `0xE582C0`: step the record towards the cell's health `target`
    /// (`hp / 6`) by `dt` seconds; false once the record has faded away.
    /// Healing makes the red bar fade out and the yellow one in before the
    /// main bar climbs (2 per second); damage fades the yellow bar out and the
    /// red one in before the main bar drops. The rollover itself fades at 4
    /// per second.
    pub fn step(&mut self, target: f32, dt: f32) -> bool {
        let fade = dt * 4.0;
        let clamp = |v: f32| v.clamp(0.0, 1.0);
        self.plate = clamp(self.plate - fade);
        let target = clamp(target);
        if target > self.shown {
            self.red = target;
            self.yellow = target;
        }
        if self.linger > 0.0 {
            self.linger -= dt;
        } else if self.alpha < 1.0 && self.shown != target {
            self.alpha = clamp(self.alpha + fade).max(self.plate);
        } else if target > self.shown {
            if self.red_alpha > 0.0 {
                self.red_alpha = clamp(self.red_alpha - fade);
            }
            if self.yellow_alpha < 1.0 {
                self.yellow_alpha = clamp(self.yellow_alpha + fade);
            } else {
                self.shown = (self.shown + 2.0 * dt).min(target);
            }
        } else if self.shown > target {
            let mut yellow_gone = true;
            if self.yellow_alpha > 0.0 {
                self.yellow_alpha = clamp(self.yellow_alpha - fade);
                yellow_gone = false;
            }
            if self.red_alpha < 1.0 && self.red > self.shown {
                self.red_alpha = clamp(self.red_alpha + fade);
            } else {
                self.red_alpha = 1.0;
                if yellow_gone {
                    self.shown = (self.shown - 2.0 * dt).max(target);
                }
            }
        } else if self.yellow_alpha > 0.0 && self.yellow > self.shown {
            // 0xE585CC: a settled bar lets the red one fade (1 per second).
            self.red_alpha = clamp(self.red_alpha - dt);
        } else if self.hold > 0.0 {
            if self.alpha < 1.0 {
                self.alpha = clamp(self.alpha + fade).max(self.plate);
            } else {
                self.hold -= dt;
            }
        } else {
            self.alpha = (self.alpha - fade).max(self.plate);
            if self.alpha <= 0.0 {
                return false;
            }
        }
        true
    }
}

/// Records in the retail pool (`sCellUI + 0x9C`, 12 objects): each owns its
/// own `CellHealth_New` layout, so the HUD spawns as many layout roots.
pub const ROLLOVER_POOL: usize = 12;

/// Which pool record a spawned `CellHealth_New` root shows.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RolloverSlot(pub usize);

/// One pool record and the cell data it was last fed.
#[derive(Clone, Debug)]
pub struct RolloverRecord {
    pub bars: RolloverBars,
    pub cell: CellRollover,
    alive: bool,
}

/// The health rollover pool (`0xE62200` creates, `0xE582C0` steps, `0xE57FD0`
/// shows). Records are created by `ShowHealthRollover` (damage the avatar
/// dealt or took, a heal of the avatar: `CellRollover::trigger`) and by the
/// cursor resting on a cell (`CellRollover::hovered`); a cell that already
/// has a record only has it kept longer.
#[derive(Resource, Default)]
pub struct RolloverState {
    records: Vec<RolloverRecord>,
}

impl RolloverState {
    /// Feed the cells of this frame and step every record by `dt`; the
    /// records still alive are returned in creation order (their index is
    /// the layout slot).
    pub fn update(&mut self, cells: &[CellRollover], dt: f32) -> &[RolloverRecord] {
        for cell in cells {
            let at = self.records.iter().position(|r| r.cell.entity == cell.entity);
            match (at, cell.trigger, cell.hovered) {
                // `0xE62200` hands back the existing record: only the linger
                // time is set (`0xE62340`).
                (Some(i), trigger, hovered) => {
                    let record = &mut self.records[i];
                    record.cell = cell.clone();
                    if hovered {
                        record.bars.refresh();
                    } else if trigger.is_some() {
                        record.bars.touch();
                    }
                }
                // A new record starts from the health before the change.
                (None, Some(before), hovered) => {
                    self.create(cell, health_of(before, cell.max_health), hovered);
                }
                (None, None, true) => self.create(cell, health_fraction(cell), true),
                (None, None, false) => {}
            }
        }
        for record in &mut self.records {
            let target = health_fraction(&record.cell);
            record.alive = record.bars.step(target, dt);
        }
        self.records.retain(|r| r.alive);
        &self.records
    }

    fn create(&mut self, cell: &CellRollover, fraction: f32, hovered: bool) {
        // The pool holds 12 objects; `CreateObject` fails when it is full.
        if self.records.len() >= ROLLOVER_POOL {
            return;
        }
        let mut bars = RolloverBars::new(fraction);
        if hovered {
            bars.refresh();
        } else {
            bars.touch();
        }
        self.records.push(RolloverRecord { bars, cell: cell.clone(), alive: true });
    }
}

fn health_of(health: i32, max_health: i32) -> f32 {
    if max_health > 0 {
        (health as f32 / max_health as f32).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn health_fraction(r: &CellRollover) -> f32 {
    health_of(r.health, r.max_health)
}

fn alpha_color(alpha: f32) -> u32 {
    ((alpha.clamp(0.0, 1.0) * 255.0).round() as u32) << 24 | 0x00FF_FFFF
}

/// `0xE57FD0`: show a health rollover record in its `CellHealth_New` layout.
/// `plate_allowed`: the name plate shows only while the `OptionCellControls`
/// config option is off (`0xE57FEF`).
fn bind_rollover(ui: &mut SpuiState, view: Option<(&RolloverBars, &CellRollover)>, plate_allowed: bool) {
    let Some(root) = window(ui, id::ROLL_ROOT) else {
        return;
    };
    let Some((bars, r)) = view else {
        ui.set_visible(root, false);
        return;
    };
    ui.set_visible(root, true);
    // The root is a zero-size window the code moves to the cell: 0.8 radius
    // above it, or on it for a cell far larger than the player (0xE5856A).
    ui.set_area(root, [r.screen.x, r.screen.y, r.screen.x, r.screen.y]);
    ui.set_shade(root, alpha_color(bars.alpha));
    // Name plate and the part icons of the (up to two) locked parts the cell
    // carries, put on the plate's two slot windows (0xE57EC0).
    if let Some(plate) = window(ui, id::ROLL_PLATE) {
        let show = bars.plate > 0.0 && plate_allowed;
        ui.set_visible(plate, show);
        ui.set_shade(plate, alpha_color(bars.plate));
    }
    if let Some(w) = window(ui, id::ROLL_NAME) {
        ui.set_text(w, r.name.clone());
    }
    for slot in 0..13 {
        if let Some(w) = window(ui, id::ROLL_PART_ICON + slot) {
            ui.set_visible(w, false);
        }
    }
    let icons: Vec<usize> = r
        .part_slots
        .iter()
        .copied()
        .filter(|s| window(ui, id::ROLL_PART_ICON + *s as u32).is_some())
        .take(2)
        .collect();
    for (n, slot) in icons.iter().enumerate() {
        let (Some(icon), Some(place)) = (
            window(ui, id::ROLL_PART_ICON + *slot as u32),
            window(ui, id::ROLL_PART_SLOT + n as u32),
        ) else {
            continue;
        };
        let area = ui.layout.window(place).area;
        ui.set_area(icon, area);
        ui.set_visible(icon, true);
    }
    // Bars: the frame window gives the full size; the main bar is the silver
    // one for a cell far larger than the player, else the green one, and the
    // red / yellow bars lie in the same window with their own fractions and
    // alphas (0xE57FD0 / 0xE50490).
    let Some(frame) = window(ui, id::ROLL_FRAME) else {
        return;
    };
    let authored = ui.layout.window(frame).area;
    let full = authored[2] - authored[0];
    let (main, other) = if r.giant { (id::ROLL_SILVER, id::ROLL_GREEN) } else { (id::ROLL_GREEN, id::ROLL_SILVER) };
    let sized = |ui: &mut SpuiState, control: u32, fraction: f32, alpha: Option<f32>| {
        let Some(w) = window(ui, control) else {
            return;
        };
        let mut area = ui.layout.window(w).area;
        area[2] = area[0] + full * fraction;
        ui.set_area(w, area);
        ui.set_visible(w, true);
        if let Some(alpha) = alpha {
            ui.set_shade(w, alpha_color(alpha));
        }
    };
    sized(ui, main, bars.shown, None);
    if let Some(w) = window(ui, other) {
        ui.set_visible(w, false);
    }
    sized(ui, id::ROLL_RED, bars.red, Some(bars.red_alpha));
    sized(ui, id::ROLL_YELLOW, bars.yellow, Some(bars.yellow_alpha));
}

/// Push [`CellHudState`] into the layouts.
fn bind(
    time: Res<Time>,
    hud: Res<CellHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut colors: ResMut<ProgressColors>,
    mut clock: ResMut<BubbleClock>,
    mut rollovers: ResMut<RolloverState>,
    mut roots: Query<(&mut SpuiState, &CellHudRoot), Without<RolloverSlot>>,
    mut slots: Query<(&mut SpuiState, &RolloverSlot)>,
) {
    if !colors.loaded && !originals.resource_index.records.is_empty() {
        colors.load(&originals.resource_index);
    }
    step_clock(&mut clock, &hud, time.delta_secs());
    // No health rollovers start while the options menu is open.
    let fed: &[CellRollover] = if hud.paused { &[] } else { &hud.rollovers };
    let records = rollovers.update(fed, time.delta_secs());
    let plate_allowed = !crate::front_end::settings::cell_follows_cursor();
    let tooltips = crate::world_objects::name_id("tooltips");
    let tips = [0x0678_C04A_u32, 0x0678_C04B].map(|id| assets.localized(&originals.resource_index, tooltips, id));
    for (mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match kind {
            CellHudRoot::Main => bind_main(&mut ui, &hud, &colors, &clock),
            CellHudRoot::Card => bind_card(&mut ui, &hud, &mut assets, &originals.resource_index),
            // Bound per pool slot below.
            CellHudRoot::Rollover => {}
            CellHudRoot::Orb => bind_orb_cell(&mut ui, &hud, tips.clone()),
            // Bound by `dialogs::bind`.
            CellHudRoot::Hint | CellHudRoot::Callout => {}
        }
    }
    for (mut ui, slot) in &mut slots {
        if !ui.ready {
            continue;
        }
        let view = records.get(slot.0).map(|r| (&r.bars, &r.cell));
        bind_rollover(&mut ui, view, plate_allowed);
    }
}

/// Button clicks become the keyboard actions the engine already handles.
#[allow(clippy::too_many_arguments)]
fn actions(
    mut roots: Query<(&mut SpuiState, &CellHudRoot)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<crate::player_hud::InjectedKeys>,
    mut wheel: MessageWriter<MouseWheel>,
    mut menu: MessageWriter<crate::front_end::MenuCommand>,
) {
    for (mut ui, kind) in &mut roots {
        if !matches!(kind, CellHudRoot::Main | CellHudRoot::Orb) {
            continue;
        }
        for click in ui.take_clicks() {
            let control = ui.layout.window(click).control_id;
            // The options menu and Sporepedia are the front end's (the same
            // in every stage).
            if let Some(command) = crate::front_end::menu_command(control) {
                menu.write(command);
                continue;
            }
            let mut press = |key: KeyCode| {
                if !keys.pressed(key) {
                    keys.press(key);
                    injected.push(key);
                }
            };
            match control {
                id::MATE_BUTTON => press(crate::cell_flow::MATE_KEY),
                id::ADVANCE => press(crate::cell_flow::ADVANCE_KEY),
                id::PAUSE_TOGGLE | id::OPTIONS_ORB => press(KeyCode::Escape),
                id::ZOOM_IN | id::ZOOM_OUT => {
                    // The play camera zooms with the wheel.
                    wheel.write(MouseWheel {
                        unit: MouseScrollUnit::Line,
                        x: 0.0,
                        y: if control == id::ZOOM_IN { 1.0 } else { -1.0 },
                        window: Entity::PLACEHOLDER,
                        phase: bevy::input::touch::TouchPhase::Moved,
                    });
                }
                // History has no counterpart in the rewrite yet.
                _ => {}
            }
        }
    }
}

/// Marks the letterbox layout root (`Tutorial_letterbox_txt`).
#[derive(Component)]
pub struct CellOverlayRoot;

/// The cinematic letterbox layout: `0xE4F640(1)` (`cinematic mode on`) calls
/// the cinematic manager's `0xAD7DC0`, which shows the top and bottom bar
/// windows of the layout `0xAD8280` loads (`Tutorial_letterbox_txt`, the same
/// layout the cinematic scripts' `letterbox -on` uses) and sets
/// `ui.field_934[2]`, which hides the Cell HUD layouts.
const LETTERBOX_LAYOUT: &str = "Tutorial_letterbox_txt";
const LETTERBOX_BARS: [u32; 2] = [0x0450_12CA, 0x0450_23D0];
/// The layout's other windows (text, buttons, black screen, subtitle styles)
/// stay hidden: only the bars belong to the cinematic mode switch.
const LETTERBOX_HIDDEN: [u32; 12] = [
    0x0445_C9AB, // text
    0x0798_0D2C, // upper text
    0x067A_0FC6, // scrolling text
    0x0445_C99D, // continue button
    0xFFFF_FFF2, // close
    0x062E_F353, // next button
    0x05B5_A2C8, // button container
    0x0756_3379, // speech style
    0x0756_3386, // thought style
    0x078D_4130, // caption style
    0x07C7_6404, // radio style
    0x05DB_9C49, // black screen
];

/// Hide the HUD layouts in cinematics (`hud_hidden`, retail
/// `ui.field_934[2]`). Callouts, hints and the summary window are retail
/// layouts bound by [`dialogs`] and the summary module.
fn overlays(hud: Res<CellHudState>, mut layouts: Query<(&mut Visibility, &CellHudRoot)>) {
    for (mut v, _) in &mut layouts {
        let want = if hud.hud_hidden { Visibility::Hidden } else { Visibility::Inherited };
        if *v != want {
            *v = want;
        }
    }
}

/// The letterbox bars of the cinematic mode: the installed layout, spawned
/// while `letterbox` is set so that its Fade / Glide effects play on entry.
#[allow(clippy::too_many_arguments)]
fn letterbox(
    mut commands: Commands,
    hud: Res<CellHudState>,
    stage: Res<State<GameStage>>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    mut roots: Query<(Entity, &mut SpuiState), With<CellOverlayRoot>>,
) {
    let want = *stage.get() == GameStage::Cell && hud.letterbox;
    if !want {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    if roots.is_empty() {
        let (Ok(camera), false) = (camera.single(), originals.resource_index.records.is_empty()) else {
            return;
        };
        if let Some(layout) = assets.layout(&originals.resource_index, LETTERBOX_LAYOUT) {
            let root = view::spawn(&mut commands, layout, RootPolicy::Fit, Some(camera), 150);
            commands.entity(root).insert((CellOverlayRoot, crate::stages::StageOwned));
        }
        return;
    }
    for (_, mut ui) in &mut roots {
        if !ui.ready {
            continue;
        }
        for (controls, on) in [(&LETTERBOX_BARS[..], true), (&LETTERBOX_HIDDEN[..], false)] {
            for control in controls {
                if let Some(w) = ui.find(*control) {
                    if ui.visible(w) != on {
                        ui.set_visible(w, on);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hud(mate: bool, dna: i32) -> CellHudState {
        CellHudState {
            show_mate_button: mate,
            dna_points: dna,
            dna_since_creator: dna,
            ..default()
        }
    }

    #[test]
    fn mate_bubbles_follow_the_retail_timings() {
        let state = hud(true, 20);
        // First 6 s after the button shows: the introduction.
        assert_eq!(bubbles(&BubbleClock { mate: 1.0, ..default() }, &state), (true, false, false));
        // Then the reminder shows 10 s of every 30 s.
        assert_eq!(bubbles(&BubbleClock { mate: 7.0, ..default() }, &state), (false, true, false));
        assert_eq!(bubbles(&BubbleClock { mate: 20.0, ..default() }, &state), (false, false, false));
        assert_eq!(bubbles(&BubbleClock { mate: 37.0, ..default() }, &state), (false, true, false));
        // Not enough DNA earned since the creator visit: no reminder, however
        // much is spendable.
        assert_eq!(bubbles(&BubbleClock { mate: 7.0, ..default() }, &hud(true, 10)), (false, false, false));
        let mut spent = hud(true, 40);
        spent.dna_since_creator = 14;
        assert_eq!(bubbles(&BubbleClock { mate: 7.0, ..default() }, &spent), (false, false, false));
        spent.dna_since_creator = 15;
        assert_eq!(bubbles(&BubbleClock { mate: 7.0, ..default() }, &spent), (false, true, false));
        // No button, no bubbles.
        assert_eq!(bubbles(&BubbleClock { mate: 1.0, ..default() }, &hud(false, 20)), (false, false, false));
    }

    #[test]
    fn the_mate_introduction_shows_once() {
        let mut clock = BubbleClock::default();
        let shown = hud(true, 20);
        // First appearance: 6 s of introduction.
        step_clock(&mut clock, &shown, 1.0);
        assert_eq!(bubbles(&clock, &shown), (true, false, false));
        step_clock(&mut clock, &shown, 6.0);
        assert!(clock.intro_done);
        // The button goes away (a mate was called) and comes back: no
        // introduction again, straight into the reminder cycle.
        step_clock(&mut clock, &hud(false, 20), 0.5);
        assert_eq!(clock.mate, 0.0);
        step_clock(&mut clock, &shown, 0.1);
        assert_eq!(bubbles(&clock, &shown), (false, true, false));
        // Paused (menu): the clock does not run.
        let before = clock.mate;
        let mut paused = hud(true, 20);
        paused.paused = true;
        step_clock(&mut clock, &paused, 5.0);
        assert_eq!(clock.mate, before);
    }

    #[test]
    fn bubbles_wait_while_a_callout_or_the_menu_is_up() {
        let mut state = hud(true, 20);
        state.callouts.push("Callout_CLG_Opening".into());
        assert_eq!(bubbles(&BubbleClock { mate: 1.0, ..default() }, &state), (false, false, false));
        state.callouts.clear();
        state.paused = true;
        assert_eq!(bubbles(&BubbleClock { mate: 1.0, ..default() }, &state), (false, false, false));
    }

    fn settled(shown: f32) -> RolloverBars {
        RolloverBars { alpha: 1.0, linger: 0.0, ..RolloverBars::new(shown) }
    }

    #[test]
    fn a_new_rollover_lingers_then_fades_in_and_out() {
        let mut bars = RolloverBars::new(1.0);
        // 0.25 s of linger (no fade), then 4 per second.
        for _ in 0..3 {
            assert!(bars.step(1.0, 0.1));
        }
        assert_eq!(bars.alpha, 0.0);
        for _ in 0..2 {
            assert!(bars.step(1.0, 0.1));
        }
        assert!((bars.alpha - 0.8).abs() < 1e-5, "{}", bars.alpha);
        for _ in 0..2 {
            assert!(bars.step(1.0, 0.1));
        }
        assert_eq!(bars.alpha, 1.0);
        // Still bars: the hold time (0.5 s) counts down, then it fades away.
        let mut frames = 0;
        while bars.step(1.0, 0.1) {
            frames += 1;
            assert!(frames < 40, "the rollover never faded");
        }
        assert!(frames >= 5);
    }

    #[test]
    fn damage_shows_the_lost_chunk_in_red_while_the_main_bar_drops() {
        let mut bars = settled(1.0);
        for _ in 0..3 {
            bars.step(0.5, 0.1);
        }
        assert_eq!(bars.red_alpha, 1.0);
        assert_eq!(bars.red, 1.0);
        assert!((bars.shown - 0.5).abs() < 1e-6, "2 per second: {}", bars.shown);
    }

    #[test]
    fn healing_shows_the_gain_in_yellow_before_the_main_bar_climbs() {
        let mut bars = settled(0.5);
        bars.step(1.0, 0.1);
        assert_eq!((bars.yellow, bars.red), (1.0, 1.0));
        assert_eq!(bars.shown, 0.5, "the main bar waits for the yellow one");
        for _ in 0..3 {
            bars.step(1.0, 0.1);
        }
        assert_eq!(bars.yellow_alpha, 1.0);
        for _ in 0..4 {
            bars.step(1.0, 0.1);
        }
        assert!((bars.shown - 1.0).abs() < 1e-6, "{}", bars.shown);
    }

    fn cell(entity: Entity, health: i32) -> CellRollover {
        CellRollover {
            entity,
            name: "Cell".into(),
            health,
            max_health: 6,
            carries_part: false,
            part_slots: Vec::new(),
            giant: false,
            screen: Vec2::ZERO,
            hovered: false,
            trigger: None,
            avatar: false,
        }
    }

    #[test]
    fn the_rollover_state_keeps_fading_after_the_cursor_leaves() {
        let hovered = CellRollover { hovered: true, ..cell(Entity::PLACEHOLDER, 6) };
        let mut state = RolloverState::default();
        assert_eq!(state.update(&[hovered], 0.1).len(), 1);
        let mut frames = 0;
        while !state.update(&[], 0.1).is_empty() {
            frames += 1;
            assert!(frames < 60);
        }
        assert!(frames > 5, "the record outlives the hover");
    }

    #[test]
    fn a_watched_cell_without_a_trigger_or_hover_makes_no_record() {
        let mut state = RolloverState::default();
        assert!(state.update(&[cell(Entity::PLACEHOLDER, 6)], 0.1).is_empty());
    }

    #[test]
    fn damage_creates_a_record_that_starts_at_the_old_health() {
        // `ShowHealthRollover(cell, initialHealth)` runs before the hit: the
        // record shows the old 5/6 and then drops to 2/6 (red chunk first).
        let e = Entity::PLACEHOLDER;
        let hit = CellRollover { trigger: Some(5), ..cell(e, 2) };
        let mut state = RolloverState::default();
        let records = state.update(&[hit], 0.0);
        assert_eq!(records.len(), 1);
        assert!((records[0].bars.shown - 5.0 / 6.0).abs() < 1e-6);
        assert_eq!(records[0].bars.hold, 0.5);
        assert_eq!(records[0].bars.plate, 0.0, "no name plate without the cursor");
        let (mut lowest, mut red) = (1.0_f32, 0.0_f32);
        for _ in 0..40 {
            let records = state.update(&[cell(e, 2)], 0.1);
            let Some(r) = records.first() else { break };
            lowest = lowest.min(r.bars.shown);
            red = red.max(r.bars.red_alpha);
        }
        assert!((lowest - 2.0 / 6.0).abs() < 1e-5, "the bar ends at the new health: {lowest}");
        assert_eq!(red, 1.0, "the lost chunk shows in red");
        assert!(state.update(&[], 0.1).is_empty(), "and the record fades away");
    }

    #[test]
    fn a_second_hit_only_keeps_the_existing_record_longer() {
        let e = Entity::PLACEHOLDER;
        let mut state = RolloverState::default();
        state.update(&[CellRollover { trigger: Some(6), ..cell(e, 4) }], 0.0);
        for _ in 0..5 {
            state.update(&[cell(e, 4)], 0.1);
        }
        let before = state.records[0].bars;
        state.update(&[CellRollover { trigger: Some(4), ..cell(e, 2) }], 0.0);
        let after = state.records[0].bars;
        assert_eq!(after.hold, 0.5);
        assert_eq!(after.shown, before.shown, "no new initial health for a live record");
    }

    #[test]
    fn the_pool_holds_twelve_records() {
        let mut state = RolloverState::default();
        let mut world = World::new();
        let cells: Vec<CellRollover> = (0..20)
            .map(|_| CellRollover { trigger: Some(6), ..cell(world.spawn_empty().id(), 3) })
            .collect();
        assert_eq!(state.update(&cells, 0.0).len(), ROLLOVER_POOL);
    }

    #[test]
    fn stage_end_bubble_cycles_while_advance_is_shown() {
        let mut state = hud(false, 0);
        state.advance_visible = true;
        assert_eq!(bubbles(&BubbleClock { advance: 2.0, ..default() }, &state), (false, false, true));
        assert_eq!(bubbles(&BubbleClock { advance: 15.0, ..default() }, &state), (false, false, false));
        assert_eq!(bubbles(&BubbleClock { advance: 31.0, ..default() }, &state), (false, false, true));
    }
}
