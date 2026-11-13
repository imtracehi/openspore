//! Goal cards, hints and callouts of the Creature HUD, all from the retail
//! data: the mission cards are the installed `CRGMissionCard` layout filled
//! with strings of the `CreatureGame` locale table, the hints are the
//! `crg-*` property files (texts of `HintsCRG`) shown on the `hint` layout and
//! the callouts are the `Callout_CRG_*` property files shown on the callout
//! dialog layout (the generic machinery of `cell_hud::dialogs`).
//!
//! The hint bar is driven by a port of the retail hint manager
//! (`cHintManager`, [`HintMachine`]): the hint property files carry the
//! conditions, delays, display time and priority, the manager's seven-state
//! update (0x67C410) and its condition checker for the Creature game mode
//! (0xE07E50) are read from the code. The error hints of the ability buttons
//! (`crg-spitclose`, `-notarget`, `-socialalreadyfriends`, `-abilitynotready`)
//! are explicit `ShowHint` calls of the button handler 0xD49560: every failure
//! the actions code records (`Encounter::error` / `error_serial`) shows its
//! hint here. What stays ADAPTED is which goal is current and the order of the
//! callouts (the retail goal manager and the callout trigger are not decoded),
//! and the inputs the checker reads (the camera / movement conditions, see
//! [`Conditions`]).

use std::collections::HashMap;

use bevy::prelude::*;

