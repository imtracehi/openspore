//! The player's nest mates: the herd of the player's own species that lives at
//! the nest (notes C4B "Avatarspecies", C1G, `notes\creature-bones-notes.md`).
//!
//! RETAIL (VERIFIED in the installed data; the code that places the herd
//! was not traced):
//! * `herdtypes!avatarspecies` (the archetype of the player's own species):
//!   `NumCreatures` (6, 6), `percentBabiesRange` 0.167 (one baby of six),
//!   `HasNest` 1, `NumNests` 1, `NumGuardLocations` 4 x `guardsPerLocation` 1,
//!   `guardLocationRadiusRange` (10, 15), `guardPositionDurationRange`
//!   (10, 30) s, `initialAvatarRelationship` 6 (best friends: never hostile),
//!   `attacked_SpeciesHelpRadius` 10, `RespawnRate` 5 s, `evoPointsRewarded` 0.
//! * The `CLG2CRG` cinematic (the Creature stage start) adds four buddies of
//!   the player's species ("playerSpecies" actors) that walk to the avatar
//!   and then "around the nest"; its last state (`setupCreature`) snaps the
//!   avatar and the buddies around the nest. The welcome card promises that
//!   the creature is "surrounded by your nest mates".
//! * The manual: nest mates migrate with the nest, and the mate answers the
//!   mating call from the current nest.
//!
//! * DEFENCE (VERIFIED, notes C8B): when a creature is hit (0xC075E0) every
//!   creature around it gets the attack event, and an unflagged responder
//!   answers (0xD8F1C0) when the attacker is within `creatureHelpAllyInRadius`
//!   (10, `attacked_SpeciesHelpRadius` of `avatarspecies` is 10 too, its
//!   `attacked_MaxSpeciesToAlert` 0) of it and it is not too hurt; the attacker
//!   is remembered for 15 s. The mates are creatures of the avatar's species,
//!   so they answer an attack on the avatar or on another mate like the pack
//!   does (`creature_support::defenders`).
//! * RESPAWN (VERIFIED in the herd update 0xC6D960, the part at
//!   0xC6DE0E..0xC6E0D1; `RespawnRate` +0x11C is the archetype's 5 s, only a
//!   zero is replaced by 10 / 30 in the herd init 0xC6E690): while the herd has
//!   fewer members than `mTargetHerdSize` (+0xF4, `NumCreatures` 6) its timer
//!   (+0x1A0) is started at the first shortfall; once it has run longer than
//!   `RespawnRate` seconds a wave of `min(shortfall, 2)` (global 0x1582F34)
//!   replacements is created at the nest (INFERRED spawn spread 2 to 6 from the
//!   two float arguments of `cCreatureAnimal::Create`) and the timer restarts.
//!   The timer keeps running while the herd is whole, so a later loss is
//!   replaced at once when the last wave was more than `RespawnRate` ago.
//!
//! ADAPTED (marked at the code):
//! * The mates are copies of the avatar's drawn body (the same copy the
//!   cinematic buddies are, `cinematics::host::spawn_avatar_copy`), because
//!   OpenSpore has no simulator creatures of the player's species. They take
//!   the avatar's scale (babies the retail baby scale 0.6).
//! * Behaviour: four mates take guard posts at the retail radius (10 to 15)
//!   and change post after the retail dwell (10 to 30 s); the other two stroll
//!   within a few metres of the nest. The first dwell starts at the stage
//!   start, so the opening view still has everyone around the nest (the exact
//!   `setupCreature` spots and the herd's guard AI were not traced). They walk
//!   at a fraction of the avatar's speed.
//! * Combat: a mate has the avatar's hit points (a baby `nerfMultForBabies`)
//!   and the avatar's own attack abilities (it wears the same parts); hostile
//!   creatures pick mates like pack members (retail victim choice by
//!   relationship not traced). A responder is "too hurt" below a tenth of
//!   its hit points (0xC0BCF0, `TOO_HURT`) and heals at the
//!   nest's `creatureHealingHealthRecovery` within `creatureHealingRadius`.
//!   A killed mate is only hidden (no death clip) and comes back at a start
//!   spot; the egg layer branch of the herd update (a living member lays the
//!   replacement as an egg) is not drawn, the replacement appears at once.
//! * They are hidden while the `CLG2CRG` cinematic plays its own buddies.
//! * Where the nest moves (migration) they are put around the new nest.

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

use bevy::{
    mesh::skinning::SkinnedMesh,
    prelude::*,
};

use crate::{
    creature_actions::{self, Encounter, EncounterMode},
    creature_clips::{self, ClipDrive, ClipPlayer, ClipRig},
    creature_ecology::{mix64, unit},
    creature_rules,
    creature_support,
    game::{Health, Mover, Npc, Player, RunProgress},
    persistence::WorldId,
    stages::{GameStage, StageOwned},
    Originals,
};

/// `avatarspecies` `NumCreatures`.
pub const MATE_COUNT: usize = 6;
/// `percentBabiesRange` 0.167 x 6.
pub const BABY_COUNT: usize = 1;
/// `NumGuardLocations` x `guardsPerLocation`.
pub const GUARD_COUNT: usize = 4;
/// `guardLocationRadiusRange` (m from the nest).
pub const GUARD_RADIUS: (f32, f32) = (10.0, 15.0);
/// `guardPositionDurationRange` (s).
pub const POST_DWELL: (f32, f32) = (10.0, 30.0);
/// `creatureBabyScale` (speciestuning): a baby is this fraction of an adult.
pub const BABY_SCALE: f32 = 0.6;
/// ADAPTED: where the mates stand when the stage starts (around the nest).
pub const START_RING: (f32, f32) = (2.5, 7.0);
/// ADAPTED: the strolling mates stay this close to the nest.
pub const STROLL_RADIUS: f32 = 6.0;
/// ADAPTED: walking speed as a fraction of the avatar's speed.
pub const WALK_FRACTION: f32 = 0.45;
/// How often the avatar's look is compared with the mates' (s).
const LOOK_CHECK: f32 = 0.5;
/// The avatar's skinned body replaces its plain mesh a few frames into the
/// stage (`creature_clips::drive_player`); the mates wait for it this long.
const SKIN_WAIT: f32 = 3.0;
/// Mates must stay out of the water: candidate tries per spot.
const SPOT_TRIES: u32 = 12;

/// Mirror of terrain.rs `BASE_Y` (the flat-world baseline of grounding).
const BASE_Y: f32 = -0.6;

