//! Space endgame (helper `space-endgame`; RE in `notes\space-endgame-notes.md`):
//!
//! * The galactic core: zooming into the core star the first time (0x1009C90
//!   -> 0x1002DD0) heals the UFO, fills its energy and plays
//!   `SPG_CenterOfGalaxy`; the script's `sendMessage`s are answered here the
//!   way `cPlayer::HandleMessage` (0xC77ED0) answers them: the UFO is placed
//!   (0x06667038 / 0x06666683), the journey and return effects run and their
//!   end continues the script (0x05E902D3 -> 0x445F729), Steve's gift
//!   (0x06526395: the Staff of Life with 42 charges and the rare "Talked to
//!   Steve", once) and the return to the galaxy (0x06527EAF).
//! * Before Steve was met the drive's reach shrinks near the core
//!   (`space_rules::core_travel_range`, 0xFFC6A0).
//! * Badges: every level reached is kept (`EndgameState::earned`; the badge
//!   manager's `mBadgeMap` never takes one back), the counters of the
//!   code-driven families (flights, special stars, routes, purchases, money
//!   held) are kept here, and a new badge is announced (event 0x6DA7F9A3).
//!   Promotions play `SPG_MasterBadge` (`cinematics::moments`).
//! * The UFO's attitude in the solar view: it turns to its flight direction,
//!   banks into turns and noses into thrust (`spaceufo~` limits).

use bevy::ecs::message::MessageCursor;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::cinematics::{CinematicMessage, Cinematics};
use crate::game::{Player, RunProgress};
use crate::space::SpaceCampaign;
use crate::space_galaxy::{Galaxy, StarKind};
use crate::space_rules as rules;

/// The `sendMessage` ids of `SPG_CenterOfGalaxy` and what answers them
/// (cPlayer listener 0xC75380 / handler 0xC77ED0, VERIFIED).
pub mod msg {
    /// kMsgSteveStartUFO: the UFO to solar (500, 0, 0).
    pub const STEVE_START_UFO: u32 = 0x0666_7038;
    /// Starts the journey effect 0xC04ACC27, stops the quasar effects.
    pub const JOURNEY_START: u32 = 0x0652_4498;
    /// kMsgSteveMoveUFO: the UFO to solar (100, 0, 0).
    pub const STEVE_MOVE_UFO: u32 = 0x0666_6683;
    /// Steve's gift (once, flag 0x061F1454).
    pub const GIVE_TOOLS: u32 = 0x0652_6395;
    /// Starts the reverse-travel effect 0xAF4031AD.
    pub const RETURN_START: u32 = 0x0652_7231;
    /// Back to the galaxy view (0xFD9C50 + 0xFDA5D0, the zoom-out path).
    pub const TRANSITION: u32 = 0x0652_7EAF;
}

pub const CINEMATIC_CENTER_OF_GALAXY: &str = rules::CINEMATIC_CENTER_OF_GALAXY;
/// The journey and reverse-travel effects (VERIFIED ids, 0xC77ED0).
pub const JOURNEY_EFFECT: u32 = 0xC04A_CC27;
pub const RETURN_EFFECT: u32 = 0xAF40_31AD;
/// Retail solar positions (x) the two UFO messages set.
pub const STEVE_START_X: f32 = 500.0;
pub const STEVE_END_X: f32 = 100.0;
/// ADAPTED: `SPG_CenterOfGalaxy`'s absolute lengths (the core-framing camera
/// offsets, the UFO positions) in the solar scene. The core is drawn 30 units
/// wide for its retail solar radius 300 (`space_worldgen::StarKind::radius`),
/// so they are scaled by the same 0.1; the camera specs that frame the UFO
/// keep their retail lengths (`cinematics::host::start_camera`).
pub const CORE_CINEMATIC_SCALE: f32 = 0.1;
/// ADAPTED safety: the waits that the effects' end continues give up after
/// this long when the effect is not installed or never ends (the journey's
/// scrolling text runs 73 s).
pub const JOURNEY_SAFETY_SECS: f32 = 75.0;
pub const RETURN_SAFETY_SECS: f32 = 8.0;
/// `cinematics::host` lets these two `-forContinue` waits run this long on
/// its own fallback; this module continues them on the effects' end.
pub const CONTINUE_WAIT_SECS: f32 = 90.0;