use super::{CreatureHudActive, CreatureHudRoot, CreatureHudState, Diet};
use crate::{
    asset_catalog::ResourceIndex,
    cell_contract::CellGoalCard,
    cell_hud::dialogs::{self, CalloutDef, HintDef},
    creature_actions::Encounter,
    creature_controls::{read_axes, CreatureControls},
    game::{Player, RunProgress},
    spui::{
        model::{ImageRef, WinId},
        solve::Rect,
        view::{SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

/// Goal cards shown at once (retail stacks the current goals).
pub const MAX_CARDS: usize = 3;

/// Control ids of `CRGMissionCard` (the same as the Cell's `CLGMissionCard`).
#[allow(dead_code)]
pub mod card {
    pub const ROOT: u32 = 0x055E_BC68;
    pub const KIND: u32 = 0x0523_50C8;
    pub const COUNT: u32 = 0x740E_A83E;
    pub const COUNT_BACKDROP: u32 = 0x055D_3E68;
    pub const HEADLINE: u32 = 0x0523_5140;
    pub const DESCRIPTION: u32 = 0x74CB_0C14;
    pub const PANEL: u32 = 0x54BF_5692;
    pub const GROUP: u32 = 0x74BF_56AE;
}

/// ADAPTED: where the first card's root sits (its header edge matches the
/// Cell card's) and the gap between stacked cards.
const CARD_X: f32 = 5.0;
const CARD_Y: f32 = 15.0;
const CARD_GAP: f32 = 4.0;

/// Locale ids of the goal strings (`CreatureGame`, from the installed table).
mod text {
    pub const EAT_KIND: &str = "loc:CreatureGame:0509f9d9";
    pub const EAT_BODY: [&str; 3] = [
        // Herbivore, carnivore, omnivore.
        "loc:CreatureGame:0509f9dc",
        "loc:CreatureGame:0509f9da",
        "loc:CreatureGame:0509f9de",
    ];
    pub const EAT_HEADLINE: [&str; 3] =
        ["loc:CreatureGame:0524d325", "loc:CreatureGame:0524d324", "loc:CreatureGame:0524d326"];
    pub const CREATURES_KIND: &str = "loc:CreatureGame:0509fb92";
    pub const CREATURES_HEADLINE: &str = "loc:CreatureGame:0524d328";
    pub const CREATURES_BODY: &str = "loc:CreatureGame:0509fb93";
    pub const MATE_KIND: &str = "loc:CreatureGame:0509fc4e";
    pub const MATE_HEADLINE: &str = "loc:CreatureGame:0524d329";
    pub const MATE_BODY: &str = "loc:CreatureGame:0509fc4f";
    pub const PACK_KIND: &str = "loc:CreatureGame:0509fc96";
    pub const PACK_HEADLINE: &str = "loc:CreatureGame:0524d32a";
    pub const PACK_BODY: &str = "loc:CreatureGame:0509fc97";
    pub const MIGRATE_KIND: &str = "loc:CreatureGame:0514f98e";
    pub const MIGRATE_HEADLINE: &str = "loc:CreatureGame:0524d32c";
    pub const MIGRATE_BODY: &str = "loc:CreatureGame:0514f993";
    pub const EVOLVE_KIND: &str = "loc:CreatureGame:0546ab9a";
    pub const EVOLVE_HEADLINE: &str = "loc:CreatureGame:0546aba4";
    pub const EVOLVE_BODY: &str = "loc:CreatureGame:0546ab9e";
}

/// What the goal selection looks at.
#[derive(Clone, Debug, Default)]
pub struct GoalInputs {
    pub diet: Diet,
    /// Food `0.0..=1.0`.
    pub hunger: f32,
    /// A species has been befriended or driven extinct.
    pub met_species: bool,
    pub allied_species: usize,
    pub pack_len: usize,
    pub pack_capacity: usize,
    /// A nest migration is pending.
    pub migrating: bool,
    /// The stage's DNA goal is reached.
    pub advance: bool,
}

/// The goal cards for the current situation, first goal first. ADAPTED: the
/// retail goal manager is not decoded; the cards are the retail goals (strings
/// and wording from the `CreatureGame` table) picked in the order a new
/// creature meets them.
pub fn goal_cards(g: &GoalInputs) -> Vec<CellGoalCard> {
    let diet = match g.diet {
        Diet::Herbivore => 0,
        Diet::Carnivore => 1,
        Diet::Omnivore => 2,
    };
    let card = |kind: &str, headline: &str, body: &str, progress: Option<(i32, i32)>| CellGoalCard {
        kind: kind.into(),
        progress,
        title: headline.into(),
        body: body.into(),
    };
    let mut cards = Vec::new();
    if g.advance {
        cards.push(card(text::EVOLVE_KIND, text::EVOLVE_HEADLINE, text::EVOLVE_BODY, None));
    }
    if g.hunger < 0.5 {
        cards.push(card(text::EAT_KIND, text::EAT_HEADLINE[diet], text::EAT_BODY[diet], None));
    }
    if !g.met_species {
        cards.push(card(text::CREATURES_KIND, text::CREATURES_HEADLINE, text::CREATURES_BODY, None));
    }
    if g.migrating {
        cards.push(card(text::MIGRATE_KIND, text::MIGRATE_HEADLINE, text::MIGRATE_BODY, None));
    }
    if g.allied_species > 0 && g.pack_len < g.pack_capacity {
        cards.push(card(
            text::PACK_KIND,
            text::PACK_HEADLINE,
            text::PACK_BODY,
            Some((g.pack_len as i32, g.pack_capacity as i32)),
        ));
    }
    if g.met_species {
        cards.push(card(text::MATE_KIND, text::MATE_HEADLINE, text::MATE_BODY, None));
    }
    cards.truncate(MAX_CARDS);
    cards
}

/// Resolve `loc:<Table>:<hex id>` references (one per line) to locale text;
/// anything else is literal text.
pub fn resolve(assets: &mut SpuiAssets, index: &ResourceIndex, text: &str) -> String {
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

/// Bottom edge of a card's panel on screen (cards stack and the pack list
/// hangs below them).
pub fn card_bottom(ui: &SpuiState) -> f32 {
    ui.find(card::PANEL).map_or(0.0, |w| ui.rect(w).y2)
}

/// Stack the headline and the description inside the panel and size the panel
/// to them. ADAPTED like the Cell card: the retail code sizes these windows to
/// their text (`IText::GetTextBounds`).
fn fit_card(ui: &mut SpuiState) {
    let (Some(panel), Some(group), Some(head), Some(desc)) =
        (ui.find(card::PANEL), ui.find(card::GROUP), ui.find(card::HEADLINE), ui.find(card::DESCRIPTION))
    else {
        return;
    };
    let area = |ui: &SpuiState, w: WinId| ui.layout.window(w).area;
    let (a_panel, a_group, a_head, a_desc) = (area(ui, panel), area(ui, group), area(ui, head), area(ui, desc));
    let height = |ui: &SpuiState, w: WinId| ui.text_extent(w).map_or(0.0, |e| e.y);
    // One 12 px headline line is 15 px high; the description wraps freely.
    let head_h = height(ui, head).max(15.0);
    let desc_h = height(ui, desc);
    // The authored headline sits 13 px below the panel top in this layout (11
    // in the Cell card, where the matched retail offset is 6).
    const PAD_TOP: f32 = 10.0;
    const PAD_BOTTOM: f32 = 8.0;
    let head_top = a_panel[1] + PAD_TOP;
    let desc_top = head_top + head_h;
    ui.set_area(head, [a_head[0], head_top, a_head[2], head_top + head_h]);
    ui.set_area(group, [a_group[0], a_group[1], a_group[2], desc_top + desc_h + 2.0]);
    let rel = desc_top - a_group[1];
    ui.set_area(desc, [a_desc[0], rel, a_desc[2], rel + desc_h]);
    ui.set_area(panel, [a_panel[0], a_panel[1], a_panel[2], desc_top + desc_h + PAD_BOTTOM]);
}

/// Fill one mission card and put its root at `(CARD_X, y)`.
fn bind_card(ui: &mut SpuiState, goal: &CellGoalCard, y: f32, assets: &mut SpuiAssets, index: &ResourceIndex) {
    let Some(root) = ui.find(card::ROOT) else {
        return;
    };
    ui.set_visible(root, true);
    let authored = ui.layout.window(root).area;
    ui.set_area(root, [CARD_X, y, CARD_X + authored[2] - authored[0], y + authored[3] - authored[1]]);
    let kind = resolve(assets, index, &goal.kind);
    let headline = resolve(assets, index, &goal.title);
    let description = resolve(assets, index, &goal.body);
    for (control, value) in [(card::KIND, kind), (card::HEADLINE, headline), (card::DESCRIPTION, description)] {
        if let Some(w) = ui.find(control) {
            ui.set_text(w, value);
        }
    }
    fit_card(ui);
    let count = goal.progress.map(|(a, b)| format!("{a}/{b}")).unwrap_or_default();
    if let Some(w) = ui.find(card::COUNT) {
        ui.set_text(w, count);
    }
    if let Some(w) = ui.find(card::COUNT_BACKDROP) {
        ui.set_visible(w, goal.progress.is_some());
    }
}

/// How long an encounter message stays on the hint bar, seconds.
pub const MESSAGE_SECONDS: f32 = 4.0;

// ---------------------------------------------------------------------------
// The retail hint manager
// ---------------------------------------------------------------------------

// The hint manager itself is shared by the four stage guides (`hint_manager`).
pub use crate::hint_manager::{HintMachine, HintRule};
use crate::hint_manager::HintBar;

/// Condition names the checker answers in the Creature game mode (function
/// 0xE07E50, VERIFIED): everything else is false.
#[allow(dead_code)]
pub mod cond {
    /// The game mode is Creature (0x1654C01).
    #[allow(unused_imports)]
    pub use crate::hint_manager::cond::{CRG, INACTIVE, USER_BUSY};
    /// The avatar's health fraction (0xBFC5D0) is at least 0.5 (0x1582E1C)
    /// and it is not dead.
    pub const HEALTHY: u32 = 0xAB57_83E0;
    /// The avatar's hunger (0xC0BB20, 0..100) is below `MotiveCritical`
    /// (0x1687A10, 10) and it is not dead.
    pub const STARVING: u32 = 0x3E79_C333;
    /// The camera interface (0x2A7ECD2) field `+0x14` is above 25.0.
    pub const CAMERA_ROTATED: u32 = 0x4729_8C4B;
    /// `cCreatureGameData` flag `+0xE` or `+0x10`: the movement input.
    pub const CREATURE_MOVED: u32 = 0xD3DF_9E3F;
}

/// Rotation of the camera (degrees) that counts as `CameraRotated`: the retail
/// condition tests a camera field above 25.0 (0xE08140..0xE0816B).
/// ADAPTED: the field's meaning was not identified; the rewrite measures the
/// player's turn of the camera around the avatar with the same threshold.
pub const CAMERA_TURNED_DEGREES: f32 = 25.0;

/// The Creature hints the guide runs, in tie-break order (the explicit error /
/// event hints first, then the tutorial and the two warnings).
pub const HINT_IDS: [&str; 10] = [
    "crg-abilitynotready",
    "crg-spitclose",
    "crg-notarget",
    "crg-socialalreadyfriends",
    "crg-wrongfood",
    "crg-socialgame",
    "crg-rotatecamera",
    "crg-movecreature",
    "crg-lowhealth",
    "crg-starving",
];
pub const HINT_ROTATE: &str = "crg-rotatecamera";
pub const HINT_MOVE: &str = "crg-movecreature";
pub const HINT_SOCIAL_GAME: &str = "crg-socialgame";

/// The condition answers of the Creature checker.
#[derive(Clone, Copy, Debug, Default)]
pub struct Conditions {
    pub healthy: bool,
    pub starving: bool,
    pub camera_rotated: bool,
    pub creature_moved: bool,
}

impl Conditions {
    pub fn eval(&self, name: u32) -> bool {
        match name {
            cond::CRG => true,
            cond::HEALTHY => self.healthy,
            cond::STARVING => self.starving,
            cond::CAMERA_ROTATED => self.camera_rotated,
            cond::CREATURE_MOVED => self.creature_moved,
            // `UserBusy`, `Inactive` and every name the Creature checker does not know.
            _ => false,
        }
    }
}

/// `healthy` and `starving` from the avatar (0xE07FC3 / 0xE0800B): both need a
/// living avatar; `healthy` is a health fraction of at least one half,
/// `starving` a hunger below `MotiveCritical` (10 of 100).
pub fn health_conditions(health: f32, max_health: f32, hunger_fraction: f32) -> (bool, bool) {
    let alive = health > 0.0;
    let fraction = if max_health > 0.0 { health / max_health } else { 0.0 };
    (alive && fraction >= 0.5, alive && hunger_fraction * 100.0 < MOTIVE_CRITICAL_HUNGER)
}

/// `MotiveCritical` (global 0x1687A10) as a hunger value.
const MOTIVE_CRITICAL_HUNGER: f32 = 10.0;

/// The camera's turn around the avatar, in degrees: the signed angle between
/// the direction behind the avatar and the direction to the camera, on the
/// plane the avatar stands on.
pub fn camera_turn(avatar: &Transform, camera: Vec3) -> f32 {
    let up = avatar.up().as_vec3();
    let flat = |v: Vec3| v - up * v.dot(up);
    let back = flat(avatar.rotation * Vec3::Z);
    let to_camera = flat(camera - avatar.translation);
    if back.length_squared() < 1e-8 || to_camera.length_squared() < 1e-8 {
        return 0.0;
    }
    let sign = if back.cross(to_camera).dot(up) < 0.0 { -1.0 } else { 1.0 };
    sign * back.angle_between(to_camera).to_degrees()
}

/// An angle difference in degrees wrapped to -180..180.
fn wrap_degrees(d: f32) -> f32 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

/// Retail hint for an encounter message, if its meaning matches one: the
/// message line of an ability error says what the retail hint says (the hint
/// is shown instead of the text, see `AbilityError`).
pub fn retail_hint_for(message: &str) -> Option<&'static str> {
    crate::creature_actions::AbilityError::from_message(message).map(|error| error.hint())
}

/// The error hint to raise now: `Some` when the actions code recorded a failure
/// since the last call. The first call of a visit only reads the counter, so a
/// failure of an earlier visit does not show again.
pub fn new_error_hint(seen: &mut Option<u32>, encounter: &Encounter) -> Option<&'static str> {
    let serial = encounter.error_serial;
    let before = seen.replace(serial)?;
    (before != serial).then(|| encounter.error.map(|error| error.hint())).flatten()
}

/// The hint the encounter message stands for when the retail game has no
/// hint of that meaning: the text itself. ADAPTED: the message line is the
/// rewrite's feedback channel (retail has the error hints above only).
pub fn message_hint(message: &str, age: f32) -> Option<String> {
    (!message.is_empty() && age < MESSAGE_SECONDS && retail_hint_for(message).is_none()).then(|| format!("msg:{message}"))
}

/// The retail introduction: welcome, progress bar, goals (each text leads to
/// the next: "Let's take a look...", "Let's go to your first goal...").
pub const OPENING_CALLOUTS: [&str; 3] = ["Callout_CRG_Opening", "Callout_CRG_ProgressBar", "Callout_CRG_FirstGoals"];
/// The stance explanation, the first time a creature is targeted
/// (the retail first-time hint `CRG_TargetedCreature`).
pub const STANCE_CALLOUT: &str = "Callout_CRG_Stance";
/// "Congratulations", then the Advance button, at the last brain level.
pub const FINAL_CALLOUTS: [&str; 2] = ["Callout_CRG_LastBrainUpgrade", "Callout_CRG_TransitionToTribe"];
/// The pack bar, when the first pack slot opens.
pub const PACK_CALLOUT: &str = "Callout_CRG_NewPackMember";
/// ADAPTED: seconds after the HUD appears before the introduction starts.
const CALLOUT_DELAY: f32 = 1.5;

/// Callout names that start with this are cinematic text windows of the stage
/// flow (`moment:CRG_BrainUpgrade1`): the closing text of the cinematic script,
/// shown on the callout dialog.
pub const MOMENT_PREFIX: &str = "moment:";

/// The callout of a cinematic's text window: its caption (the script's
/// `description`) as the title and the installed `cinematics_*` string.
pub fn moment_callout(assets: &mut SpuiAssets, index: &ResourceIndex, cinematic: &str) -> Option<CalloutDef> {
    let def = crate::creature_cinematics::cinematic(cinematic)?;
    let (table, id) = def.text?;
    let text = assets.localized(index, crate::world_objects::name_id(table.name()), id)?;
    Some(CalloutDef { title: def.caption.unwrap_or_default().to_string(), text: dialogs::paragraphs(&text), targets: Vec::new() })
}

/// Flags of the capture demo (`--creature-hud-*`).
#[derive(Clone, Debug, Default)]
pub struct GuideFlags {
    /// No introduction callouts and no first-minute hints.
    pub quiet: bool,
    pub callout: Option<String>,
    pub hint: Option<String>,
}

impl GuideFlags {
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned());
        Self {
            quiet: args.iter().any(|a| a == "--creature-hud-quiet"),
            callout: value("--creature-hud-callout"),
            hint: value("--creature-hud-hint"),
        }
    }
}