/// Mate `index` is the victim / attacker id `MATE_ID_BASE + index` in
/// `Encounter` (world ids of the creatures stay far below it).
pub const MATE_ID_BASE: u32 = 0xF000_0000;
/// `avatarspecies` `RespawnRate` (s).
pub const RESPAWN_RATE: f32 = 5.0;
/// Replacements per wave: the global 0x1582F34 (2) the herd update caps the
/// shortfall with.
pub const RESPAWN_WAVE: usize = 2;
/// A responder keeps the attacker for the attack memory (0xC075E0 posts it
/// for 15 s); the attack event itself lasts 5 s.
pub const ATTACK_MEMORY: f32 = creature_support::ALERT_MEMORY_SECONDS;
pub const ATTACK_EVENT: f32 = 5.0;
/// Below this share of its hit points a creature does not join a fight.
/// VERIFIED (creature-audit-a): 0xC0BCF0 is `(0xBFC5D0(creature + 0x5A8) = the
/// meter's value / its maximum) x [0x1687A00] < [0x1687A10]`; the globals are
/// the motive tuning's `MotiveMax` (100, stored at +0 by the loader 0xB70AB1)
/// and `MotiveCritical` (10, +0x10 stored by 0xB70B55), so a creature is too
/// hurt below 10 % of its hit points.
pub const TOO_HURT: f32 = 0.10;
/// ADAPTED: how close to its target a fighting mate stands (world units, as
/// a fraction of the ability's reach).
const STANDOFF_FRACTION: f32 = 0.6;

pub fn mate_id(index: usize) -> u32 {
    MATE_ID_BASE + index as u32
}

/// ADAPTED: the nest mate that answers the player's mating call. Retail creates
/// a creature of the player's species for the call (0xD52A18, general flag 0x10,
/// cleared by 0xD4B13A after a 5 s timer, 0x1582F88); OpenSpore has no such
/// creature, so the first living adult nest mate stands in for it while the
/// call is active. Its cursor is the mate cursor (action 9).
pub fn answering_mate<'a>(mates: impl IntoIterator<Item = &'a NestMate>) -> Option<usize> {
    mates.into_iter().filter(|mate| !mate.baby && !mate.dead).map(|mate| mate.index).min()
}

/// A mate's hit points: it is of the avatar's species, so the avatar's maximum
/// (`healthTable[brain level]` plus the parts bonus), a baby
/// `nerfMultForBabies`.
pub fn mate_max_hp(avatar_max: f32, baby: bool) -> f32 {
    avatar_max.max(1.0) * if baby { creature_rules::BABY_HIT_POINT_MULTIPLIER } else { 1.0 }
}

/// 0xD8F1C0: the mates that answer an attack by a creature at `attacker`.
/// `mates` are (index, ground position, hit point fraction) of the living
/// ones; a responder is within `creatureHelpAllyInRadius` of the attacker and
/// not too hurt.
pub fn responders(attacker: Vec2, mates: &[(usize, Vec2, f32)]) -> Vec<usize> {
    let able: Vec<(u32, Vec2)> = mates
        .iter()
        .filter(|(_, _, fraction)| *fraction > TOO_HURT)
        .map(|(index, at, _)| (*index as u32, *at))
        .collect();
    creature_support::defenders(attacker, &able).into_iter().map(|i| i as usize).collect()
}

/// The herd update's respawn law (0xC6D960): `timer` is the herd timer
/// (None: not started), advanced by `dt`. Returns how many replacements to
/// create now for `shortfall` missing members.
pub fn respawn_step(timer: &mut Option<f32>, dt: f32, shortfall: usize) -> usize {
    if let Some(t) = timer {
        *t += dt.max(0.0);
    }
    if shortfall == 0 {
        return 0;
    }
    match timer {
        None => {
            *timer = Some(0.0);
            0
        }
        Some(t) if *t > RESPAWN_RATE => {
            *t = 0.0;
            shortfall.min(RESPAWN_WAVE)
        }
        Some(_) => 0,
    }
}

/// Where a fighting mate waits: `standoff` from the attacker on the side it
/// comes from.
pub fn standoff_spot(attacker: Vec2, mate: Vec2, standoff: f32) -> Vec2 {
    let away = (mate - attacker).try_normalize().unwrap_or(Vec2::X);
    attacker + away * standoff.max(0.0)
}

/// What one mate is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatePlan {
    pub baby: bool,
    pub guard: bool,
}

/// The six mates of the herd: one baby and one adult stroll near the nest,
/// four adults stand guard.
pub fn plan() -> [MatePlan; MATE_COUNT] {
    let mut out = [MatePlan { baby: false, guard: true }; MATE_COUNT];
    for (i, p) in out.iter_mut().enumerate() {
        p.baby = i < BABY_COUNT;
        p.guard = i >= MATE_COUNT - GUARD_COUNT;
    }
    out
}

fn key(seed: u64, index: usize, visit: u32, salt: u64) -> u64 {
    mix64(seed ^ (index as u64).wrapping_mul(0x9E37_79B9) ^ ((visit as u64) << 32) ^ salt)
}

fn lerp((lo, hi): (f32, f32), t: f32) -> f32 {
    lo + (hi - lo) * t
}

/// Where mate `index` stands when the stage (or a new nest) starts: spread
/// round the nest, `START_RING` away.
pub fn start_spot(seed: u64, index: usize, nest: Vec2) -> Vec2 {
    let slice = std::f32::consts::TAU / MATE_COUNT as f32;
    let angle = (index as f32 + 0.15 + 0.7 * unit(key(seed, index, 0, 1))) * slice;
    let radius = lerp(START_RING, unit(key(seed, index, 0, 2)));
    nest + Vec2::new(angle.cos(), angle.sin()) * radius
}

/// The `visit`-th spot mate `index` goes to: a guard post round the nest at
/// the retail radius (guard `k` of 4 keeps to its own quarter), or a stroll
/// within `STROLL_RADIUS` of the nest.
pub fn next_spot(seed: u64, index: usize, visit: u32, nest: Vec2, tries: u32) -> Vec2 {
    let plan = plan()[index.min(MATE_COUNT - 1)];
    let base = key(seed, index, visit, 3 + tries as u64);
    if plan.guard {
        let post = index - (MATE_COUNT - GUARD_COUNT);
        let quarter = std::f32::consts::TAU / GUARD_COUNT as f32;
        let angle = (post as f32 + unit(base)) * quarter;
        let radius = lerp(GUARD_RADIUS, unit(base ^ 0x55));
        nest + Vec2::new(angle.cos(), angle.sin()) * radius
    } else {
        let angle = unit(base) * std::f32::consts::TAU;
        let radius = STROLL_RADIUS * unit(base ^ 0x55).sqrt();
        nest + Vec2::new(angle.cos(), angle.sin()) * radius
    }
}

