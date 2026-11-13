//! Creature stage events: the event manager and the rain storm and meteor
//! strike events (helper `creature-events`, notes `creature-events-notes.md`).
//!
//! Retail (march2017 addresses, read with `swarm-volume\target\re\dis.py`):
//! * The event manager (singleton [0x169E43C], size 0x70, created lazily by
//!   0xD51610) loads every property of group 0x0EBCFBC7 in its initialiser
//!   0xD4E020 (the `type` property 0xB10F785D picks the class: 1 night
//!   predators, 2 UFO abduction, 3 meteor strike, 4 baby game, 5 rain storm,
//!   6 sea monster, 8 summon creature; 0 / 7 are Tribal events) and polls them
//!   in its update 0xD4E450 (see [`step`]).
//! * Base event (0xD4AF30 reads `EventMask` 0xC55C98BB -> bit `1 << value`,
//!   `ActivationType` 0x06BFE2ED, `ActivationThreshold` 0xD2E804EE,
//!   `RequiresAvatar` 0x346DBE16; 0xD4D4E0 starts an event: frequency - 1 when
//!   finite, duration timer = `duration` 0xB1F42539, then the class's start
//!   (vtable +0x30); the stop is vtable +0x34, the start condition +0x38, the
//!   done test +0x3C (default 0xD4BB60: duration timer < 0), the per-frame
//!   update +0x40).
//! * `ActivationType` (the switch at 0xD4E7A5, jump table 0xD4E878): 0 and 5+
//!   start whenever the class condition holds; 2 compares a fresh random
//!   float of the (lo, hi) threshold with the player's stage-total DNA
//!   (0xD2E360 = cPlayer +0x10F0, `mCurrentGoalProgress`) and starts when the
//!   DNA is at least that float; 3 is the time-of-day window (0xD4ABF0, the
//!   night predators, kept in `creature_ecology`); 4 never starts from the
//!   poll (the baby game is started by 0xD412F0 after a migration).
//! * `RainStorm` / `RainStormLate` (cRainStormEvent, vtable 0x147B068): the
//!   condition 0xD4BCC0 needs the avatar's position and the day phase 2 (day,
//!   0xBC2630: 07:00 to 17:00); the start 0xD4BC10 asks the planet's weather
//!   manager (Terrain::cWeatherManager, 0xFC6EC0) for a local storm at the
//!   avatar's position for `duration` seconds: effect `weatherLocalStormEffect`
//!   0x05555FE0 (property 0x055AB884 / 88B / 88E of the planet's `weather`
//!   prop, the same id for cold / warm / hot), aligned to the surface.
//! * `MeteorStrike` (cMeteorStrikeEvent, vtable 0x147B590): the condition
//!   0xD4FF00 needs the avatar and no forest within `meteorStrikeDistFromForests`
//!   (20, 0xB092A0 over the planet's list at 0x1E5F0); the start 0xD51840
//!   switches the audio system's event state and 0xD50330 reads the
//!   properties; the update 0xD50550 runs two streams (0 one big meteor,
//!   1 many small ones): each launches a meteor effect (0xD4FFF0) at the
//!   avatar plus `strikeCenterPosFromAvatar[i]` along the camera's up vector
//!   projected on the ground (0xD50550: `up - radial * (up . radial)`),
//!   lifted by `MeteorSpawnHeight` (80) along the surface normal, when its
//!   delay timer (`MeteorInitialDelay[i]`, then a random value of
//!   `MeteorSpawnDelayRange[i]`) is up, until `NumMeteorStrikes[i]` were
//!   launched. The done test 0xD4FE50 is the duration timer < 0 (25 s) or all
//!   meteors launched and their effects over.
//!
//! `UFOAbduction` (cUFOAbductionEvent, vtable 0x147B1D8): no start condition; the
//! start 0xD4F8A0 creates a UFO above the avatar, sets the audio input `crg_ufo`
//! 1.0 and the stop 0xD4FAD0 sets it 0.0 and destroys the UFO; the abduction
//! itself (the UFO's two behaviours) is `ufo` (the child module, which has the
//! decoded addresses and the ADAPTED parts).
//!
//! Not run here: `SummonCreature` (started by the omnivore
//! superpower, ability type 51, 0xD8E92A; the Creature stage has no
//! superpowers yet) and `BabyGame` (started by the migration 0xD40FA0 at
//! 0xD412F0). `NightPredators` and `SeaMonster` keep their own modules
//! (`creature_ecology`, `creature_support`).

pub mod ufo;

use bevy::prelude::*;

use crate::game::{Health, Player, RunProgress};
use crate::stages::GameStage;

/// The property group of the event templates (`creature_events~`).
#[allow(dead_code)]
pub const EVENT_GROUP: u32 = 0x0EBC_FBC7;

/// The class an event template (`type` 0xB10F785D) selects (0xD4E143 table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    NightPredators,
    UfoAbduction,
    MeteorStrike,
    BabyGame,
    RainStorm,
    SeaMonster,
    SummonCreature,
}

/// `ActivationType` (0x06BFE2ED) as the update 0xD4E450 reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// 0: only the class condition decides.
    Condition,
    /// 2: a fresh random float of the threshold against the stage-total DNA.
    StageDna,
    /// 3: the time-of-day window (hours).
    TimeOfDay,
    /// 4: never from the poll (started explicitly by other code).
    Explicit,
}

