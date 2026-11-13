//! The cinematics the retail stage code plays at its moments, and the stage
//! transitions they hold back (`notes\cinematics-re-notes.md` section 4, the
//! stage notes name the callers).
//!
//! Three mechanisms, none of which needs a stage module to hold a
//! `ResMut<Cinematics>`:
//! * [`request`] / [`hold_death`]: one-line hooks for stage code (a static
//!   queue drained once per frame). They return `false` when the player is not
//!   running (smoke test, unit tests, `--quick-capture` without
//!   `--cinematics`), so the stage keeps its own stand-in there;
//! * [`detect`]: watches `RunProgress` and the stage's entities for the events
//!   at which the retail code calls `PlayCinematic` (tribe allied / conquered /
//!   levelled up, a city attacked, tech levels unlocked, migration, a Space
//!   `pending_cinematic`, ...). The first-time moments are one-shot flags of
//!   the run (`RunProgress::cinematics_played`);
//! * [`gate`]: a `NextState<GameStage>` change that the retail code wraps in
//!   cinematics (Tribe -> Civ: `TRG2CVG_PreEditor`, `TRG2CVG_ShowCityHall`;
//!   Civ -> Space: `CVG_UnlockUFO`) is held until they have played.
//!
//! The game messages the scripts send (`sendMessage`) are handled in
//! [`handle_messages`]: `kEnterCreatureEditor` opens the Creature Editor after
//! `CRG_LaidEgg`, `kRestartAtNest` releases the avatar's respawn after a death
//! cinematic.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;

use bevy::ecs::message::MessageCursor;
use bevy::prelude::*;

use super::{CinematicEnded, CinematicMessage, CinematicTarget, Cinematics};
use crate::game::{Player, RunProgress};
use crate::stages::GameStage;
use crate::tribal::RivalStatus;
use crate::world_objects::name_id;

/// Retail `sendMessage` ids the stages listen to.
pub mod message {
    /// `kEnterCreatureEditor`: the end of `CRG_LaidEgg`.
    pub const ENTER_CREATURE_EDITOR: u32 = 0x04D9_686F;
    /// `kRestartAtNest`: the end of the death cinematics.
    pub const RESTART_AT_NEST: u32 = 0x04F6_0B92;
    /// `kHatchAvatarEggs`: `CRG_SpeciesMigrate`'s hatch state (and its exit
    /// state, for a skip) hatches the eggs of the avatar's nest (0xD43840).
    pub const HATCH_AVATAR_EGGS: u32 = 0x0656_6531;
    /// `kUpgradeNest`: `CRG_ClaimNest` swaps the claimed nest's model for the
    /// brain level's (0xD438AC).
    pub const UPGRADE_NEST: u32 = 0x0655_5ABC;
    /// "restart tribe": the end of `TRG_PRE_PopDestroyed` /
    /// `TRG_PRE_HutDestroyed` (tribe.trigger; the Tribal handler 0xCD81F0
    /// answers it with the persistence manager's restart 0xB27150).
    pub const RESTART_TRIBE: u32 = 0x0420_E32A;
}

/// A cinematic with the stage objects its script names.
#[derive(Clone, Debug)]
pub struct Play {
    pub name: String,
    pub pause_game: bool,
    /// `PlayCinematic`'s queue flag: false = refused while another cinematic
    /// plays (the retail call sites that pass queue 0).
    pub queue: bool,
    pub bind: Vec<(&'static str, CinematicTarget)>,
    /// `textFromCode` keys the script reads, with their text.
    pub text: Vec<(&'static str, String)>,
}

impl Play {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), pause_game: false, queue: true, bind: Vec::new(), text: Vec::new() }
    }

    pub fn bind(mut self, name: &'static str, target: CinematicTarget) -> Self {
        self.bind.push((name, target));
        self
    }

    pub fn text(mut self, key: &'static str, text: Option<String>) -> Self {
        if let Some(text) = text {
            self.text.push((key, text));
        }
        self
    }
}

// ---------------------------------------------------------------------------
// The request queue (one-line hooks for stage code)
// ---------------------------------------------------------------------------

static AVAILABLE: AtomicBool = AtomicBool::new(false);
static REQUESTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Whether the cinematic player will play what is requested (interactive game
/// or a capture with `--cinematics`; never in unit tests or the smoke test).
pub fn available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}

/// Ask for a cinematic at a retail moment; the stage's bindings are added.
/// `false` when the player is off: the caller keeps its stand-in.
pub fn request(name: &str) -> bool {
    if !available() {
        return false;
    }
    if let Ok(mut queue) = REQUESTS.lock() {
        queue.push(name.to_string());
        return true;
    }
    false
}

static STOPS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static SKIPPED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Code stop (0xAD7D40) of a cinematic the stage started with [`request`]
/// (the "hold" cinematics `CLG_intro` / `cell_ending`): the running one ends,
/// a queued one is dropped.
pub fn request_stop(name: &str) {
    if !available() {
        return;
    }
    if let Ok(mut queue) = REQUESTS.lock() {
        queue.retain(|n| n != name);
    }
    if let Ok(mut stops) = STOPS.lock() {
        stops.push(name.to_string());
    }
}

/// The cinematic `name` was ended by the player (Escape / its exit state)
/// since the last call: the stage treats it as its own skip.
pub fn take_skipped(name: &str) -> bool {
    let Ok(mut skipped) = SKIPPED.lock() else { return false };
    let before = skipped.len();
    skipped.retain(|n| n != name);
    skipped.len() != before
}

/// Why the avatar died (the retail death code picks the cinematic by cause,
/// 0xD38E60: 5 starvation, 9 deep water / sea monster, else `CRG_AvatarDied`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathCause {
    Combat,
    Starvation,
    SeaMonster,
}

static LAYING: AtomicBool = AtomicBool::new(false);

/// The mating key was pressed and `CRG_LaidEgg` plays: the Creature Editor
/// opens on its `kEnterCreatureEditor` message, so the stage code must not
/// finish the mating before.
pub fn request_laid_egg() -> bool {
    if request("CRG_LaidEgg") {
        LAYING.store(true, Ordering::Relaxed);
        return true;
    }
    false
}

/// `CRG_LaidEgg` has been requested and has not yet opened the editor.
pub fn laying_egg() -> bool {
    LAYING.load(Ordering::Relaxed)
}

const DEATH_IDLE: u8 = 0;
const DEATH_PLAYING: u8 = 1;
const DEATH_RESTART: u8 = 2;
static DEATH: AtomicU8 = AtomicU8::new(DEATH_IDLE);

/// Call while the avatar is dead, before the respawn: `true` while a death
/// cinematic holds the respawn (retail: the avatar stays down until the
/// cinematic's `kRestartAtNest`), `false` when the respawn may go ahead.
pub fn hold_death(cause: DeathCause) -> bool {
    if !available() {
        return false;
    }
    match DEATH.load(Ordering::Relaxed) {
        DEATH_PLAYING => true,
        DEATH_RESTART => {
            DEATH.store(DEATH_IDLE, Ordering::Relaxed);
            false
        }
        _ => {
            let name = match cause {
                DeathCause::Combat => "CRG_AvatarDied",
                DeathCause::Starvation => "CRG_AvatarDied_Starvation",
                DeathCause::SeaMonster => "CRG_AvatarDied_SeaMonster",
            };
            if request(name) {
                DEATH.store(DEATH_PLAYING, Ordering::Relaxed);
                true
            } else {
                false
            }
        }
    }
}