/// `guardPositionDurationRange`: how long mate `index` stays at its `visit`-th
/// spot.
pub fn dwell(seed: u64, index: usize, visit: u32) -> f32 {
    lerp(POST_DWELL, unit(key(seed, index, visit, 9)))
}

/// A nest mate (the root of a copy of the avatar's body).
#[derive(Component, Debug)]
pub struct NestMate {
    pub index: usize,
    pub baby: bool,
    visit: u32,
    target: Vec2,
    wait: f32,
    /// Ground speed this frame (world units per second).
    pub speed: f32,
    /// The scale the mate stands at (its clearance above the ground follows).
    scale: f32,
    /// Hit points and their maximum (`mate_max_hp`).
    pub hp: f32,
    pub max_hp: f32,
    /// Knocked out: hidden until the herd's respawn wave brings it back.
    pub dead: bool,
    /// The creature it fights (world id) and the seconds it remembers it.
    fight: Option<(u32, f32)>,
    attack_timer: f32,
    /// Runs (the avatar's speed) instead of strolling.
    hurry: bool,
    /// Where the creature it fights stands (ground plane), while fighting.
    aim: Option<Vec2>,
}

/// The mates of the stage and the look they copy.
#[derive(Resource, Default)]
pub struct NestMates {
    signature: Option<u64>,
    nest: Option<Vec2>,
    check: f32,
    /// Seconds since the stage started (the wait for the avatar's skin).
    age: f32,
    /// Mates spawned in total (tests, diagnostics).
    pub spawned: u32,
    /// The herd timer of the respawn law (`respawn_step`).
    timer: Option<f32>,
    /// Mates knocked out / brought back / fights joined (tests, diagnostics).
    pub killed: u32,
    pub respawned: u32,
    pub defended: u32,
}

/// Hash of what the avatar looks like (body, parts, brain level, generation,
/// the number of drawn pieces under it): the mates are rebuilt when it changes.
fn look_signature(run: &RunProgress, look: &Look) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", run.body).hash(&mut h);
    format!("{:?}", run.parts).hash(&mut h);
    run.creature_progression.brain_level.hash(&mut h);
    run.creature_lifecycle.generation.hash(&mut h);
    look.pieces.hash(&mut h);
    look.skin.hash(&mut h);
    h.finish()
}

/// What the avatar's drawn hierarchy is made of right now.
#[derive(Debug, Clone, Copy, Default)]
struct Look {
    /// Entities that draw a mesh.
    pieces: usize,
    /// The skinned body's inverse bind poses (None: a plain mesh).
    skin: Option<bevy::asset::AssetId<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
}

fn look_of(world: &World, root: Entity) -> Look {
    let mut look = Look::default();
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if world.get::<Mesh3d>(e).is_some() {
            look.pieces += 1;
        }
        if let Some(skin) = world.get::<SkinnedMesh>(e) {
            look.skin = Some(skin.inverse_bindposes.id());
        }
        if let Some(children) = world.get::<Children>(e) {
            stack.extend(children.iter());
        }
    }
    look
}

fn is_water(world: &World, p: Vec2) -> bool {
    world
        .get_resource::<crate::terrain::LandSurface>()
        .is_some_and(|s| s.is_water(p.x, p.y))
}

/// A dry spot for a candidate producer (a few retries, else the nest side).
fn dry(world: &World, nest: Vec2, mut pick: impl FnMut(u32) -> Vec2) -> Vec2 {
    for tries in 0..SPOT_TRIES {
        let p = pick(tries);
        if !is_water(world, p) {
            return p;
        }
    }
    nest
}

fn seed_of(run: &RunProgress) -> u64 {
    crate::creature::world_seed(run)
}

/// Spawn the mates (once per look): copies of the avatar's body round the nest.
pub fn spawn(world: &mut World) {
    if *world.resource::<State<GameStage>>().get() != GameStage::Creature {
        return;
    }
    let dt = world.resource::<Time>().delta_secs();
    let Some(player) = world.query_filtered::<Entity, With<Player>>().iter(world).next() else {
        return;
    };
    {
        let mut mates = world.resource_mut::<NestMates>();
        mates.age += dt;
        mates.check -= dt;
        if mates.check > 0.0 && mates.signature.is_some() {
            return;
        }
        mates.check = LOOK_CHECK;
    }
    let run = world.resource::<RunProgress>();
    let home = Vec2::new(run.creature_lifecycle.home[0], run.creature_lifecycle.home[2]);
    let seed = seed_of(run);
    let look = look_of(world, player);
    let age = world.resource::<NestMates>().age;
    // The avatar swaps its plain mesh for the skinned body in the first
    // frames; copies made before would stay legless.
    if look.skin.is_none() && age < SKIN_WAIT {
        return;
    }
    let signature = look_signature(world.resource::<RunProgress>(), &look);
    let (known, nest) = {
        let mates = world.resource::<NestMates>();
        (mates.signature, mates.nest)
    };
    if known == Some(signature) {
        // Same look: only a moved nest (migration) puts the mates elsewhere.
        if nest.is_some_and(|n| n.distance(home) > 1.0) {
            relocate(world, seed, home);
        }
        return;
    }
    // A new look (or the first): replace every mate.
    let old: Vec<Entity> = world.query_filtered::<Entity, With<NestMate>>().iter(world).collect();
    for e in old {
        if let Ok(e) = world.get_entity_mut(e) {
            e.despawn();
        }
    }
    let scale = world.get::<Transform>(player).map_or(1.0, |t| t.scale.x);
    let avatar_hp = world.get::<Health>(player).map_or(10.0, |h| h.max);
    let plans = plan();
    for (index, p) in plans.iter().enumerate() {
        let at = dry(world, home, |_| start_spot(seed, index, home));
        let root = crate::cinematics::spawn_avatar_copy(world, player, &format!("nest mate {index}"));
        let wait = dwell(seed, index, 0);
        let mate_scale = scale * if p.baby { BABY_SCALE } else { 1.0 };
        let facing = Vec2::new(home.x - at.x, home.y - at.y);
        let yaw = (-facing.x).atan2(-facing.y);
        if let Some(mut t) = world.get_mut::<Transform>(root) {
            t.translation.x = at.x;
            t.translation.z = at.y;
            t.rotation = Quat::from_rotation_y(yaw);
            t.scale = Vec3::splat(mate_scale);
        }
        world.entity_mut(root).remove::<crate::cinematics::CinematicActor>().insert((
            NestMate {
                index,
                baby: p.baby,
                visit: 0,
                target: at,
                wait,
                speed: 0.0,
                scale: mate_scale,
                hp: mate_max_hp(avatar_hp, p.baby),
                max_hp: mate_max_hp(avatar_hp, p.baby),
                dead: false,
                fight: None,
                attack_timer: 0.0,
                hurry: false,
                aim: None,
            },
            // The avatar's rule: the root stands 0.7 x its scale above the ground.
            crate::terrain::Grounded::new(0.7 * mate_scale - BASE_Y),
            StageOwned,
            Name::new(format!("nest mate {index}")),
        ));
    }
    info!(
        "nest mates: {} copies of the avatar round the nest at ({:.0}, {:.0}), {} drawn pieces, skinned {}, look {:016x}",
        MATE_COUNT, home.x, home.y, look.pieces, look.skin.is_some(), signature
    );
    let mut mates = world.resource_mut::<NestMates>();
    mates.signature = Some(signature);
    mates.nest = Some(home);
    mates.spawned += MATE_COUNT as u32;
}