/// Timers and the callout queue of the guide.
#[derive(Resource, Default)]
pub struct GuideState {
    flags: Option<GuideFlags>,
    /// Seconds the HUD has been shown in this visit of the stage.
    pub elapsed: f32,
    message: String,
    message_age: f32,
    /// `Encounter::error_serial` as last seen (None: not yet read in this visit).
    error_serial: Option<u32>,
    started: bool,
    /// This visit began with a new lineage and shows the introduction, the
    /// introduction is still being read, and when it ended.
    fresh_visit: bool,
    intro_active: bool,
    intro_end: Option<f32>,
    stance_shown: bool,
    /// The retail hint manager with the Creature hints, and what drives its
    /// conditions: the camera's turn when the tutorial hints began, whether it
    /// passed [`CAMERA_TURNED_DEGREES`], and whether a social contest was on.
    machine: Option<HintMachine>,
    /// Times the manager cleared the bar at once (the binder's fade restarts).
    hint_cuts: u32,
    tutorial_hints_shown: bool,
    camera_reference: Option<f32>,
    camera_turned: bool,
    contest_open: bool,
    /// Brain level of the previous frame (the last level shows its callouts
    /// once) and the first pack slot's callout has been shown.
    last_brain: u8,
    pack_shown: bool,
    queue: Vec<String>,
}

impl GuideState {
    /// Forget everything about the visit (the stage was left).
    fn reset(&mut self) {
        let flags = self.flags.take();
        *self = Self { flags, ..default() };
    }

    /// Dismiss the front callout.
    pub fn dismiss(&mut self) {
        if !self.queue.is_empty() {
            self.queue.remove(0);
        }
    }
}

/// Loaded callout and hint definitions and the hint bar's fade (the retail
/// 1 s in, 2 s out, [`HintBar`]).
#[derive(Resource, Default)]
pub struct GuideCache {
    callouts: HashMap<String, Option<CalloutDef>>,
    hints: HashMap<String, Option<HintDef>>,
    hint_bar: HintBar,
}