/// One event template of group 0x0EBC_FBC7 (VERIFIED against the installed
/// properties by the ignored test `installed_event_templates_match_the_table`).
#[derive(Clone, Copy, Debug)]
pub struct EventDef {
    pub name: &'static str,
    /// Instance id of the template (`creature_events~!name`).
    pub instance: u32,
    pub kind: EventKind,
    /// `CheckTime` (0x920A2318): seconds between two polls of the start rule.
    pub check_time: f32,
    /// `duration` (0xB1F42539), seconds (-1: no timer).
    pub duration: f32,
    pub activation: Activation,
    /// `ActivationThreshold` (0xD2E804EE).
    pub threshold: (f32, f32),
    /// `frequency` (0x332C289D): starts left (-1: unlimited).
    pub frequency: i32,
    /// `1 << EventMask` (0xC55C98BB), 0 for mask 0: events sharing a bit never
    /// run together.
    pub mask: u32,
    /// `RequiresAvatar` (0x346DBE16).
    pub requires_avatar: bool,
    /// Whether this module runs the event (the others are listed in the header).
    pub run_here: bool,
}

/// The templates in the manager's order (its map is keyed by `type`; the two
/// rain storms share type 5).
pub const EVENTS: [EventDef; 8] = [
    EventDef {
        name: "NightPredators",
        instance: 0xB9E1_1B03,
        kind: EventKind::NightPredators,
        check_time: 5.0,
        duration: 1440.0,
        activation: Activation::TimeOfDay,
        threshold: (21.0, 3.0),
        frequency: -1,
        mask: 0,
        requires_avatar: true,
        run_here: false,
    },
    EventDef {
        name: "UFOAbduction",
        instance: 0x6A81_7A5C,
        kind: EventKind::UfoAbduction,
        check_time: 10.0,
        duration: 90.0,
        activation: Activation::StageDna,
        threshold: (800.0, 850.0),
        frequency: 1,
        mask: 2,
        requires_avatar: true,
        run_here: true,
    },
    EventDef {
        name: "MeteorStrike",
        instance: 0x8834_5FAB,
        kind: EventKind::MeteorStrike,
        check_time: 10.0,
        duration: 25.0,
        activation: Activation::StageDna,
        threshold: (450.0, 500.0),
        frequency: 1,
        mask: 2,
        requires_avatar: true,
        run_here: true,
    },
    EventDef {
        name: "BabyGame",
        instance: 0xA412_98F3,
        kind: EventKind::BabyGame,
        check_time: 0.0,
        duration: -1.0,
        activation: Activation::Explicit,
        threshold: (0.0, 0.0),
        frequency: -1,
        mask: 2,
        requires_avatar: true,
        run_here: false,
    },
    EventDef {
        name: "RainStorm",
        instance: 0x117A_083A,
        kind: EventKind::RainStorm,
        check_time: 20.0,
        duration: 90.0,
        activation: Activation::StageDna,
        threshold: (200.0, 450.0),
        frequency: 1,
        mask: 2,
        requires_avatar: true,
        run_here: true,
    },
    EventDef {
        name: "RainStormLate",
        instance: 0x7E62_A866,
        kind: EventKind::RainStorm,
        check_time: 20.0,
        duration: 90.0,
        activation: Activation::StageDna,
        threshold: (600.0, 900.0),
        frequency: 1,
        mask: 2,
        requires_avatar: true,
        run_here: true,
    },
    EventDef {
        name: "SeaMonster",
        instance: 0xCCA5_5EC8,
        kind: EventKind::SeaMonster,
        check_time: 2.0,
        duration: -1.0,
        activation: Activation::Condition,
        threshold: (0.0, 0.0),
        frequency: -1,
        mask: 0,
        requires_avatar: true,
        run_here: false,
    },
    EventDef {
        name: "SummonCreature",
        instance: 0x546D_B6C7,
        kind: EventKind::SummonCreature,
        check_time: 5000.0,
        duration: 120.0,
        activation: Activation::Condition,
        threshold: (0.0, 0.0),
        frequency: -1,
        mask: 0,
        requires_avatar: true,
        run_here: false,
    },
];

// ---------------------------------------------------------------- day phases

/// `cTimeOfDay::GetPhase` (0xBC2630): thresholds in seconds of an 86400 s day
/// (globals 0x156CFB8..0x156CFC4 = 18000, 25200, 61200, 68400; scaled by
/// `SetDayDuration`, so they are fractions of the day).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayPhase {
    Night,
    Dawn,
    Day,
    Dusk,
}

pub fn day_phase(hour: f32) -> DayPhase {
    let seconds = hour.rem_euclid(24.0) * 3600.0;
    if seconds < 18000.0 {
        DayPhase::Night
    } else if seconds < 25200.0 {
        DayPhase::Dawn
    } else if seconds < 61200.0 {
        DayPhase::Day
    } else if seconds < 68400.0 {
        DayPhase::Dusk
    } else {
        DayPhase::Night
    }
}

// ------------------------------------------------------------------- manager