/// The nest moved: stand the mates round the new one.
fn relocate(world: &mut World, seed: u64, home: Vec2) {
    let spots: Vec<(Entity, Vec2)> = world
        .query::<(Entity, &NestMate)>()
        .iter(world)
        .map(|(e, m)| (e, dry(world, home, |_| start_spot(seed, m.index, home))))
        .collect();
    for (e, at) in spots {
        if let Some(mut t) = world.get_mut::<Transform>(e) {
            t.translation.x = at.x;
            t.translation.z = at.y;
        }
        if let Some(mut m) = world.get_mut::<NestMate>(e) {
            m.target = at;
            m.visit = 0;
            m.wait = dwell(seed, m.index, 0);
        }
    }
    world.resource_mut::<NestMates>().nest = Some(home);
}

/// Idle, then walk to the next post or stroll spot; face the way they go; hide
/// while the cinematic plays its own buddies.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<RunProgress>,
    player: Query<(&Transform, &Mover), (With<Player>, Without<NestMate>)>,
    cinematics: Option<Res<crate::cinematics::Cinematics>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut mates: Query<(Entity, &mut Transform, &mut NestMate, &mut Visibility), Without<Player>>,
) {
    let dt = time.delta_secs().min(0.1);
    let Ok((pose, mover)) = player.single() else {
        return;
    };
    let seed = seed_of(&run);
    let home = Vec2::new(run.creature_lifecycle.home[0], run.creature_lifecycle.home[2]);
    let intro = cinematics
        .as_deref()
        .is_some_and(|c| c.is_playing() && c.current().is_some_and(|(_, name, _)| name == "CLG2CRG"));
    for (entity, mut t, mut mate, mut visibility) in &mut mates {
        *visibility = if intro || mate.dead { Visibility::Hidden } else { Visibility::Inherited };
        if mate.dead {
            mate.speed = 0.0;
            continue;
        }
        let speed = mover.speed * if mate.hurry { 1.0 } else { WALK_FRACTION };
        // The avatar's size follows the brain level (babies stay small).
        let scale = pose.scale.x * if mate.baby { BABY_SCALE } else { 1.0 };
        t.scale = Vec3::splat(scale);
        if (scale - mate.scale).abs() > 1e-4 {
            mate.scale = scale;
            commands
                .entity(entity)
                .insert(crate::terrain::Grounded::new(0.7 * scale - BASE_Y));
        }
        if dt <= 0.0 {
            continue;
        }
        let here = Vec2::new(t.translation.x, t.translation.z);
        if mate.wait > 0.0 {
            mate.wait -= dt;
            mate.speed = 0.0;
            if mate.wait <= 0.0 {
                mate.visit += 1;
                let mut spot = home;
                for tries in 0..SPOT_TRIES {
                    spot = next_spot(seed, mate.index, mate.visit, home, tries);
                    if !surface.as_ref().is_some_and(|s| s.is_water(spot.x, spot.y)) {
                        break;
                    }
                    spot = home;
                }
                mate.target = spot;
            }
            continue;
        }
        let to = mate.target - here;
        let distance = to.length();
        if distance < 0.35 {
            mate.speed = 0.0;
            mate.wait = dwell(seed, mate.index, mate.visit);
            // Facing the creature it fights.
            if let Some(aim) = mate.aim.filter(|_| mate.fight.is_some()) {
                let face = aim - here;
                if face.length() > 1e-3 {
                    let wanted = Quat::from_rotation_y((-face.x).atan2(-face.y));
                    t.rotation = t.rotation.slerp(wanted, (8.0 * dt).min(1.0));
                }
            }
            continue;
        }
        let step = (speed * dt).min(distance);
        let direction = to / distance;
        t.translation.x += direction.x * step;
        t.translation.z += direction.y * step;
        mate.speed = step / dt;
        let wanted = Quat::from_rotation_y((-direction.x).atan2(-direction.y));
        t.rotation = t.rotation.slerp(wanted, (6.0 * dt).min(1.0));
    }
}

/// A fight is over: back to the posts.
fn end_fight(mate: &mut NestMate) {
    mate.fight = None;
    mate.aim = None;
    mate.hurry = false;
    // Positive, so `update` picks the next post at once.
    mate.wait = 0.01;
}