/// Fill the goal cards, hint and callouts of [`CreatureHudState`].
#[allow(clippy::too_many_arguments)]
pub fn publish_guide(
    time: Res<Time>,
    stage: Res<State<GameStage>>,
    active: Res<CreatureHudActive>,
    run: Res<RunProgress>,
    encounter: Res<Encounter>,
    keys: Res<ButtonInput<KeyCode>>,
    flow: Res<crate::creature_flow::CreatureFlow>,
    originals: Option<Res<crate::Originals>>,
    controls: Option<Res<CreatureControls>>,
    player: Query<&Transform, With<Player>>,
    camera: Query<&Transform, (With<crate::GameCamera>, Without<Player>)>,
    mut guide: ResMut<GuideState>,
    mut hud: ResMut<CreatureHudState>,
) {
    if *stage.get() != GameStage::Creature {
        // The visit is over: the next one starts the introduction again.
        guide.reset();
    }
    if *stage.get() != GameStage::Creature || !active.0 {
        // No HUD (the creator is open, or the layouts are not available): the
        // guide waits where it is and shows nothing.
        hud.callouts.clear();
        hud.hint = None;
        hud.goal_cards.clear();
        return;
    }
    let flags = guide.flags.get_or_insert_with(GuideFlags::from_args).clone();
    let dt = time.delta_secs();
    guide.elapsed += dt;
    // The retail hint manager with the Creature hints (loaded once per visit).
    if guide.machine.is_none() {
        if let Some(originals) = originals.as_deref().filter(|o| !o.resource_index.records.is_empty()) {
            let rules = HINT_IDS.iter().filter_map(|id| HintRule::read(&originals.resource_index, id)).collect();
            guide.machine = Some(HintMachine::new(rules));
        }
    }
    let new_message = guide.message != encounter.message;
    if new_message {
        guide.message = encounter.message.clone();
        guide.message_age = 0.0;
    } else {
        guide.message_age += dt;
    }
    // An ability button that failed raises its retail error hint (the button
    // handler 0xD49560 switches on the tray check's code and calls `ShowHint`);
    // a failure while the hint is already on the bar changes nothing (0x67C809).
    if let Some(hint) = new_error_hint(&mut guide.error_serial, &encounter) {
        if let Some(machine) = guide.machine.as_mut() {
            machine.show(hint);
        }
    }
    // Callouts: a fresh lineage gets the introduction once per visit.
    let fresh = hud.stage_dna <= 0.0
        && run.creature_progression.brain_level == 0
        && run.creature_lifecycle.generation <= 1
        && run.creature_pack.members.is_empty();
    if !guide.started && guide.elapsed >= CALLOUT_DELAY {
        guide.started = true;
        if let Some(name) = &flags.callout {
            guide.queue = vec![name.clone()];
        } else if fresh && !flags.quiet {
            guide.queue = OPENING_CALLOUTS.iter().map(|n| n.to_string()).collect();
            guide.fresh_visit = true;
            guide.intro_active = true;
        }
    }
    if guide.intro_active && guide.queue.is_empty() {
        guide.intro_active = false;
        guide.intro_end = Some(guide.elapsed);
    }
    // Later callouts, once the introduction has been offered: the stance the
    // first time a creature is targeted, the first pack slot (a brain level
    // opens it) and the last brain level.
    if guide.started && !flags.quiet {
        if guide.fresh_visit && !guide.stance_shown && encounter.target.is_some() {
            guide.stance_shown = true;
            guide.queue.push(STANCE_CALLOUT.to_string());
        }
        let last = crate::creature_progression::MAX_BRAIN_LEVEL;
        if hud.brain_level >= last && guide.last_brain < last {
            guide.queue.extend(FINAL_CALLOUTS.iter().map(|n| n.to_string()));
        }
        if !guide.pack_shown && hud.pack_capacity > 0 && hud.pack.is_empty() {
            guide.pack_shown = true;
            guide.queue.push(PACK_CALLOUT.to_string());
        }
    }
    guide.last_brain = hud.brain_level;
    // Enter closes the moment's text window first (the flow reads the key
    // itself); the guide's own callouts wait behind it.
    let moment_showing = hud.callouts.first().is_some_and(|c| c.starts_with(MOMENT_PREFIX));
    if keys.just_pressed(crate::cell_flow::DISMISS_KEY) && !moment_showing {
        guide.dismiss();
    }
    hud.callouts = guide.queue.clone();
    if let Some(cinematic) = flow.moment {
        hud.callouts.insert(0, crate::creature_flow::moment_name(cinematic));
    }
    // Hint: the demo flag, else the hint manager.
    let species = &run.species_world;
    let outcomes = species.outcome_counts();
    let contest_open = hud.social_meter.is_some();
    if contest_open && !guide.contest_open && outcomes.allied == 0 {
        // The first social contest of a lineage tells how to answer (0xDA4840:
        // `ShowHint` at the start of the proposal until the first friend, the
        // one-time flag 0x34220E8E is set by the befriend path 0xD9BBD7).
        if let Some(machine) = guide.machine.as_mut() {
            machine.show(HINT_SOCIAL_GAME);
        }
    }
    guide.contest_open = contest_open;
    // The tutorial hints: after the introduction callouts, with Tutorials on
    // (0xD43434 / 0xD43445: `ShowHint` rotate camera, then move creature).
    let tutorials = crate::front_end::settings::tutorials_enabled() && !flags.quiet;
    if tutorials && guide.intro_end.is_some() && !guide.tutorial_hints_shown {
        guide.tutorial_hints_shown = true;
        if let Some(machine) = guide.machine.as_mut() {
            machine.show(HINT_ROTATE);
            machine.show(HINT_MOVE);
        }
    }
    // The checker's inputs.
    if guide.tutorial_hints_shown {
        if let (Ok(avatar), Ok(view)) = (player.single(), camera.single()) {
            let angle = camera_turn(avatar, view.translation);
            let reference = *guide.camera_reference.get_or_insert(angle);
            if wrap_degrees(angle - reference).abs() > CAMERA_TURNED_DEGREES {
                guide.camera_turned = true;
            }
        }
    }
    let (healthy, starving) = health_conditions(hud.health, hud.max_health, hud.hunger_fraction());
    let steering = controls.as_deref().is_some_and(|c| c.goal.is_some() || c.held_move || c.autorun)
        || read_axes(&keys).any();
    let conditions = Conditions { healthy, starving, camera_rotated: guide.camera_turned, creature_moved: steering };
    let callout_open = !hud.callouts.is_empty();
    let hints_on = crate::front_end::settings::hints_enabled();
    if !callout_open {
        // The game is held while a callout is up: the hints wait.
        if let Some(machine) = guide.machine.as_mut() {
            machine.update(dt, &|name| conditions.eval(name), hints_on);
        }
    }
    guide.hint_cuts = guide.machine.as_ref().map_or(0, HintMachine::cuts);
    hud.hint = match &flags.hint {
        Some(id) => Some(id.clone()),
        None if flags.quiet || callout_open => None,
        None => message_hint(&guide.message, guide.message_age)
            .filter(|_| hints_on)
            .or_else(|| guide.machine.as_ref().and_then(|m| m.visible()).map(str::to_string)),
    };
    // Goals.
    let allied_species = species.species.iter().filter(|s| species.species_allied(s.id)).count();
    let inputs = GoalInputs {
        diet: hud.diet,
        hunger: hud.hunger_fraction(),
        met_species: outcomes.allied + outcomes.defeated > 0,
        allied_species,
        pack_len: hud.pack.len(),
        pack_capacity: hud.pack_capacity,
        migrating: run.creature_progression.pending_migration.is_some(),
        advance: hud.advance_visible,
    };
    hud.goal_cards = goal_cards(&inputs);
}

/// The rectangle of a window of the Main, Card or Posse layouts (callout
/// targets), if it is shown.
fn target_rect(roots: &Query<(&mut SpuiState, &CreatureHudRoot)>, control: u32) -> Option<Rect> {
    roots.iter().find_map(|(ui, kind)| {
        if !ui.ready || !matches!(kind, CreatureHudRoot::Main | CreatureHudRoot::Card(0) | CreatureHudRoot::Posse) {
            return None;
        }
        ui.find(control).filter(|w| ui.visible(*w)).map(|w| ui.rect(w))
    })
}