/// The runtime state of one event of [`EVENTS`] (the event object's +0xC
/// active flag, +0x28 check timer, +0x2C duration timer).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clock {
    pub active: bool,
    pub check: f32,
    pub left: f32,
}

/// What the manager sees of the game each update.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub dt: f32,
    /// The stage-total DNA (cPlayer +0x10F0).
    pub stage_dna: f32,
    /// Hours of the day at the avatar.
    pub hour: f32,
    /// `Some(alive)` with an avatar.
    pub avatar: Option<bool>,
    /// A cinematic plays (0xB3D5D0 + 0x2C is 1 or 2: no event starts).
    pub cinematic: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    Start(usize),
    Stop(usize),
}

/// A fresh random float of `threshold` (0x4DF340: `lo + roll * (hi - lo)`).
pub fn threshold_roll(threshold: (f32, f32), roll: f32) -> f32 {
    threshold.0 + (threshold.1 - threshold.0) * roll
}

/// The activation test of an idle event (0xD4E7A5..0xD4E834).
pub fn activation_passes(def: &EventDef, view: &View, roll: f32) -> bool {
    match def.activation {
        Activation::Condition => true,
        Activation::StageDna => threshold_roll(def.threshold, roll) <= view.stage_dna,
        Activation::TimeOfDay => crate::creature_ecology::in_window(view.hour.rem_euclid(24.0), def.threshold),
        Activation::Explicit => false,
    }
}

/// One update of the event manager (0xD4E450) over the events `defs` whose
/// runtime state is `clocks`. `started` lists the events whose `frequency` was
/// spent (it is the saved part of the manager); `roll` draws the threshold
/// floats; `condition` is the class's start test (vtable +0x38) and `done` the
/// class's extra done test (the meteor strike's, vtable +0x3C; the default
/// duration test is made here). Events are visited in order.
pub fn step(
    defs: &[EventDef],
    clocks: &mut [Clock],
    started: &mut Vec<u32>,
    view: &View,
    roll: &mut dyn FnMut() -> f32,
    condition: &mut dyn FnMut(&EventDef) -> bool,
    done: &mut dyn FnMut(&EventDef) -> bool,
) -> Vec<Transition> {
    let mut out = Vec::new();
    // `edi` of 0xD4E450: the masks of the events running at the start of the pass.
    let mut running_mask = 0u32;
    for (def, clock) in defs.iter().zip(clocks.iter()) {
        if clock.active {
            running_mask |= def.mask;
        }
    }
    for (index, def) in defs.iter().enumerate() {
        if !def.run_here {
            continue;
        }
        let clock = &mut clocks[index];
        let avatar_gone = def.requires_avatar && view.avatar != Some(true);
        if clock.active {
            clock.left -= view.dt;
            let mut stop = avatar_gone;
            if def.activation == Activation::TimeOfDay
                && !crate::creature_ecology::in_window(view.hour.rem_euclid(24.0), def.threshold)
            {
                stop = true;
            }
            if !stop {
                stop = (def.duration >= 0.0 && clock.left < 0.0) || done(def);
            }
            if stop {
                clock.active = false;
                running_mask &= !def.mask;
                out.push(Transition::Stop(index));
            }
            continue;
        }
        if view.cinematic {
            continue;
        }
        let spent = started.contains(&def.instance);
        if def.frequency >= 0 && spent {
            continue;
        }
        if def.mask & running_mask != 0 {
            continue;
        }
        if avatar_gone {
            continue;
        }
        clock.check -= view.dt;
        if clock.check > 0.0 {
            continue;
        }
        clock.check = def.check_time;
        if !activation_passes(def, view, roll()) || !condition(def) {
            continue;
        }
        clock.active = true;
        clock.left = def.duration;
        if def.frequency >= 0 && !spent {
            started.push(def.instance);
        }
        running_mask |= def.mask;
        out.push(Transition::Start(index));
    }
    out
}

// ------------------------------------------------------------- rain storm

/// `weatherLocalStormEffect` of the planet's `weather` prop (properties
/// 0x055AB884 / 0x055AB88B / 0x055AB88E, all 0x05555FE0 in the installed
/// `weather` 0x0243AD2B!0x12E95823), the effect 0xFC6EC0 plays.
pub const LOCAL_STORM_EFFECT: u32 = 0x0555_5FE0;
/// Tag of the storm effect (`SwarmStop`).
pub const TAG_RAIN: u64 = 0xC4E0_0000_0000_0001;
/// Tag of the meteor effects.
pub const TAG_METEOR: u64 = 0xC4E0_0000_0000_0002;

/// The rain storm's start condition (0xD4BCC0): an avatar and the day phase.
pub fn rain_condition(avatar: bool, hour: f32) -> bool {
    avatar && day_phase(hour) == DayPhase::Day
}

// ------------------------------------------------------------ meteor strike

