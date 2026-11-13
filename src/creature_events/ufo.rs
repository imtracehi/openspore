//! The UFO abduction event of the Creature stage (`cUFOAbductionEvent`, helper
//! `creature-ufo`, notes `creature-events-notes.md` E4).
//!
//! Retail (march2017 addresses, read with `swarm-volume\target\re\dis.py`; the
//! event's class vtable is 0x147B1D8):
//! * Template `UFOAbduction` (group 0x0EBCFBC7): type 2, ActivationType 2
//!   (stage DNA 800..850), CheckTime 10, duration 90, frequency 1, EventMask 1
//!   (the table in `creature_events`), and the tuning
//!   `abductionUFOAvatarDistance` 75, `abductionUFOSteadyAltitude` 15,
//!   `abductionUFOAbductionAltitude` 15, `abductionUFOMoveTime` 2.5,
//!   `abductionUFOMinAbductedCreatureDistanceFromAvatar` 5,
//!   `abductionUFONumCreatures` 3, `chanceAbductionUFOIsGrob` 0.25 and the float
//!   0x9650F75F = 50 (the query radius below).
//! * Start 0xD4F8A0: with a non-zero avatar position P it rolls the Grob chance
//!   (`chance >= random`), creates the vehicle with 0xD4C150 -> `SpawnUFO`
//!   0x102ACC0 (UFO type 7, `cGameDataUFO::Initialize` 0xC3E210 whose type 7 case
//!   adds no weapon) and teleports it (0xC3ABA0) to `P + up * SteadyAltitude`
//!   (zoom altitude |P| + 15, 0xC3DC00), adds goal 2 to its goal list with the
//!   event duration as its time out (0xBC9960), stores `abductionUFONumCreatures`
//!   as the creatures still to take (event map [0], read 0xD51650, written
//!   0xD51680), registers the vehicle (0xD4ED80) and sets `crg_ufo` 1.0 (0xD4FABC,
//!   Creature mode). The restore of a saved event (0xD4D130) does the same.
//! * Update 0xD4FBA0: runs the vehicle's behaviours (`cGameDataUFO` 0xC388A0 ->
//!   0xBC9AF0 goal timers, 0xBC9160 behaviour engine) and destroys it when its
//!   goal 2 is gone. IsDone 0xD4D1D0: the duration timer < 0, or the vehicle is
//!   missing, or it lost goal 2. Stop 0xD4FAD0: **`crg_ufo` 0.0** (the inlined
//!   0x5CA970 with `fldz`, 0xD4FAF4) and the vehicle is destroyed
//!   (0x102C350), so `mix_event_crg_ufo` plays `sfx_ufo_depart`.
//! * The behaviours of goal 2 (0xDC26B0, table row 0x0519E372, enter 0xDBF1A0:
//!   timer 2.5, flag 0, state 0) and of goal 1 (0xDC1500, row 0x0519ABF8, enter
//!   0xDC1430 / exit 0xDC14D0) are [`Patrol`] and [`Abduct`] below.
//!
//! ADAPTED (not traced): which behaviour the engine 0xBC9160 runs when both goals
//! are set (goal 1 replaces goal 2 until it ends, then goal 2 is entered again
//! from its start), the UFO's flight (retail: `cLocomotiveObject` 0xC3BFD0
//! SetDestination asks the locomotion for max speed `SpaceUFOKinesthetics`
//! 0x0331E543 (6) x `GetScale()`, the NPC UFO's scale is its display strategy's),
//! the model (retail: the Sporepedia template database's first `ufo` creation,
//! 0x102C800; the install holds only `genufo`), the Grob variant (same model),
//! the abduction itself (the creature's `cSpaceGameBehaviorAbducted` / the
//! `cAbductionBeamLocomotion` run when the UFO's combatant target is set,
//! 0xDC1659; their update 0x1032BD0 is the Space stage's and was not decoded: the
//! creature is lifted into the UFO under the abduct tool's beam and ground
//! effects over [`LIFT_SECONDS`]), the order of the retail view query's result
//! (the last creature found wins, 0xDC2B00..0xDC2BF9; here the highest member id)
//! and the species rule (retail: another species than the avatar's).

use bevy::prelude::*;

use crate::creature_actions::{CreatureMind, Encounter};
use crate::creature_flow::MixEvents;
use crate::game::{Health, Npc, Player, RunProgress};
use crate::persistence::WorldId;
use crate::stages::StageOwned;
use crate::swarm_fx::SwarmEffect;

/// `chanceAbductionUFOIsGrob` (0x51BE5FAE).
pub const CHANCE_GROB: f32 = 0.25;
/// `abductionUFOAvatarDistance` (0x0005F8CC): how far from the avatar the UFO
/// moves in states 1 to 3 of [`Patrol`].
pub const AVATAR_DISTANCE: f32 = 75.0;
/// `abductionUFOSteadyAltitude` (0x9B0EF346): the UFO's height above the ground
/// while it patrols (and when it is created, 0xD4C150).
pub const STEADY_ALTITUDE: f32 = 15.0;
/// `abductionUFOAbductionAltitude` (0xA58ACB11): its height above a creature it
/// abducts (0xDC1554).
pub const ABDUCTION_ALTITUDE: f32 = 15.0;
/// `abductionUFOMoveTime` (0x34E39082): seconds between two moves of [`Patrol`].
pub const MOVE_TIME: f32 = 2.5;
/// `abductionUFOMinAbductedCreatureDistanceFromAvatar` (0x829DF2E1).
pub const MIN_CREATURE_DISTANCE: f32 = 5.0;
/// `abductionUFONumCreatures` (0x60BE63B8).
pub const NUM_CREATURES: i32 = 3;
/// The property 0x9650F75F = 50: the radius around the avatar of the creature
/// query (0xDC2AB3, argument of the view manager's 0xB3D340 query vtable +0x48).
pub const SEARCH_RADIUS: f32 = 50.0;
/// The leave altitude is `SteadyAltitude` x 10 (0xDC2D35, constant 0x1473BF0).
pub const LEAVE_FACTOR: f32 = 10.0;
/// Seconds between two moves of [`Abduct`] toward its target (0xDC158D, 4.0).
pub const RETARGET_TIME: f32 = 4.0;
/// [`Abduct`] fires when the UFO is within `sqrt(altitude^2 + 400)` of the creature
/// (0xDC1640, constant 0x13F0E64 = 400).
pub const RANGE_EXTRA: f32 = 400.0;
/// The goal 2 time out and event duration (`duration` 90).
pub const GOAL_SECONDS: f32 = 90.0;