/// Bind the mission cards, the hint bar and the callout dialog.
#[allow(clippy::too_many_arguments)]
pub fn bind_guide(
    time: Res<Time>,
    hud: Res<CreatureHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<GuideCache>,
    mut guide: ResMut<GuideState>,
    mut roots: Query<(&mut SpuiState, &CreatureHudRoot)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<crate::player_hud::InjectedKeys>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    // Callout: definition and the rectangle of its target window.
    let callout_name = hud.callouts.first().cloned();
    let moment = callout_name.as_deref().and_then(|n| n.strip_prefix(MOMENT_PREFIX)).map(str::to_string);
    if let Some(name) = &callout_name {
        if moment.is_none() && !cache.callouts.contains_key(name) {
            let def = dialogs::read_callout(index, &mut assets, name);
            cache.callouts.insert(name.clone(), def);
        }
    }
    let def = match &moment {
        Some(cinematic) => moment_callout(&mut assets, index, cinematic),
        None => callout_name.as_ref().and_then(|n| cache.callouts.get(n)).cloned().flatten(),
    };
    let target = def.as_ref().and_then(|d| d.targets.first()).and_then(|control| target_rect(&roots, *control));
    // Hint: the retail fade (`HintFade`: 1 s in, 2 s out) towards the published hint.
    let dt = time.delta_secs();
    cache.hint_bar.step(hud.hint.as_deref(), guide.hint_cuts, dt);
    let hint_def = cache.hint_bar.shown().map(str::to_string).and_then(|id| {
        if let Some(text) = id.strip_prefix("msg:") {
            return Some(HintDef { before: text.to_string(), ..default() });
        }
        if !cache.hints.contains_key(&id) {
            let def = dialogs::read_hint(index, &mut assets, &id);
            cache.hints.insert(id.clone(), def);
        }
        cache.hints.get(&id).cloned().flatten()
    });
    let icon = hint_def.as_ref().and_then(|d| d.icon).and_then(|key| {
        assets.texture(index, &mut images, key).map(|_| ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None })
    });
    let alpha = cache.hint_bar.alpha();
    // Cards stack downwards by their measured height.
    let mut next_y = CARD_Y;
    let mut cards: Vec<(u8, f32)> = roots
        .iter()
        .filter_map(|(_, kind)| match kind {
            CreatureHudRoot::Card(n) => Some((*n, 0.0)),
            _ => None,
        })
        .collect();
    cards.sort_by_key(|(n, _)| *n);
    for (n, y) in &mut cards {
        *y = next_y;
        let height = roots.iter().find(|(_, kind)| **kind == CreatureHudRoot::Card(*n)).map_or(0.0, |(ui, _)| {
            if ui.ready {
                card_bottom(ui) - ui.rect(ui.find(card::ROOT).unwrap_or(0)).y1
            } else {
                0.0
            }
        });
        next_y += height.max(60.0) + CARD_GAP;
    }
    let mut dismiss = false;
    for (mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match kind {
            CreatureHudRoot::Card(n) => {
                if let (Some(goal), Some((_, y))) =
                    (hud.goal_cards.get(*n as usize), cards.iter().find(|(c, _)| c == n))
                {
                    bind_card(&mut ui, goal, *y, &mut assets, index);
                }
            }
            CreatureHudRoot::Callout => {
                dialogs::bind_callout(&mut ui, def.as_ref(), target, 0.5);
                for click in ui.take_clicks() {
                    if ui.layout.window(click).control_id == dialogs::callout::OK {
                        dismiss = true;
                    }
                }
            }
            CreatureHudRoot::Hint => dialogs::bind_hint(&mut ui, hint_def.as_ref(), alpha, icon.clone()),
            _ => {}
        }
    }
    if dismiss {
        if moment.is_some() {
            // The flow closes its window on the dismiss key (the same key as
            // Enter), so the click presses it.
            if !keys.pressed(crate::cell_flow::DISMISS_KEY) {
                keys.press(crate::cell_flow::DISMISS_KEY);
                injected.push(crate::cell_flow::DISMISS_KEY);
            }
        } else {
            guide.dismiss();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hint_manager::{KIND_NORMAL, KIND_ONESHOT, KIND_TRIGGERED};

    fn kinds(cards: &[CellGoalCard]) -> Vec<&str> {
        cards.iter().map(|c| c.kind.as_str()).collect()
    }

    #[test]
    fn a_new_hungry_herbivore_is_told_to_eat_and_to_meet_creatures() {
        let cards = goal_cards(&GoalInputs { diet: Diet::Herbivore, hunger: 0.3, ..default() });
        assert_eq!(kinds(&cards), ["loc:CreatureGame:0509f9d9", "loc:CreatureGame:0509fb92"]);
        // The diet picks the headline and the description.
        assert_eq!(cards[0].title, "loc:CreatureGame:0524d325");
        assert_eq!(cards[0].body, "loc:CreatureGame:0509f9dc");
        let carnivore = goal_cards(&GoalInputs { diet: Diet::Carnivore, hunger: 0.1, ..default() });
        assert_eq!(carnivore[0].title, "loc:CreatureGame:0524d324");
    }

    #[test]
    fn a_fed_creature_only_needs_to_meet_others() {
        let cards = goal_cards(&GoalInputs { hunger: 1.0, ..default() });
        assert_eq!(kinds(&cards), ["loc:CreatureGame:0509fb92"]);
    }

    #[test]
    fn after_the_first_friend_the_pack_and_the_mate_come_up() {
        let cards = goal_cards(&GoalInputs {
            hunger: 1.0,
            met_species: true,
            allied_species: 1,
            pack_len: 0,
            pack_capacity: 2,
            ..default()
        });
        assert_eq!(kinds(&cards), ["loc:CreatureGame:0509fc96", "loc:CreatureGame:0509fc4e"]);
        assert_eq!(cards[0].progress, Some((0, 2)));
        // A full pack drops the pack goal.
        let full = goal_cards(&GoalInputs {
            hunger: 1.0,
            met_species: true,
            allied_species: 1,
            pack_len: 2,
            pack_capacity: 2,
            ..default()
        });
        assert_eq!(kinds(&full), ["loc:CreatureGame:0509fc4e"]);
    }

    #[test]
    fn the_end_of_the_stage_comes_first_and_the_stack_is_bounded() {
        let cards = goal_cards(&GoalInputs {
            hunger: 0.1,
            migrating: true,
            allied_species: 1,
            pack_capacity: 3,
            advance: true,
            ..default()
        });
        assert_eq!(cards.len(), MAX_CARDS);
        assert_eq!(cards[0].kind, "loc:CreatureGame:0546ab9a");
    }

    /// The retail tutorial pair (`crg-rotatecamera` 100, `crg-movecreature` 90,
    /// both Triggered, shown until their condition holds).
    fn tutorial_machine() -> HintMachine {
        let rotate = HintRule {
            until: vec![cond::CAMERA_ROTATED],
            priority: 100,
            ..HintRule::new(HINT_ROTATE)
        };
        let moving = HintRule { until: vec![cond::CREATURE_MOVED], priority: 90, ..HintRule::new(HINT_MOVE) };
        HintMachine::new(vec![rotate, moving])
    }

    fn run(machine: &mut HintMachine, seconds: f32, c: Conditions) {
        let steps = (seconds * 10.0).round() as u32;
        for _ in 0..steps {
            machine.update(0.1, &|name| c.eval(name), true);
        }
    }

    #[test]
    fn the_tutorial_hints_come_one_after_the_other_until_their_condition_holds() {
        let mut m = tutorial_machine();
        run(&mut m, 5.0, Conditions::default());
        assert_eq!(m.visible(), None, "nothing is shown before the explicit call");
        // 0xD4342E / 0xD4343F: both hints are asked for; the higher priority is on the bar.
        m.show(HINT_ROTATE);
        m.show(HINT_MOVE);
        run(&mut m, 0.1, Conditions::default());
        assert_eq!(m.visible(), Some(HINT_ROTATE));
        // It stays as long as the camera was not turned (no display time).
        run(&mut m, 60.0, Conditions::default());
        assert_eq!(m.visible(), Some(HINT_ROTATE));
        // The camera turned: 2 s of fade out, then the movement hint follows at once.
        let turned = Conditions { camera_rotated: true, ..default() };
        run(&mut m, 0.2, turned);
        assert_eq!(m.state(HINT_ROTATE), 6);
        assert_eq!(m.visible(), None, "fading out");
        run(&mut m, 2.0, turned);
        assert_eq!(m.state(HINT_ROTATE), 0, "a Triggered hint is done for good");
        assert_eq!(m.visible(), Some(HINT_MOVE));
        let moving = Conditions { camera_rotated: true, creature_moved: true, ..default() };
        run(&mut m, 1.5, moving);
        assert_eq!((m.state(HINT_MOVE), m.visible()), (6, None), "moving ends it after its fade in");
        run(&mut m, 2.5, moving);
        assert_eq!((m.state(HINT_MOVE), m.visible()), (0, None));
    }

    #[test]
    fn a_hint_whose_condition_already_holds_is_never_shown() {
        let mut m = tutorial_machine();
        m.show(HINT_ROTATE);
        m.show(HINT_MOVE);
        // The player moved before the movement hint got the bar: state 3 -> 0.
        let moved = Conditions { creature_moved: true, ..default() };
        run(&mut m, 0.1, moved);
        assert_eq!((m.state(HINT_MOVE), m.visible()), (0, Some(HINT_ROTATE)));
        run(&mut m, 0.3, Conditions { camera_rotated: true, ..moved });
        run(&mut m, 4.0, Conditions { camera_rotated: true, ..moved });
        assert_eq!(m.visible(), None, "the movement hint was skipped");
    }

    fn low_health_rule() -> HintRule {
        // `crg-lowhealth` as installed: A [crg], B [healthy, UserBusy], C [healthy],
        // delays 3 s and 30 s, shown 10 s, Oneshot, priority 1.
        HintRule {
            all_of: vec![cond::CRG],
            none_of: vec![cond::HEALTHY, cond::USER_BUSY],
            until: vec![cond::HEALTHY],
            first_delay: 3.0,
            second_delay: 30.0,
            display: 10.0,
            priority: 1,
            kind: KIND_ONESHOT,
            ..HintRule::new("crg-lowhealth")
        }
    }

    #[test]
    fn low_health_must_last_before_the_warning_shows() {
        let mut m = HintMachine::new(vec![low_health_rule()]);
        let hurt = Conditions::default();
        run(&mut m, 32.0, hurt);
        assert_eq!(m.visible(), None, "3 s + 30 s of being hurt come first");
        run(&mut m, 2.0, hurt);
        assert_eq!(m.visible(), Some("crg-lowhealth"));
        // Shown for 1 s of fade in and 10 s, then it fades out for 2 s and starts over.
        run(&mut m, 12.0, hurt);
        assert_eq!(m.visible(), None);
        assert_eq!(m.state("crg-lowhealth"), 7);
        run(&mut m, 2.2, hurt);
        assert_eq!(m.state("crg-lowhealth"), 1, "a Oneshot hint whose time ran out waits again");
    }

    #[test]
    fn healing_before_the_second_check_cancels_the_warning() {
        let mut m = HintMachine::new(vec![low_health_rule()]);
        run(&mut m, 10.0, Conditions::default());
        assert_eq!(m.state("crg-lowhealth"), 2);
        // Healthy again when the 30 s are over: back to waiting, nothing shown.
        run(&mut m, 31.0, Conditions { healthy: true, ..default() });
        assert_eq!((m.state("crg-lowhealth"), m.visible()), (1, None));
    }

    #[test]
    fn a_warning_ends_when_the_creature_is_healthy_again_and_a_oneshot_never_returns() {
        let mut m = HintMachine::new(vec![low_health_rule()]);
        run(&mut m, 34.0, Conditions::default());
        assert_eq!(m.visible(), Some("crg-lowhealth"));
        run(&mut m, 1.5, Conditions { healthy: true, ..default() });
        assert_eq!(m.state("crg-lowhealth"), 6, "condition C ends the hint");
        run(&mut m, 2.2, Conditions { healthy: true, ..default() });
        assert_eq!(m.state("crg-lowhealth"), 0, "Oneshot: not again");
        run(&mut m, 120.0, Conditions::default());
        assert_eq!(m.visible(), None);
    }

    #[test]
    fn an_error_hint_takes_the_bar_from_a_warning() {
        let error = HintRule { display: 5.0, priority: 100, ..HintRule::new("crg-abilitynotready") };
        let mut m = HintMachine::new(vec![error, low_health_rule()]);
        run(&mut m, 34.0, Conditions::default());
        assert_eq!(m.visible(), Some("crg-lowhealth"));
        m.show("crg-abilitynotready");
        run(&mut m, 0.1, Conditions::default());
        assert_eq!(m.visible(), Some("crg-abilitynotready"));
        // 1 s in, 5 s shown, 2 s out: then the warning is a candidate again.
        run(&mut m, 7.0, Conditions::default());
        assert_eq!(m.state("crg-abilitynotready"), 7);
        run(&mut m, 2.0, Conditions::default());
        assert_eq!(m.state("crg-abilitynotready"), 0);
    }

    #[test]
    fn with_game_hints_off_only_the_always_hints_are_selected() {
        let plain = HintRule { priority: 100, ..HintRule::new(HINT_ROTATE) };
        let always = HintRule { priority: 100, always: true, ..HintRule::new("crg-abilitynotready") };
        let mut m = HintMachine::new(vec![plain, always]);
        m.show(HINT_ROTATE);
        m.update(0.1, &|_| false, false);
        assert_eq!(m.visible(), None);
        m.show("crg-abilitynotready");
        m.update(0.1, &|_| false, false);
        assert_eq!(m.visible(), Some("crg-abilitynotready"));
    }

    #[test]
    fn hints_that_are_not_triggered_wait_twenty_seconds_after_another_hint() {
        let starving = HintRule {
            all_of: vec![cond::CRG],
            first_delay: 1.0,
            second_delay: 1.0,
            display: 2.0,
            priority: 1,
            kind: KIND_NORMAL,
            ..HintRule::new("crg-starving")
        };
        let error = HintRule { display: 1.0, priority: 100, ..HintRule::new("crg-abilitynotready") };
        let mut m = HintMachine::new(vec![error, starving]);
        run(&mut m, 5.0, Conditions::default());
        assert_eq!(m.visible(), Some("crg-starving"), "nothing was shown before: no gap");
        m.show("crg-abilitynotready");
        run(&mut m, 0.1, Conditions::default());
        assert_eq!(m.visible(), Some("crg-abilitynotready"));
        // The warning is a candidate again at once but must wait out the gap.
        run(&mut m, 4.0, Conditions::default());
        assert_eq!(m.visible(), None, "within 20 s of the last hint");
        run(&mut m, 18.0, Conditions::default());
        assert_eq!(m.visible(), Some("crg-starving"));
    }

    #[test]
    fn the_checker_answers_the_creature_conditions() {
        // The ids are the FNV hashes of the names the hint files use.
        for (id, name) in [
            (cond::CRG, "crg"),
            (cond::HEALTHY, "healthy"),
            (cond::STARVING, "starving"),
            (cond::USER_BUSY, "UserBusy"),
            (cond::CAMERA_ROTATED, "CameraRotated"),
            (cond::CREATURE_MOVED, "CreatureMoved"),
            (cond::INACTIVE, "Inactive"),
            (KIND_TRIGGERED, "Triggered"),
            (KIND_NORMAL, "Normal"),
            (KIND_ONESHOT, "Oneshot"),
        ] {
            assert_eq!(id, crate::world_objects::name_id(name), "{name}");
        }
        let all = Conditions { healthy: true, starving: true, camera_rotated: true, creature_moved: true };
        assert!(all.eval(cond::CRG) && all.eval(cond::HEALTHY) && all.eval(cond::CREATURE_MOVED));
        // `UserBusy` and `Inactive` are constant false, unknown names too.
        assert!(!all.eval(cond::USER_BUSY) && !all.eval(cond::INACTIVE) && !all.eval(0x1234_5678));
        // `healthy` needs half of the health and a living avatar; `starving` a
        // hunger below 10 of 100.
        assert_eq!(health_conditions(50.0, 100.0, 1.0), (true, false));
        assert_eq!(health_conditions(49.0, 100.0, 0.5), (false, false));
        assert_eq!(health_conditions(100.0, 100.0, 0.099), (true, true));
        assert_eq!(health_conditions(100.0, 100.0, 0.1), (true, false));
        assert_eq!(health_conditions(0.0, 100.0, 0.0), (false, false), "the dead are neither");
    }

    #[test]
    fn the_camera_turn_is_the_angle_around_the_avatar() {
        let avatar = Transform::from_translation(Vec3::new(2.0, 0.0, 3.0));
        // The default heading is -Z, so "behind" is +Z: a camera there has turned 0.
        let behind = avatar.translation + Vec3::new(0.0, 4.0, 10.0);
        assert!(camera_turn(&avatar, behind).abs() < 1e-3);
        let beside = avatar.translation + Vec3::new(10.0, 4.0, 0.0);
        let turned = camera_turn(&avatar, beside);
        assert!((turned.abs() - 90.0).abs() < 1e-3, "{turned}");
        let other = avatar.translation + Vec3::new(-10.0, 4.0, 0.0);
        assert!((camera_turn(&avatar, other) + turned).abs() < 1e-3, "the other side has the other sign");
        assert!((wrap_degrees(350.0) + 10.0).abs() < 1e-4 && (wrap_degrees(-190.0) - 170.0).abs() < 1e-4);
    }

    #[test]
    fn every_ability_failure_shows_its_retail_hint_once_per_failure() {
        use crate::creature_actions::AbilityError;
        let mut encounter = Encounter::default();
        let mut seen = None;
        // The first read of a visit only syncs the counter.
        encounter.fail(AbilityError::NotReady);
        assert_eq!(new_error_hint(&mut seen, &encounter), None);
        assert_eq!(new_error_hint(&mut seen, &encounter), None, "nothing new");
        for error in AbilityError::ALL {
            encounter.fail(error);
            assert_eq!(new_error_hint(&mut seen, &encounter), Some(error.hint()));
            assert_eq!(new_error_hint(&mut seen, &encounter), None);
            // The message line of the failure is the hint, not extra text.
            assert_eq!(retail_hint_for(&encounter.message), Some(error.hint()));
            assert_eq!(message_hint(&encounter.message, 0.0), None);
        }
        // The same failure again (another press) is a new failure.
        encounter.fail(AbilityError::NoTarget);
        assert_eq!(new_error_hint(&mut seen, &encounter), Some("crg-notarget"));
        encounter.fail(AbilityError::NoTarget);
        assert_eq!(new_error_hint(&mut seen, &encounter), Some("crg-notarget"));
    }

    #[test]
    fn an_error_hint_is_shown_on_the_bar_for_five_seconds_over_the_warnings() {
        let error = HintRule { display: 5.0, priority: 100, always: true, ..HintRule::new("crg-spitclose") };
        let mut m = HintMachine::new(vec![error, low_health_rule()]);
        run(&mut m, 34.0, Conditions::default());
        assert_eq!(m.visible(), Some("crg-lowhealth"));
        m.show("crg-spitclose");
        run(&mut m, 0.1, Conditions::default());
        assert_eq!(m.visible(), Some("crg-spitclose"));
        // Another press of the failing button while it is up changes nothing.
        let cuts = m.cuts();
        m.show("crg-spitclose");
        assert_eq!(m.cuts(), cuts);
        // 1 s in, 5 s shown, 2 s out.
        run(&mut m, 10.0, Conditions::default());
        assert_eq!(m.state("crg-spitclose"), 0, "a Triggered hint is done after its time");
    }

    #[test]
    fn messages_without_a_retail_hint_stay_text_for_a_few_seconds() {
        assert_eq!(message_hint("Target out of range", 0.0).as_deref(), Some("msg:Target out of range"));
        assert_eq!(message_hint("Target out of range", MESSAGE_SECONDS + 0.1), None);
        assert_eq!(message_hint("", 0.0), None);
        // A message with a retail hint raises it instead (see `publish_guide`).
        assert_eq!(retail_hint_for("Ability is cooling down"), Some("crg-abilitynotready"));
        assert_eq!(retail_hint_for("You're too close"), Some("crg-spitclose"));
        assert_eq!(message_hint("Ability is cooling down", 0.0), None);
    }

    #[test]
    fn dismissing_pops_the_front_callout() {
        let mut guide = GuideState { queue: vec!["a".into(), "b".into()], ..default() };
        guide.dismiss();
        assert_eq!(guide.queue, ["b"]);
        guide.dismiss();
        guide.dismiss();
        assert!(guide.queue.is_empty());
    }

    #[test]
    fn installed_goal_strings_and_card_windows_exist() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        // Every goal string resolves to retail text.
        let all = goal_cards(&GoalInputs {
            hunger: 0.0,
            migrating: true,
            allied_species: 1,
            pack_capacity: 3,
            advance: true,
            met_species: true,
            ..default()
        });
        let mut strings: Vec<String> = Vec::new();
        for diet in [Diet::Herbivore, Diet::Carnivore, Diet::Omnivore] {
            for card in goal_cards(&GoalInputs { diet, hunger: 0.0, ..default() }).into_iter().chain(all.clone()) {
                strings.extend([card.kind, card.title, card.body]);
            }
        }
        for s in strings {
            assert!(!resolve(&mut assets, &index, &s).is_empty(), "{s}");
        }
        let layout = assets.layout(&index, super::super::CARD_LAYOUT).expect("CRGMissionCard");
        for control in [
            card::ROOT,
            card::KIND,
            card::COUNT,
            card::COUNT_BACKDROP,
            card::HEADLINE,
            card::DESCRIPTION,
            card::PANEL,
            card::GROUP,
        ] {
            assert!(layout.find(control).is_some(), "card window {control:08X}");
        }
    }

    #[test]
    fn installed_creature_callouts_and_hints_decode() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        let opening = dialogs::read_callout(&index, &mut assets, "Callout_CRG_Opening").expect("opening");
        assert_eq!(opening.title, "Welcome to Land!");
        assert!(opening.text.starts_with("Now you are a creature"));
        assert!(opening.targets.is_empty());
        let bar = dialogs::read_callout(&index, &mut assets, "Callout_CRG_ProgressBar").expect("progress bar");
        assert_eq!(bar.title, "Making Progress");
        // The arrow points at the marker's pointer, the whole bar is lit.
        assert_eq!(bar.targets, vec![0x0672_0386, 0x051B_5478]);
        let goals = dialogs::read_callout(&index, &mut assets, "Callout_CRG_FirstGoals").expect("goals");
        assert_eq!(goals.title, "Goals");
        assert_eq!(goals.targets, vec![card::PANEL]);
        let stance = dialogs::read_callout(&index, &mut assets, "Callout_CRG_Stance").expect("stance");
        assert_eq!(stance.title, "Stance");
        assert_eq!(stance.targets, vec![super::super::id::STANCE_COMBAT]);
        for name in OPENING_CALLOUTS.iter().chain(&FINAL_CALLOUTS).chain(&[PACK_CALLOUT, STANCE_CALLOUT]) {
            assert!(dialogs::read_callout(&index, &mut assets, name).is_some(), "{name}");
        }
        let pack = dialogs::read_callout(&index, &mut assets, PACK_CALLOUT).unwrap();
        assert_eq!(pack.title, "Build a Pack");
        assert_eq!(pack.targets, vec![super::super::world::id::POSSE_ROOT]);
        let tribe = dialogs::read_callout(&index, &mut assets, FINAL_CALLOUTS[1]).unwrap();
        assert_eq!(tribe.title, "Advance to Tribe");
        assert_eq!(tribe.targets, vec![super::super::id::ADVANCE]);
        let last = dialogs::read_callout(&index, &mut assets, FINAL_CALLOUTS[0]).unwrap();
        assert_eq!(last.title, "Congratulations!");
        assert!(last.targets.is_empty());
        // The moments' text windows: the installed `cinematics_crg` strings.
        for (cinematic, start) in [
            ("CRG_BrainUpgrade1", "Brain power!"),
            ("CRG_BrainUpgrade4", "Congratulations!"),
            ("CRG_FirstDNA", "Well done!"),
            ("CLG2CRG", "It is the beginning"),
            ("CRG2TRG", "Congratulations! You are about to enter"),
        ] {
            let def = moment_callout(&mut assets, &index, cinematic).unwrap_or_else(|| panic!("{cinematic}"));
            assert!(def.text.starts_with(start), "{cinematic}: {}", def.text);
        }
        let low = dialogs::read_hint(&index, &mut assets, "crg-lowhealth").expect("low health");
        assert!(low.before.starts_with("You're very low on health"));
        let moving = dialogs::read_hint(&index, &mut assets, "crg-movecreature").expect("move");
        assert!(moving.before.contains("move your creature"), "the retail text exists: {moving:?}");
        let bite = dialogs::read_hint(&index, &mut assets, "crg-useabilitybite").expect("bite");
        assert_eq!((bite.before.as_str(), bite.after.as_str()), ("", "to learn to bite"));
        assert!(bite.icon.is_some());
        let mimic = dialogs::read_hint(&index, &mut assets, "crg-socialgame").expect("mimic");
        assert!(mimic.before.starts_with("Mimic the other creature"));
        for id in ["crg-starving", "crg-rotatecamera", "crg-abilitynotready"] {
            assert!(dialogs::read_hint(&index, &mut assets, id).is_some(), "{id}");
        }
        // The targets of the callouts exist in the layouts they live in.
        let main = assets.layout(&index, super::super::MAIN_LAYOUT).unwrap();
        for control in [0x0672_0386, 0x051B_5478, super::super::id::STANCE_COMBAT] {
            assert!(main.find(control).is_some(), "callout target {control:08X}");
        }
    }

    #[test]
    fn installed_hint_rules_match_the_decode() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let read = |id: &str| HintRule::read(&index, id).unwrap_or_else(|| panic!("{id}"));
        // `crg-lowhealth`: A [crg], B [healthy, UserBusy], C [healthy], 3 s + 30 s, shown 10 s.
        let low = read("crg-lowhealth");
        assert_eq!(low.all_of, [cond::CRG]);
        assert_eq!(low.none_of, [cond::HEALTHY, cond::USER_BUSY]);
        assert_eq!(low.until, [cond::HEALTHY]);
        assert_eq!((low.first_delay, low.second_delay, low.display, low.priority), (3.0, 30.0, 10.0, 1));
        assert_eq!((low.kind, low.always), (KIND_ONESHOT, false));
        // The fades are the constructor defaults: no installed hint overrides them.
        assert_eq!((low.fade_in, low.fade_out), (1.0, 2.0));
        let starving = read("crg-starving");
        assert_eq!(starving.all_of, [cond::CRG, cond::STARVING, cond::HEALTHY]);
        assert_eq!((starving.none_of.clone(), starving.until.is_empty()), (vec![cond::USER_BUSY], true));
        // The tutorial pair: Triggered, until the condition holds, rotate first.
        let rotate = read(HINT_ROTATE);
        let moving = read(HINT_MOVE);
        assert_eq!((rotate.until.clone(), rotate.display, rotate.priority, rotate.kind), (vec![cond::CAMERA_ROTATED], 0.0, 100, KIND_TRIGGERED));
        assert_eq!((moving.until.clone(), moving.display, moving.priority, moving.kind), (vec![cond::CREATURE_MOVED], 0.0, 90, KIND_TRIGGERED));
        // The error hints are shown for 5 s and ignore the Game Hints setting.
        let slow = read("crg-abilitynotready");
        assert_eq!((slow.display, slow.priority, slow.always, slow.kind), (5.0, 100, true, KIND_TRIGGERED));
        assert_eq!(read(HINT_SOCIAL_GAME).display, 2.0);
        // The other error hints are the same (the installed `crg-spitclose` has priority 1).
        for (id, priority) in [("crg-spitclose", 1), ("crg-notarget", 100), ("crg-socialalreadyfriends", 100)] {
            let hint = read(id);
            assert_eq!((hint.display, hint.priority, hint.always, hint.kind), (5.0, priority, true, KIND_TRIGGERED), "{id}");
        }
        // Every hint of the guide's list exists.
        for id in HINT_IDS {
            assert!(HintRule::read(&index, id).is_some(), "{id}");
        }
    }
}