/// `meteorStrikeDistFromForests` (0xB753F503): the avatar must be farther than
/// this from every forest (0xD4FF00).
pub const METEOR_FOREST_DISTANCE: f32 = 20.0;
/// `NumMeteorStrikes` (0x5616C7E8): meteors per stream.
pub const STREAM_TOTALS: [u32; 2] = [1, 200];
/// `MeteorInitialDelay` (0x9DC91E04): seconds before a stream's first meteor.
pub const INITIAL_DELAY: [f32; 2] = [0.0, 9.0];
/// `MeteorSpawnDelayRange` (0x9920C7CA): seconds between a stream's meteors.
pub const SPAWN_DELAY: [(f32, f32); 2] = [(0.0, 0.1), (0.1, 0.5)];
/// `strikeCenterPosFromAvatar` (0x8A788159): distance of a stream's centre
/// ahead of the avatar.
pub const STRIKE_CENTER: [f32; 2] = [130.0, 15.0];
/// `MeteorStrikeRange` (0x15585C0A): the stream's radius around its centre.
pub const STRIKE_RANGE: [f32; 2] = [1.0, 30.0];
/// `MeteorSpawnHeight` (0x9BA81197).
pub const SPAWN_HEIGHT: f32 = 80.0;
/// `MeteorEffects` (0xA45EB62D): the effect of each stream.
pub const METEOR_EFFECTS: [u32; 2] = [0x6B5C_AF79, 0x81DF_BBC1];

/// The two meteor streams of one strike (the loop of 0xD50550).
#[derive(Clone, Debug, PartialEq)]
pub struct Shower {
    pub launched: [u32; 2],
    pub timer: [f32; 2],
    /// Seconds since the last launch (the effect needs a frame to appear).
    pub quiet: f32,
}

impl Default for Shower {
    fn default() -> Self {
        Self { launched: [0; 2], timer: INITIAL_DELAY, quiet: 0.0 }
    }
}

impl Shower {
    /// Advance by `dt`; returns the streams that launch a meteor now. A stream
    /// that launched all its meteors is skipped, otherwise its timer counts
    /// down and a launch at <= 0 reloads it with a random delay of the range.
    pub fn step(&mut self, dt: f32, roll: &mut dyn FnMut() -> f32) -> Vec<usize> {
        let mut out = Vec::new();
        self.quiet += dt;
        for i in 0..2 {
            if self.launched[i] >= STREAM_TOTALS[i] {
                continue;
            }
            self.timer[i] -= dt;
            if self.timer[i] > 0.0 {
                continue;
            }
            let (lo, hi) = SPAWN_DELAY[i];
            self.timer[i] = (lo + roll() * (hi - lo)).clamp(lo, hi);
            self.launched[i] += 1;
            self.quiet = 0.0;
            out.push(i);
        }
        out
    }

    pub fn all_launched(&self) -> bool {
        self.launched == STREAM_TOTALS
    }
}

/// The ground direction a strike is aimed along (0xD50550): the camera's up
/// vector without its component along the surface normal at the camera.
pub fn strike_direction(camera_up: Vec3, radial: Vec3) -> Vec3 {
    let d = camera_up.dot(radial);
    let v = camera_up - radial * d;
    v * (1.0 / (v.length_squared() + 1e-8).sqrt())
}

// ----------------------------------------------------------------- Bevy side

/// ADAPTED: retail draws from the game's `sMainRNG`; a splitmix64 stream gives
/// the same uniform floats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rng(pub u64);

impl Rng {
    pub fn roll(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Run state of the events (not saved: an event running at a save does not
/// resume).
#[derive(Resource, Debug)]
pub struct EventRuntime {
    clocks: [Clock; EVENTS.len()],
    shower: Option<Shower>,
    rng: Rng,
    /// `OPENSPORE_FORCE_EVENT=rain|rainlate|meteor|ufo`: start that event at the
    /// first update, whatever its conditions (captures).
    force: Option<usize>,
    /// Where the running rain storm began (the avatar's position at its start:
    /// retail leaves the storm cloud there). `creature_music` derives
    /// `rain_density` from it.
    storm_at: Option<Vec3>,
}

impl Default for EventRuntime {
    fn default() -> Self {
        let force = std::env::var("OPENSPORE_FORCE_EVENT").ok().and_then(|v| {
            let name = match v.to_ascii_lowercase().as_str() {
                "rain" => "RainStorm",
                "rainlate" => "RainStormLate",
                "meteor" => "MeteorStrike",
                "ufo" => "UFOAbduction",
                _ => return None,
            };
            EVENTS.iter().position(|e| e.name == name)
        });
        Self { clocks: [Clock::default(); EVENTS.len()], shower: None, rng: Rng(0), force, storm_at: None }
    }
}

impl EventRuntime {
    /// Whether an event of `kind` is running (the event object's active flag,
    /// +0xC). The meteor strike's start / stop (0xD51840 / 0xD50910) set the
    /// audio parameter `crg_meteors` to 1 / 0 (`creature_music`).
    pub fn is_active(&self, kind: EventKind) -> bool {
        EVENTS.iter().zip(self.clocks.iter()).any(|(def, clock)| def.kind == kind && clock.active)
    }

    /// The point a running rain storm was started at.
    pub fn storm_at(&self) -> Option<Vec3> {
        self.storm_at.filter(|_| self.is_active(EventKind::RainStorm))
    }

    /// Tests: mark the first event of `kind` as running (a long timer) without
    /// the effects its start plays, with the storm's point for a rain storm.
    #[cfg(test)]
    pub(crate) fn run_for_test(&mut self, kind: EventKind, on: bool, storm_at: Option<Vec3>) {
        if let Some((_, clock)) = EVENTS.iter().zip(self.clocks.iter_mut()).find(|(def, _)| def.kind == kind) {
            clock.active = on;
            clock.left = 1.0e6;
        }
        self.storm_at = storm_at;
    }
}

pub struct CreatureEventsPlugin;

impl Plugin for CreatureEventsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EventRuntime>()
            .init_resource::<ufo::UfoEvent>()
            .add_systems(OnEnter(GameStage::Creature), (reset_runtime, ufo::reset))
            .add_systems(OnExit(GameStage::Creature), ufo::leave)
            .add_systems(
                Update,
                (run_events, ufo::run_ufo, ufo::mount_visual)
                    .chain()
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Creature))
                    .run_if(resource_exists::<Messages<crate::swarm_fx::SwarmPlay>>),
            );
    }
}