/// ADAPTED: a UFO within this distance of its destination has arrived (retail:
/// the locomotion's stop distances, 2.0 in 0xC3BFD0's request).
pub const ARRIVE_DISTANCE: f32 = 2.0;
/// ADAPTED: the UFO's scale (retail: `cSpatialObject::GetScale` of the NPC UFO).
pub const UFO_SCALE: f32 = 4.0;
/// Retail max speed per unit of scale (`SpaceUFOKinesthetics` 0x0331E543, read
/// into the UFO's stats at 0xC3DD78: `tuning[+0x1C] * GetScale()`).
pub const SPEED_PER_SCALE: f32 = 6.0;
/// The UFO's flight speed (`SPEED_PER_SCALE` x [`UFO_SCALE`]).
pub const UFO_SPEED: f32 = SPEED_PER_SCALE * UFO_SCALE;
/// ADAPTED: the model's largest dimension.
pub const UFO_EXTENT: f32 = 3.0 * UFO_SCALE;
/// ADAPTED: seconds a creature takes to rise into the UFO.
pub const LIFT_SECONDS: f32 = 3.0;

/// The abduct tool's effects (`spacetools~!abduct`, `toolBeamEffectID`,
/// `toolHitGroundEffectID`); the muzzle `SG_ufo_abduct_Muzzle` is not installed.
pub const BEAM_EFFECT_NAME: &str = "SG_ufo_abduct_beam";
pub const HIT_EFFECT_NAME: &str = "SG_ufo_abduct_HitGround";
/// Tag of the effects (`SwarmStop`).
pub const TAG_UFO: u64 = 0xC4E0_0000_0000_0003;

// ------------------------------------------------------------------ behaviours

/// What the behaviours ask of the world.
pub trait Ground {
    /// `cPlanetModel::ToSurface` (0xB81780): the surface point of `p`.
    fn surface(&self, p: Vec3) -> Vec3;
    /// The local up (away from the planet's centre) at `p`.
    fn up(&self, p: Vec3) -> Vec3;
}

/// The avatar as the behaviours read it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Avatar {
    pub at: Vec3,
    /// The transform's direction (vtable +0x5C).
    pub forward: Vec3,
}

/// A creature the UFO may abduct (the caller applied the rules that need the
/// game: alive, not an ally, not the avatar's species, not busy).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub id: u32,
    pub at: Vec3,
}

/// The behaviour of goal 2 (0xDC26B0): where the UFO goes next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patrol {
    /// Seconds to the next move (`[state + 0]`).
    pub timer: f32,
    /// Set by the leave move (`[state + 4]`).
    pub leaving: bool,
    /// 0: above the avatar, 1: ahead of it, 2: a random side, 3: search.
    pub state: u8,
}

/// What [`Patrol::step`] decided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PatrolStep {
    /// Returns true with nothing to do.
    Wait,
    /// Goal 2 is removed (0xDC27D6): the event destroys the vehicle.
    Done,
    /// Fly to `point` (projected to the surface) at `steady` or, when leaving, `LEAVE_FACTOR x steady`.
    Go { point: Vec3, leave: bool },
    /// State 3 with creatures left: search around the avatar; when none is
    /// found, fly to `wander` like a normal move.
    Search { wander: Vec3 },
}

impl Patrol {
    /// 0xDBF1A0: the behaviour's start (the timer constant 0x148563C = 2.5).
    pub const fn enter() -> Self {
        Self { timer: 2.5, leaving: false, state: 0 }
    }

    /// One frame (0xDC26B0). `random` is `Math::RandomUnitVector` (0xB7E6B0, three
    /// random floats in [-1, 1] normalised).
    pub fn step(
        &mut self,
        dt: f32,
        avatar: Option<Avatar>,
        at_destination: bool,
        left: i32,
        random: &mut dyn FnMut() -> Vec3,
    ) -> PatrolStep {
        // 0xDC274D / 0xDC2777: no avatar, or its position is the zero vector.
        let Some(avatar) = avatar.filter(|a| a.at != Vec3::ZERO) else {
            return PatrolStep::Wait;
        };
        // 0xDC27AF: `mbAtDestination` (0x74C) clear: wait, the timer is not touched.
        if !at_destination {
            return PatrolStep::Wait;
        }
        // 0xDC27BC: the leave move was made: the goal ends.
        if self.leaving {
            return PatrolStep::Done;
        }
        // 0xDC27E8.
        self.timer -= dt;
        if self.timer > 0.0 {
            return PatrolStep::Wait;
        }
        // 0xDC28CC: the timer is reloaded with `abductionUFOMoveTime`.
        self.timer = MOVE_TIME;
        let p = avatar.at;
        match self.state {
            0 => {
                self.state = 1;
                PatrolStep::Go { point: p, leave: false }
            }
            1 => {
                self.state = 2;
                PatrolStep::Go { point: p + avatar.forward * AVATAR_DISTANCE, leave: false }
            }
            2 => {
                self.state = 3;
                PatrolStep::Go { point: p + random() * AVATAR_DISTANCE, leave: false }
            }
            _ => {
                let wander = p + random() * AVATAR_DISTANCE;
                if left <= 0 {
                    // 0xDC2A47 with the done flag: the leave move, then the
                    // goal ends once the UFO has arrived (the 45 s timer
                    // 0xDC2D5C is overwritten by that removal).
                    self.leaving = true;
                    self.timer = STEADY_ALTITUDE * 3.0;
                    PatrolStep::Go { point: wander, leave: true }
                } else {
                    PatrolStep::Search { wander }
                }
            }
        }
    }
}

/// The creature search of state 3 (0xDC2A47..0xDC2C19): inside
/// [`SEARCH_RADIUS`] of the avatar, at least [`MIN_CREATURE_DISTANCE`] from it;
/// the last one of the list wins.
pub fn pick_target(avatar: Vec3, candidates: &[Candidate]) -> Option<Candidate> {
    candidates
        .iter()
        .rev()
        .find(|c| {
            let d = c.at.distance(avatar);
            d <= SEARCH_RADIUS && d * d >= MIN_CREATURE_DISTANCE * MIN_CREATURE_DISTANCE
        })
        .copied()
}

/// The behaviour of goal 1 (0xDC1500): follow a creature and take it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Abduct {
    pub timer: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AbductStep {
    /// The target is gone: one creature fewer to take (0xDC1690), the behaviour ends.
    Lost,
    Follow {
        /// Fly to the creature, `ABDUCTION_ALTITUDE` above it (0xDC159A..0xDC15DD).
        retarget: bool,
        /// Within range: `cCombatant::SetTarget` (0xDC1659), the abduction.
        in_range: bool,
    },
}