/// Publish whether the player runs (once per frame).
pub fn publish_availability(cinematics: Res<Cinematics>) {
    if cfg!(test) {
        return;
    }
    let on = cinematics.settings.hooks && cinematics.runtime.enabled && cinematics.loaded();
    AVAILABLE.store(on, Ordering::Relaxed);
    if !on {
        if let Ok(mut queue) = REQUESTS.lock() {
            queue.clear();
        }
        if let Ok(mut stops) = STOPS.lock() {
            stops.clear();
        }
        if let Ok(mut skipped) = SKIPPED.lock() {
            skipped.clear();
        }
        DEATH.store(DEATH_IDLE, Ordering::Relaxed);
        LAYING.store(false, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// World lookups
// ---------------------------------------------------------------------------

fn stage_of(world: &World) -> Option<GameStage> {
    world.get_resource::<State<GameStage>>().map(|s| s.get().clone())
}

fn pos_of(world: &World, e: Entity) -> Option<Vec3> {
    world.get::<Transform>(e).map(|t| t.translation)
}

fn player_entity(world: &mut World) -> Option<Entity> {
    let mut q = world.query_filtered::<Entity, With<Player>>();
    q.iter(world).next()
}

fn home_prop(world: &mut World, kind: crate::tribal_rts::HomePropKind) -> Option<Entity> {
    let mut q = world.query::<(Entity, &crate::tribal_rts::HomeProp)>();
    q.iter(world).find(|(_, p)| p.kind == kind).map(|(e, _)| e)
}

fn village_entity(world: &mut World, index: usize) -> Option<Entity> {
    let mut q = world.query::<(Entity, &crate::tribal::RivalVillage)>();
    q.iter(world).find(|(_, v)| v.0 == index).map(|(e, _)| e)
}

fn city_entity(world: &mut World, index: usize) -> Option<Entity> {
    let mut q = world.query::<(Entity, &crate::civ::CityMarker)>();
    q.iter(world).find(|(_, c)| c.0 == index).map(|(e, _)| e)
}

fn target_of(world: &World, e: Option<Entity>, fallback: Vec3) -> CinematicTarget {
    match e.filter(|e| world.get_entity(*e).is_ok()) {
        Some(e) => CinematicTarget::Entity(e),
        None => CinematicTarget::Point { position: fallback, yaw: 0.0 },
    }
}

fn played(world: &World, name: &str) -> bool {
    world.get_resource::<RunProgress>().is_some_and(|r| r.cinematics_played.iter().any(|n| n == name))
}

/// Remember a one-shot cinematic in the run. The retail "profile flag" calls (0xCF7990 test /
/// 0xCF7950 set, 0xC77460, the Space hint players) are the player's one-time event flags
/// (`cPlayer::mOneTimeEventFlags` at +0x38, keys {id, 0, 0xAA9A8ED7}), a serialized attribute
/// of the player object that the saved game holds (VERIFIED, `front-end-audit` notes): per
/// game, not per profile, so the run is the right owner.
pub fn mark_played(world: &mut World, name: &str) {
    if let Some(mut run) = world.get_resource_mut::<RunProgress>() {
        if !run.cinematics_played.iter().any(|n| n == name) {
            run.cinematics_played.push(name.to_string());
            run.request_save();
        }
    }
}

// ---------------------------------------------------------------------------
// Bindings of the scripts' named objects, per stage
// ---------------------------------------------------------------------------

/// The names the retail stage code binds (0xAE0930) for the current stage.
/// ADAPTED where OpenSpore has no such object (mate, egg, mother, fire pit).
pub fn bindings(world: &mut World) -> Vec<(&'static str, CinematicTarget)> {
    let _layout = crate::tribal_planet::scope_world(world);
    let Some(stage) = stage_of(world) else { return Vec::new() };
    let mut out: Vec<(&'static str, CinematicTarget)> = Vec::new();
    let player = player_entity(world);
    match stage {
        GameStage::Creature => {
            let mut q = world.query_filtered::<Entity, With<crate::creature_home::HomeNest>>();
            let nest = q.iter(world).next();
            let here = nest.and_then(|n| pos_of(world, n)).unwrap_or(Vec3::ZERO);
            if let Some(nest) = nest {
                out.push(("AvatarNest", CinematicTarget::Entity(nest)));
            }
            // ADAPTED: no mate or mother creature is simulated: the avatar
            // stands in for `MateLead`, points beside the nest for the egg
            // and the teacher.
            if let Some(p) = player {
                out.push(("MateLead", CinematicTarget::Entity(p)));
            }
            let egg = CinematicTarget::Point { position: here + Vec3::new(0.0, 0.0, 0.0), yaw: 0.0 };
            out.push(("Egg", egg));
            out.push(("egg", egg));
            out.push(("teacher", CinematicTarget::Point { position: here + Vec3::new(-3.0, 0.0, 2.0), yaw: 0.0 }));
            let destination = world
                .get_resource::<RunProgress>()
                .and_then(|r| r.creature_progression.pending_migration)
                .map(|m| Vec3::from_array(m.destination));
            if let Some(d) = destination {
                out.push(("DestinationNest", CinematicTarget::Point { position: d, yaw: 0.0 }));
            }
        }
        GameStage::Tribal => {
            out.extend(super::hooks::tribal_bindings(world));
            let totem = home_prop(world, crate::tribal_rts::HomePropKind::Totem);
            out.push(("TotemPole", target_of(world, totem, crate::tribal::totem_position())));
            let hut = home_prop(world, crate::tribal_rts::HomePropKind::Hut);
            let hut_target = target_of(world, hut, crate::tribal::home_position());
            out.push(("hut", hut_target));
            out.push(("TRGCityHallLocation", hut_target));
            let mat = home_prop(world, crate::tribal_rts::HomePropKind::FoodPile);
            out.push(("foodmat", target_of(world, mat, crate::tribal::food_pile_position())));
            if let Some(p) = player {
                out.push(("chieftain", CinematicTarget::Entity(p)));
            }
            // The NPC tribe the moment is about (the detector binds the real
            // one; played by name it is the first village).
            let village = village_entity(world, 0);
            let npc = target_of(world, village, crate::tribal::village_position(0));
            for name in ["npcHut", "npcFire", "Tribe"] {
                out.push((name, npc));
            }
            // The fire pit of `TRG_Upgrade` is the camp fire (see `tribal_bindings`).
            if let Some((_, fire)) = out.iter().find(|(n, _)| *n == "TribeFire").copied() {
                out.push(("firepit", fire));
            }
        }
        GameStage::Civilization => {
            let capital = city_entity(world, crate::civ::HOME_INDEX);
            let spot = capital.and_then(|c| pos_of(world, c)).unwrap_or(Vec3::ZERO);
            let city = target_of(world, capital, spot);
            for name in ["PlayerCityPlaza", "CityPlaza", "CityHall", "CityHallBeingBuilt", "AttackedCityHall", "Plaza"] {
                out.push((name, city));
            }
            // The tutorial's vehicle and geyser: the player's nearest vehicle
            // and the first geyser.
            let mut q = world.query::<(Entity, &Transform, &crate::civ_rts::CivUnit)>();
            let vehicle = q
                .iter(world)
                .filter(|(_, _, u)| u.nation == crate::civ::HOME_INDEX)
                .min_by(|a, b| a.1.translation.distance(spot).total_cmp(&b.1.translation.distance(spot)))
                .map(|(e, _, _)| e);
            if let Some(v) = vehicle {
                out.push(("PlayerVehicle", CinematicTarget::Entity(v)));
            }
            out.push((
                "SpiceCommodityNode",
                CinematicTarget::Point { position: crate::civ::geyser_position(0), yaw: 0.0 },
            ));
            // 0xAE3AB0 binds "NewCityLocation" (0xB232EF6D) to a marker at the
            // city it places; here the city the rule reveals next, bound before
            // the camera of `CVG_NewCityAppears` looks for it.
            let next = world.get_resource::<RunProgress>().and_then(|r| {
                crate::civ_reveal::next_hidden(&r.civ_campaign).and_then(|i| r.civ_campaign.cities.get(i)).map(|c| c.center())
            });
            if let Some(p) = next {
                out.push(("NewCityLocation", CinematicTarget::Point { position: p, yaw: 0.0 }));
            }
        }
        GameStage::Space => {
            // ADAPTED: the home city plaza of the UFO's launch is not in the
            // Space stage; the UFO's position stands in for it.
            if let Some(p) = player {
                out.push(("PlayerCityPlaza", CinematicTarget::Entity(p)));
                out.push(("playerdeathposition", CinematicTarget::Entity(p)));
            }
        }
        _ => {}
    }
    out
}

// ---------------------------------------------------------------------------
// Playing
// ---------------------------------------------------------------------------

/// Text the scripts take from code (`textFromCode`): the locale string of the
/// installed `cinematics_*` table, or an ADAPTED line where the retail code
/// builds it from other data.
pub(super) fn prepare(world: &mut World, play: &Play) {
    let mut texts: Vec<(&'static str, String)> = play.text.clone();
    let fixed = match play.name.as_str() {
        // 0xD3D18C: the first-DNA hint.
        "CRG_FirstDNA" => Some(("FirstDNAString", "cinematics_crg", 0x0459_9F0E)),
        // 0xFF5D90: the UFO's launch ends on the "Lift off" text.
        "CVG2SPG" | "CVG2SPG_short" => Some(("UFOCreateText", "cinematics_spg", 0x0000_0001)),
        _ => None,
    };
    if let Some((key, table, id)) = fixed {
        if let Some(text) = super::host::localized_by_name(world, table, id, &[]) {
            texts.push((key, text));
        }
    }
    // The Tribal moments normally bring their own text (the detector knows
    // the tribe and the totem piece); played by name (captures, `runcinematic`)
    // they read the first alliance line and the first hut level.
    let has = |texts: &[(&'static str, String)], key: &str| texts.iter().any(|(k, _)| *k == key);
    if matches!(play.name.as_str(), "TRG_TotemPole" | "TRG_Upgrade") && !has(&texts, "TotemString") {
        if let Some(text) = totem_text(world, true, 1, 0) {
            texts.push(("TotemString", text));
        }
    }
    // Played by name, the capture card reads the line of a military city
    // taken by force (0x803).
    if matches!(play.name.as_str(), "CVG_Capture" | "CVG_Capture_Nuked") && !has(&texts, "CaptureString") {
        if let Some(text) = super::host::localized_by_name(world, "cinematics_cvg", capture_text(0, 0, false), &[]) {
            texts.push(("CaptureString", text));
        }
    }
    if play.name == "TRG_Upgrade" && !has(&texts, "TRG_Upgrade_Text") {
        if let Some(text) = upgrade_text(world, 1, 1) {
            texts.push(("TRG_Upgrade_Text", text));
        }
    }
    if let Some(mut c) = world.get_resource_mut::<Cinematics>() {
        for (key, text) in texts {
            c.set_text_from_code(key, text);
        }
    }
}

/// Play `play` with the stage's bindings (and the event's own).
pub fn play(world: &mut World, play: Play) -> bool {
    prepare(world, &play);
    let mut all = bindings(world);
    all.extend(play.bind.iter().cloned());
    let Some(mut cin) = world.get_resource_mut::<Cinematics>() else { return false };
    let Some(token) = cin.play(&play.name, play.pause_game, play.queue) else {
        return false;
    };
    for (n, target) in all {
        cin.bind_object(token, n, target);
    }
    true
}

/// The moments system (one exclusive system per frame): start what the stage
/// code asked for with [`request`], watch the stage for the retail moments,
/// answer the scripts' `sendMessage`s.
pub fn update(
    world: &mut World,
    mut seen: Local<Seen>,
    mut cursor: Local<MessageCursor<CinematicMessage>>,
    mut ended: Local<MessageCursor<CinematicEnded>>,
    mut laying_idle: Local<u32>,
) {
    drain_requests(world);
    detect(world, &mut seen);
    handle_messages(world, &mut cursor, &mut laying_idle);
    handle_ended(world, &mut ended);
    await_new_city(world);
}

/// The city `CVG_PRE_NewCityAppears` asked for (usize::MAX: none).
static NEW_CITY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

/// 0xB999F0: when the city is built and `CVG_PRE_NewCityAppears` plays, it is
/// bound as "actualNewCityLocation" (0x9AA7F68F) and 0x445F729 ends the
/// cinematic's `wait -forContinue`. Here: once the city's entity is on the
/// map.
fn await_new_city(world: &mut World) {
    let index = NEW_CITY.load(Ordering::Relaxed);
    if index == usize::MAX {
        return;
    }
    let present = world.get_resource::<RunProgress>().is_some_and(|r| r.civ_campaign.present(index));
    let city = city_entity(world, index);
    let playing = world
        .get_resource::<Cinematics>()
        .and_then(|c| c.current().map(|(t, name, _)| (t, name == crate::civ_reveal::NEW_CITY_APPEARS)));
    match (playing, city) {
        (Some((token, true)), Some(e)) if present => {
            NEW_CITY.store(usize::MAX, Ordering::Relaxed);
            let mut cin = world.resource_mut::<Cinematics>();
            cin.bind_object(token, "actualNewCityLocation", CinematicTarget::Entity(e));
            cin.continue_cinematic();
        }
        (Some((_, true)), _) => {}
        // The cinematic is over (or another one plays): nothing to answer.
        _ => NEW_CITY.store(usize::MAX, Ordering::Relaxed),
    }
}

/// The pauseGame argument of the retail call sites of the requested
/// cinematics: `CVG_PRE_NewCityAppears` (0xAE61C0) and `TRG_PRE_HutDestroyed`
/// (0xCD81F0) pause, the others are played with pause off.
fn request_pauses(name: &str) -> bool {
    matches!(name, "CVG_PRE_NewCityAppears" | "TRG_PRE_HutDestroyed")
}

/// Start what the stage code asked for with [`request`], stop what it asked
/// to stop with [`request_stop`].
fn drain_requests(world: &mut World) {
    let stops: Vec<String> = STOPS.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    if !stops.is_empty() {
        if let Some(mut cin) = world.get_resource_mut::<Cinematics>() {
            for name in &stops {
                cin.stop_named(name);
            }
        }
    }
    let names: Vec<String> = match REQUESTS.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => return,
    };
    for name in names {
        let mut p = Play::new(name.clone());
        p.pause_game = request_pauses(&name);
        if !play(world, p) {
            warn!("cinematic {name}: requested but not played");
            if name.starts_with("CRG_AvatarDied") {
                DEATH.store(DEATH_RESTART, Ordering::Relaxed);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Detectors
// ---------------------------------------------------------------------------

/// What the detectors remember between frames.
#[derive(Default)]
pub struct Seen {
    stage: Option<GameStage>,
    frames: u32,
    // Creature
    migrating: bool,
    migrations: u8,
    deaths: u32,
    generation: u32,
    /// Frames left before the birth's egg fade plays (see [`BIRTH_WAIT_FRAMES`]).
    birth_wait: u8,
    // Tribal
    villages: Vec<(bool, RivalStatus)>,
    hut_level: usize,
    births: usize,
    /// Somebody of the player's tribe is left (a member, a baby, or the
    /// chieftain not waiting for his respawn).
    tribe_alive: bool,
    /// The home hut has been razed.
    hut_destroyed: bool,
    /// The totem moment after a conquest, waiting for the Tribal timer
    /// (`TotemTimer`).
    pub totem_due: Option<TotemTimer>,
    // Civ
    attacked: Vec<bool>,
    owned_cities: usize,
    geysers: Vec<bool>,
    /// Each city is the player's.
    owners: Vec<bool>,
    /// `captures` (military, religious, economic) and the superweapon flag.
    captures: [u32; 3],
    superweapon: bool,
    // Space
    space: Option<SpaceSeen>,
    // Cell
    part_cinematic: bool,
}

/// VERIFIED (Tribal update 0xCDF080): while the timer at Tribe+0x58 runs and
/// has passed 60,000 ms, the code sends 0x445F729 (the continue of every
/// `wait -forContinue/-forButton`) and calls 0xCDAC90, which stops the timer,
/// carves the newest totem piece and plays `TRG_TotemPole` / `TRG_Upgrade`.
/// The timer is restarted (0xBC32A0) by 0xCDB520, which the conquest handler
/// 0xC2F7F0 calls after `TRG_TotemPole_Conquer` when a totem piece is due; the
/// alliance (0xD9CED0) calls 0xCDAC90 at once instead.
pub const TOTEM_AFTER_CONQUEST_SECONDS: f32 = 60.0;

/// The totem moment a conquest has scheduled.
#[derive(Clone, Debug)]
pub struct TotemTimer {
    pub remaining: f32,
    pub play: Play,
}

/// Watch the stage for the retail moments (see the module docs). Runs every
/// frame; the first frames of a stage adopt the state without playing.
fn detect(world: &mut World, seen: &mut Seen) {
    let Some(stage) = stage_of(world) else { return };
    let on = world.get_resource::<Cinematics>().is_some_and(|c| c.settings.hooks && c.runtime.enabled && c.loaded());
    if !on || !world.contains_resource::<RunProgress>() {
        seen.stage = None;
        return;
    }
    if world.get_resource::<crate::editor::EditorState>().is_some_and(|e| e.open) {
        return;
    }
    if seen.stage.as_ref() != Some(&stage) {
        seen.stage = Some(stage.clone());
        seen.frames = 0;
    }
    seen.frames = seen.frames.saturating_add(1);
    // The stage's entities appear a few frames after entry: adopt first.
    let silent = seen.frames < 6;
    let plays = match stage {
        GameStage::Creature => creature(world, seen, silent),
        GameStage::Tribal => tribal(world, seen, silent),
        GameStage::Civilization => civ(world, seen, silent),
        GameStage::Space => space(world, seen, silent),
        GameStage::Cell => cell(world, seen, silent),
        _ => Vec::new(),
    };
    for p in plays {
        info!("cinematic moment: {}", p.name);
        play(world, p);
    }
}

/// Frames the egg fade of a birth waits for the migration decision (the
/// lifecycle counters and the migration start are set by different systems of
/// the same frame or the next).
const BIRTH_WAIT_FRAMES: u8 = 3;

fn creature(world: &mut World, seen: &mut Seen, silent: bool) -> Vec<Play> {
    let (pending, done, deaths, generation) = {
        let run = world.resource::<RunProgress>();
        (
            run.creature_progression.pending_migration.is_some(),
            run.creature_progression.migrations_completed,
            run.creature_lifecycle.deaths,
            run.creature_lifecycle.generation,
        )
    };
    let mut plays = Vec::new();
    if !silent {
        // 0xD40FA0 runs at every birth of the avatar at its nest: after a death
        // (kRestartAtNest, 0xD4379E) and on the return from the editor (0xD44ADF,
        // the hatch of the mating). It starts the migration when
        // `MigrationDNALevels` was crossed (`creature_home`) and then plays
        // `CRG_SpeciesMigrate` (0xD40E20); every other birth plays
        // `CRG_Fade_From_Black_Egg` (0xD4131F). Never both.
        if deaths > seen.deaths || generation > seen.generation {
            seen.birth_wait = BIRTH_WAIT_FRAMES;
        }
        let migrate_now = pending && !seen.migrating;
        if migrate_now {
            seen.birth_wait = 0;
            plays.push(Play::new("CRG_SpeciesMigrate"));
        } else if seen.birth_wait > 0 {
            seen.birth_wait -= 1;
            if seen.birth_wait == 0 {
                plays.push(Play::new("CRG_Fade_From_Black_Egg"));
            }
        }
        // The destination nest is claimed ("CRG_ClaimNest", 0xD41510).
        if done > seen.migrations {
            plays.push(Play::new("CRG_ClaimNest"));
        }
    } else {
        seen.birth_wait = 0;
    }
    seen.migrating = pending;
    seen.migrations = done;
    seen.deaths = deaths;
    seen.generation = generation;
    plays
}

/// `TotemString` (`cinematics_trg` 0x300.. when the tribe was conquered,
/// 0x400.. when it was allied; the piece number picks the line).
fn totem_text(world: &mut World, ally: bool, resolved: usize, village: usize) -> Option<String> {
    let id = if ally { 0x400 } else { 0x300 } + resolved.saturating_sub(1).min(4) as u32;
    let name = crate::tribal_hud::world::village_name(village);
    super::host::localized_by_name(world, "cinematics_trg", id, &[("tribe_name", name)])
}

/// `TRG_Upgrade_Text` (`cinematics_trg` 0x109 / 0x10A: the new hut level).
fn upgrade_text(world: &mut World, level: usize, resolved: usize) -> Option<String> {
    let id = if level >= 2 { 0x10A } else { 0x109 };
    let cap = crate::tribal::population_cap(resolved).to_string();
    super::host::localized_by_name(world, "cinematics_trg", id, &[("max_population", cap)])
}

fn tribal(world: &mut World, seen: &mut Seen, silent: bool) -> Vec<Play> {
    let _layout = crate::tribal_planet::scope_world(world);
    let (villages, level, resolved, births, alive, hut_destroyed) = {
        let run = world.resource::<RunProgress>();
        let c = &run.tribal_campaign;
        (
            c.villages.iter().map(|v| (v.appeared, v.status)).collect::<Vec<_>>(),
            crate::tribal::hut_level(c.resolved_count()),
            c.resolved_count(),
            c.births.len(),
            !run.tribe.is_empty() || !c.births.is_empty() || c.chieftain_respawn <= 0.0,
            c.home_hut_destroyed(),
        )
    };
    let mut plays = Vec::new();
    if !silent && !seen.villages.is_empty() {
        let player = player_entity(world);
        // INFERRED trigger: the first baby of the run appears at the hut. No
        // code site holds the name's address (the PlayCinematic call is not
        // traced); the Tribal handler 0xCD81F0 follows its end (0x44F1189)
        // with a tutorial card, like `TRG_Begin` / `TRG_PRE_FirstTribeAppears`.
        // The script places `Vignettes_Tribe!baby_born` at "NewBaby" and
        // frames "TribeHut".
        if births > seen.births && !played(world, "TRG_FirstBaby") {
            mark_played(world, "TRG_FirstBaby");
            let hut = home_prop(world, crate::tribal_rts::HomePropKind::Hut);
            plays.push(
                Play::new("TRG_FirstBaby")
                    .bind("NewBaby", target_of(world, hut, crate::tribal::home_position())),
            );
        }
        // 0xCD81F0, message 0x5C80207: the player's tribe is wiped out (no
        // member, no baby, the chieftain down waiting for his respawn):
        // `TRG_PRE_PopDestroyed` (pause on) frames "TribeMember" (the one who
        // fell, bound 0xF1A267BD); its last state sends 0x0420E32A (restart).
        if seen.tribe_alive && !alive {
            let at = player.and_then(|p| pos_of(world, p)).unwrap_or_else(crate::tribal::home_position);
            let mut p = Play::new("TRG_PRE_PopDestroyed").bind("TribeMember", target_of(world, player, at));
            p.pause_game = true;
            plays.push(p);
        }
        // 0xCD81F0, message 0x58BADDD (an object destroyed) when it is the
        // player's hut: `TRG_PRE_HutDestroyed` (pause on). The script frames
        // "npcHut" (here the player's hut) and lets "attacker1".."attacker5"
        // celebrate (the raiders at the hut; the script notes they may be
        // missing); its last state sends 0x0420E32A (restart).
        if hut_destroyed && !seen.hut_destroyed {
            let hut = home_prop(world, crate::tribal_rts::HomePropKind::Hut);
            let home = crate::tribal::home_position();
            let mut p = Play::new("TRG_PRE_HutDestroyed").bind("npcHut", target_of(world, hut, home));
            let mut q = world.query_filtered::<(Entity, &Transform), With<crate::tribal::Raider>>();
            let mut near: Vec<(Entity, f32)> = q
                .iter(world)
                .map(|(e, t)| (e, Vec3::new(t.translation.x - home.x, 0.0, t.translation.z - home.z).length()))
                .filter(|(_, d)| *d < 15.0)
                .collect();
            near.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (name, (e, _)) in ["attacker1", "attacker2", "attacker3", "attacker4", "attacker5"].into_iter().zip(near) {
                p = p.bind(name, CinematicTarget::Entity(e));
            }
            p.pause_game = true;
            plays.push(p);
        }
        // The last tribe allied (true) / conquered (false) this frame.
        let mut carved: Option<(bool, usize)> = None;
        for (i, now) in villages.iter().enumerate() {
            let before = seen.villages.get(i).copied().unwrap_or(*now);
            let village = village_entity(world, i);
            let hut = target_of(world, village, crate::tribal::village_position(i));
            if now.0 && !before.0 && !played(world, "TRG_PRE_FirstTribeAppears") {
                // 0xCC686F: the first NPC tribe comes into view.
                mark_played(world, "TRG_PRE_FirstTribeAppears");
                plays.push(
                    Play::new("TRG_PRE_FirstTribeAppears")
                        .bind("npcHut", hut)
                        .bind("npcFire", hut)
                        .bind("Tribe", hut),
                );
            }
            if before.1 == RivalStatus::Hostile && now.1 == RivalStatus::Allied {
                // 0xD9CED0: the alliance moment (new tool and accessory):
                // PlayCinematic("TRG_TotemPole_Social", pause 1, 1, queue 0),
                // "chieftain" (0xCD5F0CA4) = the avatar. Both tribes celebrate
                // (behaviour flag 0x800000, source the allied tribe: bound as
                // "Tribe" for the host, `host::start_continue_clips`); the
                // chieftain's clip sends the continue of `..._Social1`.
                let mut p = Play::new("TRG_TotemPole_Social").bind("Tribe", hut);
                if let Some(player) = player {
                    p = p.bind("chieftain", CinematicTarget::Entity(player));
                }
                p.pause_game = true;
                p.queue = false;
                plays.push(p);
                carved = Some((true, i));
            }
            if before.1 == RivalStatus::Hostile && now.1 == RivalStatus::Conquered {
                // 0xC2F7F0: the conquest moment.
                plays.push(Play::new("TRG_TotemPole_Conquer").bind("npcHut", hut));
                carved = Some((false, i));
            }
        }
        // 0xCDAC90 plays the totem moment after every ally / conquest; at a
        // level threshold the upgrade moment (0xCDA4F0, it contains the totem
        // drop) follows instead. An alliance calls it at once (0xD9CED0, it
        // also stops a conquest's timer); a conquest 60 s later
        // (`TOTEM_AFTER_CONQUEST_SECONDS`).
        if let Some((ally, village)) = carved {
            let totem = totem_text(world, ally, resolved, village);
            let moment = if level > seen.hut_level {
                let upgrade = upgrade_text(world, level, resolved);
                Play::new("TRG_Upgrade").text("TotemString", totem).text("TRG_Upgrade_Text", upgrade)
            } else {
                Play::new("TRG_TotemPole").text("TotemString", totem)
            };
            if ally {
                seen.totem_due = None;
                plays.push(moment);
            } else {
                seen.totem_due = Some(TotemTimer { remaining: TOTEM_AFTER_CONQUEST_SECONDS, play: moment });
            }
        }
    }
    // The conquest's timer (game time: it stands while a cinematic pauses
    // the game); when it has run out, the continue message and the moment.
    if let Some(due) = seen.totem_due.as_mut() {
        let paused = world.get_resource::<Cinematics>().is_some_and(|c| c.pause_mode() != super::PauseMode::None);
        if !paused {
            due.remaining -= world.get_resource::<Time>().map_or(0.0, |t| t.delta_secs());
        }
        if due.remaining <= 0.0 {
            let play = seen.totem_due.take().map(|d| d.play);
            if let Some(mut cin) = world.get_resource_mut::<Cinematics>() {
                cin.continue_cinematic();
            }
            plays.extend(play);
        }
    }
    seen.villages = villages;
    seen.hut_level = level;
    seen.births = births;
    seen.tribe_alive = alive;
    seen.hut_destroyed = hut_destroyed;
    plays
}

fn civ(world: &mut World, seen: &mut Seen, silent: bool) -> Vec<Play> {
    let (attacked, clock, owned, geysers) = {
        let c = &world.resource::<RunProgress>().civ_campaign;
        (
            c.cities
                .iter()
                .map(|city| city.owned() && city.calm < crate::civ::REGEN_DELAY && city.hp > 0.0)
                .collect::<Vec<_>>(),
            c.clock,
            c.owned_count(),
            c.geysers.iter().map(|g| g.owner == Some(crate::civ::HOME_INDEX)).collect::<Vec<_>>(),
        )
    };
    let (owners, captures, launched, kinds) = {
        let c = &world.resource::<RunProgress>().civ_campaign;
        let specialty = |a: crate::civ::Approach| match a {
            crate::civ::Approach::Military => 0usize,
            crate::civ::Approach::Religious => 1,
            crate::civ::Approach::Economic => 2,
        };
        (
            c.cities.iter().map(|city| city.owned()).collect::<Vec<_>>(),
            [c.captures.military, c.captures.religious, c.captures.economic],
            c.superweapon_launched,
            c.cities.iter().map(|city| specialty(city.kind)).collect::<Vec<_>>(),
        )
    };
    let mut plays = Vec::new();
    if !silent {
        // `CVG_UnlockAir` is not played when the tech level rises: VERIFIED
        // (0xCFDCE2 -> 0xCFA220 -> 0xCF9510) it plays after a `CVG_Capture` /
        // `CVG_Capture_Nuked` cinematic has stopped, see `after_capture`.
        // `CVG_UnlockSea` has no play site in the retail code (its name is
        // only compared in the stop branch 0xCFDBBE), so nothing plays it.
        // 0xC00C40: a player city comes under attack (first time).
        for (i, now) in attacked.iter().enumerate() {
            let before = seen.attacked.get(i).copied().unwrap_or(false);
            if *now && !before && !played(world, "CVG_PlayerCityAttacked") {
                mark_played(world, "CVG_PlayerCityAttacked");
                let city = city_entity(world, i);
                let at = city.and_then(|c| pos_of(world, c)).unwrap_or(Vec3::ZERO);
                plays.push(Play::new("CVG_PlayerCityAttacked").bind("AttackedCityHall", target_of(world, city, at)));
            }
        }
        // 0xCF9D00 step 5 (VERIFIED, `notes\civ-flow-notes.md` C2): the vehicle
        // tutorial plays once the four forced steps (rotate, pan, zoom, select
        // a vehicle) are done, only with Tutorials on (the chain returns at
        // option 0x4EA96CB), binding the player's vehicle. It was a 45 s timer.
        let forced_done = world.get_resource::<crate::civ_hud::guide::GuideState>().is_some_and(|g| g.forced_complete());
        let _ = clock;
        if forced_done
            && crate::front_end::settings::tutorials_enabled()
            && !played(world, "CVG_SendVehicleToCommodity")
            && played(world, "TRG2CVG_CivStart")
        {
            mark_played(world, "CVG_SendVehicleToCommodity");
            plays.push(Play::new("CVG_SendVehicleToCommodity"));
        }
        // INFERRED trigger: the first spice geyser the player takes (a derrick
        // built on it). No code site holds the name's address; the script is
        // the tutorial card that frames "SpiceNode".
        for (i, now) in geysers.iter().enumerate() {
            let before = seen.geysers.get(i).copied().unwrap_or(*now);
            if *now && !before && !played(world, "CVG_FirstSpiceNodeCaptured") {
                mark_played(world, "CVG_FirstSpiceNodeCaptured");
                let at = CinematicTarget::Point { position: crate::civ::geyser_position(i), yaw: 0.0 };
                plays.push(Play::new("CVG_FirstSpiceNodeCaptured").bind("SpiceNode", at));
            }
        }
        // 0xCFD500 (message 0x0164B4EB, a city changes hands): the player's
        // civilization has no city left -> `CVG_Lose` (the capital framed as
        // "CityHall" / "Plaza"); its end restarts the stage (0x44F1189 branch:
        // persistence restart 0xB27150, see `handle_ended`).
        if seen.owned_cities > 0 && owned == 0 {
            plays.push(Play::new("CVG_Lose"));
        }
        // cCity::SetOwner 0xBE9430(owner, type, quiet, nuked, _): a city the
        // player's civilization takes in Civ mode (not quiet, not
        // mbSkipCaptureCinematic) plays `CVG_Capture`, or `CVG_Capture_Nuked`
        // when the capture came with the nuked flag (pause on, not queued:
        // of several cities taken at once only the first plays).
        for (i, now) in owners.iter().enumerate() {
            let before = seen.owners.get(i).copied().unwrap_or(*now);
            if !*now || before || i == crate::civ::HOME_INDEX || seen.owners.is_empty() {
                continue;
            }
            // The capture type of the counter that rose (0 military, 1
            // religious / conversion 0xBEAB80, 2 economic / purchase 0xBE99A0).
            let kind = (0..3).find(|k| captures[*k] > seen.captures[*k]).unwrap_or(0);
            // INFERRED: the nuked flag (0xBE9AD0's argument, from the damage
            // path 0xBE9C70) is the ICBM superweapon's military capture.
            let nuked = kind == 0 && launched && !seen.superweapon;
            let specialty = kinds.get(i).copied().unwrap_or(0);
            let id = capture_text_id(world, kind, specialty);
            let text = super::host::localized_by_name(world, "cinematics_cvg", id, &[]);
            let city = city_entity(world, i);
            let at = city.and_then(|c| pos_of(world, c)).unwrap_or(Vec3::ZERO);
            let target = target_of(world, city, at);
            // 0xBE94CD: the civilization remembers the newest city (+0x44C);
            // the air unlock after the capture cinematic frames it.
            LAST_CAPTURED.store(i, Ordering::Relaxed);
            let mut p = Play::new(if nuked { "CVG_Capture_Nuked" } else { "CVG_Capture" })
                .bind("CityHall", target)
                .bind("CityPlaza", target)
                .text("CaptureString", text);
            p.pause_game = true;
            p.queue = false;
            plays.push(p);
        }
    }
    seen.attacked = attacked;
    seen.owned_cities = owned;
    seen.geysers = geysers;
    seen.owners = owners;
    seen.captures = captures;
    seen.superweapon = launched;
    plays
}

/// The `CaptureString` locale id of 0xBE9430 (`cinematics_cvg`): the capture
/// type matches the city's specialty -> 0x803 + type; otherwise by the
/// specialty, with the first capture of a city of that specialty (profile
/// flags 0x46BDDAA / 0x46BDDDC / 0x46BDDE0, set then; here one-shot flags of
/// the run) picking the 0x900 line instead of the later one. `kind` and
/// `specialty`: 0 military, 1 religious, 2 economic (INFERRED: the same order
/// for `mVehicleSpecialty` as for the capture type).
fn capture_text_id(world: &mut World, kind: usize, specialty: usize) -> u32 {
    let flag = ["CVG_FirstCapture_Military", "CVG_FirstCapture_Religious", "CVG_FirstCapture_Economic"]
        [specialty.min(2)];
    let first = !played(world, flag);
    if first {
        mark_played(world, flag);
    }
    capture_text(kind, specialty, first)
}

/// The id table of 0xBE9430 (see `capture_text_id`).
pub fn capture_text(kind: usize, specialty: usize, first: bool) -> u32 {
    let k = kind as u32;
    if kind == specialty {
        return 0x803 + k;
    }
    match (specialty, first) {
        (0, true) => 0x902 + 2 * u32::from(k != 1),
        (0, false) => 0x908 + 8 * u32::from(k != 1),
        (1, true) => 0x900 + if k != 0 { 5 } else { 0 },
        (1, false) => 0x906 + if k != 0 { 0xB } else { 0 },
        (_, true) => 0x901 + 2 * u32::from(k != 0),
        (_, false) => 0x907 + 2 * u32::from(k != 0),
    }
}

/// What the Space hint detector compares between frames.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpaceSeen {
    system: u32,
    empire: Option<crate::space_worldgen::EmpireId>,
    at_war: bool,
    spice: u32,
    colonies: usize,
    routes: usize,
    uplifted: u32,
    /// Promotions (`badgecollector-1..10` earned).
    rank: u8,
    /// The core limits the drive's reach here (0xFFDA50).
    near_core: bool,
    /// The system's empire would accept an alliance (relationship at
    /// `createAllianceBase` 7 or more, not allied).
    alliance_possible: bool,
    /// A biosphere collapse is under way in this system.
    biosphere_collapse: bool,
    /// The world under the planet view (planet entry: 0x10003C0, 0xFDFBB0,
    /// 0xC600E0).
    planet: Option<u32>,
    /// The context: 0 galaxy, 1 solar system, 2 planet (0x102D1C0 / 0x102DF30
    /// pick the surrender cinematic by it).
    view: u8,
    /// Systems bought (0x102D1C0) and worlds taken by force (0x102DF30).
    purchased: u32,
    captured: usize,
    /// The comm screen is open (the surrender cinematic is handed to the comm
    /// manager, 0xAEA1B0, and plays after it).
    comm: bool,
    /// Story 201 while it runs: (target star, wreck world, objectives done).
    story: Option<(Option<u32>, Option<u32>, u32)>,
    /// The system is a SETI star (0xBBA120: another empire's star; the
    /// installed false-positive / -negative chances are 0).
    seti: bool,
    /// The colonize tutorial mission's target planet while it runs.
    colonize_target: Option<u32>,
}

fn space_seen(world: &World) -> SpaceSeen {
    let c = &world.resource::<RunProgress>().space_campaign;
    let view = world.get_resource::<crate::space_view::SpaceView>();
    let planet = view.and_then(|v| v.planet.as_ref().map(|p| p.world));
    SpaceSeen {
        planet,
        view: match view.map(|v| v.zoom()) {
            Some(crate::space_view::SpaceZoom::Galaxy) => 0,
            _ if planet.is_some() => 2,
            _ => 1,
        },
        purchased: c.endgame.systems_purchased,
        captured: c.conquered_worlds.len().saturating_sub(c.endgame.purchased_worlds as usize),
        comm: world.get_resource::<crate::space_comm::CommSession>().is_some_and(|s| s.open),
        story: (c.tutorial.step == crate::space_rules::TutorialStep::Story201)
            .then_some((c.tutorial.story_star, c.tutorial.story_wreck, c.tutorial.progress)),
        seti: {
            let player_star = c.current_system == crate::space_galaxy::HOME_STAR
                || c.colonies.iter().any(|w| crate::space_galaxy::system_of_world(*w) == c.current_system);
            // The one-time roll of 0xBBA020 (a hash of the star stands in for
            // the RNG draw kept in the star flags).
            let roll = (c.current_system.wrapping_mul(0x9E37_79B9) >> 8) as f32 / (1u32 << 24) as f32;
            crate::space_rules::seti_star(c.current_system_empire.is_some(), player_star, roll)
        },
        system: c.current_system,
        empire: c.current_system_empire,
        at_war: c.current_system_empire.is_some_and(|e| c.wars.contains(&e)),
        spice: c.cargo_spice.values().map(|n| u32::from(*n)).sum(),
        colonies: c.colonies.len(),
        routes: c.trade_routes.len(),
        uplifted: c.species_uplifted,
        rank: c.captain_rank(),
        near_core: world
            .get_resource::<crate::space_galaxy::Galaxy>()
            .is_some_and(|g| crate::space_endgame::core_limits_reach(c, g)),
        alliance_possible: c.current_system_empire.is_some_and(|e| {
            e != 0 && !c.allies.contains(&e) && c.relationship(e) >= crate::space_rules::CREATE_ALLIANCE_THRESHOLD
        }),
        biosphere_collapse: c.active_distress.is_some_and(|d| {
            d.kind == crate::space::DistressKind::BiohazardOutbreak && d.system_id == c.current_system
        }),
        colonize_target: (c.tutorial.step == crate::space_rules::TutorialStep::TeachColonize)
            .then_some(c.tutorial.colonize_target)
            .flatten(),
    }
}

/// The Space hint player 0x1041C60 (name, player flag, force): a hint plays
/// once per game (`RunProgress::cinematics_played` stands in for the flag,
/// set before playing); without `force` it waits while a cinematic starts or
/// runs (flag untouched); with `force` it is set even when the PlayCinematic
/// (pause on, not queued) is refused because another cinematic runs.
fn space_hint(world: &mut World, name: &str, force: bool) -> Option<Play> {
    if played(world, name) {
        return None;
    }
    let busy = world.get_resource::<Cinematics>().is_some_and(|c| c.is_playing() || c.runtime.queued() > 0);
    if busy && !force {
        return None;
    }
    mark_played(world, name);
    if busy {
        return None;
    }
    let mut p = Play::new(name);
    p.pause_game = true;
    Some(p)
}

/// The queued Space hint 0x100A660 (name, flag): returns when the player
/// flag is set, else the hint waits in the list until no cinematic plays.
/// Here: marked played at once and queued behind a running cinematic
/// (ADAPTED: the SpaceTuning 0x80ACC889 delay of 1.5 s is not kept).
fn space_hint_queued(world: &mut World, name: &str) -> Option<Play> {
    if played(world, name) {
        return None;
    }
    mark_played(world, name);
    let mut p = Play::new(name);
    p.pause_game = true;
    p.queue = true;
    Some(p)
}

/// Space: the cinematic the rules module left in `pending_cinematic`
/// (`SPG_PlayerDestroyed_*`, `SPG_FlightSchool101Complete`,
/// `SPG_CenterOfGalaxy`), taken once; and the hint cinematics at the retail
/// moments the campaign shows (see `notes\cinematics-re-notes.md` 8).
fn space(world: &mut World, seen: &mut Seen, silent: bool) -> Vec<Play> {
    let name = world.get_resource_mut::<RunProgress>().and_then(|mut r| r.space_campaign.pending_cinematic.take());
    let mut plays = match name {
        Some(name) if world.get_resource::<Cinematics>().is_some_and(|c| c.runtime.has_state(&name)) => {
            // After the destruction the UFO is rebuilt at the home city: the
            // shortened launch cinematic (0xC3C680 -> 0xFF5D90).
            let rebuilt = name.starts_with("SPG_PlayerDestroyed");
            // 0x1002DD0: PlayCinematic("SPG_CenterOfGalaxy", pause = 1).
            let pause = name == crate::space_rules::CINEMATIC_CENTER_OF_GALAXY;
            let mut first = Play::new(name);
            first.pause_game = pause;
            let mut plays = vec![first];
            if rebuilt {
                plays.push(Play::new("CVG2SPG_short"));
            }
            plays
        }
        _ => Vec::new(),
    };
    let mut now = space_seen(world);
    if let (true, Some(b)) = (now.comm, seen.space.as_ref()) {
        // A surrender agreed on the comm screen plays once it is closed.
        now.purchased = b.purchased;
        now.captured = b.captured;
    }
    let before = seen.space.replace(now.clone());
    let Some(before) = before.filter(|_| !silent) else { return plays };
    let entered = now.system != before.system;
    plays.extend(space_planet_hints(world, &before, &now));
    // 0xFDADD0 (system entry): a system of another empire (not the Grox,
    // whose meeting is `SPG_GrobFirstEncounter`, 0xFDFBB0): first contact
    // (flag 0xDAB99813, forced). ADAPTED: the interceptor the retail code
    // sends (0x102AC60 type 8) is not spawned; "npcufo" stays unbound.
    if entered && now.empire.is_some_and(|e| e != 0) {
        plays.extend(space_hint(world, "SPG_FirstContact_Intercept", true));
    }
    // 0xFDADD0: the signal of a SETI star without an empire (a false
    // positive) -> `SPG_SetiAmbush` (flag 0x05B1A673, forced) and the ambush
    // 0x1009690; never with the installed SETI tool (chance 0.0; the script
    // calls it "currently disabled in code").
    if entered && now.seti && now.empire.is_none() {
        plays.extend(space_hint(world, "SPG_SetiAmbush", true));
    }
    // 0x10087B0: the first entry into the territory of an empire at war
    // with the player (flag 0x5079F9FA, forced).
    if (entered || !before.at_war) && now.at_war {
        plays.extend(space_hint(world, "SPG_FirstEntryEnemyTerritory", true));
    }
    // 0xFFFEA0: spice picked up (flag 0x338E034E, forced).
    if now.spice > before.spice {
        plays.extend(space_hint(world, "SPG_FirstSpicePickup", true));
    }
    // 0x1051860, message 0x653A95B: a colony placed (flag 0x9F3A466, forced;
    // the retail code also requires the colonize tutorial mission
    // 0x398ACA5E on the active planet: ADAPTED, the first colony). The
    // script frames "planetposition".
    if now.colonies > before.colonies {
        plays.extend(space_hint(world, "SPG_ColonizeMissionComplete", true).map(|p| {
            let at = player_entity(world);
            let here = at.and_then(|e| pos_of(world, e)).unwrap_or(Vec3::ZERO);
            p.bind("planetposition", target_of(world, at, here))
        }));
    }
    // 0xC4E2D0 (the colonize mission's context event, VERIFIED): the mission
    // running (state 3) with a target planet: arriving at the target's star
    // (context 3) -> `SPG_ColonizeMissionSolar` (flag 0x45A6EFC2, forced);
    // entering the target planet (context 1) -> `SPG_ColonizeMissionPlanet`
    // (flag 0xB1A88FD1, forced).
    if let Some(target) = now.colonize_target {
        if entered && now.system == crate::space_galaxy::system_of_world(target) {
            plays.extend(space_hint(world, "SPG_ColonizeMissionSolar", true));
        }
        if now.planet == Some(target) && before.planet != Some(target) {
            plays.extend(space_hint(world, "SPG_ColonizeMissionPlanet", true));
        }
    }
    // INFERRED: the first trade route (0xC626D0 holds the name; trigger not
    // decompiled).
    if now.routes > before.routes {
        plays.extend(space_hint(world, "SPG_TutorialTradeRoute", true));
    }
    // 0x1002040 (arrival) -> 0xFFDA50: the core limits the drive here
    // (flag 0x05F776D6, not forced).
    if entered && now.near_core {
        plays.extend(space_hint(world, "SPG_TravelNearCore", false));
    }
    // 0xFDFBB0: the first meeting with the Grox (flag 0x04AC8010).
    // INFERRED trigger: entering a Grox system.
    if entered && now.empire == Some(0) {
        plays.extend(space_hint(world, "SPG_GrobFirstEncounter", true));
    }
    // 0xFDADD0 (system entry): an empire not allied whose alliance chance
    // (0x1040830) is 1, i.e. relationship >= createAllianceBase (flag
    // 0xF317692A, forced).
    if entered && now.alliance_possible {
        plays.extend(space_hint(world, "SPG_FirstAlliancePossible", true));
    }
    // 0xFDFBB0 -> 0xFDDA00: in the system of a biosphere-collapse event
    // (mission 0x311CC10F; flag 0x04922E1B).
    if now.biosphere_collapse && (entered || !before.biosphere_collapse) {
        plays.extend(space_hint(world, "SPG_BiosphereCollapse", true));
    }
    // 0xFE58B0: every promotion badge plays `SPG_MasterBadge` (pause 1) with
    // the rank's line as "MasterBadgeText" (0xFE3D20: cinematics_spg
    // 0x063BB6D0..).
    if now.rank > before.rank && now.rank > 0 {
        let id = crate::space_rules::MASTER_BADGE_TEXT[(now.rank as usize - 1).min(9)];
        let text = super::host::localized_by_name(world, "cinematics_spg", id, &[]);
        let mut p = Play::new(crate::space_rules::CINEMATIC_MASTER_BADGE).text(crate::space_rules::MASTER_BADGE_TEXT_KEY, text);
        p.pause_game = true;
        plays.push(p);
    }
    // 0x1051860: a species uplifted plays `SPG_CreatureTribeUplift` every
    // time (PlayCinematic(name, 1, ...), no flag). ADAPTED: the monolith is
    // not an object here; the UFO stands in for "monolith".
    if now.uplifted > before.uplifted {
        let mut p = Play::new("SPG_CreatureTribeUplift");
        p.pause_game = true;
        if let Some(e) = player_entity(world) {
            p = p.bind("monolith", CinematicTarget::Entity(e));
        }
        plays.push(p);
    }
    // 0xFFF540 (system entry): a SETI star -> `SPG_SETI` (flag 0x05B15124,
    // not forced: it waits while another cinematic starts). ADAPTED: the
    // 0x5F4A3CDB binding is not made.
    if entered && now.seti && plays.is_empty() {
        plays.extend(space_hint(world, "SPG_SETI", false));
    }
    plays
}

/// The planet entity of a campaign world (the scripts' "planet").
fn planet_target(world: &mut World, id: u32) -> Option<CinematicTarget> {
    let mut q = world.query::<(Entity, &crate::persistence::WorldId, &crate::game::Planet)>();
    let e = q.iter(world).find(|(_, w, _)| w.0 == id).map(|(e, _, _)| e)?;
    Some(CinematicTarget::Entity(e))
}

fn with_planet(world: &mut World, mut p: Play, id: Option<u32>) -> Play {
    if let Some(t) = id.and_then(|id| planet_target(world, id)) {
        p = p.bind("planet", t);
    }
    p
}

/// The Space hints of planet entry, Story 201 and system surrender (helper
/// `space-final`, `notes\space-final-notes.md` F1; VERIFIED call sites, the
/// OpenSpore state they read is listed per hint).
fn space_planet_hints(world: &mut World, before: &SpaceSeen, now: &SpaceSeen) -> Vec<Play> {
    let mut plays = Vec::new();
    let entered_planet = now.planet.filter(|p| before.planet != Some(*p));
    if let Some(id) = entered_planet {
        let info = crate::space_galaxy::world_info(id);
        let empty_system = world
            .get_resource::<crate::space_galaxy::Galaxy>()
            .zip(info.as_ref())
            .is_some_and(|(g, i)| g.system(i.system).tech <= crate::space_worldgen::TechLevel::Creature);
        let rare_close_seen = played(world, "SPG_RareClose");
        let (has_rare, detector, owned, war_planet, has_tool) = {
            let mut run = world.resource_mut::<RunProgress>();
            let c = &mut run.space_campaign;
            // 0xBB9EB0: the rare object still lies on the planet.
            let has_rare = crate::space::world_artifact(id).is_some() && !c.collected_artifacts.contains(&id);
            // A tool artifact (item type 2) lies on the planet.
            let has_tool = crate::space_tool_artifacts::lying_on(c, id);
            // 0xBBA160: false positives only on a creature planet (tech 1) of
            // a star that is not an Empire star.
            let creature_planet = empty_system
                && c.current_system_empire.is_none()
                && info.as_ref().is_some_and(|i| i.tech == crate::space_worldgen::TechLevel::Creature);
            let detector = c.rares.detector(id, has_rare, creature_planet, rare_close_seen);
            let owned = c.colonies.contains(&id) || c.current_system_empire.is_some();
            let war_planet = c.world_empire(id).is_some_and(|e| c.wars.contains(&e));
            (has_rare, detector, owned, war_planet, has_tool)
        };
        // 0x10003C0 (planet entry): the rare detector reads a rare here
        // (0xBBA270) -> `SPG_RareDetector` (flag 0x05B1C895, not forced; binds
        // 0x5F848353 "planet").
        if detector {
            if let Some(p) = space_hint(world, "SPG_RareDetector", false) {
                plays.push(with_planet(world, p, Some(id)));
            }
        }
        // 0xFDFBB0: a false positive (detector on, no rare) -> queued
        // `SPG_RareAmbush` (flag 0x05B15554), the ambush 0x10096B0 (pirates at
        // the world, `space_universe::spawn_raiders`) and the reading reset
        // (0xBB9CC0). A rare object on the planet -> queued `SPG_RareClose`
        // (flag 0x04AC800C).
        if detector && !has_rare {
            plays.extend(space_hint_queued(world, "SPG_RareAmbush"));
            let mut run = world.resource_mut::<RunProgress>();
            run.space_campaign.rares.ambush = Some(id);
            run.space_campaign.rares.reset_detector(id);
        }
        if has_rare {
            plays.extend(space_hint_queued(world, "SPG_RareClose"));
        }
        // 0xFDFBB0, the same artifact loop: a tool artifact (type 2) ->
        // queued `SPG_TerraformToolClose` (flag 0xE21E2660; binds 0x5DCE504A
        // to the artifact, ADAPTED: OpenSpore's tools lie on the world, so no
        // position is bound).
        if has_tool {
            plays.extend(space_hint_queued(world, "SPG_TerraformToolClose"));
        }
        // 0xC63940: the war mission's planet entered -> queued
        // `SPG_WarEnteredPlanet` (once per mission, +0x1C8). ADAPTED: OpenSpore
        // has no war missions; a planet of an empire at war with the player
        // stands for the mission's planet.
        if war_planet {
            plays.extend(space_hint_queued(world, "SPG_WarEnteredPlanet"));
        }
        // 0xFDFBB0 (planet hints, queued through 0x100A660): in a system whose
        // star and planet have creature-level life only (0xC8B730 / 0xC70FA0
        // == 1): T-score 0 -> `SPG_FirstBarrenPlanetInEmptySystem` (flag
        // 0x05B18A93), else a creature near the UFO (0xFDFB00) ->
        // `SPG_FirstWildPlanetInEmptySystem` (flag 0x05B18A82, binds it).
        // ADAPTED: a T1+ world of such a system stands in for the creature
        // found; the queue's delay (SpaceTuning 0x80ACC889 = 1.5 s) is not
        // kept; 0xFDFBB0 is skipped between first contact (flag 0x05F1F0BC)
        // and 0x05F1F0CA, which OpenSpore does not model.
        if let (true, false, Some(info)) = (empty_system, owned, info.as_ref()) {
            let name = if info.t_score == 0 {
                "SPG_FirstBarrenPlanetInEmptySystem"
            } else {
                "SPG_FirstWildPlanetInEmptySystem"
            };
            plays.extend(space_hint(world, name, true));
        }
    }
    // Story 201 (0xC600E0 planet entry, 0xC602B0 scans; flags 0x04922E1E..21).
    if let Some((star, wreck, _)) = now.story {
        let c = &world.resource::<RunProgress>().space_campaign;
        let at_star = star.is_some() && star == Some(c.current_system);
        if let (Some(id), true) = (entered_planet, at_star) {
            // Mission state 0 -> 1 on the wreck planet, 2 -> 3 on the ruin's.
            let name = match wreck {
                None => Some("SPG_Story201EnteredUFOWreckPlanet"),
                Some(w) if w != id => Some("SPG_Story201EnteredRuinPlanet"),
                _ => None,
            };
            if let Some(p) = name.and_then(|n| space_hint(world, n, true)) {
                plays.push(with_planet(world, p, Some(id)));
            }
        }
        // The wreck's scan (state 1 -> 2, forced).
        if wreck.is_some() && before.story.is_some_and(|(_, w, _)| w.is_none()) {
            plays.extend(space_hint(world, "SPG_Story201ScannedUFOWreck", true));
        }
    } else if before.story.is_some_and(|(_, w, _)| w.is_some()) {
        // The ruin's scan finished the mission (state 3, forced).
        plays.extend(space_hint(world, "SPG_Story201ScannedRuin", true));
    }
    // 0x102D1C0 (system bought) / 0x102DF30 (system taken): the surrender
    // cinematic of the context, handed to the comm manager (0xAEA1B0):
    // planet -> `_Planet` (binds the nearest city), solar system -> `_Solar`
    // (binds 0x5F848353 "planet", a planet of the system); none in the
    // galaxy view. Not one-shot (no profile flag).
    let surrender = if now.purchased > before.purchased {
        Some("Diplomatic")
    } else if now.captured > before.captured {
        Some("Military")
    } else {
        None
    };
    if let (Some(kind), 1 | 2) = (surrender, now.view) {
        let context = if now.view == 2 { "Planet" } else { "Solar" };
        let mut p = Play::new(format!("SPG_SystemSurrender_{kind}_{context}"));
        p.pause_game = true;
        let id = now.planet.or_else(|| world.resource::<RunProgress>().space_campaign.conquered_worlds.last().copied());
        if let Some(t) = id.and_then(|id| planet_target(world, id)) {
            p = p.bind("planet", t).bind("nearestcity", t);
        }
        plays.push(p);
    }
    plays
}

/// Cell: the part-guarantee cinematic (`goldies_demise`, RE 10A).
fn cell(world: &mut World, seen: &mut Seen, silent: bool) -> Vec<Play> {
    let now = world.resource::<RunProgress>().cell.part_cinematic_played;
    let mut plays = Vec::new();
    if !silent && now && !seen.part_cinematic {
        plays.push(Play::new("goldies_demise"));
    }
    seen.part_cinematic = now;
    plays
}

// ---------------------------------------------------------------------------
// The transition gate
// ---------------------------------------------------------------------------

/// The cinematics a stage change is wrapped in (the retail Evolve / Advance
/// handlers), in order. The Creature -> Tribe change is held by
/// `creature_flow` (the timeline follows its cinematic).
pub fn transition_chain(from: &GameStage, to: &GameStage) -> &'static [&'static str] {
    match (from, to) {
        // 0xCDBEA0 (Evolve): TRG2CVG_PreEditor; 0xCDD0F0: the city hall card.
        (GameStage::Tribal, GameStage::Civilization) => &["TRG2CVG_PreEditor", "TRG2CVG_ShowCityHall"],
        // 0xCF82C0 (Advance): CVG_UnlockUFO.
        (GameStage::Civilization, GameStage::Space) => &["CVG_UnlockUFO"],
        _ => &[],
    }
}

#[derive(Default)]
pub struct Gate {
    held: Option<GameStage>,
    queue: VecDeque<String>,
    /// A cinematic of the chain has started (it is playing or has ended).
    started: bool,
    frames: u32,
    /// The target released by the gate: let its change through once.
    released: Option<GameStage>,
}

/// Hold a stage change while its cinematics play (see the module docs). Runs
/// in `Last`, after every system that may have set `NextState`.
pub fn gate(world: &mut World, mut gate: Local<Gate>) {
    let on = world.get_resource::<Cinematics>().is_some_and(|c| c.settings.hooks && c.runtime.enabled && c.loaded());
    let Some(stage) = stage_of(world) else { return };
    let pending = match world.get_resource::<NextState<GameStage>>() {
        Some(NextState::Pending(s)) | Some(NextState::PendingIfDifferent(s)) => Some(s.clone()),
        _ => None,
    };
    if !on {
        gate.held = None;
        gate.queue.clear();
        return;
    }
    if let Some(held) = gate.held.clone() {
        // Another change (menu, quit) overrides the hold.
        if pending.as_ref().is_some_and(|p| *p != held) {
            gate.held = None;
            gate.queue.clear();
            return;
        }
        gate.frames += 1;
        let playing = world.get_resource::<Cinematics>().is_some_and(|c| c.is_playing());
        if playing {
            gate.started = true;
            return;
        }
        // Wait for the cinematic to start (a few frames), then it ended.
        if !gate.started && gate.frames < 5 {
            return;
        }
        if let Some(next) = gate.queue.pop_front() {
            gate.started = false;
            gate.frames = 0;
            if !play(world, Play::new(next)) {
                // Not playable: skip it.
                gate.started = true;
            }
            return;
        }
        // The chain is over: let the change through.
        gate.held = None;
        gate.released = Some(held.clone());
        if let Some(mut next) = world.get_resource_mut::<NextState<GameStage>>() {
            next.set(held);
        }
        return;
    }
    let Some(target) = pending else {
        gate.released = None;
        return;
    };
    if gate.released.as_ref() == Some(&target) {
        gate.released = None;
        return;
    }
    let chain = transition_chain(&stage, &target);
    if chain.is_empty() {
        return;
    }
    if let Some(mut next) = world.get_resource_mut::<NextState<GameStage>>() {
        *next = NextState::Unchanged;
    }
    info!("cinematics hold the change {stage:?} -> {target:?}: {chain:?}");
    gate.held = Some(target);
    gate.queue = chain.iter().map(|s| s.to_string()).collect();
    gate.started = true;
    gate.frames = 0;
}

// ---------------------------------------------------------------------------
// Game messages of the scripts
// ---------------------------------------------------------------------------

/// `sendMessage` ids the stages react to.
fn handle_messages(world: &mut World, cursor: &mut MessageCursor<CinematicMessage>, laying_idle: &mut u32) {
    let ids: Vec<u32> = match world.get_resource::<Messages<CinematicMessage>>() {
        Some(m) => cursor.read(m).map(|m| m.id).collect(),
        None => return,
    };
    for id in ids {
        match id {
            message::ENTER_CREATURE_EDITOR => {
                LAYING.store(false, Ordering::Relaxed);
                if stage_of(world) == Some(GameStage::Creature) {
                    if let Some(mut editor) = world.get_resource_mut::<crate::editor::EditorState>() {
                        editor.show();
                        editor.visuals_dirty = true;
                    }
                }
            }
            message::RESTART_AT_NEST => DEATH.store(DEATH_RESTART, Ordering::Relaxed),
            message::HATCH_AVATAR_EGGS => hatch_avatar_eggs(world),
            message::UPGRADE_NEST => upgrade_nest(world),
            message::RESTART_TRIBE => restart_tribe(world),
            // kCinematicMessageNewCityAppears: the city is placed now; its
            // creation (0xB999F0) answers with the continue (`await_new_city`).
            crate::civ_reveal::NEW_CITY_MESSAGE => {
                if let Some(index) = crate::civ_reveal::reveal_from_cinematic(world) {
                    NEW_CITY.store(index, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }
    // `CRG_LaidEgg` that ended without opening the editor (it failed to
    // start) must not leave the egg unlaid.
    if LAYING.load(Ordering::Relaxed) {
        let busy = world.get_resource::<Cinematics>().is_some_and(|c| c.is_playing() || c.runtime.queued() > 0)
            || REQUESTS.lock().is_ok_and(|q| !q.is_empty());
        *laying_idle = if busy { 0 } else { *laying_idle + 1 };
        if *laying_idle > 60 {
            *laying_idle = 0;
            LAYING.store(false, Ordering::Relaxed);
            if let Some(mut editor) = world.get_resource_mut::<crate::editor::EditorState>() {
                editor.show();
                editor.visuals_dirty = true;
            }
        }
    }
    // A death cinematic that ended without its restart message (it failed to
    // start, or was cut short) must not keep the avatar down.
    if DEATH.load(Ordering::Relaxed) == DEATH_PLAYING {
        let busy = world.get_resource::<Cinematics>().is_some_and(|c| c.is_playing() || c.runtime.queued() > 0)
            || REQUESTS.lock().is_ok_and(|q| !q.is_empty());
        if !busy {
            DEATH.store(DEATH_RESTART, Ordering::Relaxed);
        }
    }
}

/// `kHatchAvatarEggs` (0xD43840): every creature of the avatar's nest that is
/// still an egg (state 5 at +0x1E8) gets the hatch command (+0x1F0 = 1);
/// `CRG_SpeciesMigrate` had parked them (0xD40E20 sets +0x1F0 to 0x7FFFFFFF).
/// ADAPTED: the nest mates are copies of the avatar that are always standing,
/// so the message only puts them round the nest the avatar belongs to now
/// (`creature_nestmates` does that when the nest moves); a counter records
/// it for the tests.
fn hatch_avatar_eggs(_world: &mut World) {
    HATCHES.fetch_add(1, Ordering::Relaxed);
    info!("cinematic message: kHatchAvatarEggs");
}

/// `kUpgradeNest` (0xD438AC): the claimed nest's model is swapped for the
/// brain level's (0xC69D50 flags the nest object for the model update): the
/// home nest is drawn again with `Creature Nest - Brain Level 0..2`
/// (`creature_home::upgrade_model`). ADAPTED: the level -> model rule is the
/// level indexing the three props (retail picks by each nest's own range).
fn upgrade_nest(world: &mut World) {
    let _ = &world;
    UPGRADES.fetch_add(1, Ordering::Relaxed);
    crate::creature_home::request_upgrade();
    info!("cinematic message: kUpgradeNest");
}

static HATCHES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static UPGRADES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// How many `kHatchAvatarEggs` / `kUpgradeNest` messages the scripts sent
/// (tests, diagnostics).
pub fn nest_messages() -> (u32, u32) {
    (HATCHES.load(Ordering::Relaxed), UPGRADES.load(Ordering::Relaxed))
}

/// The ends of cinematics the stage code answers (retail listeners of
/// 0x44F1189 compare the token with the cinematic's name).
fn handle_ended(world: &mut World, cursor: &mut MessageCursor<CinematicEnded>) {
    let ended: Vec<(String, bool)> = match world.get_resource::<Messages<CinematicEnded>>() {
        Some(m) => cursor.read(m).map(|m| (m.name.clone(), m.skipped)).collect(),
        None => return,
    };
    for (name, skipped) in ended {
        // The hold cinematics' stage code listens for the player's skip.
        if skipped && name == "CLG_intro" {
            if let Ok(mut s) = SKIPPED.lock() {
                s.push(name.clone());
            }
        }
        // A `CVG_PRE_NewCityAppears` that ended before its message (refused,
        // escaped early) must not hold the city back.
        if name == crate::civ_reveal::NEW_CITY_APPEARS {
            let pending = world.get_resource::<RunProgress>().is_some_and(|r| r.civ_campaign.reveal_pending);
            if pending {
                crate::civ_reveal::reveal_from_cinematic(world);
                if let Some(mut run) = world.get_resource_mut::<RunProgress>() {
                    run.civ_campaign.reveal_pending = false;
                }
            }
        }
        // 0xCFD500, 0x44F1189 branch: after `CVG_Lose` the stage restarts.
        if name == "CVG_Lose" {
            restart_civ(world);
        }
        // 0xCFDCE2: after a capture cinematic the air unlock (0xCFA220).
        if matches!(name.as_str(), "CVG_Capture" | "CVG_Capture_Nuked") {
            after_capture(world);
        }
    }
}

/// The newest city the player took (retail `cCivilization` +0x44C, set by
/// `cCity::SetOwner`); usize::MAX: none yet.
static LAST_CAPTURED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

/// VERIFIED (0xCFA220): `CVG_UnlockAir` is due when the vehicle tech level is
/// 3 or more and the profile flag 0x050A2E65 (here: it played in this run) is
/// not set.
pub fn unlock_air_due(tech_level: u8, already_played: bool) -> bool {
    crate::civ_rules::air_unlocked(tech_level) && !already_played
}

/// The stop branch of `CVG_Capture` / `CVG_Capture_Nuked` (0xCFDCE2): the air
/// unlock cinematic (0xCF9510) binds the newest captured city as "CityHall"
/// and plays with the game paused. (The flag is set when it starts.)
fn after_capture(world: &mut World) {
    let tech = world.get_resource::<RunProgress>().map_or(1, |r| r.civ_campaign.vehicle_tech_level());
    if !unlock_air_due(tech, played(world, "CVG_UnlockAir")) {
        return;
    }
    let index = LAST_CAPTURED.load(Ordering::Relaxed);
    let city = if index == usize::MAX { None } else { city_entity(world, index) };
    let at = city.and_then(|c| pos_of(world, c)).unwrap_or(Vec3::ZERO);
    let mut p = Play::new("CVG_UnlockAir").bind("CityHall", target_of(world, city, at));
    p.pause_game = true;
    if play(world, p) {
        mark_played(world, "CVG_UnlockAir");
    }
}

/// Answer "restart tribe" (0x0420E32A): the persistence manager's restart
/// (VERIFIED 0xB27150: the planet is loaded again from its last save) =
/// `stage_restart::reload` of the stage autosave. Without one (ADAPTED, the
/// earlier stand-in): the tribe is refilled to `tribePopulationStart`
/// members, the chieftain and the hut are back, and the Tribal stage is
/// entered again from the run.
pub fn restart_tribe(world: &mut World) {
    if stage_of(world) != Some(GameStage::Tribal) {
        return;
    }
    if crate::stage_restart::reload(world, GameStage::Tribal) {
        return;
    }
    if let Some(mut run) = world.get_resource_mut::<RunProgress>() {
        run.tribal_campaign.home_hut_damage = 0.0;
        let mut tribe = std::mem::take(&mut run.tribe);
        tribe.resize(tribe.len().max(crate::tribal_rules::TRIBE_POPULATION_START), Default::default());
        run.tribal_campaign.births.clear();
        run.tribal_campaign.chieftain_respawn = 0.0;
        run.tribal_campaign.sync_roster(&mut tribe);
        run.tribe = tribe;
        run.request_save();
    }
    if let Some(mut next) = world.get_resource_mut::<NextState<GameStage>>() {
        next.set(GameStage::Tribal);
    }
}

/// Answer the end of `CVG_Lose` (VERIFIED: the persistence restart 0xB27150,
/// `stage_restart::reload`). Without an autosave (ADAPTED): the capital
/// returns to the player at full health and the Civ stage is entered again
/// from the run.
pub fn restart_civ(world: &mut World) {
    if stage_of(world) != Some(GameStage::Civilization) {
        return;
    }
    if crate::stage_restart::reload(world, GameStage::Civilization) {
        return;
    }
    if let Some(mut run) = world.get_resource_mut::<RunProgress>() {
        if let Some(city) = run.civ_campaign.cities.get_mut(crate::civ::HOME_INDEX) {
            city.owner = crate::civ::CityOwner::Player;
            city.hp = city.max_hp();
        }
        run.request_save();
    }
    if let Some(mut next) = world.get_resource_mut::<NextState<GameStage>>() {
        next.set(GameStage::Civilization);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_refused_while_the_player_is_off() {
        // Unit tests never publish availability: stage code keeps its stand-ins.
        assert!(!available());
        assert!(!request("CRG_FirstDNA"));
        assert!(!hold_death(DeathCause::Combat));
    }

    fn run_with_villages() -> RunProgress {
        let mut run = RunProgress::default();
        run.tribal_campaign.villages = vec![crate::tribal::RivalVillageState::default(); crate::tribal::RIVAL_VILLAGES];
        run
    }

    #[test]
    fn tribal_alliances_conquests_and_levels_pick_the_retail_moments() {
        let mut world = World::new();
        world.insert_resource(run_with_villages());
        let mut seen = Seen::default();
        // The first frames adopt the state.
        assert!(tribal(&mut world, &mut seen, true).is_empty());
        assert!(tribal(&mut world, &mut seen, false).is_empty());
        // The first ally: its moment, then the upgrade (the hut level rises at
        // the first tribe, tribal_rules::hut_level) which contains the totem.
        world.resource_mut::<RunProgress>().tribal_campaign.villages[0].status = RivalStatus::Allied;
        let plays = tribal(&mut world, &mut seen, false);
        let names: Vec<String> = plays.iter().map(|p| p.name.clone()).collect();
        assert_eq!(names, ["TRG_TotemPole_Social", "TRG_Upgrade"]);
        // 0xD9CED0: pause 1, queue 0; the allied tribe is bound for its
        // members' celebration.
        assert!(plays[0].pause_game && !plays[0].queue);
        assert!(plays[0].bind.iter().any(|(n, _)| *n == "Tribe"));
        // Nothing happens twice.
        assert!(tribal(&mut world, &mut seen, false).is_empty());
        // A conquest without a level change: its moment now, the totem moment
        // when the Tribal timer has run 60 s (0xCDB520 / 0xCDF080).
        world.resource_mut::<RunProgress>().tribal_campaign.villages[1].status = RivalStatus::Conquered;
        let names: Vec<String> = tribal(&mut world, &mut seen, false).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["TRG_TotemPole_Conquer"]);
        assert!(tribal(&mut world, &mut seen, false).is_empty(), "the timer runs");
        assert_eq!(seen.totem_due.as_ref().map(|d| d.remaining), Some(TOTEM_AFTER_CONQUEST_SECONDS));
        seen.totem_due.as_mut().unwrap().remaining = 0.0;
        let names: Vec<String> = tribal(&mut world, &mut seen, false).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["TRG_TotemPole"]);
        assert!(seen.totem_due.is_none());
        // A tribe coming into view the first time.
        world.resource_mut::<RunProgress>().tribal_campaign.villages[2].appeared = false;
        let mut seen = Seen::default();
        tribal(&mut world, &mut seen, true);
        world.resource_mut::<RunProgress>().tribal_campaign.villages[2].appeared = true;
        let names: Vec<String> = tribal(&mut world, &mut seen, false).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["TRG_PRE_FirstTribeAppears"]);
        assert!(played(&world, "TRG_PRE_FirstTribeAppears"));
        world.resource_mut::<RunProgress>().tribal_campaign.villages[3].appeared = false;
        tribal(&mut world, &mut seen, true);
        world.resource_mut::<RunProgress>().tribal_campaign.villages[3].appeared = true;
        assert!(tribal(&mut world, &mut seen, false).is_empty(), "first-time moments play once per run");
    }

    #[test]
    fn civ_unlocks_and_attacks_play_once() {
        let mut world = World::new();
        let mut run = RunProgress::default();
        run.civ_campaign.ensure_cities();
        world.insert_resource(run);
        let mut seen = Seen::default();
        assert!(civ(&mut world, &mut seen, true).is_empty());
        // The tech level rising plays nothing by itself any more: the air
        // unlock waits for the end of a capture cinematic (0xCFA220).
        world.resource_mut::<RunProgress>().civ_campaign.tech_level = crate::civ_rules::AIR_TECH_LEVEL;
        assert!(civ(&mut world, &mut seen, false).is_empty());
        assert!(unlock_air_due(crate::civ_rules::AIR_TECH_LEVEL, false));
        assert!(!unlock_air_due(crate::civ_rules::AIR_TECH_LEVEL, true), "once");
        assert!(!unlock_air_due(crate::civ_rules::CITY_TECH_LEVEL, false));
        // A captured city under attack: the tutorial plays once.
        {
            let mut run = world.resource_mut::<RunProgress>();
            run.civ_campaign.cities[1].owner = crate::civ::CityOwner::Player;
            run.civ_campaign.cities[1].calm = 100.0;
        }
        civ(&mut world, &mut seen, false);
        world.resource_mut::<RunProgress>().civ_campaign.cities[1].calm = 0.0;
        let names: Vec<String> = civ(&mut world, &mut seen, false).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["CVG_PlayerCityAttacked"]);
        world.resource_mut::<RunProgress>().civ_campaign.cities[1].calm = 100.0;
        civ(&mut world, &mut seen, false);
        world.resource_mut::<RunProgress>().civ_campaign.cities[1].calm = 0.0;
        assert!(civ(&mut world, &mut seen, false).is_empty());
    }

    #[test]
    fn the_vehicle_tutorial_follows_the_forced_steps_not_a_timer() {
        // 0xCF9D00 step 5: once rotate / pan / zoom / select are done, once.
        let mut world = World::new();
        let mut run = RunProgress::default();
        run.civ_campaign.ensure_cities();
        run.civ_campaign.clock = 600.0;
        run.cinematics_played.push("TRG2CVG_CivStart".into());
        world.insert_resource(run);
        let mut seen = Seen::default();
        civ(&mut world, &mut seen, true);
        let names = |world: &mut World, seen: &mut Seen| civ(world, seen, false).into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert!(names(&mut world, &mut seen).is_empty(), "ten minutes in, but the forced steps are not done");
        world.insert_resource(crate::civ_hud::guide::GuideState::with_forced_done());
        assert_eq!(names(&mut world, &mut seen), ["CVG_SendVehicleToCommodity"]);
        assert!(names(&mut world, &mut seen).is_empty(), "once per run");
    }

    #[test]
    fn tribal_first_baby_and_a_wiped_out_tribe_pick_the_retail_moments() {
        let mut world = World::new();
        let mut run = run_with_villages();
        run.tribe = vec![Default::default(); 3];
        world.insert_resource(run);
        let mut seen = Seen::default();
        assert!(tribal(&mut world, &mut seen, true).is_empty());
        // The first baby: once per run.
        let birth = |slot| crate::tribal::TribalBirth { slot, remaining: 60.0 };
        world.resource_mut::<RunProgress>().tribal_campaign.births.push(birth(3));
        let names: Vec<String> = tribal(&mut world, &mut seen, false).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["TRG_FirstBaby"]);
        world.resource_mut::<RunProgress>().tribal_campaign.births.push(birth(4));
        assert!(tribal(&mut world, &mut seen, false).is_empty());
        // Members and babies gone while the chieftain waits for his respawn.
        {
            let mut run = world.resource_mut::<RunProgress>();
            run.tribe.clear();
            run.tribal_campaign.births.clear();
        }
        assert!(tribal(&mut world, &mut seen, false).is_empty(), "the chieftain still stands");
        world.resource_mut::<RunProgress>().tribal_campaign.chieftain_respawn = 90.0;
        let plays = tribal(&mut world, &mut seen, false);
        assert_eq!(plays.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["TRG_PRE_PopDestroyed"]);
        assert!(plays[0].pause_game && plays[0].bind.iter().any(|(n, _)| *n == "TribeMember"));
        assert!(tribal(&mut world, &mut seen, false).is_empty());
    }

    #[test]
    fn civ_first_spice_node_and_the_last_city_lost() {
        let mut world = World::new();
        let mut run = RunProgress::default();
        run.civ_campaign.ensure_cities();
        run.civ_campaign.geysers = vec![Default::default(); 3];
        world.insert_resource(run);
        let mut seen = Seen::default();
        assert!(civ(&mut world, &mut seen, true).is_empty());
        world.resource_mut::<RunProgress>().civ_campaign.geysers[1].owner = Some(crate::civ::HOME_INDEX);
        let plays = civ(&mut world, &mut seen, false);
        assert_eq!(plays.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["CVG_FirstSpiceNodeCaptured"]);
        assert!(plays[0].bind.iter().any(|(n, _)| *n == "SpiceNode"));
        world.resource_mut::<RunProgress>().civ_campaign.geysers[2].owner = Some(crate::civ::HOME_INDEX);
        assert!(civ(&mut world, &mut seen, false).is_empty(), "once per run");
        // Every city lost.
        assert!(world.resource::<RunProgress>().civ_campaign.owned_count() > 0);
        for city in world.resource_mut::<RunProgress>().civ_campaign.cities.iter_mut() {
            city.owner = crate::civ::CityOwner::Rival;
        }
        let names: Vec<String> = civ(&mut world, &mut seen, false).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["CVG_Lose"]);
        assert!(civ(&mut world, &mut seen, false).is_empty());
    }

    #[test]
    fn a_razed_home_hut_plays_the_hut_destroyed_moment_once() {
        let mut world = World::new();
        let mut run = run_with_villages();
        run.tribe = vec![Default::default(); 3];
        world.insert_resource(run);
        let mut seen = Seen::default();
        assert!(tribal(&mut world, &mut seen, true).is_empty());
        world.resource_mut::<RunProgress>().tribal_campaign.home_hut_damage = 120.0;
        assert!(tribal(&mut world, &mut seen, false).is_empty(), "the hut still stands");
        let health = world.resource::<RunProgress>().tribal_campaign.home_hut_health();
        world.resource_mut::<RunProgress>().tribal_campaign.home_hut_damage = health;
        let plays = tribal(&mut world, &mut seen, false);
        assert_eq!(plays.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["TRG_PRE_HutDestroyed"]);
        assert!(plays[0].pause_game && plays[0].bind.iter().any(|(n, _)| *n == "npcHut"));
        assert!(tribal(&mut world, &mut seen, false).is_empty());
        assert!(request_pauses("TRG_PRE_HutDestroyed") && request_pauses("CVG_PRE_NewCityAppears") && !request_pauses("CRG_AvatarDied"));
    }

    #[test]
    fn captures_play_the_retail_capture_cinematic_with_its_text() {
        // 0xBE9430's text table: the type of the city's own specialty, the
        // first city of a specialty, the later ones.
        assert_eq!(capture_text(0, 0, true), 0x803);
        assert_eq!(capture_text(2, 2, false), 0x805);
        assert_eq!((capture_text(1, 0, true), capture_text(2, 0, true)), (0x902, 0x904));
        assert_eq!((capture_text(1, 0, false), capture_text(2, 0, false)), (0x908, 0x910));
        assert_eq!((capture_text(0, 1, true), capture_text(2, 1, true)), (0x900, 0x905));
        assert_eq!((capture_text(0, 1, false), capture_text(2, 1, false)), (0x906, 0x911));
        assert_eq!((capture_text(0, 2, true), capture_text(1, 2, true)), (0x901, 0x903));
        assert_eq!((capture_text(0, 2, false), capture_text(1, 2, false)), (0x907, 0x909));
        let mut world = World::new();
        let mut run = RunProgress::default();
        run.civ_campaign.ensure_cities();
        world.insert_resource(run);
        let mut seen = Seen::default();
        assert!(civ(&mut world, &mut seen, true).is_empty());
        // A conversion: `CVG_Capture`, pause on, not queued, the city framed.
        {
            let mut run = world.resource_mut::<RunProgress>();
            run.civ_campaign.cities[2].owner = crate::civ::CityOwner::Player;
            run.civ_campaign.captures.religious += 1;
            // (Not under attack: `CVG_PlayerCityAttacked` stays out of it.)
            for city in run.civ_campaign.cities.iter_mut() {
                city.calm = 100.0;
            }
        }
        let plays = civ(&mut world, &mut seen, false);
        assert_eq!(plays.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["CVG_Capture"]);
        assert!(plays[0].pause_game && !plays[0].queue);
        assert!(plays[0].bind.iter().any(|(n, _)| *n == "CityPlaza") && plays[0].bind.iter().any(|(n, _)| *n == "CityHall"));
        assert!(played(&world, "CVG_FirstCapture_Religious") || played(&world, "CVG_FirstCapture_Military") || played(&world, "CVG_FirstCapture_Economic"));
        // The ICBM takes the rest: one nuked capture plays (the others are
        // refused by the running cinematic).
        {
            let mut run = world.resource_mut::<RunProgress>();
            for i in [1, 3, 4] {
                run.civ_campaign.cities[i].owner = crate::civ::CityOwner::Player;
                run.civ_campaign.captures.military += 1;
            }
            run.civ_campaign.superweapon_launched = true;
        }
        let plays = civ(&mut world, &mut seen, false);
        assert!(plays.iter().all(|p| p.name == "CVG_Capture_Nuked" && !p.queue), "{:?}", plays.iter().map(|p| &p.name).collect::<Vec<_>>());
        assert_eq!(plays.len(), 3);
        assert!(civ(&mut world, &mut seen, false).is_empty());
    }

    fn space_names(world: &mut World, seen: &mut Seen) -> Vec<String> {
        space(world, seen, false).into_iter().map(|p| p.name).collect()
    }

    /// The names of the cinematics `creature` asks for over a few frames.
    fn creature_plays(world: &mut World, seen: &mut Seen, frames: u32) -> Vec<String> {
        let mut out = Vec::new();
        for _ in 0..frames {
            out.extend(creature(world, seen, false).into_iter().map(|p| p.name));
        }
        out
    }

    #[test]
    fn a_birth_plays_the_egg_fade_unless_it_starts_a_migration() {
        let mut world = World::new();
        world.insert_resource(RunProgress::default());
        let mut seen = Seen::default();
        assert!(creature(&mut world, &mut seen, true).is_empty());
        assert!(creature_plays(&mut world, &mut seen, 5).is_empty(), "nothing happens by itself");
        // 0xD40FA0 after a death: the avatar hatches (egg fade).
        world.resource_mut::<RunProgress>().creature_lifecycle.deaths = 1;
        assert_eq!(creature_plays(&mut world, &mut seen, 6), ["CRG_Fade_From_Black_Egg"]);
        // 0xD44ADF: the return from the editor (the mating's hatch) is a birth too.
        world.resource_mut::<RunProgress>().creature_lifecycle.generation = 2;
        assert_eq!(creature_plays(&mut world, &mut seen, 6), ["CRG_Fade_From_Black_Egg"]);
        // A birth that starts a migration (set by the next frame's system) plays the
        // migration cinematic instead of the fade.
        world.resource_mut::<RunProgress>().creature_lifecycle.deaths = 2;
        assert!(creature_plays(&mut world, &mut seen, 1).is_empty());
        world.resource_mut::<RunProgress>().creature_progression.pending_migration =
            Some(crate::creature_progression::PendingMigration { origin: [0.0; 3], destination: [90.0, 0.0, 0.0] });
        assert_eq!(creature_plays(&mut world, &mut seen, 8), ["CRG_SpeciesMigrate"]);
        // Claiming the destination plays CRG_ClaimNest (and no fade).
        {
            let mut run = world.resource_mut::<RunProgress>();
            run.creature_progression.pending_migration = None;
            run.creature_progression.migrations_completed = 1;
        }
        assert_eq!(creature_plays(&mut world, &mut seen, 8), ["CRG_ClaimNest"]);
        // A stage entry adopts the counters silently.
        let mut fresh = Seen::default();
        world.resource_mut::<RunProgress>().creature_lifecycle.deaths = 5;
        assert!(creature(&mut world, &mut fresh, true).is_empty());
        assert!(creature_plays(&mut world, &mut fresh, 6).is_empty());
    }

    #[test]
    fn the_nest_messages_are_counted_and_the_upgrade_is_requested() {
        let before = nest_messages();
        let mut world = World::new();
        hatch_avatar_eggs(&mut world);
        upgrade_nest(&mut world);
        let after = nest_messages();
        assert!(after.0 > before.0 && after.1 > before.1);
        assert_eq!(message::HATCH_AVATAR_EGGS, 0x0656_6531);
        assert_eq!(message::UPGRADE_NEST, 0x0655_5ABC);
    }

    #[test]
    fn space_hints_play_once_at_their_moments() {
        let mut world = World::new();
        world.insert_resource(RunProgress::default());
        let mut seen = Seen::default();
        assert!(space(&mut world, &mut seen, true).is_empty());
        // Entering a system of another empire, at war.
        {
            let mut run = world.resource_mut::<RunProgress>();
            let c = &mut run.space_campaign;
            c.current_system += 1;
            c.current_system_empire = Some(3);
            c.wars.push(3);
        }
        assert_eq!(space_names(&mut world, &mut seen), ["SPG_FirstContact_Intercept", "SPG_FirstEntryEnemyTerritory"]);
        let spice = crate::space::SpiceColor::Red;
        // Spice, a colony, a trade route, an uplift.
        {
            let mut run = world.resource_mut::<RunProgress>();
            let c = &mut run.space_campaign;
            c.current_system += 1;
            *c.cargo_spice.entry(spice).or_insert(0) += 2;
            c.colonies.push(7);
            c.trade_routes.insert(7, 1.0);
            c.species_uplifted += 1;
        }
        assert_eq!(
            space_names(&mut world, &mut seen),
            ["SPG_FirstSpicePickup", "SPG_ColonizeMissionComplete", "SPG_TutorialTradeRoute", "SPG_CreatureTribeUplift"]
        );
        // The hints are one-shot; the uplift is not.
        {
            let mut run = world.resource_mut::<RunProgress>();
            let c = &mut run.space_campaign;
            c.current_system += 1;
            *c.cargo_spice.entry(spice).or_insert(0) += 2;
            c.colonies.push(8);
            c.species_uplifted += 1;
        }
        assert_eq!(space_names(&mut world, &mut seen), ["SPG_CreatureTribeUplift"]);
        // The Grox are not a first contact: their first meeting is
        // `SPG_GrobFirstEncounter` (0xFDFBB0).
        let mut world = World::new();
        world.insert_resource(RunProgress::default());
        let mut seen = Seen::default();
        space(&mut world, &mut seen, true);
        {
            let mut run = world.resource_mut::<RunProgress>();
            run.space_campaign.current_system += 1;
            run.space_campaign.current_system_empire = Some(0);
        }
        assert_eq!(space_names(&mut world, &mut seen), ["SPG_GrobFirstEncounter"]);
        // The next empire star is a SETI star (0xBBA120), the player's
        // colony star is not; `SPG_SETI` once.
        {
            let mut run = world.resource_mut::<RunProgress>();
            let colony = crate::space_galaxy::world_id(run.space_campaign.current_system + 1, 1);
            run.space_campaign.colonies.push(colony);
        }
        assert_eq!(space_names(&mut world, &mut seen), ["SPG_ColonizeMissionComplete"]);
        {
            let mut run = world.resource_mut::<RunProgress>();
            run.space_campaign.current_system += 1;
        }
        assert!(space_names(&mut world, &mut seen).is_empty(), "the player's star is no SETI star");
        {
            let mut run = world.resource_mut::<RunProgress>();
            run.space_campaign.current_system += 1;
        }
        assert_eq!(space_names(&mut world, &mut seen), ["SPG_SETI"]);
        {
            let mut run = world.resource_mut::<RunProgress>();
            run.space_campaign.current_system += 1;
        }
        assert!(space_names(&mut world, &mut seen).is_empty());
        assert!(!crate::space_rules::seti_star(false, false, 0.0), "no false positives installed");
        assert!(crate::space_rules::seti_star(true, false, 0.0));
    }

    #[test]
    fn space_endgame_moments_promotions_and_biosphere_collapse() {
        let mut world = World::new();
        world.insert_resource(RunProgress::default());
        let mut seen = Seen::default();
        assert!(space(&mut world, &mut seen, true).is_empty());
        // A promotion (badgecollector-1 at 5 points) plays SPG_MasterBadge.
        {
            let mut run = world.resource_mut::<RunProgress>();
            run.space_campaign.tutorial.step = crate::space_rules::TutorialStep::Done;
            run.space_campaign.endgame.earned.insert("explorer".into(), 1);
            run.space_campaign.endgame.earned.insert("colonist".into(), 1);
        }
        let plays = space(&mut world, &mut seen, false);
        assert_eq!(plays.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["SPG_MasterBadge"]);
        assert!(plays[0].pause_game);
        assert!(space_names(&mut world, &mut seen).is_empty(), "once per promotion");
        // A biosphere collapse in this system.
        {
            let mut run = world.resource_mut::<RunProgress>();
            let here = run.space_campaign.current_system;
            run.space_campaign.active_distress = Some(crate::space::DistressSignal {
                system_id: here,
                world_id: 1,
                kind: crate::space::DistressKind::BiohazardOutbreak,
                time_remaining: 240.0,
            });
        }
        assert_eq!(space_names(&mut world, &mut seen), ["SPG_BiosphereCollapse"]);
    }

    #[test]
    fn the_colonize_mission_hints_play_at_its_target() {
        let mut world = World::new();
        world.insert_resource(RunProgress::default());
        let mut seen = Seen::default();
        let target = crate::space_galaxy::world_id(77, 1);
        {
            let mut run = world.resource_mut::<RunProgress>();
            let c = &mut run.space_campaign;
            c.tutorial.step = crate::space_rules::TutorialStep::TeachColonize;
            c.tutorial.colonize_target = Some(target);
        }
        assert!(space(&mut world, &mut seen, true).is_empty());
        // Another system: nothing; the target's system: the solar hint, once.
        world.resource_mut::<RunProgress>().space_campaign.current_system = 76;
        assert!(space_names(&mut world, &mut seen).is_empty());
        world.resource_mut::<RunProgress>().space_campaign.current_system = 77;
        assert_eq!(space_names(&mut world, &mut seen), ["SPG_ColonizeMissionSolar"]);
        world.resource_mut::<RunProgress>().space_campaign.current_system = 76;
        world.resource_mut::<RunProgress>().space_campaign.current_system = 77;
        assert!(space_names(&mut world, &mut seen).is_empty());
    }

    #[test]
    fn space_story201_scans_and_surrenders_play_their_cinematics() {
        let mut world = World::new();
        world.insert_resource(RunProgress::default());
        let mut seen = Seen::default();
        {
            let mut run = world.resource_mut::<RunProgress>();
            let c = &mut run.space_campaign;
            c.tutorial.step = crate::space_rules::TutorialStep::Story201;
            c.tutorial.story_star = Some(c.current_system);
        }
        assert!(space(&mut world, &mut seen, true).is_empty());
        // The wreck's scan, then the ruin's (the mission ends).
        world.resource_mut::<RunProgress>().space_campaign.tutorial.story_wreck = Some(5);
        assert_eq!(space_names(&mut world, &mut seen), ["SPG_Story201ScannedUFOWreck"]);
        world.resource_mut::<RunProgress>().space_campaign.tutorial.step = crate::space_rules::TutorialStep::TeachColonize;
        assert_eq!(space_names(&mut world, &mut seen), ["SPG_Story201ScannedRuin"]);
        // A system bought, then one taken (solar context: no planet view).
        world.resource_mut::<RunProgress>().space_campaign.endgame.systems_purchased += 1;
        assert_eq!(space_names(&mut world, &mut seen), ["SPG_SystemSurrender_Diplomatic_Solar"]);
        world.resource_mut::<RunProgress>().space_campaign.conquered_worlds.push(9);
        assert_eq!(space_names(&mut world, &mut seen), ["SPG_SystemSurrender_Military_Solar"]);
        assert!(space_names(&mut world, &mut seen).is_empty());
    }

    #[test]
    fn space_takes_its_pending_cinematic_once() {
        let mut world = World::new();
        let mut run = RunProgress::default();
        run.space_campaign.pending_cinematic = Some("SPG_NoSuchMoment".into());
        world.insert_resource(run);
        // Without the scripts loaded nothing plays, but the request is consumed.
        assert!(space(&mut world, &mut Seen::default(), false).is_empty());
        assert!(world.resource::<RunProgress>().space_campaign.pending_cinematic.is_none());
    }

    #[test]
    fn the_gated_changes_are_the_retail_evolve_and_advance() {
        assert_eq!(
            transition_chain(&GameStage::Tribal, &GameStage::Civilization),
            ["TRG2CVG_PreEditor", "TRG2CVG_ShowCityHall"]
        );
        assert_eq!(transition_chain(&GameStage::Civilization, &GameStage::Space), ["CVG_UnlockUFO"]);
        assert!(transition_chain(&GameStage::Creature, &GameStage::Tribal).is_empty());
        assert!(transition_chain(&GameStage::MainMenu, &GameStage::Creature).is_empty());
        let _ = name_id("x");
    }
}