/// Length multiplier of the running Space cinematic in the stage world
/// (`cinematics::host::stage_length`); 1 except while `SPG_CenterOfGalaxy`
/// plays.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SpaceCinematicScale(pub f32);

impl Default for SpaceCinematicScale {
    fn default() -> Self {
        Self(1.0)
    }
}

/// Saved endgame state (`SpaceCampaign::endgame`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EndgameState {
    /// Steve's gift was given (flag 0x061F1454): the core no longer limits
    /// the drive.
    pub met_steve: bool,
    /// The core was entered and the gift is still to come (the cinematic is
    /// on its way, or it gives the gift when it cannot play).
    pub steve_pending: bool,
    /// Badge levels reached per family (`space_rules::BADGE_FAMILIES` ids).
    pub earned: BTreeMap<String, u8>,
    /// Single badges earned: `captain`, `grobally`, `GrobDefeat`, `joker-1`.
    pub specials: Vec<String>,
    /// `FlightsMade`: interstellar jumps.
    pub flights_made: u32,
    /// `ToolsPurchased`: items bought at stores (each unit).
    pub tools_purchased: u32,
    /// `AestheticTools`: colour / sculpt tool uses (OpenSpore has none yet).
    pub aesthetic_tools: u32,
    /// `Sightseer`: special stars visited (galaxy indices).
    pub sightseer_stars: Vec<u32>,
    /// Galactic-object rares found (1..=10, `rare-galacticobject<n>`).
    pub galactic_objects: Vec<u8>,
    /// `SystemsPurchased` and the worlds they gave (kept apart from conquest).
    pub systems_purchased: u32,
    pub purchased_worlds: u32,
    /// `TradeRoutes`: worlds a route was ever opened to.
    pub route_worlds: Vec<u32>,
    /// `ReachedCash`: the most Sporebucks held.
    pub peak_cash: f64,
    /// The system the counters last saw (a change is a flight).
    pub last_system: Option<u32>,
}

impl EndgameState {
    pub fn level(&self, family: &str) -> u8 {
        self.earned.get(family).copied().unwrap_or(0)
    }

    pub fn has_special(&self, id: &str) -> bool {
        self.specials.iter().any(|s| s == id)
    }

    fn add_special(&mut self, id: &str) -> bool {
        if self.has_special(id) {
            return false;
        }
        self.specials.push(id.to_string());
        true
    }

    fn find_rare(&mut self, n: u8) -> bool {
        if self.galactic_objects.contains(&n) {
            return false;
        }
        self.galactic_objects.push(n);
        true
    }
}

/// The galactic-object rare of a special star (0xFDD590: core 1, black hole
/// 2, proto-planetary disk 3, binaries OO / OM / OG / GG / GM / MM 4..9).
pub fn galactic_object(kind: StarKind) -> Option<u8> {
    match kind.retail_type() {
        t @ 1..=3 => Some(t),
        t @ 7..=12 => Some(t - 3),
        _ => None,
    }
}

/// Distance of a star from the core in galaxy units (the layout is centred on
/// the core).
pub fn core_distance(galaxy: &Galaxy, star: u32) -> f32 {
    let core = galaxy.star(galaxy.core).map_or(Vec2::ZERO, |s| s.position);
    galaxy.star(star).map_or(f32::INFINITY, |s| s.position.distance(core) / rules::MAP_UNITS_PER_GALAXY_UNIT)
}

/// The interstellar drive's reach from `from` (0xFFC6A0): the core limits it
/// until Steve was met.
pub fn jump_reach(campaign: &SpaceCampaign, galaxy: &Galaxy, from: u32) -> f32 {
    let range = campaign.max_jump_distance() / rules::MAP_UNITS_PER_GALAXY_UNIT;
    rules::core_travel_range(range, core_distance(galaxy, from), campaign.endgame.met_steve)
        * rules::MAP_UNITS_PER_GALAXY_UNIT
}