impl Abduct {
    /// 0xDC1430 / 0xDC149E: the timer starts at 0.
    pub const fn enter() -> Self {
        Self { timer: 0.0 }
    }

    pub fn step(&mut self, dt: f32, target: Option<Vec3>, ufo: Vec3) -> AbductStep {
        let Some(target) = target else {
            return AbductStep::Lost;
        };
        self.timer -= dt;
        let retarget = self.timer <= 0.0;
        if retarget {
            self.timer = RETARGET_TIME;
        }
        let in_range = ufo.distance_squared(target) <= ABDUCTION_ALTITUDE * ABDUCTION_ALTITUDE + RANGE_EXTRA;
        AbductStep::Follow { retarget, in_range }
    }
}

// ------------------------------------------------------------------- the UFO

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Patrol(Patrol),
    Abduct(Abduct, u32),
}

/// A creature rising under the beam.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lift {
    pub id: u32,
    pub age: f32,
    /// Where the creature stood when the beam took it.
    pub ground: Vec3,
}

/// What the UFO did this frame that the world has to show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    /// The abduction beam starts on creature `id`.
    Beam(u32),
    /// The creature reached the UFO: remove it.
    Taken(u32),
    /// Goal 2 ended: the vehicle is destroyed.
    Departed,
}

/// The vehicle and its goals (`cGameDataUFO` +0x6C8 goal list, behaviour states).
#[derive(Clone, Debug, PartialEq)]
pub struct UfoSim {
    pub pos: Vec3,
    pub dest: Vec3,
    pub at_destination: bool,
    /// Creatures still to take (event map [0]).
    pub left: i32,
    /// Seconds of goal 2 left.
    pub goal2: f32,
    pub mode: Mode,
    pub lift: Option<Lift>,
    pub grob: bool,
    /// Flight speed (ADAPTED, [`UFO_SPEED`]).
    pub speed: f32,
}

impl UfoSim {
    /// 0xD4F8A0 / 0xD4C150: the UFO is created `STEADY_ALTITUDE` above the avatar.
    pub fn start(avatar: Vec3, ground: &dyn Ground, grob_roll: f32) -> Self {
        let pos = avatar + ground.up(avatar) * STEADY_ALTITUDE;
        Self {
            pos,
            dest: pos,
            at_destination: false,
            left: NUM_CREATURES,
            goal2: GOAL_SECONDS,
            mode: Mode::Patrol(Patrol::enter()),
            lift: None,
            grob: CHANCE_GROB >= grob_roll,
            speed: UFO_SPEED,
        }
    }

    /// 0xC3BFD0 SetDestination + 0xC3DC00 SetZoomAltitude: fly to `point` held at `altitude`
    /// above the surface.
    pub fn set_destination(&mut self, point: Vec3, altitude: f32, ground: &dyn Ground) {
        let surface = ground.surface(point);
        self.dest = surface + ground.up(surface) * altitude;
        self.at_destination = false;
    }

    /// The position of the creature under the beam, if any.
    pub fn target_position(&self, candidates: &[Candidate], id: u32) -> Option<Vec3> {
        match self.lift {
            Some(l) if l.id == id => Some(l.ground),
            _ => candidates.iter().find(|c| c.id == id).map(|c| c.at),
        }
    }

    /// Height of the lifted creature above its ground point.
    pub fn lift_height(&self, ground: &dyn Ground) -> Option<(u32, Vec3)> {
        let lift = self.lift?;
        let up = ground.up(lift.ground);
        let top = (self.pos - lift.ground).dot(up) - UFO_EXTENT * 0.35;
        let t = (lift.age / LIFT_SECONDS).clamp(0.0, 1.0);
        let s = t * t * (3.0 - 2.0 * t);
        Some((lift.id, lift.ground + up * (top.max(0.0) * s)))
    }

    /// One frame of the event update (0xD4FBA0): the behaviours, then the flight.
    pub fn step(
        &mut self,
        dt: f32,
        avatar: Option<Avatar>,
        candidates: &[Candidate],
        ground: &dyn Ground,
        random: &mut dyn FnMut() -> Vec3,
    ) -> Vec<Out> {
        let mut out = Vec::new();
        // 0xBC9AF0: the goal timer.
        self.goal2 -= dt;
        if self.goal2 <= 0.0 {
            out.push(Out::Departed);
            return out;
        }
        match self.mode {
            Mode::Patrol(mut patrol) => {
                let step = patrol.step(dt, avatar, self.at_destination, self.left, random);
                self.mode = Mode::Patrol(patrol);
                match step {
                    PatrolStep::Wait => {}
                    PatrolStep::Done => {
                        out.push(Out::Departed);
                        return out;
                    }
                    PatrolStep::Go { point, leave } => {
                        let altitude = if leave { STEADY_ALTITUDE * LEAVE_FACTOR } else { STEADY_ALTITUDE };
                        self.set_destination(point, altitude, ground);
                    }
                    PatrolStep::Search { wander } => {
                        let found = avatar.and_then(|a| pick_target(a.at, candidates));
                        match found {
                            // 0xDC2C55: goal 1 with the creature; the UFO stops
                            // (0xDC1496) and the abduct behaviour starts.
                            Some(c) => self.mode = Mode::Abduct(Abduct::enter(), c.id),
                            None => self.set_destination(wander, STEADY_ALTITUDE, ground),
                        }
                    }
                }
            }
            Mode::Abduct(mut behaviour, id) => {
                let target = self.target_position(candidates, id);
                match behaviour.step(dt, target, self.pos) {
                    AbductStep::Lost => {
                        // 0xDC1668: goal 2 is still there: one creature fewer.
                        self.left -= 1;
                        self.lift = None;
                        self.mode = Mode::Patrol(Patrol::enter());
                    }
                    AbductStep::Follow { retarget, in_range } => {
                        self.mode = Mode::Abduct(behaviour, id);
                        if let Some(t) = target {
                            if retarget {
                                self.set_destination(t, ABDUCTION_ALTITUDE, ground);
                            }
                            if in_range && self.lift.is_none() {
                                self.lift = Some(Lift { id, age: 0.0, ground: t });
                                out.push(Out::Beam(id));
                            }
                        }
                    }
                }
            }
        }
        // The beam: the creature rises, then it is taken.
        if let Some(lift) = self.lift.as_mut() {
            lift.age += dt;
            if lift.age >= LIFT_SECONDS {
                out.push(Out::Taken(lift.id));
                self.lift = None;
            }
        }
        self.fly(dt, self.speed);
        out
    }

