//! Mission cards, hints and callouts of the Space HUD, from the retail data:
//! the cards are the installed `SPGMissionCard` layout, the hints are the
//! `spg-*` property files (texts of `HintsSPG`) shown on the `hint` layout and
//! the callouts are the `Callout_SPG_*` property files shown on the callout
//! dialog layout (the generic machinery of `cell_hud::dialogs`).
//!
//! Every hint goes through the retail hint manager (`hint_manager`, one
//! `HintMachine` with the `spg-*` property files): the forced chain is
//! `ShowHint` / `HideHint` at the places of the retail chain (0x1065D30, 0xFFBEE0,
//! 0x101B69E, 0xFDE9EA, 0xFDA6E0), the conditional hints (`spg-opencommfirsttime`,
//! `-idle`, `-solarmove`, `-startravel`) are answered by the Space branch of the
//! condition checker 0xE07E50 ([`Conditions`]). What the retail code decides
//! (which mission is shown, in which order the callouts come) is not decoded
//! yet; those rules are ADAPTED
//! from the state the Space gameplay modules expose and marked as such. The
//! tutorial missions' cards take the retail `mission_tutorial_*` conversation
//! texts where the objective exists in the rewrite
//! (`comm::publish_event_cards`, `comm::tutorial_logs`); the other cards show
//! the campaign's own text.

use std::collections::HashMap;

use bevy::prelude::*;