/// Before the creatures act: tell them where the living mates stand (so that
/// they can be attacked, `Encounter::mates`), let the mates answer the attack
/// reported last frame (`Encounter::defend`: those within
/// `creatureHelpAllyInRadius` of the attacker, 0xD8F1C0) and strike when in
/// reach (`Encounter::mate_strikes`, landed by `creature_actions::update`).
#[allow(clippy::too_many_arguments)]
pub fn perceive(
    time: Res<Time>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    encounter: Option<ResMut<Encounter>>,
    mut state: ResMut<NestMates>,
    mut mates: Query<(&Transform, &mut NestMate)>,
    creatures: Query<(&WorldId, &Transform, &Npc), (Without<NestMate>, Without<Player>)>,
    mut previous: Local<f32>,
) {
    let Some(mut encounter) = encounter else { return };
    let dt = time.delta_secs().min(0.1);
    // Strikes the creature system did not take last frame (it did not run)
    // are stale.
    encounter.mate_strikes.clear();
    encounter.mates.clear();
    encounter
        .mates
        .extend(mates.iter().filter(|(_, m)| !m.dead).map(|(t, m)| (mate_id(m.index), t.translation)));
    let new_hit = encounter.defend_time > *previous + 1e-3;
    *previous = encounter.defend_time;
    let attacker = encounter
        .defend
        .filter(|_| encounter.defend_time > 0.0)
        .and_then(|id| creatures.iter().find(|(w, _, npc)| w.0 == id && npc.hp > 0.0).map(|(w, t, _)| (w.0, t.translation.xz())));
    // The attack event lasts 5 s: the responders are picked in that window.
    if let Some((id, at)) = attacker.filter(|_| ATTACK_MEMORY - encounter.defend_time <= ATTACK_EVENT) {
        let able: Vec<(usize, Vec2, f32)> = mates
            .iter()
            .filter(|(_, m)| !m.dead)
            .map(|(t, m)| (m.index, t.translation.xz(), m.hp / m.max_hp.max(1e-3)))
            .collect();
        let answering = responders(at, &able);
        for (_, mut m) in &mut mates {
            if !answering.contains(&m.index) || m.dead {
                continue;
            }
            let joined = m.fight.map(|f| f.0) != Some(id);
            if joined {
                state.defended += 1;
                info!("nest mates: mate {} answers the attack by creature {id}", m.index);
            }
            if joined || new_hit {
                m.fight = Some((id, ATTACK_MEMORY));
            }
            m.hurry = true;
        }
    }
    // Fight, or go back to the posts.
    let mut abilities = None;
    for (t, mut m) in &mut mates {
        if m.dead {
            continue;
        }
        m.attack_timer = (m.attack_timer - dt).max(0.0);
        let Some((target, memory)) = m.fight else {
            m.aim = None;
            continue;
        };
        let found = creatures.iter().find(|(w, _, npc)| w.0 == target && npc.hp > 0.0);
        let memory = memory - dt;
        let Some((_, there, _)) = found.filter(|_| memory > 0.0) else {
            end_fight(&mut m);
            continue;
        };
        m.fight = Some((target, memory));
        let (ranks, scale) = *abilities.get_or_insert_with(|| {
            creature_actions::fight_back_ranks(creature_actions::levels(
                creature_actions::player_abilities(&run, &originals),
                EncounterMode::Combat,
            ))
        });
        let catalog = &originals.creature_abilities;
        let here = t.translation.xz();
        let there = there.translation.xz();
        m.aim = Some(there);
        // Hold the best ability's reach; strike with the best one in reach.
        let standoff = creature_actions::pack_attack(catalog, ranks, 0.0)
            .map_or(1.0, |(_, a)| (a.range * STANDOFF_FRACTION).clamp(0.8, 4.0));
        m.target = standoff_spot(there, here, standoff);
        m.wait = 0.0;
        if m.attack_timer <= 0.0 {
            if let Some((_, ability)) = creature_actions::pack_attack(catalog, ranks, here.distance(there)) {
                let baby = if m.baby { creature_rules::BABY_ATTACK_MULTIPLIER } else { 1.0 };
                encounter.mate_strikes.push((target, ability.damage * scale * baby));
                m.attack_timer = ability.recharge;
            }
        }
    }
}

/// After the creatures acted: the hits they landed on mates, the mates'
/// hit points following the avatar's, the nest's healing and the herd's
/// respawn wave (`respawn_step`).
#[allow(clippy::too_many_arguments)]
pub fn settle(
    time: Res<Time>,
    run: Res<RunProgress>,
    encounter: Option<ResMut<Encounter>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    player: Query<&Health, (With<Player>, Without<NestMate>)>,
    mut state: ResMut<NestMates>,
    mut mates: Query<(&mut Transform, &mut NestMate), Without<Player>>,
) {
    let dt = time.delta_secs().min(0.1);
    let hits: Vec<(u32, f32, u32)> = match encounter {
        Some(mut e) => {
            let hits = std::mem::take(&mut e.mate_hits);
            if !hits.is_empty() {
                e.message = format!("A nest mate is under attack; a fallen one returns after {RESPAWN_RATE:.0} s");
            }
            hits
        }
        None => Vec::new(),
    };
    let avatar_max = player.single().map_or(10.0, |h| h.max);
    let home = Vec2::new(run.creature_lifecycle.home[0], run.creature_lifecycle.home[2]);
    let mut knocked = 0;
    for (t, mut m) in &mut mates {
        // The mate's maximum follows the avatar's (brain level, parts).
        let max = mate_max_hp(avatar_max, m.baby);
        if (max - m.max_hp).abs() > 1e-3 {
            m.hp = m.hp / m.max_hp.max(1e-3) * max;
            m.max_hp = max;
        }
        if m.dead {
            continue;
        }
        for (mate, damage, _attacker) in &hits {
            if *mate == mate_id(m.index) && m.hp > 0.0 {
                m.hp = (m.hp - damage).max(0.0);
            }
        }
        if m.hp <= 0.0 {
            m.dead = true;
            m.fight = None;
            m.aim = None;
            m.hurry = false;
            knocked += 1;
            info!("nest mates: mate {} is knocked out", m.index);
            continue;
        }
        // The nest heals the creatures standing at it (creatureHealingRadius).
        if m.fight.is_none() && t.translation.xz().distance(home) <= creature_rules::HEALING_RADIUS {
            m.hp = (m.hp + m.max_hp * creature_rules::HEALING_HEALTH_RECOVERY * dt).min(m.max_hp);
        }
    }
    state.killed += knocked;
    // The herd's respawn wave.
    let missing: Vec<usize> = {
        let mut dead: Vec<usize> = mates.iter().filter(|(_, m)| m.dead).map(|(_, m)| m.index).collect();
        dead.sort_unstable();
        dead
    };
    let wave = respawn_step(&mut state.timer, dt, missing.len());
    if wave == 0 {
        return;
    }
    let seed = seed_of(&run);
    for index in missing.into_iter().take(wave) {
        let at = (0..SPOT_TRIES)
            .map(|_| start_spot(seed, index, home))
            .find(|p| !surface.as_ref().is_some_and(|s| s.is_water(p.x, p.y)))
            .unwrap_or(home);
        for (mut t, mut m) in &mut mates {
            if m.index != index {
                continue;
            }
            m.dead = false;
            m.hp = m.max_hp;
            m.fight = None;
            m.aim = None;
            m.hurry = false;
            m.target = at;
            m.wait = dwell(seed, index, 0);
            t.translation.x = at.x;
            t.translation.z = at.y;
        }
        state.respawned += 1;
        info!("nest mates: mate {index} is back at the nest");
    }
}