    /// ADAPTED flight: straight toward the destination at `speed`.
    pub fn fly(&mut self, dt: f32, speed: f32) {
        let to = self.dest - self.pos;
        let distance = to.length();
        if distance > ARRIVE_DISTANCE {
            self.pos += to / distance * (speed * dt).min(distance);
        }
        if self.dest.distance(self.pos) <= ARRIVE_DISTANCE {
            self.at_destination = true;
        }
    }

    /// The same rigid move for every stored point (a re-anchor of the planet).
    pub fn shift(&mut self, f: impl Fn(Vec3) -> Vec3) {
        self.pos = f(self.pos);
        self.dest = f(self.dest);
        if let Some(l) = self.lift.as_mut() {
            l.ground = f(l.ground);
        }
    }
}

// --------------------------------------------------------------------- Bevy

/// The abduction UFO's body.
#[derive(Component)]
pub struct UfoVehicle;

/// The beam and ground effect entities of an abduction.
#[derive(Component)]
pub struct UfoBeam {
    pub ground_hit: bool,
}

#[derive(Component)]
pub struct UfoMounted;

/// The parent of the vehicle (see `Run::holder`).
#[derive(Component)]
pub struct UfoHolder;

/// A creature under the beam: it is held still (`CreatureMind::stunned`).
#[derive(Component)]
pub struct Abducted;

struct Run {
    sim: UfoSim,
    /// A root without `StageOwned`: `terrain::ground` snaps every `StageOwned` root to a fixed
    /// clearance over the terrain and the planet re-anchor turns them, neither fits a UFO that
    /// changes its altitude, so the UFO (and the creature in its beam) are children of this holder
    /// (identity transform: local = world); it is despawned with the run (`leave` on exit).
    holder: Entity,
    entity: Entity,
    beams: Vec<Entity>,
    /// The creature entity under the beam.
    lifted: Option<(Entity, u32)>,
}

/// The event's state (not saved: a running abduction does not resume).
#[derive(Resource, Default)]
pub struct UfoEvent {
    run: Option<Run>,
    /// This activation of the event spawned (or failed to spawn) its UFO.
    handled: bool,
    /// The vehicle is gone: `IsDone` (0xD4D1D0).
    ended: bool,
    rng: super::Rng,
    reanchored: u32,
    /// Creatures taken this visit (tests, debugging).
    pub taken: Vec<u32>,
}

impl UfoEvent {
    /// 0xD4D1D0: the vehicle is missing or lost goal 2.
    pub fn finished(&self) -> bool {
        self.ended
    }

    pub fn present(&self) -> bool {
        self.run.is_some()
    }

    pub fn position(&self) -> Option<Vec3> {
        self.run.as_ref().map(|r| r.sim.pos)
    }

    pub fn sim(&self) -> Option<&UfoSim> {
        self.run.as_ref().map(|r| &r.sim)
    }

    pub fn sim_mut(&mut self) -> Option<&mut UfoSim> {
        self.run.as_mut().map(|r| &mut r.sim)
    }

    /// The vehicle's entity.
    pub fn entity(&self) -> Option<Entity> {
        self.run.as_ref().map(|r| r.entity)
    }
}

/// `Math::RandomUnitVector` (0xB7E6B0): three floats in [-1, 1], normalised.
fn random_unit(rng: &mut super::Rng) -> Vec3 {
    loop {
        let v = Vec3::new(rng.roll() * 2.0 - 1.0, rng.roll() * 2.0 - 1.0, rng.roll() * 2.0 - 1.0);
        if v.length_squared() > 1e-4 {
            return v.normalize();
        }
    }
}

struct LandGround(crate::creature_planet::Land);

impl Ground for LandGround {
    fn surface(&self, p: Vec3) -> Vec3 {
        self.0.world_point(Vec2::new(p.x, p.z))
    }

    fn up(&self, p: Vec3) -> Vec3 {
        self.0.radial(p)
    }
}

/// Leaving the stage: the UFO and its holder go (the stage's own entities are despawned by
/// `StageOwned`), `crg_ufo` is released.
pub(super) fn leave(mut commands: Commands, mut ufo: ResMut<UfoEvent>, mix: Option<ResMut<MixEvents>>) {
    if let Some(r) = ufo.run.take() {
        commands.entity(r.holder).despawn();
    }
    *ufo = UfoEvent::default();
    if let Some(mut mix) = mix {
        if mix.0.contains_key("crg_ufo") {
            mix.set("crg_ufo", false);
        }
    }
}

pub(super) fn reset(mut ufo: ResMut<UfoEvent>, mix: Option<ResMut<MixEvents>>) {
    *ufo = UfoEvent::default();
    if let Some(mut mix) = mix {
        if mix.0.contains_key("crg_ufo") {
            mix.set("crg_ufo", false);
        }
    }
}