fn reset_runtime(mut rt: ResMut<EventRuntime>) {
    let force = rt.force;
    *rt = EventRuntime::default();
    rt.force = force;
}

/// Where a strike lands: the stream's centre (the avatar plus `center` along
/// `direction`) scattered inside the stream's range. ADAPTED: retail hands the
/// range to the surface-transform builder 0xB818D0 together with the point
/// and no random draw was found in the launcher 0xD4FFF0, so a uniform scatter
/// over the disc stands in (200 meteors on one point is not the look).
pub fn strike_point(avatar: Vec2, direction: Vec2, stream: usize, rolls: [f32; 2]) -> Vec2 {
    let centre = avatar + direction * STRIKE_CENTER[stream];
    let r = STRIKE_RANGE[stream] * rolls[0].sqrt();
    let a = rolls[1] * std::f32::consts::TAU;
    centre + Vec2::new(a.cos(), a.sin()) * r
}

#[allow(clippy::too_many_arguments)]
fn run_events(
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    mut rt: ResMut<EventRuntime>,
    player: Query<(&Transform, &Health), With<Player>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    cinematics: Option<Res<crate::cinematics::Cinematics>>,
    live: Query<&crate::swarm_fx::SwarmEffect>,
    ufo: Res<ufo::UfoEvent>,
    mut plays: MessageWriter<crate::swarm_fx::SwarmPlay>,
    mut stops: MessageWriter<crate::swarm_fx::SwarmStop>,
) {
    if !run.species_world.generated {
        return;
    }
    let dt = time.delta_secs();
    if dt <= 0.0 || !dt.is_finite() {
        return;
    }
    if rt.rng.0 == 0 {
        rt.rng = Rng(run.species_world.seed ^ 0xE7E1_7C0D);
    }
    let avatar = player.single().ok().map(|(t, h)| (t.translation, !h.dead()));
    let hour = run.species_world.time_of_day;
    let land = crate::creature_planet::Land::from_surface(surface.as_deref());
    let view = View {
        dt,
        stage_dna: run.earned_creature_dna(),
        hour,
        avatar: avatar.map(|(_, alive)| alive),
        cinematic: cinematics.as_ref().is_some_and(|c| c.is_playing()),
    };

    let mut started = run.creature_progression.events_started.clone();
    let before = started.len();
    let mut clocks = rt.clocks;
    let mut transitions: Vec<Transition> = Vec::new();
    // A forced event (capture) starts first, outside the manager's rules.
    if avatar.is_some() {
        if let Some(index) = rt.force.take() {
            clocks[index].active = true;
            clocks[index].left = EVENTS[index].duration;
            transitions.push(Transition::Start(index));
        }
    }
    let live_meteors = live.iter().filter(|e| e.tag == TAG_METEOR).count();
    let all_launched = rt.shower.as_ref().is_some_and(|s| s.all_launched() && s.quiet > 0.5);
    let mut rng = rt.rng;
    let mut roll = || rng.roll();
    let mut condition = |def: &EventDef| -> bool {
        let Some((at, _)) = avatar else {
            return false;
        };
        match def.kind {
            EventKind::RainStorm => rain_condition(true, hour),
            // ADAPTED: retail asks the planet's forest list (0xB092A0 over the manager at
            // 0x1E5F0); the planet's flora groves (`Land::forests`) stand in for it.
            EventKind::MeteorStrike => land.forests(Vec2::new(at.x, at.z), METEOR_FOREST_DISTANCE).is_empty(),
            // The UFO event has no start condition (vtable +0x38 is the shared stub).
            EventKind::UfoAbduction => true,
            _ => false,
        }
    };
    // 0xD4FE50: the strike is over once every meteor is launched and gone.
    let mut done = |def: &EventDef| match def.kind {
        EventKind::MeteorStrike => all_launched && live_meteors == 0,
        // 0xD4D1D0: the vehicle is gone (it lost goal 2, or was never created).
        EventKind::UfoAbduction => ufo.finished(),
        _ => false,
    };
    transitions.extend(step(&EVENTS, &mut clocks, &mut started, &view, &mut roll, &mut condition, &mut done));
    rt.rng = rng;
    rt.clocks = clocks;
    if started.len() != before {
        run.creature_progression.events_started = started;
        run.request_save();
    }

    for transition in transitions {
        match transition {
            Transition::Start(index) => {
                let def = &EVENTS[index];
                info!("creature event {} started", def.name);
                match def.kind {
                    EventKind::RainStorm => {
                        let Some((at, _)) = avatar else { continue };
                        let mut play = crate::swarm_fx::SwarmPlay::at(LOCAL_STORM_EFFECT, at);
                        // ADAPTED: aligned to the surface by the shortest arc (retail builds
                        // the orientation with 0xB7F2E0; its heading is not traced).
                        play.rotation = Quat::from_rotation_arc(Vec3::Y, land.radial(at));
                        play.tag = TAG_RAIN;
                        plays.write(play);
                        rt.storm_at = Some(at);
                    }
                    EventKind::MeteorStrike => rt.shower = Some(Shower::default()),
                    _ => {}
                }
            }
            Transition::Stop(index) => {
                let def = &EVENTS[index];
                info!("creature event {} ended", def.name);
                match def.kind {
                    EventKind::RainStorm => {
                        rt.storm_at = None;
                        stops.write(crate::swarm_fx::SwarmStop {
                            effect: LOCAL_STORM_EFFECT,
                            attach_to: None,
                            tag: TAG_RAIN,
                            hard: false,
                        });
                    }
                    EventKind::MeteorStrike => rt.shower = None,
                    _ => {}
                }
            }
        }
    }

    // The meteor streams (0xD50550) while the strike runs.
    let Some(mut shower) = rt.shower.take() else {
        return;
    };
    let (Some((at, _)), Some(camera)) = (avatar, cameras.iter().next()) else {
        rt.shower = Some(shower);
        return;
    };
    let mut rng = rt.rng;
    let launches = shower.step(dt, &mut || rng.roll());
    if !launches.is_empty() {
        let camera_radial = land.radial(camera.translation());
        let dir3 = strike_direction(Vec3::from(camera.up()), camera_radial);
        let dir = Vec2::new(dir3.x, dir3.z);
        let dir = if dir.length_squared() > 1e-6 { dir.normalize() } else { Vec2::Y };
        for stream in launches {
            let rolls = [rng.roll(), rng.roll()];
            let chart = strike_point(Vec2::new(at.x, at.z), dir, stream, rolls);
            let ground = land.world_point(chart);
            let up = land.radial(ground);
            let mut play = crate::swarm_fx::SwarmPlay::at(METEOR_EFFECTS[stream], ground + up * SPAWN_HEIGHT);
            play.rotation = Quat::from_rotation_arc(Vec3::Y, up);
            play.tag = TAG_METEOR;
            plays.write(play);
        }
    }
    rt.rng = rng;
    rt.shower = Some(shower);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(dna: f32, hour: f32) -> View {
        View { dt: 1.0, stage_dna: dna, hour, avatar: Some(true), cinematic: false }
    }

    fn manager(
        clocks: &mut [Clock],
        started: &mut Vec<u32>,
        v: &View,
        roll: f32,
        cond: bool,
    ) -> Vec<Transition> {
        step(&EVENTS, clocks, started, v, &mut || roll, &mut |_| cond, &mut |_| false)
    }

    fn index(name: &str) -> usize {
        EVENTS.iter().position(|e| e.name == name).unwrap()
    }

    #[test]
    fn day_phases_follow_the_retail_thresholds() {
        assert_eq!(day_phase(4.99), DayPhase::Night);
        assert_eq!(day_phase(5.0), DayPhase::Dawn);
        assert_eq!(day_phase(6.99), DayPhase::Dawn);
        assert_eq!(day_phase(7.0), DayPhase::Day);
        assert_eq!(day_phase(16.99), DayPhase::Day);
        assert_eq!(day_phase(17.0), DayPhase::Dusk);
        assert_eq!(day_phase(18.99), DayPhase::Dusk);
        assert_eq!(day_phase(19.0), DayPhase::Night);
        assert_eq!(day_phase(23.9), DayPhase::Night);
        assert!(rain_condition(true, 12.0) && !rain_condition(true, 20.0) && !rain_condition(false, 12.0));
    }

    #[test]
    fn stage_dna_events_start_when_the_dna_reaches_a_fresh_threshold_roll() {
        let rain = &EVENTS[index("RainStorm")];
        // (200, 450): the roll 0 needs 200 DNA, the roll 1 needs 450.
        assert!(!activation_passes(rain, &view(199.0, 12.0), 0.0));
        assert!(activation_passes(rain, &view(200.0, 12.0), 0.0));
        assert!(!activation_passes(rain, &view(300.0, 12.0), 1.0));
        assert!(activation_passes(rain, &view(450.0, 12.0), 1.0));
        assert_eq!(threshold_roll((200.0, 450.0), 0.5), 325.0);
        // The explicit baby game is never started by the poll.
        let baby = &EVENTS[index("BabyGame")];
        assert!(!activation_passes(baby, &view(1e9, 12.0), 0.0));
    }

    #[test]
    fn the_manager_runs_a_one_shot_event_once_and_polls_at_check_time() {
        let rain = index("RainStorm");
        let mut clocks = [Clock::default(); EVENTS.len()];
        let mut started = Vec::new();
        // Below the threshold nothing starts; the first poll is immediate.
        assert!(manager(&mut clocks, &mut started, &view(100.0, 12.0), 0.0, true).is_empty());
        assert_eq!(clocks[rain].check, 20.0);
        // The next poll is 20 s later.
        for _ in 0..19 {
            assert!(manager(&mut clocks, &mut started, &view(300.0, 12.0), 0.0, true).is_empty());
        }
        let t = manager(&mut clocks, &mut started, &view(300.0, 12.0), 0.0, true);
        assert_eq!(t, vec![Transition::Start(rain)]);
        assert!(clocks[rain].active && clocks[rain].left == 90.0);
        assert_eq!(started, vec![EVENTS[rain].instance]);
        // It ends when the duration timer is below zero (0xD4BB60), 90 s + 1 frame.
        let mut stop = Vec::new();
        for _ in 0..92 {
            stop = manager(&mut clocks, &mut started, &view(300.0, 12.0), 0.0, true);
            if !stop.is_empty() {
                break;
            }
        }
        assert_eq!(stop, vec![Transition::Stop(rain)]);
        // frequency 1: it never starts again, however long we wait.
        for _ in 0..200 {
            assert!(manager(&mut clocks, &mut started, &view(300.0, 12.0), 0.0, true).is_empty());
        }
    }

    #[test]
    fn events_sharing_a_mask_bit_never_run_together() {
        let (rain, meteor) = (index("RainStorm"), index("MeteorStrike"));
        let mut clocks = [Clock::default(); EVENTS.len()];
        let mut started = Vec::new();
        // At 460 DNA both qualify (roll 0): the meteor (type 3) is visited first.
        let t = manager(&mut clocks, &mut started, &view(460.0, 12.0), 0.0, true);
        assert_eq!(t, vec![Transition::Start(meteor)]);
        assert!(!clocks[rain].active, "the rain storm waits for the shared mask bit");
        // A stopped meteor strike frees the bit for the next poll of the rain.
        clocks[meteor].left = -1.0;
        for _ in 0..40 {
            let t = manager(&mut clocks, &mut started, &view(460.0, 12.0), 0.0, true);
            if t.contains(&Transition::Start(rain)) {
                return;
            }
        }
        panic!("the rain storm never started after the meteor strike ended");
    }

    #[test]
    fn no_event_starts_without_a_living_avatar_or_during_a_cinematic() {
        let mut clocks = [Clock::default(); EVENTS.len()];
        let mut started = Vec::new();
        let mut v = view(500.0, 12.0);
        v.avatar = None;
        assert!(manager(&mut clocks, &mut started, &v, 0.0, true).is_empty());
        v.avatar = Some(false);
        assert!(manager(&mut clocks, &mut started, &v, 0.0, true).is_empty());
        v.avatar = Some(true);
        v.cinematic = true;
        assert!(manager(&mut clocks, &mut started, &v, 0.0, true).is_empty());
        v.cinematic = false;
        assert!(!manager(&mut clocks, &mut started, &v, 0.0, true).is_empty());
        // A dead avatar stops a running event.
        let running = clocks.iter().position(|c| c.active).unwrap();
        v.avatar = Some(false);
        let t = manager(&mut clocks, &mut started, &v, 0.0, true);
        assert!(t.contains(&Transition::Stop(running)));
    }

    #[test]
    fn a_failed_start_condition_keeps_the_event_pending() {
        let meteor = index("MeteorStrike");
        let mut clocks = [Clock::default(); EVENTS.len()];
        let mut started = Vec::new();
        // A forest next to the avatar: the poll is spent, the event is not.
        assert!(manager(&mut clocks, &mut started, &view(480.0, 12.0), 0.0, false).is_empty());
        assert!(started.is_empty());
        for _ in 0..9 {
            assert!(manager(&mut clocks, &mut started, &view(480.0, 12.0), 0.0, true).is_empty());
        }
        assert_eq!(
            manager(&mut clocks, &mut started, &view(480.0, 12.0), 0.0, true),
            vec![Transition::Start(meteor)]
        );
    }

    #[test]
    fn the_meteor_streams_launch_per_the_properties() {
        let mut shower = Shower::default();
        let mut seq = 0.0f32;
        let mut roll = || {
            seq = (seq + 0.37) % 1.0;
            seq
        };
        // t = 0: the single big meteor of stream 0 goes at once, stream 1 waits 9 s.
        assert_eq!(shower.step(0.016, &mut roll), vec![0]);
        assert_eq!(shower.launched, [1, 0]);
        let mut small = Vec::new();
        let mut t = 0.016f32;
        let mut last = None;
        while t < 25.0 {
            t += 0.016;
            for s in shower.step(0.016, &mut roll) {
                assert_eq!(s, 1, "stream 0 launches exactly once");
                if let Some(prev) = last {
                    let gap: f32 = t - prev;
                    assert!(gap >= 0.1 - 0.02 && gap <= 0.5 + 0.02, "gap {gap}");
                }
                last = Some(t);
                small.push(t);
            }
        }
        assert!(small[0] >= 9.0 && small[0] < 9.1, "first small meteor at {}", small[0]);
        // 25 s event: about (25 - 9) / 0.3 small meteors (the stream's 200 are a cap).
        assert!((40..70).contains(&small.len()), "{} small meteors", small.len());
        assert!(!shower.all_launched());
        // Left alone, the streams finish at their totals.
        for _ in 0..30_000 {
            shower.step(0.016, &mut roll);
        }
        assert!(shower.all_launched());
        assert_eq!(shower.launched, [1, 200]);
    }

    #[test]
    fn strikes_are_aimed_along_the_camera_up_on_the_ground() {
        let radial = Vec3::Y;
        // A camera looking down and forward (+Z) has an up vector tilted to +Z.
        let up = Vec3::new(0.0, 0.8, 0.6);
        let d = strike_direction(up, radial);
        assert!((d - Vec3::Z).length() < 1e-3, "{d}");
        assert!(d.dot(radial).abs() < 1e-3);
        // On the far side of a planet the surface normal rotates the plane.
        let radial = Vec3::new(1.0, 1.0, 0.0).normalize();
        let d = strike_direction(Vec3::new(0.3, 0.1, 0.9), radial);
        assert!(d.dot(radial).abs() < 1e-3 && (d.length() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn strike_points_stay_inside_the_stream_range_around_its_centre() {
        let avatar = Vec2::new(10.0, -4.0);
        let dir = Vec2::new(0.0, 1.0);
        for (stream, k) in [(0usize, 0.0f32), (0, 0.9), (1, 0.0), (1, 0.5), (1, 0.99)] {
            let p = strike_point(avatar, dir, stream, [k, k * 7.0 % 1.0]);
            let centre = avatar + dir * STRIKE_CENTER[stream];
            assert!(p.distance(centre) <= STRIKE_RANGE[stream] + 1e-3);
        }
        // The big meteor lands 130 ahead (within 1), the small ones around 15.
        let p = strike_point(avatar, dir, 0, [0.5, 0.25]);
        assert!((p.y - (-4.0 + 130.0)).abs() <= 1.01);
    }

    #[test]
    fn runtime_rolls_are_uniform_unit_floats() {
        let mut rng = Rng(1234);
        let (mut lo, mut hi, mut sum) = (1.0f32, 0.0f32, 0.0f32);
        for _ in 0..20_000 {
            let r = rng.roll();
            assert!((0.0..1.0).contains(&r));
            lo = lo.min(r);
            hi = hi.max(r);
            sum += r;
        }
        assert!(lo < 0.01 && hi > 0.99 && (sum / 20_000.0 - 0.5).abs() < 0.02);
    }

    #[test]
    #[ignore = "requires the installed Spore packages"]
    fn installed_event_templates_match_the_table() {
        let install = crate::spore_install::discover().expect("original Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        assert!(index.errors.is_empty(), "package index errors: {:?}", index.errors);
        let get = |p: &crate::prop::PropertyList, id: u32| p.0.get(&id).and_then(|p| p.number());
        for def in EVENTS {
            let key = crate::dbpf::ResourceKey {
                group_id: EVENT_GROUP,
                instance_id: def.instance,
                type_id: crate::asset_catalog::PROP,
            };
            assert_eq!(def.instance, crate::game::spore_hash(def.name), "{}", def.name);
            let props = index.properties(key).unwrap_or_else(|e| panic!("{}: {e}", def.name));
            assert_eq!(get(&props, 0xB10F_785D), Some(match def.kind {
                EventKind::NightPredators => 1.0,
                EventKind::UfoAbduction => 2.0,
                EventKind::MeteorStrike => 3.0,
                EventKind::BabyGame => 4.0,
                EventKind::RainStorm => 5.0,
                EventKind::SeaMonster => 6.0,
                EventKind::SummonCreature => 8.0,
            }), "{} type", def.name);
            assert_eq!(get(&props, 0x920A_2318), Some(def.check_time), "{} CheckTime", def.name);
            assert_eq!(get(&props, 0xB1F4_2539), Some(def.duration), "{} duration", def.name);
            let activation = match def.activation {
                Activation::Condition => 0.0,
                Activation::StageDna => 2.0,
                Activation::TimeOfDay => 3.0,
                Activation::Explicit => 4.0,
            };
            assert_eq!(get(&props, 0x06BF_E2ED), Some(activation), "{} ActivationType", def.name);
            let freq = props.0.get(&0x332C_289D).and_then(|p| p.values.first()).map(|v| {
                u32::from_be_bytes(v[..4].try_into().unwrap()) as i32
            });
            assert_eq!(freq, Some(def.frequency), "{} frequency", def.name);
            let mask = get(&props, 0xC55C_98BB).unwrap() as u32;
            assert_eq!(if mask == 0 { 0 } else { 1 << mask }, def.mask, "{} EventMask", def.name);
            let threshold = props.0.get(&0xD2E8_04EE).and_then(|p| p.values.first()).map(|v| {
                (
                    f32::from_le_bytes(v[..4].try_into().unwrap()),
                    f32::from_le_bytes(v[4..8].try_into().unwrap()),
                )
            });
            assert_eq!(threshold, Some(def.threshold), "{} ActivationThreshold", def.name);
        }
    }
}