/// Clip state of one mate's body.
struct MatePose {
    rig: Arc<ClipRig>,
    unit: f32,
    joints: Vec<Entity>,
    clips: ClipPlayer,
    running: bool,
    request: u32,
    /// The installed body's part rig bones (`creature_morph`): posed from the
    /// same clips, else they would stay where the avatar was when copied.
    morph: Option<Arc<crate::creature_morph::MorphSet>>,
    morph_state: crate::creature_morph::MorphState,
}

/// Pose each mate's skinned body: the walk or run cycle by its ground speed,
/// else the idle (the same requests as the avatar's own locomotion,
/// `creature_clips::locomotion_request`). Bodies without a clip rig stand
/// as copied.
#[allow(clippy::too_many_arguments)]
pub fn animate(
    time: Res<Time>,
    originals: Res<Originals>,
    shared: Option<Res<ClipDrive>>,
    mates: Query<(Entity, &NestMate, &Transform)>,
    children: Query<&Children>,
    skins: Query<&SkinnedMesh>,
    mut joint_tf: Query<&mut Transform, Without<NestMate>>,
    mut states: Local<HashMap<Entity, Option<MatePose>>>,
) {
    states.retain(|e, _| mates.contains(*e));
    let dt = time.delta_secs().min(0.1);
    for (entity, mate, tf) in &mates {
        if mate.dead {
            continue;
        }
        let state = states.entry(entity).or_insert(None);
        // The rig of the body's skin is registered when the avatar's skin is
        // built; look again until it is found.
        if state.is_none() {
            let mut stack = vec![entity];
            while let Some(e) = stack.pop() {
                if let Ok(skin) = skins.get(e) {
                    if let Some((rig, unit)) = shared.as_ref().and_then(|d| d.rig_of_skin(skin.inverse_bindposes.id())) {
                        if skin.joints.len() >= rig.len() && !skin.joints.contains(&Entity::PLACEHOLDER) {
                            *state = Some(MatePose {
                                rig,
                                unit,
                                joints: skin.joints.clone(),
                                clips: ClipPlayer::default(),
                                running: false,
                                request: 0,
                                morph: shared.as_ref().and_then(|d| d.morph_of_skin(skin.inverse_bindposes.id())),
                                morph_state: Default::default(),
                            });
                            break;
                        }
                    }
                }
                if let Ok(c) = children.get(e) {
                    stack.extend(c.iter());
                }
            }
        }
        let Some(st) = state.as_mut() else { continue };
        // Ground speed in creature units per second (as the avatar's driver).
        let frame_unit = (tf.scale.x.abs() * st.unit).max(1e-4);
        let v = mate.speed / frame_unit;
        let id = creature_clips::locomotion_request(&originals, v, &st.rig, &mut st.running);
        if id != st.request {
            if let Some(clip) = creature_clips::clip(&originals, id) {
                let looped = clip.cyclic;
                st.clips.play(Arc::new(st.rig.bind(clip)), looped);
                st.request = id;
            }
        }
        if let Some(top) = st.clips.current() {
            if top.looped && top.bound.clip.stance_speed.is_some() {
                let rate = creature_clips::gait_rate(&top.bound.clip, v, &st.rig);
                st.clips.set_rate(rate);
            } else {
                st.clips.set_rate(1.0);
            }
        }
        st.clips.advance(dt);
        let mut deforms = creature_clips::BlockDeforms::new();
        let pose = st.rig.pose_full(&st.clips.mix(), false, None, st.morph.is_some().then_some(&mut deforms));
        for (j, &e) in st.joints.iter().enumerate().take(st.rig.len()) {
            if let (Ok(mut t), Some(m)) = (joint_tf.get_mut(e), pose.get(j)) {
                *t = Transform::from_matrix(Mat4::from(creature_clips::to_frame(m, st.unit)));
            }
        }
        if let Some(morph) = &st.morph {
            let mut out = Vec::new();
            morph.pose(&pose, &deforms, &mut st.morph_state, &mut out);
            for (j, m) in out {
                if let Some(Ok(mut t)) = st.joints.get(j).map(|&e| joint_tf.get_mut(e)) {
                    *t = Transform::from_matrix(Mat4::from(creature_clips::to_frame(&m, st.unit)));
                }
            }
        }
    }
}

/// Dev hook for captures: `OPENSPORE_MATE_TEST=fight` brings the creature
/// nearest to the nest 5 m from it at frame 30 of the stage and makes it a
/// hostile one, so the mates answer it (`perceive`) and it hits them.
pub fn fight_test(
    mut frames: Local<u32>,
    run: Res<RunProgress>,
    mut creatures: Query<
        (&mut Transform, &mut Npc, &mut creature_actions::CreatureMind),
        (Without<Player>, Without<NestMate>),
    >,
) {
    static MODE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    if MODE.get_or_init(|| std::env::var("OPENSPORE_MATE_TEST").ok()).as_deref() != Some("fight") {
        return;
    }
    *frames += 1;
    if *frames != 30 {
        return;
    }
    let home = Vec3::from_array(run.creature_lifecycle.home);
    let nearest = creatures
        .iter_mut()
        .min_by(|a, b| crate::game::dist_xz(a.0.translation, home).total_cmp(&crate::game::dist_xz(b.0.translation, home)));
    if let Some((mut t, mut npc, mut mind)) = nearest {
        info!("mate test: a creature from ({:.0}, {:.0}) to the nest, hostile", t.translation.x, t.translation.z);
        t.translation.x = home.x + 5.0;
        t.translation.z = home.z;
        npc.hostile = true;
        mind.allied = false;
        mind.provoked = true;
    }
}

/// Registers the nest mates (Creature stage).
pub struct NestMatesPlugin;