/// Mount the installed `genufo` creation as the UFO's body (ADAPTED: see the
/// header), once per vehicle.
pub(super) fn mount_visual(
    mut commands: Commands,
    originals: Option<ResMut<crate::Originals>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    images: Option<ResMut<Assets<Image>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    ufos: Query<Entity, (With<UfoVehicle>, Without<UfoMounted>)>,
) {
    if ufos.is_empty() {
        return;
    }
    let (Some(mut originals), Some(mut meshes), Some(mut images), Some(mut materials)) = (originals, meshes, images, materials)
    else {
        // Headless: the vehicle has no body (its rules run all the same).
        for entity in &ufos {
            commands.entity(entity).insert(UfoMounted);
        }
        return;
    };
    for entity in &ufos {
        commands.entity(entity).insert(UfoMounted);
        match crate::creation_assets::mount_preset(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            entity,
            crate::creation_assets::PresetKind::Spaceship,
            UFO_EXTENT,
        ) {
            Ok(true) => {}
            Ok(false) => warn!("UFO abduction: no UFO creation is installed, the vehicle stays invisible"),
            Err(error) => warn!("UFO abduction: could not mount the UFO creation: {error}"),
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn run_ufo(
    mut commands: Commands,
    time: Res<Time>,
    rt: Res<super::EventRuntime>,
    mut ufo: ResMut<UfoEvent>,
    mix: Option<ResMut<MixEvents>>,
    mut run: ResMut<RunProgress>,
    encounter: Option<Res<Encounter>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    reanchored: Option<Res<crate::creature_planet::Reanchored>>,
    player: Query<(&Transform, &Health), (With<Player>, Without<UfoVehicle>, Without<Npc>)>,
    mut vehicles: Query<&mut Transform, (With<UfoVehicle>, Without<Player>, Without<Npc>)>,
    mut npcs: Query<
        (Entity, &mut Transform, &Npc, &WorldId, Option<&mut CreatureMind>),
        (Without<Player>, Without<UfoVehicle>),
    >,
    mut beams: Query<(&mut SwarmEffect, &UfoBeam)>,
) {
    let mut mix = mix;
    let active = rt.is_active(super::EventKind::UfoAbduction);
    if !active {
        // The event stopped (0xD4FAD0): `crg_ufo` 0, the vehicle is destroyed.
        if let Some(r) = ufo.run.take() {
            teardown(&mut commands, r, &mut npcs, &mut beams);
        }
        if ufo.handled {
            if let Some(mix) = mix.as_mut() {
                mix.set("crg_ufo", false);
            }
        }
        ufo.handled = false;
        ufo.ended = false;
        return;
    }
    let dt = time.delta_secs();
    if !dt.is_finite() || dt < 0.0 {
        return;
    }
    let land = crate::creature_planet::Land::from_surface(surface.as_deref());
    let ground = LandGround(land);
    let avatar = player.single().ok().map(|(t, h)| (Avatar { at: t.translation, forward: t.rotation * Vec3::NEG_Z }, !h.dead()));

    // 0xD4F8A0: the start.
    if !ufo.handled {
        ufo.handled = true;
        if ufo.rng.0 == 0 {
            ufo.rng = super::Rng(run.species_world.seed ^ 0x0FED_C0DE_1B0A_7F05);
        }
        let Some((avatar, _)) = avatar.filter(|(a, _)| a.at != Vec3::ZERO) else {
            // No avatar position: nothing is created, the event ends at once (IsDone).
            ufo.ended = true;
            return;
        };
        let roll = ufo.rng.roll();
        let sim = UfoSim::start(avatar.at, &ground, roll);
        let mut transform = Transform::from_translation(sim.pos);
        transform.rotation = Quat::from_rotation_arc(Vec3::Y, ground.up(avatar.at));
        let holder = commands.spawn((UfoHolder, Transform::IDENTITY, Visibility::Inherited, Name::new("UFO holder"))).id();
        let entity = commands
            .spawn((ChildOf(holder), UfoVehicle, transform, Visibility::Inherited, Name::new("UFO")))
            .id();
        info!("creature event UFOAbduction: the UFO arrives{}", if sim.grob { " (Grob)" } else { "" });
        if let Some(mix) = mix.as_mut() {
            // 0xD4FABC, Creature mode only: `crg_ufo` 1.0.
            mix.set("crg_ufo", true);
        }
        ufo.run = Some(Run { sim, holder, entity, beams: Vec::new(), lifted: None });
        ufo.reanchored = reanchored.as_ref().map_or(0, |r| r.generation);
        return;
    }
    let ufo = &mut *ufo;
    let Some(r) = ufo.run.as_mut() else {
        return;
    };
    // A re-anchor of the planet moved everything that stands on it.
    if let Some(re) = reanchored.as_ref() {
        if re.generation != ufo.reanchored {
            ufo.reanchored = re.generation;
            r.sim.shift(|p| re.apply(p));
        }
    }

    // The creatures the UFO may take (the rules that need the game).
    let avatar_data = avatar.filter(|(_, alive)| *alive).map(|(a, _)| a);
    let busy = encounter.as_ref().and_then(|e| e.target);
    let mut candidates: Vec<(Candidate, Entity)> = npcs
        .iter()
        .filter(|(_, _, npc, id, mind)| {
            npc.hp > 0.0
                && Some(id.0) != busy
                && mind.as_ref().is_none_or(|m| !m.allied && !m.defeated && m.downed <= 0.0 && m.stunned <= 0.0)
        })
        .map(|(e, t, _, id, _)| (Candidate { id: id.0, at: t.translation }, e))
        .collect();
    candidates.sort_by_key(|(c, _)| c.id);
    let list: Vec<Candidate> = candidates.iter().map(|(c, _)| *c).collect();
    let mut rng = ufo.rng;
    let (was_dest, was_mode) = (r.sim.dest, std::mem::discriminant(&r.sim.mode));
    let outs = r.sim.step(dt, avatar_data, &list, &ground, &mut || random_unit(&mut rng));
    ufo.rng = rng;
    if r.sim.dest != was_dest || std::mem::discriminant(&r.sim.mode) != was_mode {
        info!(
            "creature event UFOAbduction: the UFO {} -> {:.0}, {:.0} from the avatar ({} left to take)",
            if matches!(r.sim.mode, Mode::Abduct(..)) { "follows a creature" } else { "moves" },
            r.sim.dest,
            avatar_data.map_or(0.0, |a| a.at.distance(r.sim.dest)),
            r.sim.left
        );
    }

    let mut departed = false;
    for out in outs {
        match out {
            Out::Beam(id) => {
                let Some((_, entity)) = candidates.iter().find(|(c, _)| c.id == id).copied() else { continue };
                // The creature is held still while the beam has it.
                if let Ok((_, _, _, _, Some(mut mind))) = npcs.get_mut(entity) {
                    mind.stunned = 1.0e6;
                }
                // Out of `terrain::ground`'s reach while it rises (see `Run::holder`).
                commands.entity(entity).insert((Abducted, ChildOf(r.holder)));
                r.lifted = Some((entity, id));
                for (name, hit) in [(BEAM_EFFECT_NAME, false), (HIT_EFFECT_NAME, true)] {
                    let mut effect = SwarmEffect::new(crate::world_objects::name_id(name));
                    effect.tag = TAG_UFO;
                    effect.scale = 1.0;
                    let e = commands.spawn((StageOwned, effect, UfoBeam { ground_hit: hit })).id();
                    r.beams.push(e);
                }
            }
            Out::Taken(id) => {
                if let Some((entity, lifted)) = r.lifted.take() {
                    if lifted == id {
                        // The creature is gone for good (never respawned), no DNA.
                        run.species_world.record_defeat(id);
                        run.request_save();
                        commands.entity(entity).despawn();
                        ufo.taken.push(id);
                        info!("creature event UFOAbduction: creature {id} abducted");
                    }
                }
                end_beam(&mut r.beams, &mut beams);
                r.sim.lift = None;
            }
            Out::Departed => departed = true,
        }
    }

    // Show the UFO, the rising creature and the beam.
    let up = ground.up(r.sim.pos);
    if let Ok(mut t) = vehicles.get_mut(r.entity) {
        let to = r.sim.dest - r.sim.pos;
        let flat = (to - up * to.dot(up)).try_normalize();
        t.translation = r.sim.pos;
        let facing = flat.map(|f| Transform::IDENTITY.looking_to(f, up).rotation).unwrap_or(t.rotation);
        t.rotation = t.rotation.slerp(facing, (dt * 3.0).clamp(0.0, 1.0));
    }
    if let (Some((id, at)), Some((entity, _))) = (r.sim.lift_height(&ground), r.lifted) {
        let _ = id;
        if let Ok((_, mut t, _, _, _)) = npcs.get_mut(entity) {
            t.translation = at;
        }
        let from = r.sim.pos - up * (UFO_EXTENT * 0.45);
        for e in &r.beams {
            if let Ok((mut effect, beam)) = beams.get_mut(*e) {
                if beam.ground_hit {
                    effect.position = r.sim.lift.map_or(at, |l| l.ground);
                    effect.rotation = Quat::from_rotation_arc(Vec3::Y, up);
                } else {
                    effect.position = from;
                    effect.path = vec![from, at];
                }
            }
        }
    }
    if departed {
        info!("creature event UFOAbduction: the UFO leaves ({} creatures taken)", ufo.taken.len());
        if let Some(r) = ufo.run.take() {
            teardown(&mut commands, r, &mut npcs, &mut beams);
        }
        ufo.ended = true;
    }
}

fn end_beam(entities: &mut Vec<Entity>, beams: &mut Query<(&mut SwarmEffect, &UfoBeam)>) {
    for e in entities.drain(..) {
        if let Ok((mut effect, _)) = beams.get_mut(e) {
            effect.stop = true;
        }
    }
}

#[allow(clippy::type_complexity)]
fn teardown(
    commands: &mut Commands,
    mut r: Run,
    npcs: &mut Query<
        (Entity, &mut Transform, &Npc, &WorldId, Option<&mut CreatureMind>),
        (Without<Player>, Without<UfoVehicle>),
    >,
    beams: &mut Query<(&mut SwarmEffect, &UfoBeam)>,
) {
    end_beam(&mut r.beams, beams);
    if let Some((entity, _)) = r.lifted.take() {
        // The beam lets go of a creature still in it.
        if let Ok((_, _, _, _, Some(mut mind))) = npcs.get_mut(entity) {
            mind.stunned = 0.0;
        }
        commands.entity(entity).remove::<(Abducted, ChildOf)>();
    }
    commands.entity(r.holder).despawn();
}

// -------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat;

    impl Ground for Flat {
        fn surface(&self, p: Vec3) -> Vec3 {
            Vec3::new(p.x, 0.0, p.z)
        }

        fn up(&self, _: Vec3) -> Vec3 {
            Vec3::Y
        }
    }

    fn avatar() -> Avatar {
        Avatar { at: Vec3::new(10.0, 0.3, 20.0), forward: Vec3::NEG_Z }
    }

    #[test]
    fn the_ufo_is_created_above_the_avatar_and_may_be_a_grob() {
        let a = avatar();
        let sim = UfoSim::start(a.at, &Flat, 0.1);
        assert_eq!(sim.pos, a.at + Vec3::Y * STEADY_ALTITUDE);
        assert!(sim.grob, "chance 0.25 >= roll 0.1");
        assert!(!UfoSim::start(a.at, &Flat, 0.3).grob);
        assert_eq!((sim.left, sim.goal2), (3, 90.0));
        assert_eq!(sim.mode, Mode::Patrol(Patrol { timer: 2.5, leaving: false, state: 0 }));
    }

    #[test]
    fn the_patrol_moves_every_move_time_once_it_has_arrived() {
        let mut p = Patrol::enter();
        let a = avatar();
        let mut random = || Vec3::X;
        // Away from its destination the timer does not run.
        for _ in 0..100 {
            assert_eq!(p.step(0.1, Some(a), false, 3, &mut random), PatrolStep::Wait);
        }
        assert_eq!(p.timer, 2.5);
        // At the destination: 2.5 s to the first move (above the avatar).
        let mut t = 0.0;
        let first = loop {
            t += 0.1;
            let s = p.step(0.1, Some(a), true, 3, &mut random);
            if s != PatrolStep::Wait {
                break s;
            }
        };
        assert!((2.4..2.7).contains(&t), "{t}");
        assert_eq!(first, PatrolStep::Go { point: a.at, leave: false });
        assert_eq!(p.timer, MOVE_TIME);
        // State 1: 75 ahead of the avatar; state 2: a random side; state 3: search.
        let next = |p: &mut Patrol, left| loop {
            let s = p.step(0.5, Some(a), true, left, &mut || Vec3::X);
            if s != PatrolStep::Wait {
                return s;
            }
        };
        assert_eq!(next(&mut p, 3), PatrolStep::Go { point: a.at + Vec3::NEG_Z * 75.0, leave: false });
        assert_eq!(next(&mut p, 3), PatrolStep::Go { point: a.at + Vec3::X * 75.0, leave: false });
        assert_eq!(next(&mut p, 3), PatrolStep::Search { wander: a.at + Vec3::X * 75.0 });
        // Still searching while creatures are left; with none left it leaves, then ends.
        assert_eq!(next(&mut p, 1), PatrolStep::Search { wander: a.at + Vec3::X * 75.0 });
        assert_eq!(next(&mut p, 0), PatrolStep::Go { point: a.at + Vec3::X * 75.0, leave: true });
        assert!(p.leaving);
        assert_eq!(p.step(0.016, Some(a), false, 0, &mut random), PatrolStep::Wait);
        assert_eq!(p.step(0.016, Some(a), true, 0, &mut random), PatrolStep::Done);
        // No avatar (or at the zero vector): nothing happens.
        let mut q = Patrol::enter();
        assert_eq!(q.step(10.0, None, true, 3, &mut random), PatrolStep::Wait);
        let zero = Avatar { at: Vec3::ZERO, forward: Vec3::Z };
        assert_eq!(q.step(10.0, Some(zero), true, 3, &mut random), PatrolStep::Wait);
        assert_eq!(q.timer, 2.5);
    }

    #[test]
    fn the_search_takes_the_last_creature_between_five_and_fifty_from_the_avatar() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        let c = |id, x: f32| Candidate { id, at: Vec3::new(x, 0.0, 0.0) };
        assert_eq!(pick_target(a, &[]), None);
        // Too close (< 5) and too far (> 50) are skipped.
        assert_eq!(pick_target(a, &[c(1, 4.9), c(2, 50.1)]), None);
        assert_eq!(pick_target(a, &[c(1, 5.0)]).map(|c| c.id), Some(1));
        assert_eq!(pick_target(a, &[c(1, 20.0), c(2, 3.0), c(3, 49.0), c(4, 80.0)]).map(|c| c.id), Some(3));
    }

    #[test]
    fn the_abduct_behaviour_follows_the_creature_and_fires_in_range() {
        let target = Vec3::new(30.0, 0.0, 0.0);
        let mut b = Abduct::enter();
        // The first frame moves at once (timer 0), out of range.
        let ufo = Vec3::new(0.0, 15.0, 0.0);
        assert_eq!(b.step(0.016, Some(target), ufo), AbductStep::Follow { retarget: true, in_range: false });
        // Then every 4 s.
        let mut retargets = 0;
        for _ in 0..(8.0 / 0.016) as usize {
            if let AbductStep::Follow { retarget: true, .. } = b.step(0.016, Some(target), ufo) {
                retargets += 1;
            }
        }
        assert!((1..=3).contains(&retargets), "{retargets}");
        // In range: horizontal distance 20 at altitude 15 is sqrt(625).
        let near = Vec3::new(19.9, 15.0, 0.0);
        assert_eq!(b.step(0.016, Some(Vec3::ZERO), near), AbductStep::Follow { retarget: false, in_range: true });
        let far = Vec3::new(20.1, 15.0, 0.0);
        assert_eq!(b.step(0.016, Some(Vec3::ZERO), far), AbductStep::Follow { retarget: false, in_range: false });
        assert_eq!(b.step(0.016, None, ufo), AbductStep::Lost);
    }

    fn run(sim: &mut UfoSim, seconds: f32, avatar: Avatar, creatures: &mut Vec<Candidate>) -> Vec<(f32, Out)> {
        let mut rng = super::super::Rng(7);
        let mut t = 0.0;
        let mut all = Vec::new();
        while t < seconds {
            let outs = sim.step(1.0 / 30.0, Some(avatar), creatures, &Flat, &mut || random_unit(&mut rng));
            t += 1.0 / 30.0;
            for out in outs {
                if let Out::Taken(id) = out {
                    creatures.retain(|c| c.id != id);
                }
                let leave = out == Out::Departed;
                all.push((t, out));
                if leave {
                    return all;
                }
            }
        }
        all
    }

    #[test]
    fn the_ufo_takes_three_creatures_and_leaves() {
        let a = avatar();
        let mut sim = UfoSim::start(a.at, &Flat, 0.9);
        let mut creatures: Vec<Candidate> = (0..5)
            .map(|i| Candidate { id: 100 + i, at: a.at + Vec3::new(10.0 + 6.0 * i as f32, -0.3, 8.0) })
            .collect();
        let events = run(&mut sim, 95.0, a, &mut creatures);
        let taken: Vec<u32> = events.iter().filter_map(|(_, o)| if let Out::Taken(id) = o { Some(*id) } else { None }).collect();
        assert_eq!(taken, [104, 103, 102], "the last creature of the list first: {events:?}");
        assert_eq!(creatures.len(), 2, "two creatures are left alone");
        let beams = events.iter().filter(|(_, o)| matches!(o, Out::Beam(_))).count();
        assert_eq!(beams, 3);
        let (when, last) = events.last().copied().unwrap();
        assert_eq!(last, Out::Departed, "{events:?}");
        assert_eq!(sim.left, 0, "all three are counted");
        // With these timings (4 patrol states of 2.5 s before every search, the
        // flights, the 3 s lift) the third creature goes at ~66 s: the leave
        // flight is cut short by the 90 s goal time out.
        assert!(when <= 90.1, "{when}");
        // The first beam cannot come before the patrol states 0..3 (4 x 2.5 s).
        let first_beam = events.iter().find(|(_, o)| matches!(o, Out::Beam(_))).unwrap().0;
        assert!(first_beam > 10.0, "{first_beam}");
    }

    #[test]
    fn a_fast_ufo_that_finishes_leaves_by_itself() {
        let a = avatar();
        let mut sim = UfoSim::start(a.at, &Flat, 0.9);
        // Flights that take no time: only the behaviours' timers remain.
        sim.speed = 100.0 * UFO_SPEED;
        let mut creatures: Vec<Candidate> =
            (0..3).map(|i| Candidate { id: 7 + i, at: a.at + Vec3::new(6.0 + i as f32, -0.3, 0.0) }).collect();
        let events = run(&mut sim, 95.0, a, &mut creatures);
        let taken = events.iter().filter(|(_, o)| matches!(o, Out::Taken(_))).count();
        let (when, last) = events.last().copied().unwrap();
        assert_eq!((taken, last), (3, Out::Departed), "{events:?}");
        assert!(creatures.is_empty());
        // 4 states x 2.5 s + the 3 s lift, three times (~39 s), then the patrol states again up to
        // the leave move (10 s, the arrival at once): it goes by itself, well before the time out.
        assert!((45.0..60.0).contains(&when), "{when}");
        assert!(sim.goal2 > 5.0, "it left on its own, not by the time out: {}", sim.goal2);
    }

    #[test]
    fn without_creatures_the_ufo_wanders_until_the_goal_times_out() {
        let a = avatar();
        let mut sim = UfoSim::start(a.at, &Flat, 0.9);
        let events = run(&mut sim, 95.0, a, &mut Vec::new());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Out::Departed);
        assert!((89.9..90.2).contains(&events[0].0), "{}", events[0].0);
        // It stayed around the avatar at the steady altitude.
        assert!(sim.pos.distance(a.at) < 400.0);
    }

    #[test]
    fn the_lifted_creature_rises_to_the_ufo() {
        let a = avatar();
        let ground = Vec3::new(a.at.x + 8.0, 0.0, a.at.z);
        let mut sim = UfoSim::start(a.at, &Flat, 0.9);
        sim.pos = ground + Vec3::Y * ABDUCTION_ALTITUDE;
        sim.lift = Some(Lift { id: 5, age: 0.0, ground });
        let h0 = sim.lift_height(&Flat).unwrap().1.y;
        sim.lift.as_mut().unwrap().age = LIFT_SECONDS * 0.5;
        let h1 = sim.lift_height(&Flat).unwrap().1.y;
        sim.lift.as_mut().unwrap().age = LIFT_SECONDS;
        let h2 = sim.lift_height(&Flat).unwrap().1.y;
        assert_eq!(h0, 0.0);
        assert!(h0 < h1 && h1 < h2);
        assert!((h2 - (ABDUCTION_ALTITUDE - UFO_EXTENT * 0.35)).abs() < 1e-4, "{h2}");
    }

    #[test]
    fn the_event_defaults_are_the_installed_template_values() {
        // `installed_event_templates_match_the_table` checks the table; the tuning
        // values are checked against the installed property by `installed_ufo_tuning_matches`.
        assert_eq!((AVATAR_DISTANCE, STEADY_ALTITUDE, ABDUCTION_ALTITUDE), (75.0, 15.0, 15.0));
        assert_eq!((MOVE_TIME, MIN_CREATURE_DISTANCE, NUM_CREATURES, SEARCH_RADIUS), (2.5, 5.0, 3, 50.0));
        assert_eq!(UFO_SPEED, 24.0);
    }

    fn creature_app() -> App {
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        let mut app = crate::runtime::headless_app(run);
        app.world_mut().resource_mut::<NextState<crate::stages::GameStage>>().set(crate::stages::GameStage::Creature);
        for _ in 0..4 {
            app.update();
        }
        app
    }

    fn avatar_at(app: &mut App) -> Vec3 {
        let world = app.world_mut();
        world.query_filtered::<&Transform, With<Player>>().iter(world).next().map(|t| t.translation).unwrap()
    }

    #[test]
    fn the_event_runs_in_the_stage_sets_crg_ufo_and_takes_a_creature() {
        use super::super::{EventKind, EventRuntime};
        let mut app = creature_app();
        let avatar = avatar_at(&mut app);
        let crg_ufo = |app: &App| app.world().resource::<crate::creature_music::CreatureAudioParams>().0.gfparam("crg_ufo");
        assert_eq!(crg_ufo(&app), Some(0.0));
        // A creature of the herds, put 12 from the avatar.
        let (npc, id) = {
            let world = app.world_mut();
            let mut q = world.query_filtered::<(Entity, &WorldId), With<Npc>>();
            let (e, id) = q.iter(world).next().map(|(e, id)| (e, id.0)).expect("a herd member");
            (e, id)
        };
        app.world_mut().resource_mut::<EventRuntime>().run_for_test(EventKind::UfoAbduction, true, None);
        app.update();
        app.update();
        // The UFO is above the avatar, `crg_ufo` is 1.
        assert!(app.world().resource::<UfoEvent>().present());
        let pos = app.world().resource::<UfoEvent>().position().unwrap();
        assert!((pos - avatar).y > STEADY_ALTITUDE - 1.0 && (pos - avatar).y < STEADY_ALTITUDE + 3.0, "{pos} over {avatar}");
        assert_eq!(app.world().resource::<MixEvents>().0.get("crg_ufo"), Some(&1.0));
        assert_eq!(crg_ufo(&app), Some(1.0));
        let entity = app.world().resource::<UfoEvent>().entity().unwrap();
        assert!(app.world().entity(entity).contains::<UfoVehicle>());
        // Fast flights; the creature stays 12 away until the beam has it.
        app.world_mut().resource_mut::<UfoEvent>().sim_mut().unwrap().speed = 5000.0;
        let mut taken = false;
        for _ in 0..(40 * 60) {
            if !app.world().entity(npc).contains::<Abducted>() {
                let at = avatar_at(&mut app) + Vec3::new(12.0, 0.0, 0.0);
                if let Some(mut t) = app.world_mut().get_mut::<Transform>(npc) {
                    t.translation = at;
                }
            }
            app.update();
            if app.world().resource::<UfoEvent>().taken.contains(&id) {
                taken = true;
                break;
            }
        }
        assert!(taken, "the creature was abducted within 40 s");
        app.update();
        assert!(app.world().get_entity(npc).is_err(), "the abducted creature is gone");
        assert!(
            !app.world().resource::<RunProgress>().species_world.member(id).is_some_and(|m| m.relationship.is_alive()),
            "and never comes back"
        );
        // The event ends: `crg_ufo` 0, the UFO is destroyed.
        app.world_mut().resource_mut::<EventRuntime>().run_for_test(EventKind::UfoAbduction, false, None);
        app.update();
        app.update();
        assert!(!app.world().resource::<UfoEvent>().present());
        assert!(app.world().get_entity(entity).is_err(), "the vehicle is destroyed at the stop");
        assert_eq!(crg_ufo(&app), Some(0.0));
        assert_eq!(app.world().resource::<MixEvents>().0.get("crg_ufo"), Some(&0.0));
    }

    #[test]
    #[ignore = "requires the installed Spore packages"]
    fn installed_ufo_tuning_matches() {
        let install = crate::spore_install::discover().expect("original Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let key = crate::dbpf::ResourceKey {
            group_id: super::super::EVENT_GROUP,
            instance_id: 0x6A81_7A5C,
            type_id: crate::asset_catalog::PROP,
        };
        let props = index.properties(key).expect("UFOAbduction template");
        let num = |id: u32| props.0.get(&id).and_then(|p| p.number()).unwrap_or(f32::NAN);
        assert_eq!(num(0x0005_F8CC), AVATAR_DISTANCE);
        assert_eq!(num(0x9B0E_F346), STEADY_ALTITUDE);
        assert_eq!(num(0xA58A_CB11), ABDUCTION_ALTITUDE);
        assert_eq!(num(0x34E3_9082), MOVE_TIME);
        assert_eq!(num(0x829D_F2E1), MIN_CREATURE_DISTANCE);
        assert_eq!(num(0x60BE_63B8), NUM_CREATURES as f32);
        assert_eq!(num(0x9650_F75F), SEARCH_RADIUS);
        assert_eq!(num(0x51BE_5FAE), CHANCE_GROB);
        // The abduct tool's beam and ground hit.
        let tool = crate::dbpf::ResourceKey { group_id: 0x3060_8F0B, instance_id: 0xF58C_1654, type_id: crate::asset_catalog::PROP };
        let tool = index.properties(tool).expect("abduct tool");
        let effect = |id: u32| tool.0.get(&id).and_then(|p| p.key()).map(|k| k.instance_id);
        assert_eq!(effect(0x1138_C0B9), Some(crate::world_objects::name_id(BEAM_EFFECT_NAME)));
        assert_eq!(effect(0x3138_C0BF), Some(crate::world_objects::name_id(HIT_EFFECT_NAME)));
    }
}