/// 0xFFDA50: the reach at the current star is below the drive's own range
/// (the `SPG_TravelNearCore` hint).
pub fn core_limits_reach(campaign: &SpaceCampaign, galaxy: &Galaxy) -> bool {
    let full = campaign.max_jump_distance();
    full > 0.0 && jump_reach(campaign, galaxy, campaign.current_system) + 1e-4 < full
}

/// Steve's gift (0x06526395 -> 0xFF3CA0, once): each `meaningOfLifeRewardTools`
/// entry with a reward count above 0 is added with that many charges (only
/// the Staff of Life, 42), the rare "Talked to Steve" is found (0xA9E151E4)
/// and the event 0xDE5EDDE2 is logged. Returns the event line (None when the
/// gift was already given).
pub fn give_steve_gift(campaign: &mut SpaceCampaign) -> Option<String> {
    campaign.endgame.steve_pending = false;
    if campaign.endgame.met_steve {
        return None;
    }
    campaign.endgame.met_steve = true;
    for (tool, count) in rules::core_reward_grants() {
        campaign.grant_tool(tool);
        if tool == rules::tool::GENESIS_DEVICE {
            campaign.staff_of_life_charges = count.min(rules::STAFF_OF_LIFE_MAX_CHARGES) as u8;
        } else {
            campaign.tool_charges.insert(tool.to_string(), count);
        }
    }
    campaign.endgame.find_rare(rules::RARE_TALKED_TO_STEVE);
    // ADAPTED wording: the retail event 0xDE5EDDE2 text is not read here.
    let line = format!(
        "Steve gave you the Staff of Life ({} charges). Collected: Talked to Steve.",
        campaign.staff_of_life_charges
    );
    campaign.message = line.clone();
    Some(line)
}

/// Player empire size in stars (`badgeEmpiresize`): the systems holding the
/// homeworld or a colony.
pub fn empire_stars(campaign: &SpaceCampaign) -> u64 {
    let mut systems: Vec<u32> = campaign
        .colonies
        .iter()
        .map(|w| crate::space_galaxy::system_of_world(*w))
        .chain(std::iter::once(crate::space_galaxy::HOME_STAR))
        .collect();
    systems.sort_unstable();
    systems.dedup();
    systems.len() as u64
}

/// Flight 101 is behind the player (the `captain` badge's
/// `badgeFlight101Complete`). ADAPTED: declining the tutorial counts as done.
pub fn flight101_complete(campaign: &SpaceCampaign) -> bool {
    let order = &rules::TutorialStep::ORDER;
    let at = |s: rules::TutorialStep| order.iter().position(|o| *o == s).unwrap_or(0);
    at(campaign.tutorial.step) >= at(rules::TutorialStep::Story201)
}

/// Bring the counters the badge requirements read up to date: a change of
/// system is a flight (0x1002040 adds `FlightsMade` per jump) and a first
/// visit of a special star is `Sightseer` with its galactic-object rare
/// (0xFDD590); routes opened, the most money held.
pub fn update_counters(campaign: &mut SpaceCampaign, wallet: f32, galaxy: Option<&Galaxy>) {
    let here = campaign.current_system;
    let e = &mut campaign.endgame;
    match e.last_system {
        Some(last) if last != here => e.flights_made += 1,
        _ => {}
    }
    if e.last_system != Some(here) {
        e.last_system = Some(here);
        if let Some(kind) = galaxy.and_then(|g| g.star(here)).map(|s| s.kind) {
            if let Some(n) = galactic_object(kind) {
                if !e.sightseer_stars.contains(&here) {
                    e.sightseer_stars.push(here);
                    e.find_rare(n);
                }
            }
        }
    }
    for w in campaign.trade_routes.keys() {
        if !campaign.endgame.route_worlds.contains(w) {
            campaign.endgame.route_worlds.push(*w);
        }
    }
    campaign.endgame.peak_cash = campaign.endgame.peak_cash.max(wallet as f64);
}