impl Plugin for NestMatesPlugin {
    fn build(&self, app: &mut App) {
        // The mates act before the creatures do (`perceive` tells them where
        // the mates stand and queues the mates' strikes) and settle what the
        // creatures did to them afterwards (`settle`).
        app.add_plugins(crate::creature_eggs::EggsPlugin)
            .init_resource::<NestMates>()
            .add_systems(OnEnter(GameStage::Creature), reset)
            .add_systems(
                Update,
                (spawn, perceive, update)
                    .chain()
                    .before(creature_actions::update)
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Creature))
                    .run_if(resource_exists::<Originals>)
                    .run_if(resource_exists::<RunProgress>),
            )
            .add_systems(
                Update,
                fight_test
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Creature))
                    .run_if(resource_exists::<RunProgress>),
            )
            .add_systems(
                Update,
                settle
                    .after(creature_actions::update)
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Creature))
                    .run_if(resource_exists::<RunProgress>),
            )
            .add_systems(
                Update,
                animate
                    .after(update)
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Creature))
                    .run_if(resource_exists::<Originals>)
                    .run_if(resource_exists::<RunProgress>),
            );
    }
}

/// A fresh stage visit starts with no mates (the stage's entities are gone).
fn reset(mut mates: ResMut<NestMates>) {
    *mates = NestMates::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mate(index: usize, baby: bool, dead: bool) -> NestMate {
        NestMate {
            index,
            baby,
            visit: 0,
            target: Vec2::ZERO,
            wait: 0.0,
            speed: 0.0,
            scale: 1.0,
            hp: 1.0,
            max_hp: 1.0,
            dead,
            fight: None,
            attack_timer: 0.0,
            hurry: false,
            aim: None,
        }
    }

    #[test]
    fn the_first_living_adult_answers_the_mating_call() {
        let mates = [mate(0, true, false), mate(1, false, true), mate(2, false, false), mate(3, false, false)];
        assert_eq!(answering_mate(&mates), Some(2));
        let none = [mate(0, true, false), mate(1, false, true)];
        assert_eq!(answering_mate(&none), None);
        assert_eq!(answering_mate(std::iter::empty::<&NestMate>()), None);
    }

    #[test]
    fn the_herd_is_six_with_one_baby_and_four_guards() {
        let p = plan();
        assert_eq!(p.len(), 6);
        assert_eq!(p.iter().filter(|m| m.baby).count(), 1);
        assert_eq!(p.iter().filter(|m| m.guard).count(), 4);
        assert!(p.iter().all(|m| !(m.baby && m.guard)));
    }

    #[test]
    fn start_spots_stand_round_the_nest() {
        let nest = Vec2::new(3.0, -4.0);
        for index in 0..MATE_COUNT {
            let d = start_spot(7, index, nest).distance(nest);
            assert!((START_RING.0 - 1e-3..=START_RING.1 + 1e-3).contains(&d), "{index}: {d}");
        }
        // Deterministic per world.
        assert_eq!(start_spot(7, 2, nest), start_spot(7, 2, nest));
        assert_ne!(start_spot(7, 2, nest), start_spot(8, 2, nest));
    }

    #[test]
    fn guards_keep_to_their_quarter_at_the_retail_radius() {
        let nest = Vec2::ZERO;
        for index in MATE_COUNT - GUARD_COUNT..MATE_COUNT {
            for visit in 1..12 {
                let p = next_spot(11, index, visit, nest, 0);
                let d = p.length();
                assert!((GUARD_RADIUS.0 - 1e-3..=GUARD_RADIUS.1 + 1e-3).contains(&d), "{d}");
                let post = index - (MATE_COUNT - GUARD_COUNT);
                let quarter = std::f32::consts::TAU / GUARD_COUNT as f32;
                let angle = p.y.atan2(p.x).rem_euclid(std::f32::consts::TAU);
                assert!(angle >= post as f32 * quarter - 1e-3 && angle <= (post + 1) as f32 * quarter + 1e-3);
            }
        }
    }

    #[test]
    fn strollers_stay_close_and_dwell_matches_the_range() {
        for index in 0..MATE_COUNT - GUARD_COUNT {
            for visit in 1..12 {
                assert!(next_spot(5, index, visit, Vec2::ZERO, 0).length() <= STROLL_RADIUS + 1e-3);
            }
        }
        for visit in 0..40 {
            let d = dwell(3, 1, visit);
            assert!((POST_DWELL.0..=POST_DWELL.1).contains(&d), "{d}");
        }
    }

    #[test]
    fn mates_answer_within_the_help_radius_and_not_when_too_hurt() {
        let attacker = Vec2::new(0.0, 0.0);
        let mates = [
            (0, Vec2::new(3.0, 4.0), 1.0),   // 5 m
            (1, Vec2::new(9.9, 0.0), 0.9),   // just inside
            (2, Vec2::new(10.0, 0.0), 1.0),  // exactly on the radius: not inside
            (3, Vec2::new(1.0, 1.0), 0.09),  // too hurt
            (4, Vec2::new(-2.0, 0.0), 0.11), // hurt but able
        ];
        assert_eq!(responders(attacker, &mates), vec![0, 1, 4]);
        assert!(responders(attacker, &[]).is_empty());
        assert_eq!(creature_support::HELP_ALLY_IN_RADIUS, 10.0);
    }

    #[test]
    fn the_herd_timer_replaces_two_per_wave_after_the_respawn_rate() {
        // The first shortfall starts the timer; nothing comes back until it has
        // run longer than RespawnRate (5 s).
        let mut timer = None;
        assert_eq!(respawn_step(&mut timer, 0.25, 1), 0);
        assert_eq!(timer, Some(0.0));
        // Up to exactly 5.0 s: not "longer than" the rate.
        for _ in 0..20 {
            assert_eq!(respawn_step(&mut timer, 0.25, 1), 0);
        }
        assert_eq!(timer, Some(5.0));
        assert_eq!(respawn_step(&mut timer, 0.25, 1), 1);
        assert_eq!(timer, Some(0.0), "the wave restarts the timer");
        // Three missing: a wave brings back at most two (global 0x1582F34).
        let mut timer = Some(6.0);
        assert_eq!(respawn_step(&mut timer, 0.0, 3), 2);
        assert_eq!(respawn_step(&mut timer, 0.1, 1), 0);
        // A whole herd keeps the timer running: a later loss is replaced at once.
        let mut timer = Some(0.0);
        for _ in 0..100 {
            assert_eq!(respawn_step(&mut timer, 0.1, 0), 0);
        }
        assert_eq!(respawn_step(&mut timer, 0.0, 1), 1);
        assert_eq!((RESPAWN_RATE, RESPAWN_WAVE), (5.0, 2));
    }

    #[test]
    fn mates_have_the_avatars_hit_points_and_hold_a_standoff() {
        assert_eq!(mate_max_hp(40.0, false), 40.0);
        assert_eq!(mate_max_hp(40.0, true), 20.0, "nerfMultForBabies");
        let spot = standoff_spot(Vec2::new(10.0, 0.0), Vec2::new(14.0, 0.0), 2.0);
        assert!(spot.distance(Vec2::new(12.0, 0.0)) < 1e-5);
        // On top of the attacker: any side, still at the standoff.
        let spot = standoff_spot(Vec2::ZERO, Vec2::ZERO, 1.5);
        assert!((spot.length() - 1.5).abs() < 1e-5);
        assert_eq!(mate_id(0), MATE_ID_BASE);
    }

    /// A hostile creature of world id 9000 next to the nest (the ecology's
    /// members have ids from 300 up).
    fn spawn_attacker(app: &mut App, at: Vec3) -> Entity {
        app.world_mut()
            .spawn((
                Transform::from_translation(at),
                Npc { speed: 0.0, hostile: true, hp: 400.0 },
                crate::creature_actions::CreatureMind::default(),
                WorldId(9000),
                Visibility::Visible,
            ))
            .id()
    }

    fn started_app() -> (App, Vec2) {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Creature);
        for _ in 0..(SKIN_WAIT * 60.0) as usize + 60 {
            app.update();
        }
        let home = {
            let run = app.world().resource::<RunProgress>();
            Vec2::new(run.creature_lifecycle.home[0], run.creature_lifecycle.home[2])
        };
        (app, home)
    }

    #[test]
    fn a_fallen_mate_returns_after_the_respawn_rate() {
        let (mut app, home) = started_app();
        let alive = |app: &mut App| {
            app.world_mut().query::<&NestMate>().iter(app.world()).filter(|m| !m.dead).count()
        };
        assert_eq!(alive(&mut app), MATE_COUNT);
        // A hit far beyond its hit points knocks mate 2 out.
        app.world_mut().resource_mut::<Encounter>().mate_hits.push((mate_id(2), 1.0e6, 300));
        app.update();
        assert_eq!(alive(&mut app), MATE_COUNT - 1);
        assert_eq!(app.world().resource::<NestMates>().killed, 1);
        // Hidden and not a victim any more.
        app.update();
        assert!(app.world().resource::<Encounter>().mates.iter().all(|(id, _)| *id != mate_id(2)));
        // Not back after four seconds, back after six.
        for _ in 0..240 {
            app.update();
        }
        assert_eq!(alive(&mut app), MATE_COUNT - 1, "RespawnRate has not passed");
        for _ in 0..120 {
            app.update();
        }
        assert_eq!(alive(&mut app), MATE_COUNT);
        assert_eq!(app.world().resource::<NestMates>().respawned, 1);
        // It stands round the nest again with full hit points.
        let mut q = app.world_mut().query::<(&NestMate, &Transform)>();
        let (mate, tf) = q.iter(app.world()).find(|(m, _)| m.index == 2).unwrap();
        assert_eq!(mate.hp, mate.max_hp);
        assert!(Vec2::new(tf.translation.x, tf.translation.z).distance(home) < START_RING.1 + 1.0);
    }

    #[test]
    fn mates_near_the_attacker_defend_and_the_others_stay_out() {
        let (mut app, home) = started_app();
        // The attacker stands 4 m from the nest; the guards post 10-15 m away.
        let attacker = spawn_attacker(&mut app, Vec3::new(home.x + 4.0, 0.0, home.y));
        {
            let mut e = app.world_mut().resource_mut::<Encounter>();
            e.defend = Some(9000);
            e.defend_time = ATTACK_MEMORY;
        }
        for _ in 0..30 {
            app.update();
        }
        // Exactly the mates within 10 m of the attacker fight it.
        let at = Vec2::new(home.x + 4.0, home.y);
        let mut q = app.world_mut().query::<(&NestMate, &Transform)>();
        let near = q
            .iter(app.world())
            .filter(|(m, _)| m.fight.is_some())
            .map(|(m, t)| (m.index, Vec2::new(t.translation.x, t.translation.z)))
            .collect::<Vec<_>>();
        assert!(!near.is_empty(), "mates within 10 m of the attacker answer");
        let state = app.world().resource::<NestMates>();
        assert!(state.defended as usize >= near.len());
        // Not everybody joins: a mate out of the help radius stays at its post.
        assert!(near.len() < MATE_COUNT, "{near:?} around {at}");
        let npc = app.world().get::<Npc>(attacker).unwrap();
        assert!(npc.hp < 400.0, "the mates' strikes landed: {}", npc.hp);
    }

    #[test]
    fn stage_start_spawns_the_herd_and_follows_the_nest() {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        // A plain mesh (no skinned body) is copied after the wait for the skin.
        for _ in 0..(SKIN_WAIT * 60.0) as usize + 60 {
            app.update();
        }
        let mut q = app.world_mut().query::<&NestMate>();
        assert_eq!(q.iter(app.world()).count(), MATE_COUNT);
        assert_eq!(q.iter(app.world()).filter(|m| m.baby).count(), BABY_COUNT);
        // Everyone starts within the ring around the home nest.
        let home = {
            let run = app.world().resource::<RunProgress>();
            Vec2::new(run.creature_lifecycle.home[0], run.creature_lifecycle.home[2])
        };
        let mut t = app.world_mut().query_filtered::<&Transform, With<NestMate>>();
        for tr in t.iter(app.world()) {
            let d = Vec2::new(tr.translation.x, tr.translation.z).distance(home);
            assert!(d < START_RING.1 + 1.0, "{d}");
        }
        // The nest moves: the mates are put round the new one.
        let moved = [home.x + 120.0, 0.0, home.y + 40.0];
        app.world_mut().resource_mut::<RunProgress>().creature_lifecycle.home = moved;
        for _ in 0..60 {
            app.update();
        }
        let mut t = app.world_mut().query_filtered::<&Transform, With<NestMate>>();
        for tr in t.iter(app.world()) {
            let d = Vec2::new(tr.translation.x, tr.translation.z).distance(Vec2::new(moved[0], moved[2]));
            assert!(d < START_RING.1 + 1.0, "{d}");
        }
    }
}