use super::{
    collections::{CollectionsState, Tab},
    tutorial::{self as tut, flag, Chain, CollectionsAction},
    HudZoom, SpaceHudActive, SpaceHudRoot, SpaceHudState,
};
use crate::{
    asset_catalog::ResourceIndex,
    cell_contract::CellGoalCard,
    cell_hud::dialogs::{self, CalloutDef, HintDef},
    game::{Player, RunProgress},
    space::SpaceCampaign,
    hint_manager::{cond as shared, HintBar, HintMachine},
    space_view::SpaceView,
    spui::{
        model::{ImageRef, WinId},
        view::{SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

/// Goal cards shown at once (retail stacks the current goals).
pub const MAX_CARDS: usize = 3;

/// Control ids of `SPGMissionCard` (the same as the Cell's `CLGMissionCard`).
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

/// ADAPTED: where the first card's root sits and the gap between stacked cards.
const CARD_X: f32 = 5.0;
const CARD_Y: f32 = 15.0;
const CARD_GAP: f32 = 4.0;

/// The mission cards for the campaign, first card first. ADAPTED: the retail
/// mission manager is not in the rewrite; the tutorial chain (Flight 101,
/// Story 201, the teaching missions), an alien empire's contract and a
/// distress call are shown as cards with the campaign's own text (the
/// tutorial card's text is replaced by the retail one afterwards, see
/// `comm::retail_mission_card`). Reads
/// `SpaceCampaign::{missions_finished, tutorial, mission_name, next_action,
/// active_alien_mission, active_distress}`.
pub fn goal_cards(campaign: &SpaceCampaign) -> Vec<CellGoalCard> {
    let card = |kind: &str, title: String, body: String, progress: Option<(i32, i32)>| CellGoalCard {
        kind: kind.into(),
        progress,
        title,
        body,
    };
    let mut cards = Vec::new();
    if let Some(d) = &campaign.active_distress {
        let (title, what) = match d.kind {
            crate::space::DistressKind::PirateRaid => ("Pirate raid", "Pirates are attacking one of your colonies"),
            crate::space::DistressKind::BiohazardOutbreak => {
                ("Biohazard", "An outbreak threatens one of your colonies")
            }
        };
        cards.push(card(
            "Distress call",
            title.into(),
            format!("{what}. {:.0} seconds left.", d.time_remaining.max(0.0)),
            None,
        ));
    }
    if !campaign.missions_finished() {
        let total = campaign.tutorial.step.target().max(1);
        let done = campaign.tutorial.progress.min(total);
        cards.push(card(
            "Mission",
            campaign.mission_name().to_string(),
            campaign.next_action(),
            Some((done as i32, total as i32)),
        ));
    }
    if let Some(m) = &campaign.active_alien_mission {
        let empire = crate::space_galaxy::EMPIRES.get(m.empire as usize).map_or("Alien", |e| e.0);
        let limit = m.time_left.map(|t| format!(" {:.0} s left.", t.max(0.0))).unwrap_or_default();
        cards.push(card(
            "Contract",
            m.kind.title().to_string(),
            format!("{empire}: {}{limit}", m.description),
            Some((m.current_progress(campaign) as i32, m.target_count as i32)),
        ));
    }
    if cards.is_empty() {
        cards.push(card(
            "Goals",
            "Explore the galaxy".into(),
            "Explore new worlds, terraform them and expand your empire.".into(),
            None,
        ));
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

/// Bottom edge of a card's panel on screen.
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
    // One 9 pt headline line is about 14 px high; the description wraps freely.
    let head_h = height(ui, head).max(14.0);
    let desc_h = height(ui, desc);
    // The authored headline sits 11 px below the panel top in this layout.
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

/// How long a feedback message stays on the hint bar, seconds.
pub const MESSAGE_SECONDS: f32 = 5.0;
/// Longest hint text shown (the bar is one line: the galaxy caption is cut).
pub const MAX_HINT_CHARS: usize = 150;

/// Every Space hint the guide runs (`HintsSPG`): the manager's list. The chain
/// (`spg-fly`, `-altitude`, `-zoomsolar`, `-zoomgalaxy`) is `ShowHint` /
/// `HideHint` by the chain (0x1065D30); `spg-opencommfirsttime`, `-idle`,
/// `-solarmove` and `-startravel` are conditional hints answered by
/// [`Conditions`]. NOT driven (their conditions need objects the rewrite does
/// not have): `spg-gasgiant` (a `ShowHint` at 0x1009E32 when a gas giant is
/// entered), `spg-flight101-abduct` / `-dropoff` / `spg-opencommfirstmissioncomplete`
/// (the Flight 101 report-back) and `spg-terraformmission-missingfauna` / `-flora`.
pub const ALL_HINTS: [&str; 8] = [
    tut::HINT_FLY,
    tut::HINT_ALTITUDE,
    tut::HINT_ZOOM_SOLAR,
    tut::HINT_ZOOM_GALAXY,
    "spg-opencommfirsttime",
    "spg-idle",
    "spg-solarmove",
    "spg-startravel",
];

/// Condition names the Space branch of the checker answers (0xE08BB2..0xE0948F,
/// VERIFIED); the generic ones (`spg`, `UserBusy`) are in
/// [`crate::hint_manager::cond`]. The flags are profile flags (the "once" bits
/// of `tutorial::flag`).
#[allow(dead_code)]
pub mod cond {
    /// `GetCurrentContext` (0x1020F30) is 1 / 2 / 0 (0xE08C85, 0xE08C9F, 0xE08CBD).
    pub const SOLAR_VIEW: u32 = 0xD6FF_EF0B;
    pub const GALAXY_VIEW: u32 = 0xAEB1_D92E;
    pub const PLANET_VIEW: u32 = 0x42E2_F55C;
    /// 0xE09377: the UFO's idle timer (0xFFBE90) passed the limit (`spg-idle`'s
    /// property 0x05A71AB3 = 15 s, read at 0x1000892).
    pub const USER_IDLE: u32 = 0x41EA_7A72;
    /// Flag 0x05C2D1AB, set by the move command (0xFFBEE0).
    pub const USER_HAS_MOVED: u32 = 0x081E_4B7D;
    /// Flag 0x056D1872, set when the comm screen opens (0xAECDDB).
    pub const HAS_EVER_OPENED_COMM: u32 = 0xB0C6_E178;
    /// 0xE09455: no cinematic is playing (0xAC8030 false).
    pub const NO_CINEMATIC: u32 = 0x2B62_0B6B;
    /// 0xE09479: the callout manager has nothing up (0x67CAB0).
    pub const NO_CALLOUT: u32 = 0x0A93_8A38;
    /// Flag 0x056E2B88.
    pub const SOLAR_TRAVELLED: u32 = 0x9EDF_5003;
    /// Flag 0x0574AC67 (the solar system view went to a planet).
    pub const HAS_TRANSITIONED_SOLAR_TO_PLANET: u32 = 0x383E_4D29;
    /// 0xE091AC: the active star is not the player empire's home star.
    pub const AWAY_FROM_HOME_STAR: u32 = 0x5D8F_3346;
    /// 0xE09400: some mission of the manager's list is of the class 0x0422227D
    /// with the flag at `+0x23C` (unnamed in the registry).
    pub const MISSION_FLAG: u32 = 0xA44A_CC08;
}

/// What the Space branch of the checker answers (the inputs the guide reads
/// each frame).
#[derive(Clone, Copy, Debug)]
pub struct Conditions {
    pub zoom: HudZoom,
    pub busy: bool,
    pub idle: bool,
    pub moved: bool,
    pub opened_comm: bool,
    pub no_cinematic: bool,
    pub no_callout: bool,
    pub solar_travelled: bool,
    pub solar_to_planet: bool,
    pub away_from_home: bool,
    pub mission_flag: bool,
}

impl Default for Conditions {
    fn default() -> Self {
        Self {
            zoom: HudZoom::System,
            busy: false,
            idle: false,
            moved: false,
            opened_comm: false,
            no_cinematic: true,
            no_callout: true,
            solar_travelled: false,
            solar_to_planet: false,
            away_from_home: false,
            mission_flag: false,
        }
    }
}

impl Conditions {
    pub fn eval(&self, name: u32) -> bool {
        match name {
            shared::SPG => true,
            cond::SOLAR_VIEW => self.zoom == HudZoom::System,
            cond::GALAXY_VIEW => self.zoom == HudZoom::Galaxy,
            cond::PLANET_VIEW => self.zoom == HudZoom::Planet,
            shared::USER_BUSY => self.busy,
            cond::USER_IDLE => self.idle,
            cond::USER_HAS_MOVED => self.moved,
            cond::HAS_EVER_OPENED_COMM => self.opened_comm,
            cond::NO_CINEMATIC => self.no_cinematic,
            cond::NO_CALLOUT => self.no_callout,
            cond::SOLAR_TRAVELLED => self.solar_travelled,
            cond::HAS_TRANSITIONED_SOLAR_TO_PLANET => self.solar_to_planet,
            cond::AWAY_FROM_HOME_STAR => self.away_from_home,
            cond::MISSION_FLAG => self.mission_flag,
            // The Flight 101 / terraform mission conditions and every name this
            // mode does not know (their hints are not driven, see [`ALL_HINTS`]).
            _ => false,
        }
    }
}

/// What the hint bar's text selection looks at.
#[derive(Clone, Debug, Default)]
pub struct HintContext<'a> {
    pub zoom: HudZoom,
    /// The hint the manager has on the bar (`HintMachine::visible`).
    pub manager: Option<&'a str>,
    pub message: &'a str,
    /// Seconds since the feedback message last changed.
    pub message_age: f32,
    pub callout_open: bool,
    pub caption: Option<&'a str>,
}

/// Cut a hint to the length the one-line bar can show.
pub fn clip_hint(text: &str) -> String {
    if text.chars().count() <= MAX_HINT_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(MAX_HINT_CHARS - 1).collect();
    out.push('\u{2026}');
    out
}

/// What the bar shows: a retail hint id (`spg-fly`) or `msg:<text>`. The
/// manager's hint is the retail one; ADAPTED: the campaign's feedback lines
/// (which the legacy HUD printed at the bottom) use the bar too and come first,
/// and the galaxy view's star caption fills it when the manager has nothing.
pub fn pick_hint(c: &HintContext) -> Option<String> {
    if c.callout_open {
        return None;
    }
    if !c.message.is_empty() && c.message_age < MESSAGE_SECONDS {
        return Some(format!("msg:{}", clip_hint(c.message)));
    }
    if let Some(id) = c.manager {
        return Some(id.into());
    }
    if c.zoom == HudZoom::Galaxy {
        return c.caption.map(|caption| format!("msg:{}", clip_hint(caption)));
    }
    None
}

/// Every callout this module can queue (checked against the install by a test).
#[allow(dead_code)]
pub const ALL_CALLOUTS: [&str; 18] = [
    tut::OPENING[0],
    tut::OPENING[1],
    tut::OPENING[2],
    tut::FIRST_GOALS,
    tut::MAIN_TOOLS_BIN,
    tut::CARGO_BIN,
    tut::WEAPONS_BIN,
    tut::MY_COLLECTION,
    tut::ACTIVE_TOOL,
    tut::HEALTH_ENERGY,
    tut::TOOL_UI,
    tut::BADGE_UI,
    tut::STAR_MAPS[0],
    tut::STAR_MAPS[1],
    tut::LOW_ENERGY,
    tut::PLAYER_DAMAGE,
    tut::PURCHASE_READY,
    tut::CAPTURE_PLANET,
];
/// The window a callout arrow points at. A few retail callouts name windows
/// of the tool bins that the palette's button layouts carry (`05F00660`, a
/// tool button) or of the mission log (`0577A9B0`): they point at the palette
/// and the first mission card instead (ADAPTED).
pub fn callout_target(control: u32) -> u32 {
    match control {
        0x05F0_0660 => super::palette::pid::PALETTE,
        0x0577_A9B0 => card::PANEL,
        other => other,
    }
}

/// ADAPTED: seconds after the HUD appears before the introduction starts.
const CALLOUT_DELAY: f32 = 1.5;
/// `userIdle` (0xFFBE90): `spg-idle`'s property 0x05A71AB3 is 15 (read at
/// 0x1000892, seconds, scaled to milliseconds by 0x1000D8).
const IDLE_SECONDS: f32 = 15.0;

/// A fresh Space game: nothing scanned, nothing owned but the home world, no
/// badge points (the introduction is for the first visit only).
pub fn is_fresh(campaign: &SpaceCampaign) -> bool {
    campaign.surveyed.iter().all(|id| *id == 0)
        && campaign.new_colony_count() == 0
        && campaign.visited.len() <= 1
        && campaign.claimed == 0
        && campaign.kills == 0
}

/// Flags of the capture demo (`--space-hud-*`).
#[derive(Clone, Debug, Default)]
pub struct GuideFlags {
    /// No introduction callouts and no first-minute hints.
    pub quiet: bool,
    /// No introduction callouts (the hints stay).
    pub no_intro: bool,
    pub callout: Option<String>,
    pub hint: Option<String>,
    /// `--space-hud-step N`: start the visit on tutorial step N (the index in
    /// `TutorialStep::ORDER`; for captures of the mission cards).
    pub step: Option<crate::space_rules::TutorialStep>,
}

impl GuideFlags {
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned());
        Self {
            quiet: args.iter().any(|a| a == "--space-hud-quiet"),
            no_intro: args.iter().any(|a| a == "--space-hud-nointro"),
            callout: value("--space-hud-callout"),
            hint: value("--space-hud-hint"),
            step: value("--space-hud-step")
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|i| crate::space_rules::TutorialStep::ORDER.get(i).copied()),
        }
    }
}

/// Timers and the callout queue of the guide: the Space tutorial manager's
/// state for this visit (`tutorial::Chain` and the "once" bits are saved with
/// the campaign, [`SpaceCampaign::tutorial`]).
#[derive(Resource, Default)]
pub struct GuideState {
    flags: Option<GuideFlags>,
    /// Seconds the HUD has been shown in this visit of the stage.
    pub elapsed: f32,
    message: String,
    message_age: f32,
    started: bool,
    /// The campaign's chain and bits were read this visit.
    loaded: bool,
    /// The forced hint chain (`SpaceGameUI +0x618`) and the "once" bits
    /// (`tutorial::flag`).
    pub chain: Chain,
    pub bits: u32,
    /// View of the last frame.
    zoom: Option<HudZoom>,
    /// Where the UFO was when the flight hint came up, when the visit began and
    /// when the solar system view was entered (the flight conditions measure
    /// from there).
    fly_base: Option<Vec3>,
    visit_base: Option<Vec3>,
    solar_base: Option<Vec3>,
    /// Seconds without input of the player (`userIdle`).
    idle: f32,
    /// The retail hint manager with the `spg-*` hints (loaded once per visit),
    /// the times it cleared the bar at once and the chain state its hints were
    /// last matched to.
    machine: Option<HintMachine>,
    hint_cuts: u32,
    chain_seen: Option<u8>,
    /// The planetary tool selected when the visit began (a change of it is
    /// "the player picked a tool").
    tool_base: Option<Option<&'static str>>,
    /// Callouts waiting for the HUD to be free (`SpaceGameUI +0x5EC`), the
    /// batch on screen and what is left of it.
    pending: Vec<String>,
    batch: Vec<String>,
    pub queue: Vec<String>,
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

    /// An event of the forced chain (0xFFBEE0, 0x101B69E, 0xFDE9EA, 0xFDA6E0):
    /// while the chain's hint is the one on the bar it is hidden at once
    /// (`HideHint(id, 1)`) and the chain advances (0x1065D30).
    fn chain_event(&mut self, id: &'static str, tutorials: bool) -> bool {
        if self.chain.hint() != Some(id) {
            return false;
        }
        if let Some(machine) = self.machine.as_mut() {
            if !machine.on_bar(id) {
                return false;
            }
            machine.complete(id, true);
        }
        self.chain.advance(tutorials);
        true
    }

    /// Take a "once" bit: true the first time.
    fn take(&mut self, bit: u32) -> bool {
        let first = self.bits & bit == 0;
        self.bits |= bit;
        first
    }
}

/// Loaded callout and hint definitions and the hint bar's fade.
#[derive(Resource, Default)]
pub struct GuideCache {
    callouts: HashMap<String, Option<CalloutDef>>,
    hints: HashMap<String, Option<HintDef>>,
    /// The retail hint fade (`cHintManager::Update` 0x67C410, VERIFIED: 1 s in,
    /// 2 s out; `cell_hud::dialogs::HintFade`).
    hint_bar: HintBar,
}

/// Fill the goal cards, hint and callouts of [`SpaceHudState`]. The callouts
/// and the forced hint chain follow the retail Space tutorial manager
/// (`tutorial.rs`): after `CVG2SPG` the arrival batch (`Opening`,
/// `ProgressBar`, `CommButton`, then the first mission's `FirstGoals`), whose
/// dismissal starts the chain; the situation callouts once each; every
/// callout is dropped with Tutorials off (0x67A550).
#[allow(clippy::too_many_arguments)]
pub fn publish_guide(
    time: Res<Time>,
    stage: Res<State<GameStage>>,
    active: Res<SpaceHudActive>,
    mut run: ResMut<RunProgress>,
    keys: Res<ButtonInput<KeyCode>>,
    player: Query<&Transform, With<Player>>,
    view: Option<Res<SpaceView>>,
    cinematics: Option<Res<crate::cinematics::Cinematics>>,
    originals: Option<Res<crate::Originals>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    motion: Option<Res<bevy::input::mouse::AccumulatedMouseMotion>>,
    mut collections: ResMut<CollectionsState>,
    mut guide: ResMut<GuideState>,
    mut hud: ResMut<SpaceHudState>,
) {
    if *stage.get() != GameStage::Space {
        // The visit is over: the next one reads the campaign's state again.
        guide.reset();
        return;
    }
    if !active.0 {
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
    if let Some(step) = flags.step.filter(|_| !guide.loaded) {
        run.space_campaign.tutorial.step = step;
    }
    let tutorials = crate::front_end::settings::tutorials_enabled();
    let step = run.space_campaign.tutorial.step;
    let fresh = is_fresh(&run.space_campaign);
    if !guide.loaded {
        guide.loaded = true;
        let saved = run.space_campaign.tutorial;
        guide.chain = Chain::new(saved.forced);
        guide.bits = saved.ui_flags;
        if guide.bits == 0 && guide.chain.state == 0 && !fresh {
            // ADAPTED: a campaign that was already under way when the manager
            // was added has had its first minutes: no arrival batch, no chain.
            guide.chain = Chain::new(tut::DONE);
            guide.bits = flag::INTRO
                | flag::MY_COLLECTION
                | flag::STORY_201
                | flag::BADGE_UI
                | flag::TOOL_UI
                | flag::STEP_SCAN
                | flag::STEP_FETCH
                | flag::STEP_ERADICATE
                | flag::MOVED
                | flag::SOLAR_TRAVELLED
                | flag::SOLAR_TO_PLANET;
        } else if !fresh {
            // A campaign that is under way has flown (the flags below came later).
            guide.bits |= flag::MOVED;
        }
        // The retail hint manager with the `spg-*` hints (loaded once per visit).
        if guide.machine.is_none() {
            if let Some(originals) = originals.as_deref().filter(|o| !o.resource_index.records.is_empty()) {
                guide.machine = Some(HintMachine::load(&originals.resource_index, &ALL_HINTS));
            }
        }
    }
    let pos = player.single().ok().map(|t| t.translation);
    if guide.message != hud.message {
        guide.message = hud.message.clone();
        guide.message_age = 0.0;
    } else {
        guide.message_age += dt;
    }
    // The views entered this frame (retail: the switch functions 0xFDE3D0 and
    // 0xFDA5D0 call the chain), not the one the visit started in.
    let entered = |z: HudZoom| guide.zoom.is_some() && guide.zoom != Some(z) && hud.zoom == z;
    let (entered_solar, entered_galaxy) = (entered(HudZoom::System), entered(HudZoom::Galaxy));
    if guide.zoom == Some(HudZoom::System) && hud.zoom == HudZoom::Planet {
        // `HasTransitionedSolarToPlanet` (0x0574AC67).
        guide.bits |= flag::SOLAR_TO_PLANET;
    }
    guide.zoom = Some(hud.zoom);
    if hud.comm_open {
        // `HasEverOpenedComm` (0x056D1872, 0xAECDDB).
        guide.bits |= flag::COMM_OPENED;
    }
    let playing = cinematics.as_deref().is_some_and(|c| c.is_playing());
    let quiet = flags.quiet;

    // Callouts: the arrival batch of 0x100C622 (once), shown as one batch.
    if !guide.started && guide.elapsed >= CALLOUT_DELAY {
        guide.started = true;
        if let Some(name) = &flags.callout {
            guide.queue = vec![name.clone()];
        } else if fresh && !quiet && !flags.no_intro && guide.take(flag::INTRO) {
            // ADAPTED: `FirstGoals` is queued by the first Flight 101 step
            // when it starts (0xC55BD0, step class INFERRED); it follows the
            // arrival callouts here.
            guide.pending.extend(tut::OPENING.iter().map(|n| n.to_string()));
            guide.pending.push(tut::FIRST_GOALS.to_string());
        }
    }
    if !tutorials && flags.callout.is_none() {
        // The callout manager drops everything and the chain ends (0x67A550,
        // 0x1065D46).
        guide.pending.clear();
        guide.queue.clear();
        guide.batch.clear();
        guide.chain.advance(false);
    }
    if guide.queue.is_empty() && !guide.batch.is_empty() {
        // The batch was dismissed: its last callout's listener resumes the
        // game and, with `FirstGoals` or `MyCollection` in it, advances the
        // chain (0x10687B0).
        let batch = std::mem::take(&mut guide.batch);
        guide.chain.on_batch_end(&batch, tutorials);
    }
    // The HUD releases the queue when the comm screen and My Collections are
    // closed (0x107496B), as one batch (0x10749A4).
    if guide.queue.is_empty() && !guide.pending.is_empty() && !hud.comm_open && !collections.open && !playing {
        guide.batch = std::mem::take(&mut guide.pending);
        guide.queue = guide.batch.clone();
    }

    // The chain's events.
    if !guide.chain.is_done() {
        match guide.chain.state {
            1 => {
                // 0xFFBEE0: the player told the UFO where to fly.
                if let Some(pos) = pos {
                    let base = *guide.fly_base.get_or_insert(pos);
                    if pos.distance(base) > tut::FLY_DONE {
                        guide.chain_event(tut::HINT_FLY, tutorials);
                    }
                }
            }
            2 => {
                // 0x101B69E: inside the planet camera's zoom (0x101B520) the
                // zoom input (the wheel's velocity, +-0.5 for the +/- keys,
                // as `space_view::zoom_input` feeds it) past 0.1 completes
                // the altitude hint.
                if let Some(view) = view.as_deref().filter(|v| v.planet.is_some()) {
                    let triggers = crate::space_controls::read_triggers(&keys);
                    let key = triggers.zoom_in() as i32 - triggers.zoom_out() as i32;
                    let input = if key != 0 { 0.5 * key as f32 } else { view.wheel.v };
                    if input.abs() > tut::ALTITUDE_INPUT {
                        guide.chain_event(tut::HINT_ALTITUDE, tutorials);
                    }
                }
            }
            _ => {}
        }
        if guide.chain.state != 1 {
            guide.fly_base = None;
        }
        if entered_solar {
            guide.chain_event(tut::HINT_ZOOM_SOLAR, tutorials);
        }
        if entered_galaxy {
            guide.chain_event(tut::HINT_ZOOM_GALAXY, tutorials);
        }
    }
    // `SPG_FlightSchool101Complete` ended: the `MyCollection` callout
    // (0x100C69C).
    if tut::flight_school_finished(step)
        && !playing
        && run.space_campaign.pending_cinematic.is_none()
        && guide.take(flag::MY_COLLECTION)
        && tutorials
        && !quiet
    {
        guide.pending.push(tut::MY_COLLECTION.to_string());
    }
    // Story 201 finished: `Advance` whatever the state (0xC60423).
    if tut::mission_201_finished(step) && guide.take(flag::STORY_201) {
        guide.chain.on_story_201_done(tutorials);
    }
    // State 4: the player is taken to the Badges page of My Collections
    // (0x1074B50).
    match guide.chain.collections(guide.bits & flag::BADGE_UI != 0, collections.open) {
        CollectionsAction::OpenBadges if !guide.queue.is_empty() || !guide.pending.is_empty() => {}
        CollectionsAction::OpenBadges => {
            collections.open = true;
            collections.tab = Tab::Badges;
            collections.page = 0;
        }
        CollectionsAction::Advance => guide.chain.advance(tutorials),
        CollectionsAction::None => {}
    }
    // The pages of My Collections show their callout the first time they open
    // (0x1064645, 0x107BBEB), not through the queue.
    if collections.open && !quiet {
        let page = match collections.tab {
            Tab::Badges => Some((flag::BADGE_UI, tut::BADGE_UI)),
            Tab::Tools => Some((flag::TOOL_UI, tut::TOOL_UI)),
            _ => None,
        };
        if let Some((bit, name)) = page {
            if guide.take(bit) && tutorials {
                guide.queue.push(name.to_string());
            }
        }
    }
    // Situation callouts, each once (the flags of the retail code).
    if guide.started && !quiet && flags.callout.is_none() {
        let story_done = tut::mission_201_finished(step);
        // 0xC55BD0: the tool bin of the Flight 101 step that just started.
        if let Some((bit, name)) = tut::step_callout(step) {
            if guide.take(bit) {
                guide.pending.push(name.to_string());
            }
        }
        // 0xFDAC14: the star map after Story 201.
        if hud.zoom == HudZoom::Galaxy && story_done && guide.take(flag::STAR_MAP) {
            guide.pending.extend(tut::STAR_MAPS.iter().map(|n| n.to_string()));
        }
        // 0xC38BFE: energy at or under 20 % of its maximum.
        if hud.max_energy > 0.0 && hud.energy_fraction() <= tut::WARNING_FRACTION && guide.take(flag::LOW_ENERGY) {
            guide.pending.push(tut::LOW_ENERGY.to_string());
        }
        // 0xC37420 / 0xC3DB63: health at or under 20 % once Story 201 is done.
        if story_done
            && hud.max_health > 0.0
            && hud.health_fraction() <= tut::WARNING_FRACTION
            && guide.take(flag::PLAYER_DAMAGE)
        {
            guide.pending.push(tut::PLAYER_DAMAGE.to_string());
        }
        // 0x1079448 / 0x1079492: the first tool selected, and the first one
        // that costs energy. ADAPTED: the player choosing another planetary
        // tool than the one the visit started with stands for both (the
        // palette's ability buttons press keys and have no selection).
        let selected = hud.tools.iter().find(|t| t.active).map(|t| t.def.id);
        let first_selected = *guide.tool_base.get_or_insert(selected);
        if selected != first_selected {
            if guide.take(flag::ACTIVE_TOOL) {
                guide.pending.push(tut::ACTIVE_TOOL.to_string());
            }
            if guide.take(flag::HEALTH_ENERGY) {
                guide.pending.push(tut::HEALTH_ENERGY.to_string());
            }
        }
        // 0xC8D43D (after a capture) and 0xFDAD9A (a star can be bought).
        // ADAPTED: the capture callout comes when the capture is offered.
        if let Some(capture) = &hud.capture {
            if capture.can_capture && guide.take(flag::CAPTURE_PLANET) {
                guide.pending.push(tut::CAPTURE_PLANET.to_string());
            } else if capture.can_purchase && guide.take(flag::PURCHASE_READY) {
                guide.pending.push(tut::PURCHASE_READY.to_string());
            }
        }
    }
    if !tutorials && flags.callout.is_none() {
        guide.pending.clear();
        guide.queue.clear();
    }
    if keys.just_pressed(crate::cell_flow::DISMISS_KEY) {
        guide.dismiss();
    }
    hud.callouts = guide.queue.clone();
    // Hints. The chain's hints are `ShowHint` where `Advance` shows them
    // (0x1065D30) and `HideHint` where the events complete them; with Tutorials
    // off the chain ends and takes its hints with it.
    let state = guide.chain.state;
    if guide.chain_seen != Some(state) {
        let seen = guide.chain_seen;
        if let Some(machine) = guide.machine.as_mut() {
            if !tutorials {
                for id in tut::CHAIN_HINTS {
                    machine.complete(id, true);
                }
            } else {
                match seen {
                    // A campaign resumed on a state with a hint shows it again.
                    None => {
                        if let Some(id) = Chain::new(state).hint() {
                            machine.show(id);
                        }
                    }
                    Some(before) => {
                        for left in before..state {
                            if let Some(id) = Chain::shows_on_leaving(left) {
                                machine.show(id);
                            }
                        }
                        if state >= tut::DONE {
                            for id in tut::CHAIN_HINTS {
                                machine.complete(id, true);
                            }
                        }
                    }
                }
            }
        }
        guide.chain_seen = Some(state);
    }
    // The inputs of the conditions. ADAPTED: `UserBusy` (a field of the Space
    // game object, 0xE08BC4) is "a full-screen window is open"; `userIdle` is
    // the player not touching keys, mouse or wheel for 15 s; `UserHasMoved` /
    // `SolarTravelled` are the UFO having flown (the retail flags are set by
    // the move command); the mission flag (0xA44ACC08) is "a tutorial mission
    // is active".
    let input = keys.get_pressed().next().is_some()
        || mouse.as_deref().is_some_and(|m| m.get_pressed().next().is_some())
        || motion.as_deref().is_some_and(|m| m.delta != Vec2::ZERO)
        || view.as_deref().is_some_and(|v| v.wheel.v != 0.0);
    guide.idle = if input { 0.0 } else { guide.idle + dt };
    if let Some(pos) = pos {
        let base = *guide.visit_base.get_or_insert(pos);
        if pos.distance(base) > tut::FLY_DONE {
            guide.bits |= flag::MOVED;
        }
        if hud.zoom == HudZoom::System {
            let base = *guide.solar_base.get_or_insert(pos);
            if pos.distance(base) > tut::FLY_DONE {
                guide.bits |= flag::SOLAR_TRAVELLED;
            }
        } else {
            guide.solar_base = None;
        }
    }
    let conditions = Conditions {
        zoom: hud.zoom,
        busy: hud.comm_open || collections.open,
        idle: guide.idle > IDLE_SECONDS,
        moved: guide.bits & flag::MOVED != 0,
        opened_comm: guide.bits & flag::COMM_OPENED != 0,
        no_cinematic: !playing,
        no_callout: hud.callouts.is_empty() && guide.pending.is_empty(),
        solar_travelled: guide.bits & flag::SOLAR_TRAVELLED != 0,
        solar_to_planet: guide.bits & flag::SOLAR_TO_PLANET != 0,
        away_from_home: run.space_campaign.current_system != crate::space_galaxy::HOME_STAR,
        mission_flag: !run.space_campaign.missions_finished(),
    };
    let hints = crate::front_end::settings::hints_enabled();
    let callout_open = !hud.callouts.is_empty();
    if !callout_open {
        // The game is held while a callout is up: the hints wait.
        if let Some(machine) = guide.machine.as_mut() {
            machine.update(dt, &|name| conditions.eval(name), hints);
        }
    }
    guide.hint_cuts = guide.machine.as_ref().map_or(0, HintMachine::cuts);
    let manager = guide.machine.as_ref().and_then(|m| m.visible()).map(str::to_string);
    let context = HintContext {
        zoom: hud.zoom,
        manager: manager.as_deref(),
        message: &guide.message,
        message_age: guide.message_age,
        callout_open,
        caption: hud.galaxy_caption.as_deref(),
    };
    hud.hint = match &flags.hint {
        Some(id) => Some(id.clone()),
        None if quiet => None,
        None => pick_hint(&context),
    };
    // Save the chain with the campaign (only when it moved).
    let (bits, state) = (guide.bits, guide.chain.state);
    let saved = run.space_campaign.tutorial;
    if saved.ui_flags != bits || saved.forced != state {
        run.space_campaign.tutorial.ui_flags = bits;
        run.space_campaign.tutorial.forced = state;
    }
    hud.goal_cards = goal_cards(&run.space_campaign);
}

/// Bind the mission cards, the hint bar and the callout dialog.
#[allow(clippy::too_many_arguments)]
pub fn bind_guide(
    time: Res<Time>,
    hud: Res<SpaceHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<GuideCache>,
    mut guide: ResMut<GuideState>,
    mut roots: Query<(&mut SpuiState, &SpaceHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    // Callout: definition and the rectangle of its target window.
    let callout_name = hud.callouts.first().cloned();
    if let Some(name) = &callout_name {
        if !cache.callouts.contains_key(name) {
            let def = dialogs::read_callout(index, &mut assets, name);
            cache.callouts.insert(name.clone(), def);
        }
    }
    let def = callout_name.as_ref().and_then(|n| cache.callouts.get(n)).cloned().flatten();
    // The page callouts of My Collections (`BadgeUI`, `ToolUI`, `RareUI`) name
    // 0x05F00660, the category column of the page (collections.rs): the first
    // category row stands for it.
    let column = matches!(callout_name.as_deref(), Some(tut::BADGE_UI | tut::TOOL_UI))
        .then(|| {
            roots.iter().find_map(|(ui, kind)| {
                if !ui.ready || *kind != SpaceHudRoot::Category(0) {
                    return None;
                }
                ui.find(super::collections::cid::ROW_ROOT).filter(|w| ui.visible(*w)).map(|w| ui.rect(w))
            })
        })
        .flatten();
    let target = column.or_else(|| {
        def.as_ref()
            .and_then(|d| d.targets.first())
            .and_then(|control| super::main_rect(&roots, callout_target(*control)))
    });
    // Hint: the retail fade (`HintFade`) towards the published hint.
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
            SpaceHudRoot::Card(n) => Some((*n, 0.0)),
            _ => None,
        })
        .collect();
    cards.sort_by_key(|(n, _)| *n);
    for (n, y) in &mut cards {
        *y = next_y;
        let height = roots.iter().find(|(_, kind)| **kind == SpaceHudRoot::Card(*n)).map_or(0.0, |(ui, _)| {
            if ui.ready && hud.goal_cards.get(*n as usize).is_some() {
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
            SpaceHudRoot::Card(n) => {
                match (hud.goal_cards.get(*n as usize).filter(|_| !hud.comm_open), cards.iter().find(|(c, _)| c == n)) {
                    (Some(goal), Some((_, y))) => bind_card(&mut ui, goal, *y, &mut assets, index),
                    _ => {
                        if let Some(root) = ui.find(card::ROOT) {
                            ui.set_visible(root, false);
                        }
                    }
                }
            }
            SpaceHudRoot::Callout => {
                dialogs::bind_callout(&mut ui, def.as_ref(), target, 0.5);
                for click in ui.take_clicks() {
                    if ui.layout.window(click).control_id == dialogs::callout::OK {
                        dismiss = true;
                    }
                }
            }
            // The hint bar (the message line over the HUD) does not draw over the
            // comm screen: ShowCommEvent (0xAEBE10) ends with 0xE14BF0(false), which
            // hides the HUD layouts of its list (INFERRED: the list's owner at
            // 0x167EB60 was not named), as the goal cards already are (above).
            SpaceHudRoot::Hint => {
                dialogs::bind_hint(&mut ui, hint_def.as_ref().filter(|_| !hud.comm_open), alpha, icon.clone())
            }
            _ => {}
        }
    }
    if dismiss {
        guide.dismiss();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hint_manager::HintRule;

    fn quiet() -> HintContext<'static> {
        HintContext { zoom: HudZoom::System, ..default() }
    }

    #[test]
    fn a_fresh_campaign_shows_the_first_mission_as_a_card_with_its_count() {
        let campaign = SpaceCampaign::default();
        let cards = goal_cards(&campaign);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].kind, "Mission");
        assert_eq!(cards[0].title, campaign.mission_name());
        assert_eq!(cards[0].body, campaign.next_action());
        // Flight 101 starts with the first visit still to do.
        assert_eq!(cards[0].progress, Some((0, campaign.tutorial.step.target() as i32)));
    }

    #[test]
    fn cards_follow_the_campaign_progress_and_stack_contracts_and_distress_calls() {
        let mut campaign = SpaceCampaign::default();
        campaign.tutorial.step = crate::space_rules::TutorialStep::Flight101EradicateAnimals;
        campaign.tutorial.progress = 2;
        let total = campaign.tutorial.step.target() as i32;
        assert!(total > 2);
        assert_eq!(goal_cards(&campaign)[0].progress, Some((2, total)));
        campaign.active_alien_mission = Some(crate::space::AlienMission {
            empire: 2,
            kind: crate::space::AlienMissionKind::PirateHunt,
            reward_spice: 100.0,
            progress: 1,
            target_count: 3,
            target_world: None,
            description: "Hunt pirates".into(),
            retail: None,
            time_left: Some(90.0),
            ..Default::default()
        });
        let cards = goal_cards(&campaign);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[1].kind, "Contract");
        assert_eq!(cards[1].progress, Some((1, 3)));
        assert!(cards[1].body.contains("90 s left"), "{}", cards[1].body);
        campaign.active_distress = Some(crate::space::DistressSignal {
            world_id: 1,
            system_id: 0,
            kind: crate::space::DistressKind::PirateRaid,
            time_remaining: 42.0,
        });
        let cards = goal_cards(&campaign);
        assert_eq!(cards.len(), MAX_CARDS);
        assert_eq!(cards[0].kind, "Distress call", "an emergency comes first");
        // All missions claimed and nothing else: the open goal.
        campaign.active_distress = None;
        campaign.active_alien_mission = None;
        campaign.tutorial.step = crate::space_rules::TutorialStep::Done;
        assert_eq!(goal_cards(&campaign)[0].kind, "Goals");
    }

    #[test]
    fn only_a_fresh_game_gets_the_introduction() {
        let mut campaign = SpaceCampaign::default();
        assert!(is_fresh(&campaign));
        campaign.surveyed.push(0);
        assert!(is_fresh(&campaign), "the home world does not count");
        campaign.surveyed.push(4);
        assert!(!is_fresh(&campaign));
        let mut campaign = SpaceCampaign::default();
        campaign.kills = 1;
        assert!(!is_fresh(&campaign));
    }

    #[test]
    fn messages_beat_everything_and_the_galaxy_shows_its_caption() {
        let mut c = quiet();
        assert_eq!(pick_hint(&c), None);
        c.message = "Need 100 spice";
        assert_eq!(pick_hint(&c).as_deref(), Some("msg:Need 100 spice"));
        c.message_age = MESSAGE_SECONDS + 1.0;
        assert_eq!(pick_hint(&c), None);
        c.zoom = HudZoom::Galaxy;
        c.caption = Some("Kalo - Yellow star");
        assert_eq!(pick_hint(&c).as_deref(), Some("msg:Kalo - Yellow star"));
        c.callout_open = true;
        assert_eq!(pick_hint(&c), None, "no hints under a callout");
    }

    #[test]
    fn the_managers_hint_shows_before_the_caption_and_a_callout_hides_it() {
        let mut c = quiet();
        assert_eq!(pick_hint(&c), None, "no hint on the manager's bar, no hint");
        c.manager = Some(tut::HINT_FLY);
        assert_eq!(pick_hint(&c).as_deref(), Some("spg-fly"));
        c.message = "Need 100 spice";
        assert_eq!(pick_hint(&c).as_deref(), Some("msg:Need 100 spice"), "feedback lines come first");
        c.message_age = MESSAGE_SECONDS + 1.0;
        assert_eq!(pick_hint(&c).as_deref(), Some("spg-fly"));
        c.zoom = HudZoom::Galaxy;
        c.caption = Some("Kalo - Yellow star");
        assert_eq!(pick_hint(&c).as_deref(), Some("spg-fly"), "the retail hint beats the caption");
        c.callout_open = true;
        assert_eq!(pick_hint(&c), None);
    }

    fn chain_machine() -> HintMachine {
        // The chain hints as installed: Triggered, no condition, no display time, `always`.
        HintMachine::new(
            tut::CHAIN_HINTS.iter().map(|id| HintRule { priority: 1, always: true, ..HintRule::new(id) }).collect(),
        )
    }

    fn run(m: &mut HintMachine, seconds: f32, c: Conditions) {
        for _ in 0..(seconds * 10.0).round() as u32 {
            m.update(0.1, &|name| c.eval(name), true);
        }
    }

    #[test]
    fn a_chain_event_only_counts_while_its_hint_is_on_the_bar() {
        // 0xFFBEE0: `if current->id == spg-fly { HideHint(spg-fly, 1); Advance }`.
        let mut g = GuideState { machine: Some(chain_machine()), chain: Chain::new(1), ..default() };
        // The hint was asked for but is not on the bar yet (a candidate, state 3).
        g.machine.as_mut().unwrap().show(tut::HINT_FLY);
        assert!(!g.chain_event(tut::HINT_FLY, true), "not on the bar: the flight does not count");
        assert_eq!(g.chain.state, 1);
        run(g.machine.as_mut().unwrap(), 0.2, Conditions::default());
        let cuts = g.machine.as_ref().unwrap().cuts();
        assert!(g.chain_event(tut::HINT_FLY, true));
        assert_eq!(g.chain.state, 2);
        let m = g.machine.as_ref().unwrap();
        assert_eq!((m.visible(), m.state(tut::HINT_FLY), m.cuts()), (None, 0, cuts + 1), "hidden at once, for good");
        // The wrong hint for the state never counts.
        assert!(!g.chain_event(tut::HINT_ZOOM_SOLAR, true));
        // Without a manager (no installed data) the chain still advances.
        let mut bare = GuideState { chain: Chain::new(1), ..default() };
        assert!(bare.chain_event(tut::HINT_FLY, true));
        assert_eq!(bare.chain.state, 2);
    }

    #[test]
    fn advance_shows_the_hints_where_the_retail_chain_does() {
        // 0x1065D65 / D82 / DB7 / DE0: the states 0, 1, 4 and 6 show a hint.
        let shown: Vec<_> = (0..tut::DONE).filter_map(Chain::shows_on_leaving).collect();
        assert_eq!(shown, [tut::HINT_FLY, tut::HINT_ALTITUDE, tut::HINT_ZOOM_SOLAR, tut::HINT_ZOOM_GALAXY]);
        // Each hint stays on the state that leaves it: the ones `Chain::hint` names.
        for (leaving, id) in [(0, tut::HINT_FLY), (1, tut::HINT_ALTITUDE), (4, tut::HINT_ZOOM_SOLAR), (6, tut::HINT_ZOOM_GALAXY)] {
            assert_eq!(Chain::shows_on_leaving(leaving), Some(id));
            assert_eq!(Chain::new(leaving + 1).hint(), Some(id));
        }
    }

    fn opencomm() -> HintRule {
        // `spg-opencommfirsttime`: A [spg, PlanetView, NoCinematic, NoCallout], B [UserBusy,
        // HasEverOpenedComm], C [HasEverOpenedComm], 20 s + 10 s, shown 30 s, Normal.
        HintRule {
            all_of: vec![shared::SPG, cond::PLANET_VIEW, cond::NO_CINEMATIC, cond::NO_CALLOUT],
            none_of: vec![shared::USER_BUSY, cond::HAS_EVER_OPENED_COMM],
            until: vec![cond::HAS_EVER_OPENED_COMM],
            first_delay: 20.0,
            second_delay: 10.0,
            display: 30.0,
            priority: 1,
            kind: crate::hint_manager::KIND_NORMAL,
            ..HintRule::new("spg-opencommfirsttime")
        }
    }

    #[test]
    fn the_comm_hint_comes_after_half_a_minute_in_the_planet_view_until_comm_was_opened() {
        let planet = Conditions { zoom: HudZoom::Planet, ..default() };
        let mut m = HintMachine::new(vec![opencomm()]);
        run(&mut m, 29.0, planet);
        assert_eq!(m.visible(), None, "20 s + 10 s of waiting first");
        run(&mut m, 2.0, planet);
        assert_eq!(m.visible(), Some("spg-opencommfirsttime"));
        // Opening the comm screen ends it for good (condition C); the hint stays
        // away from the solar view and under a callout.
        run(&mut m, 1.5, Conditions { opened_comm: true, ..planet });
        assert_eq!(m.state("spg-opencommfirsttime"), 6);
        let mut elsewhere = HintMachine::new(vec![opencomm()]);
        run(&mut elsewhere, 100.0, default());
        assert_eq!(elsewhere.visible(), None);
        let mut covered = HintMachine::new(vec![opencomm()]);
        run(&mut covered, 100.0, Conditions { no_callout: false, ..planet });
        assert_eq!(covered.visible(), None);
        let mut busy = HintMachine::new(vec![opencomm()]);
        run(&mut busy, 100.0, Conditions { busy: true, ..planet });
        assert_eq!(busy.visible(), None);
    }

    #[test]
    fn the_idle_hint_beats_a_chain_hint_for_the_bar() {
        // `spg-idle`: A [spg, userIdle, PlanetView, A44ACC08], B [UserHasMoved], C
        // [UserHasMoved], 1 s + 1 s, shown 5 s, Oneshot, priority 50.
        let idle = HintRule {
            all_of: vec![shared::SPG, cond::USER_IDLE, cond::PLANET_VIEW, cond::MISSION_FLAG],
            none_of: vec![cond::USER_HAS_MOVED],
            until: vec![cond::USER_HAS_MOVED],
            first_delay: 1.0,
            second_delay: 1.0,
            display: 5.0,
            priority: 50,
            kind: crate::hint_manager::KIND_ONESHOT,
            ..HintRule::new("spg-idle")
        };
        let mut rules = vec![HintRule { priority: 1, always: true, ..HintRule::new(tut::HINT_FLY) }];
        rules.push(idle);
        let mut m = HintMachine::new(rules);
        m.show(tut::HINT_FLY);
        let waiting = Conditions { zoom: HudZoom::Planet, mission_flag: true, ..default() };
        run(&mut m, 1.0, waiting);
        assert_eq!(m.visible(), Some(tut::HINT_FLY));
        // A hint that is not Triggered waits 20 s after the last promotion (0x41A00000).
        run(&mut m, 15.0, Conditions { idle: true, ..waiting });
        assert_eq!(m.visible(), Some(tut::HINT_FLY), "the 20 s gap is not over");
        run(&mut m, 6.0, Conditions { idle: true, ..waiting });
        assert_eq!(m.visible(), Some("spg-idle"), "priority 50 against 1");
        // Flying ends it (C); the fly hint it displaced was demoted to a candidate and returns.
        run(&mut m, 1.5, Conditions { moved: true, idle: true, ..waiting });
        assert_eq!(m.state("spg-idle"), 6);
        run(&mut m, 2.5, Conditions { moved: true, ..waiting });
        assert_eq!((m.state("spg-idle"), m.visible()), (0, Some(tut::HINT_FLY)));
    }

    #[test]
    fn the_solar_and_star_hints_wait_for_the_player_to_do_nothing() {
        // `spg-solarmove`: A [spg, SolarView], B [SolarTravelled, HasTransitionedSolarToPlanet,
        // UserBusy], C [SolarTravelled], 30 s + 5 s, shown 5 s, Oneshot.
        let rule = HintRule {
            all_of: vec![shared::SPG, cond::SOLAR_VIEW],
            none_of: vec![cond::SOLAR_TRAVELLED, cond::HAS_TRANSITIONED_SOLAR_TO_PLANET, shared::USER_BUSY],
            until: vec![cond::SOLAR_TRAVELLED],
            first_delay: 30.0,
            second_delay: 5.0,
            display: 5.0,
            priority: 1,
            kind: crate::hint_manager::KIND_ONESHOT,
            ..HintRule::new("spg-solarmove")
        };
        let mut m = HintMachine::new(vec![rule.clone()]);
        run(&mut m, 34.0, default());
        assert_eq!(m.visible(), None);
        run(&mut m, 3.0, default());
        assert_eq!(m.visible(), Some("spg-solarmove"));
        let mut gone = HintMachine::new(vec![rule]);
        run(&mut gone, 100.0, Conditions { solar_to_planet: true, ..default() });
        assert_eq!(gone.visible(), None, "the player already went to a planet");
    }

    #[test]
    fn the_checker_answers_the_space_conditions() {
        for (id, name) in [
            (cond::SOLAR_VIEW, "SolarView"),
            (cond::GALAXY_VIEW, "galaxyView"),
            (cond::PLANET_VIEW, "PlanetView"),
            (cond::USER_IDLE, "userIdle"),
            (cond::USER_HAS_MOVED, "UserHasMoved"),
            (cond::HAS_EVER_OPENED_COMM, "HasEverOpenedComm"),
            (cond::NO_CINEMATIC, "NoCinematic"),
            (cond::NO_CALLOUT, "NoCallout"),
            (cond::SOLAR_TRAVELLED, "SolarTravelled"),
            (cond::HAS_TRANSITIONED_SOLAR_TO_PLANET, "HasTransitionedSolarToPlanet"),
            (cond::AWAY_FROM_HOME_STAR, "AwayFromHomeStar"),
        ] {
            assert_eq!(id, crate::world_objects::name_id(name), "{name}");
        }
        let c = Conditions { zoom: HudZoom::Galaxy, idle: true, moved: true, ..default() };
        assert!(c.eval(shared::SPG) && c.eval(cond::GALAXY_VIEW) && !c.eval(cond::PLANET_VIEW) && !c.eval(cond::SOLAR_VIEW));
        assert!(c.eval(cond::USER_IDLE) && c.eval(cond::USER_HAS_MOVED) && !c.eval(cond::HAS_EVER_OPENED_COMM));
        assert!(c.eval(cond::NO_CINEMATIC) && c.eval(cond::NO_CALLOUT));
        // Only the Space mode answers: the other stages' names and the Flight 101
        // mission names (not driven) are false.
        assert!(!c.eval(shared::TRG) && !c.eval(shared::CVG) && !c.eval(0x4ADC_E397) && !c.eval(0xDF4B_F1F1));
    }

    #[test]
    fn long_captions_are_cut_for_the_one_line_bar() {
        assert_eq!(clip_hint("short"), "short");
        let long = "x".repeat(400);
        let clipped = clip_hint(&long);
        assert_eq!(clipped.chars().count(), MAX_HINT_CHARS);
        assert!(clipped.ends_with('\u{2026}'));
    }

    #[test]
    fn dismissing_pops_the_front_callout_and_the_bits_are_taken_once() {
        let mut guide = GuideState { queue: vec!["a".into(), "b".into()], ..default() };
        guide.dismiss();
        assert_eq!(guide.queue, ["b"]);
        guide.dismiss();
        guide.dismiss();
        assert!(guide.queue.is_empty());
        assert!(guide.take(flag::LOW_ENERGY));
        assert!(!guide.take(flag::LOW_ENERGY));
        assert_eq!(guide.bits, flag::LOW_ENERGY);
    }

    #[test]
    fn the_arrival_batch_is_the_retail_one_followed_by_the_first_goals() {
        assert_eq!(tut::OPENING, ["Callout_SPG_Opening", "Callout_SPG_ProgressBar", "Callout_SPG_CommButton"]);
        let mut batch: Vec<String> = tut::OPENING.iter().map(|n| n.to_string()).collect();
        assert!(!tut::batch_advances(&batch), "the arrival callouts alone have the plain listener");
        batch.push(tut::FIRST_GOALS.to_string());
        assert!(tut::batch_advances(&batch));
    }

    /// The whole retail chain through the real system: the arrival batch, the
    /// fly and altitude hints, Flight School's My Collections, the zoom hints.
    #[test]
    fn the_tutorial_manager_walks_the_retail_chain() {
        use crate::space_rules::TutorialStep;
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Space);
        app.update();
        app.update();
        app.world_mut().insert_resource(SpaceHudActive(true));
        // The installed `spg-*` hints are not needed: the chain hints as the files
        // declare them (Triggered, no condition, no display time, `always`).
        app.world_mut().resource_mut::<GuideState>().machine = Some(chain_machine());
        let step = |app: &mut App| {
            // The campaign's own feedback line would hide the hints.
            app.world_mut().resource_mut::<RunProgress>().space_campaign.message.clear();
            app.update();
        };
        let guide_state = |app: &App| {
            let g = app.world().resource::<GuideState>();
            (g.chain.state, g.queue.clone())
        };
        // 1.5 s into the visit the arrival batch comes up as one batch.
        app.world_mut().resource_mut::<GuideState>().elapsed = CALLOUT_DELAY;
        step(&mut app);
        let (state, queue) = guide_state(&app);
        let mut expected: Vec<String> = tut::OPENING.iter().map(|n| n.to_string()).collect();
        expected.push(tut::FIRST_GOALS.to_string());
        assert_eq!((state, queue), (0, expected), "the batch is Opening, ProgressBar, CommButton, FirstGoals");
        assert_eq!(app.world().resource::<SpaceHudState>().hint, None, "no hint under a callout");
        for _ in 0..4 {
            app.world_mut().resource_mut::<GuideState>().dismiss();
        }
        step(&mut app);
        {
            let g = app.world().resource::<GuideState>();
            assert_eq!(g.chain.state, 1, "the batch with FirstGoals starts the chain (batch {:?}, queue {:?}, pending {:?}, bits {:x})", g.batch, g.queue, g.pending, g.bits);
        }
        let hud = app.world().resource::<SpaceHudState>();
        assert_eq!(hud.hint.as_deref(), Some("spg-fly"));
        // Flying completes the flight hint, the vertical input the next.
        step(&mut app);
        let player = {
            let world = app.world_mut();
            world.query_filtered::<Entity, With<Player>>().single(world).unwrap()
        };
        app.world_mut().get_mut::<Transform>(player).unwrap().translation += Vec3::new(5.0, 0.0, 0.0);
        step(&mut app);
        assert_eq!(guide_state(&app).0, 2);
        assert_eq!(app.world().resource::<SpaceHudState>().hint.as_deref(), Some("spg-altitude"));
        let over_world = |app: &mut App, altitude: f32| {
            app.world_mut().resource_mut::<SpaceView>().planet =
                Some(crate::space_view::PlanetView::new(0, Vec3::ZERO, 100.0, Vec3::Y, Vec3::X, altitude));
        };
        // 0x101B69E: over a world a zoom input past 0.1 completes the hint; a
        // changed altitude without input does not.
        over_world(&mut app, 100.0);
        step(&mut app);
        assert_eq!(guide_state(&app).0, 2, "no zoom input yet");
        over_world(&mut app, 300.0);
        step(&mut app);
        assert_eq!(guide_state(&app).0, 2, "a changed altitude is not the input");
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyX);
        step(&mut app);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyX);
        assert_eq!(guide_state(&app).0, 3);
        app.world_mut().resource_mut::<SpaceView>().planet = None;
        assert_eq!(app.world().resource::<GuideState>().chain.hint(), None);
        // Flight School done: the MyCollection callout, then the Badges page.
        app.world_mut().resource_mut::<RunProgress>().space_campaign.tutorial.step = TutorialStep::Story201;
        app.world_mut().resource_mut::<RunProgress>().space_campaign.pending_cinematic = None;
        step(&mut app);
        step(&mut app);
        {
            let g = app.world().resource::<GuideState>();
            let run = app.world().resource::<RunProgress>();
            assert_eq!(
                (g.chain.state, g.queue.clone()),
                (3, vec![tut::MY_COLLECTION.to_string()]),
                "pending {:?} bits {:x} step {:?} cinematic {:?} comm {}",
                g.pending,
                g.bits,
                run.space_campaign.tutorial.step,
                run.space_campaign.pending_cinematic,
                app.world().resource::<SpaceHudState>().comm_open
            );
        }
        app.world_mut().resource_mut::<GuideState>().dismiss();
        step(&mut app);
        assert_eq!(guide_state(&app).0, 4);
        step(&mut app);
        {
            let c = app.world().resource::<CollectionsState>();
            assert!(c.open && c.tab == Tab::Badges, "state 4 opens My Collections on its Badges page");
        }
        assert_eq!(guide_state(&app).1, vec![tut::BADGE_UI.to_string()], "the page shows its callout");
        app.world_mut().resource_mut::<GuideState>().dismiss();
        step(&mut app);
        assert_eq!(guide_state(&app).0, 4, "the window is still open");
        app.world_mut().resource_mut::<CollectionsState>().open = false;
        step(&mut app);
        assert_eq!(guide_state(&app).0, 5);
        assert_eq!(app.world().resource::<SpaceHudState>().hint.as_deref(), Some("spg-zoomsolar"));
        // The galaxy view does not complete the solar hint; the system view does.
        app.world_mut().resource_mut::<SpaceView>().galaxy = true;
        step(&mut app);
        assert_eq!(guide_state(&app).0, 5);
        app.world_mut().resource_mut::<SpaceView>().galaxy = false;
        step(&mut app);
        assert_eq!(guide_state(&app).0, 6);
        // Story 201 done: the zoom-out hint; the galaxy view ends the chain.
        app.world_mut().resource_mut::<RunProgress>().space_campaign.tutorial.step = TutorialStep::TeachColonize;
        step(&mut app);
        assert_eq!(guide_state(&app).0, 7);
        assert_eq!(app.world().resource::<SpaceHudState>().hint.as_deref(), Some("spg-zoomgalaxy"));
        app.world_mut().resource_mut::<SpaceView>().galaxy = true;
        step(&mut app);
        assert_eq!(guide_state(&app).0, tut::DONE);
        let saved = app.world().resource::<RunProgress>().space_campaign.tutorial;
        assert_eq!(saved.forced, tut::DONE, "the chain is saved with the campaign");
        assert!(saved.ui_flags & flag::INTRO != 0 && saved.ui_flags & flag::BADGE_UI != 0);
    }
}