/// A badge reached for the first time.
#[derive(Clone, Debug, PartialEq)]
pub struct NewBadge {
    pub id: String,
    pub name: String,
    pub level: u8,
}

/// Record every badge level the counters have reached (kept from then on)
/// and the single badges; returns the new ones in table order.
pub fn record_badges(campaign: &mut SpaceCampaign) -> Vec<NewBadge> {
    let mut new = Vec::new();
    for f in rules::BADGE_FAMILIES.iter() {
        let live = rules::badge_level(f, campaign.badge_counter(f.counter));
        let kept = campaign.endgame.level(f.id);
        if live > kept {
            campaign.endgame.earned.insert(f.id.to_string(), live);
            for level in kept + 1..=live {
                new.push(NewBadge { id: format!("{}-{level}", f.id), name: format!("{} {level}", f.name), level });
            }
        }
    }
    if flight101_complete(campaign) && campaign.endgame.add_special("captain") {
        new.push(NewBadge { id: "captain".into(), name: "Captain".into(), level: 1 });
    }
    // `badgeReqGrobAllied` (hidden, no points): allied with the Grox.
    if campaign.danced_with_the_devil && campaign.endgame.add_special("grobally") {
        new.push(NewBadge { id: "grobally".into(), name: "Dance with the Devil".into(), level: 1 });
    }
    new
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

pub struct SpaceEndgamePlugin;

impl Plugin for SpaceEndgamePlugin {
    fn build(&self, app: &mut App) {
        use crate::stages::GameStage;
        app.init_resource::<SpaceCinematicScale>()
            .add_systems(Update, (demo, track, steve).chain().run_if(in_state(GameStage::Space)))
            .add_systems(Update, crate::space_hud::comm::portrait_emotion.run_if(in_state(GameStage::Space)))
            .add_systems(OnExit(GameStage::Space), |mut scale: ResMut<SpaceCinematicScale>| scale.0 = 1.0);
    }
}

/// Counters, badges and their announcement (the badge manager's update).
fn track(mut run: ResMut<RunProgress>, galaxy: Option<Res<Galaxy>>, mut primed: Local<bool>) {
    let wallet = run.spice;
    let campaign = &mut run.space_campaign;
    update_counters(campaign, wallet, galaxy.as_deref());
    let new = record_badges(campaign);
    // A loaded game catches up silently; later badges are announced.
    if *primed {
        if let Some(b) = new.last() {
            campaign.message = if new.len() == 1 {
                format!("New badge earned: {}!", b.name)
            } else {
                format!("{} new badges earned, the latest {}!", new.len(), b.name)
            };
        }
    }
    if !new.is_empty() {
        run.request_save();
    }
    *primed = true;
}

/// Capture demos: `--space-core` jumps to the galactic core once the stage
/// is up (with `--cinematic` the arrival leaves the cinematic to the capture);
/// `--space-promote N` earns badges up to promotion N after the warm-up;
/// `--space-fly` sends the UFO to the farthest world of the system.
fn demo(
    mut run: ResMut<RunProgress>,
    galaxy: Option<Res<Galaxy>>,
    view: Option<ResMut<crate::space_view::SpaceView>>,
    mut ship: Query<(&Transform, &mut crate::space::ShipMotion), With<Player>>,
    worlds: Query<(&Transform, &crate::persistence::WorldId), (With<crate::game::Planet>, Without<Player>)>,
    mut frames: Local<u32>,
) {
    *frames += 1;
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
    if *frames == 20 && args.iter().any(|a| a == "--space-core") {
        if let (Some(galaxy), Some(mut view)) = (galaxy, view) {
            if args.iter().any(|a| a == "--cinematic") {
                run.space_campaign.reached_core = true;
            }
            view.arrival = Some(galaxy.core);
        }
    }
    if *frames == 30 && args.iter().any(|a| a == "--space-fly") {
        if let Ok((at, mut motion)) = ship.single_mut() {
            let far = worlds
                .iter()
                .max_by(|a, b| a.0.translation.distance(at.translation).total_cmp(&b.0.translation.distance(at.translation)));
            motion.autopilot_target = far.map(|(_, id)| id.0);
        }
    }
    if *frames == 40 {
        if let Some(rank) = flag("--space-promote").and_then(|v| v.parse::<usize>().ok()).filter(|r| (1..=10).contains(r)) {
            let goal = rules::RANK_BADGE_POINTS[rank - 1];
            let c = &mut run.space_campaign;
            c.tutorial.step = rules::TutorialStep::Done;
            for f in rules::BADGE_FAMILIES.iter() {
                if c.total_badge_points() >= goal {
                    break;
                }
                for level in 1..=5u8 {
                    c.endgame.earned.insert(f.id.to_string(), level);
                    if c.total_badge_points() >= goal {
                        break;
                    }
                }
            }
        }
    }
}

/// One run of the core cinematic.
#[derive(Default)]
struct SteveRun {
    /// The cinematic was seen playing.
    seen: bool,
    /// Frames since the gift was asked for without the cinematic playing.
    waiting: u32,
    /// The effect whose end continues the current wait, and how long it ran.
    effect: Option<(Entity, f32, f32)>,
    star_hidden: bool,
}

/// `SPG_CenterOfGalaxy`'s messages and the gift's safety path.
fn steve(world: &mut World, mut cursor: Local<MessageCursor<CinematicMessage>>, mut state: Local<SteveRun>) {
    let ids: Vec<u32> = match world.get_resource::<Messages<CinematicMessage>>() {
        Some(m) => cursor.read(m).map(|m| m.id).collect(),
        None => Vec::new(),
    };
    let playing = world
        .get_resource::<Cinematics>()
        .and_then(|c| c.current().map(|(_, name, _)| name == CINEMATIC_CENTER_OF_GALAXY))
        .unwrap_or(false);
    let scale = if playing { CORE_CINEMATIC_SCALE } else { 1.0 };
    if world.resource::<SpaceCinematicScale>().0 != scale {
        world.resource_mut::<SpaceCinematicScale>().0 = scale;
    }
    for id in ids {
        match id {
            msg::STEVE_START_UFO => place_ufo(world, STEVE_START_X * CORE_CINEMATIC_SCALE),
            msg::STEVE_MOVE_UFO => place_ufo(world, STEVE_END_X * CORE_CINEMATIC_SCALE),
            msg::JOURNEY_START => {
                state.effect = start_effect(world, JOURNEY_EFFECT).map(|e| (e, 0.0, JOURNEY_SAFETY_SECS));
                if state.effect.is_none() {
                    state.effect = Some((Entity::PLACEHOLDER, 0.0, JOURNEY_SAFETY_SECS));
                }
                set_star_visible(world, false);
                state.star_hidden = true;
            }
            msg::RETURN_START => {
                state.effect = start_effect(world, RETURN_EFFECT).map(|e| (e, 0.0, RETURN_SAFETY_SECS));
                if state.effect.is_none() {
                    state.effect = Some((Entity::PLACEHOLDER, 0.0, RETURN_SAFETY_SECS));
                }
            }
            msg::GIVE_TOOLS => {
                give_steve_gift(&mut world.resource_mut::<RunProgress>().space_campaign);
                world.resource_mut::<RunProgress>().request_save();
            }
            msg::TRANSITION => {
                if let Some(mut view) = world.get_resource_mut::<crate::space_view::SpaceView>() {
                    view.open_galaxy();
                }
            }
            _ => {}
        }
    }
    // The effect's end continues the script's `wait -forContinue`.
    let dt = world.get_resource::<Time<Real>>().map_or(0.0, |t| t.delta_secs()).min(0.25);
    if let Some((entity, ran, safety)) = state.effect.as_mut() {
        *ran += dt;
        let alive = world.get::<crate::swarm_fx::SwarmEffect>(*entity).is_some_and(|e| e.alive);
        // An effect that never started (still loading) is given a moment.
        let done = (!alive && *ran > 0.5) || *ran >= *safety;
        if done {
            if let Some(mut cin) = world.get_resource_mut::<Cinematics>() {
                if cin.runtime.waiting_for_continue() == Some(crate::cinematics::command::WaitKind::ForContinue) {
                    cin.continue_cinematic();
                    state.effect = None;
                }
            }
            if !playing {
                state.effect = None;
            }
        }
    }
    // The gift when the cinematic cannot give it: not installed, skipped
    // before the gift state, or a headless run.
    let pending = world.resource::<RunProgress>().space_campaign.endgame.steve_pending;
    let queued = world.resource::<RunProgress>().space_campaign.pending_cinematic.is_some();
    if playing {
        state.seen = true;
        state.waiting = 0;
    } else if pending && !queued {
        state.waiting += 1;
        let can_play = world.get_resource::<Cinematics>().is_some_and(|c| c.runtime.has_state(CINEMATIC_CENTER_OF_GALAXY));
        if state.seen || !can_play || state.waiting > 90 {
            give_steve_gift(&mut world.resource_mut::<RunProgress>().space_campaign);
            world.resource_mut::<RunProgress>().request_save();
            state.seen = false;
            state.waiting = 0;
        }
    } else if !pending {
        state.seen = false;
        state.waiting = 0;
    }
    if !playing && state.star_hidden {
        set_star_visible(world, true);
        state.star_hidden = false;
        state.effect = None;
    }
}

/// The UFO messages set the ship's solar position (x on the system plane,
/// its altitude kept).
fn place_ufo(world: &mut World, x: f32) {
    let mut q = world.query_filtered::<&mut Transform, With<Player>>();
    if let Some(mut t) = q.iter_mut(world).next() {
        t.translation = Vec3::new(x, t.translation.y, 0.0);
    }
}

fn start_effect(world: &mut World, id: u32) -> Option<Entity> {
    let installed = world.get_resource::<crate::swarm_fx::SwarmLibrary>().is_some_and(|l| l.loaded && l.find(id).is_some());
    if !installed {
        return None;
    }
    let mut effect = crate::swarm_fx::SwarmEffect::new(id);
    effect.tag = 0x5E1F_0000_0000_0000 | id as u64;
    Some(world.spawn((effect, crate::stages::StageOwned, Name::new(format!("core effect {id:08X}")))).id())
}

/// The quasar is stopped while Steve's journey plays (0x06524498 stops the
/// galaxy / skybox effects; the system graphics are rebuilt without it).
fn set_star_visible(world: &mut World, visible: bool) {
    // The star's drawn parts are separate system bodies (`space_scene`:
    // "star core", "star glow", "accretion disk", companion star / glow).
    const PARTS: [&str; 5] = ["star core", "star glow", "accretion disk", "companion star", "companion glow"];
    let mut q = world.query_filtered::<(&Name, &mut Visibility), With<crate::space::SystemBody>>();
    for (name, mut v) in q.iter_mut(world) {
        if PARTS.contains(&name.as_str()) {
            *v = if visible { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

// ---------------------------------------------------------------------------
// The UFO's attitude in the solar view
// ---------------------------------------------------------------------------

/// `spaceufo~` limits (VERIFIED data): `ufoMaxBankTilt` / `ufoMaxNoseTilt` 30
/// degrees, `ufoBankTiltRate` 2, `ufoNoseTiltRate` 4,
/// `ufoRotationPercentageRate` 3 (share of the remaining turn per second).
pub const UFO_MAX_BANK_DEG: f32 = 30.0;
pub const UFO_MAX_NOSE_DEG: f32 = 30.0;
pub const UFO_BANK_RATE: f32 = 2.0;
pub const UFO_NOSE_RATE: f32 = 4.0;
pub const UFO_ROTATION_RATE: f32 = 3.0;

/// Heading, bank and nose of the UFO in the solar view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UfoAttitude {
    /// Yaw of the ship's front (-Z) about +Y.
    pub heading: f32,
    pub bank: f32,
    pub nose: f32,
    pub init: bool,
}

/// One step of the attitude: the front turns toward the flight direction at
/// the rotation rate, the ship banks into the turn and noses down while it
/// speeds up. ADAPTED: the retail integration was not traced (space-ctrl
/// notes 5); the limits and rates are the data's.
pub fn step_attitude(a: &mut UfoAttitude, velocity: Vec3, top_speed: f32, accelerating: bool, dt: f32) -> Quat {
    let flat = Vec2::new(velocity.x, velocity.z);
    let speed = flat.length();
    let ratio = if top_speed > 0.0 { (speed / top_speed).clamp(0.0, 1.0) } else { 0.0 };
    let mut turn = 0.0;
    if speed > 0.05 * top_speed.max(1e-3) {
        // Bevy forward is -Z: yaw 0 looks down -Z.
        let goal = (-velocity.x).atan2(-velocity.z);
        if !a.init {
            a.heading = goal;
            a.init = true;
        }
        let mut diff = goal - a.heading;
        while diff > std::f32::consts::PI {
            diff -= std::f32::consts::TAU;
        }
        while diff < -std::f32::consts::PI {
            diff += std::f32::consts::TAU;
        }
        let step = diff * (UFO_ROTATION_RATE * dt).min(1.0);
        a.heading += step;
        turn = if dt > 0.0 { step / dt } else { 0.0 };
    }
    let bank_goal = (-turn * 0.5).clamp(-1.0, 1.0) * UFO_MAX_BANK_DEG.to_radians() * (0.4 + 0.6 * ratio);
    let nose_goal = if accelerating { -ratio * UFO_MAX_NOSE_DEG.to_radians() * 0.5 } else { 0.0 };
    a.bank += (bank_goal - a.bank) * (UFO_BANK_RATE * dt).min(1.0);
    a.nose += (nose_goal - a.nose) * (UFO_NOSE_RATE * dt).min(1.0);
    Quat::from_rotation_y(a.heading) * Quat::from_rotation_z(a.bank) * Quat::from_rotation_x(a.nose)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_core_plays_steve_and_his_gift_is_the_staff_of_life() {
        let mut c = SpaceCampaign::default();
        let news = c.on_arrival(None, true);
        assert!(c.reached_core);
        assert!(c.pending_restore, "0x1002DD0 heals and fills the energy");
        assert_eq!(c.pending_cinematic.as_deref(), Some("SPG_CenterOfGalaxy"));
        assert!(c.endgame.steve_pending);
        assert!(!c.has_tool("genesisdevice"), "the gift comes with Steve, not on arrival");
        assert!(!news.is_empty());
        // A second visit replays nothing.
        c.pending_cinematic = None;
        c.on_arrival(None, true);
        assert!(c.pending_cinematic.is_none());
        let line = give_steve_gift(&mut c).unwrap();
        assert!(line.contains("42"));
        assert!(c.has_tool("genesisdevice"));
        assert_eq!(c.staff_of_life_charges, 42);
        for t in ["monolith", "placeuberturret", "antimatterbomb", "placehappinessbooster"] {
            assert!(!c.has_tool(t), "{t}: reward count 0");
        }
        assert!(c.endgame.met_steve && !c.endgame.steve_pending);
        assert!(c.endgame.galactic_objects.contains(&10));
        assert!(give_steve_gift(&mut c).is_none(), "once");
    }

    #[test]
    fn the_core_limits_the_drive_until_steve_was_met() {
        let galaxy = Galaxy::generate(None);
        let mut c = SpaceCampaign::default();
        c.grant_tool("interstellardrive");
        c.grant_tool("interstellardrive_upgrade4");
        let full = c.max_jump_distance();
        assert_eq!(full, 20.0);
        // A star deep inside the limit, one outside it.
        let near = (0..galaxy.stars.len() as u32).find(|&s| s != galaxy.core && core_distance(&galaxy, s) < 60.0).unwrap();
        let far = (0..galaxy.stars.len() as u32).find(|&s| core_distance(&galaxy, s) > 150.0).unwrap();
        assert_eq!(jump_reach(&c, &galaxy, near), 3.0);
        assert_eq!(jump_reach(&c, &galaxy, far), full);
        c.current_system = near;
        assert!(core_limits_reach(&c, &galaxy));
        c.endgame.met_steve = true;
        assert_eq!(jump_reach(&c, &galaxy, near), full);
        assert!(!core_limits_reach(&c, &galaxy));
    }

    #[test]
    fn badges_once_earned_stay_and_the_captain_needs_flight_101() {
        let mut c = SpaceCampaign::default();
        assert_eq!(c.total_badge_points(), 0, "no captain before Flight 101");
        c.tutorial.step = rules::TutorialStep::Story201;
        let new = record_badges(&mut c);
        assert_eq!(new.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), vec!["captain"]);
        assert_eq!(c.total_badge_points(), 1);
        // Golden Touch: 1,000,000 held, then spent.
        update_counters(&mut c, 1_200_000.0, None);
        let new = record_badges(&mut c);
        assert_eq!(new.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), vec!["goldentouch-1", "goldentouch-2"]);
        update_counters(&mut c, 10.0, None);
        assert!(record_badges(&mut c).is_empty());
        assert_eq!(c.endgame.level("goldentouch"), 2);
        assert_eq!(c.total_badge_points(), 1 + 2 + 4);
        // Flights: every change of system.
        for s in [5, 6, 5, 7] {
            c.current_system = s;
            update_counters(&mut c, 0.0, None);
        }
        assert_eq!(c.endgame.flights_made, 4, "every change of system after the first sighting");
        let mut fresh = SpaceCampaign::default();
        update_counters(&mut fresh, 0.0, None);
        assert_eq!(fresh.endgame.flights_made, 0, "the first sighting only primes the counter");
    }

    #[test]
    fn special_stars_count_for_sightseer_and_give_their_rare() {
        let galaxy = Galaxy::generate(None);
        let mut c = SpaceCampaign::default();
        update_counters(&mut c, 0.0, Some(&galaxy));
        let specials: Vec<u32> = (0..galaxy.stars.len() as u32)
            .filter(|&s| galactic_object(galaxy.star(s).unwrap().kind).is_some())
            .take(3)
            .collect();
        for &s in &specials {
            c.current_system = s;
            update_counters(&mut c, 0.0, Some(&galaxy));
        }
        c.current_system = specials[0];
        update_counters(&mut c, 0.0, Some(&galaxy));
        assert_eq!(c.endgame.sightseer_stars.len(), 3, "a star counts once");
        assert_eq!(c.badge_counter(rules::BadgeCounter::Sightseer), 3);
        assert!(!c.endgame.galactic_objects.is_empty());
        assert_eq!(galactic_object(StarKind::GalacticCore), Some(1));
        assert_eq!(galactic_object(StarKind::BinaryMM), Some(9));
        assert_eq!(galactic_object(StarKind::Yellow), None);
    }

    #[test]
    fn promotions_open_posse_slots_every_second_rank() {
        let mut c = SpaceCampaign::default();
        c.tutorial.step = rules::TutorialStep::Done;
        c.endgame.earned.insert("colonist".into(), 5); // 45 points
        c.endgame.earned.insert("explorer".into(), 1); // 2
        assert_eq!(c.total_badge_points(), 1 + 45 + 2);
        assert_eq!(c.captain_rank(), 3);
        assert_eq!(c.max_fleet_size(), 1);
    }

    #[test]
    fn the_ufo_turns_to_its_flight_direction_within_the_tilt_limits() {
        let mut a = UfoAttitude::default();
        let mut q = Quat::IDENTITY;
        for _ in 0..240 {
            q = step_attitude(&mut a, Vec3::new(10.0, 0.0, 0.0), 10.0, true, 1.0 / 60.0);
        }
        let front = q * Vec3::NEG_Z;
        assert!(front.x > 0.8, "faces +X: {front:?}");
        assert!(a.bank.abs() <= UFO_MAX_BANK_DEG.to_radians() + 1e-4);
        assert!(a.nose.abs() <= UFO_MAX_NOSE_DEG.to_radians() + 1e-4);
        // A turn banks the ship.
        let mut b = a;
        step_attitude(&mut b, Vec3::new(0.0, 0.0, 10.0), 10.0, true, 1.0 / 60.0);
        assert!(b.bank.abs() > 0.0);
    }
}
