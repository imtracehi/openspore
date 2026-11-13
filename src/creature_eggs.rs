//! Nest eggs of the NPC herds and the avatar eating them (helper
//! `creature-eggs`, 2026-10-26).
//!
//! Tags: **VERIFIED** (code image, march2017 / SDK addresses, read with
//! `swarm-volume\target\re\dis.py`), **INFERRED** (reading of the neighbouring
//! code), **ADAPTED** (OpenSpore choice).
//!
//! RETAIL (VERIFIED):
//! * The egg is `Simulator::cEgg` (noun 0x2A034CD, type 0x2A0349A; ctor
//!   0xC6A410, vtable 0x14719B0): `mState` +0x1E8, `mTimeToHatch` +0x1F0 (ms),
//!   `mpHerd` +0x1F4, `mBrokenTimer` +0x1F8, `mpToHatch` +0x218, scale +0x220.
//!   It is a `cCombatant`: every hit on it (`TakeDamage`, thunk 0xC6B4F0 ->
//!   0xC6A880) only advances the crack stage whatever the damage is: state 0 or
//!   6 -> 1 -> 2 -> 4 (broken); states 3 (hatched), 4 (broken) and 5 (waiting
//!   to hatch the avatar's own creature) are not advanced and not attackable.
//! * Eggs come from the herd. Every herd picks one of the ten egg models at its
//!   init (0xC6E74E: `EggIndex` -1 -> `RandomInt(10)`; the egg models of the
//!   `speciestuning` lists are `cr_egg_NN_base` / `_breaking_open` /
//!   `_hatched`). The herd update (0xC6D960, part 0xC6DE0E..0xC6E0D1) asks for
//!   `min(NumCreatures - members, 2) - eggs` eggs (the 2 is the global
//!   0x1582F34, `maxNumEggs`), starts its timer at the first shortfall and,
//!   once the timer has run longer than `RespawnRate`, orders the nearest
//!   able member (`mpEggLayer` +0x194, `cCreatureAnimal::mPregnantEggs` +0x1678)
//!   to lay them. The laying behaviour (0xD6CD..0xD6D160, behaviour 0x2D852E6)
//!   creates the egg with `cHerd::CreateEgg` 0xC6D0B0 in state 6 and plays
//!   `sfx_egg_appear` (0xD6D102).
//! * The egg ticks in the herd update (0xC6E183 -> 0xC6B1E0): `mTimeToHatch`
//!   runs down (default `egg_hatch_time` 180 s, 0x4D32D3 stores it x 1000); at
//!   zero the egg becomes state 3 and a baby of the herd's species is created
//!   at the egg (0xC6B493: `cCreatureAnimal::Create(.., age 0, herd, ..)`, the
//!   creature's `mAge` 0 is a baby). A hatched or broken egg (state 3 / 4 with
//!   `mBrokenTimer` above 0x2710 ms) is destroyed by the herd update
//!   (0xC6E143).
//! * Eating (the avatar's goal word 0x80, click action 0x15 of the cursor
//!   table, 0xD33553; behaviour 0x2D852E4: start 0xD71650, update 0xD716F0):
//!   state 0 walks to the egg (`WalkTo` 0xC1C720, stopping at the creature's
//!   radius + the egg's radius) and, once there, tells the relationship
//!   manager (0xBA4A00 with event 6); state 1 attacks the egg with the
//!   creature's own attack ability every time it is ready (0xD854C0) until the
//!   egg is broken (state 4); then the eat clip 0x282D205 plays (state 2) and,
//!   at the moment the egg breaks, the creature gains `creatureHealthPointsPerEgg`
//!   (0.33) x its maximum HP (0xD71BAC..0xD71C0F), the avatar
//!   `creatureEvoPointsPerEgg` (25, 0xD71C2B: AddEvolutionPoints) and 50 hunger
//!   (`creatureHungerSatisfiedPerEgg`, 0xD85A50 with the egg food id
//!   0x133B8599; no diet gate: the egg is the one food the diet check 0xC0B8E0 /
//!   0xC0B900 does not guard).
//!
//! ADAPTED (marked at the code):
//! * The egg laying walk and clip of the herd member are not drawn: the egg
//!   appears at the nest at the moment the retail laying behaviour would create
//!   it (the timer rule is retail's).
//! * Placement at the nest, the egg's size, the model of the intermediate
//!   crack states (1 / 2) and the hit cadence of the avatar.
//! * Births of herds that are not active (the player is far away) are not
//!   eggs: the member appears with its herd (retail does not update such a
//!   herd at all).
//! * Eggs are not saved: after a reload an incubating member hatches at once
//!   as an adult.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::{
    creature_actions::{AbilityError, Encounter},
    creature_controls::CreatureControls,
    creature_ecology::{CastingState, MemberRole},
    game::{dist_xz, Health, Hunger, Player, RunProgress},
    stages::{GameStage, StageOwned},
    Originals,
};

/// `cEgg::mState` (+0x1E8).
pub mod state {
    /// Whole egg (the state a freshly created egg has before `CreateEgg` sets it).
    pub const IDLE: u8 = 0;
    /// Cracked once / twice by the creature's attacks.
    pub const CRACKED_ONCE: u8 = 1;
    pub const CRACKED_TWICE: u8 = 2;
    /// The baby has hatched (the shell stays for 10 s).
    pub const HATCHED: u8 = 3;
    /// Broken open and eaten.
    pub const BROKEN: u8 = 4;
    /// The avatar's own egg, waiting to hatch the avatar (not a nest egg).
    pub const WAITING_TO_HATCH: u8 = 5;
    /// Just laid (the state `CreateEgg` gets from the laying behaviour).
    pub const LAID: u8 = 6;
}

/// `egg_hatch_time` (speciestuning, 180 s; the loader 0x4D32D3 multiplies it by
/// 1000 into `mTimeToHatch`'s default 0x150C8D8).
pub const HATCH_SECONDS: f32 = crate::creature_ecology::EGG_HATCH_TIME;
/// A hatched or broken egg is destroyed once its `mBrokenTimer` has run longer
/// than this (0xC6E124: 0x2710 ms).
pub const SHELL_SECONDS: f32 = 10.0;
/// `maxNumEggs`: the herd update caps the eggs asked for at once (the global
/// 0x1582F34, 2) and so the eggs of a herd.
pub const MAX_EGGS: usize = crate::creature_ecology::MAX_NUM_EGGS as usize;
/// The ten egg models of the `speciestuning` lists (`eggModelKeys`).
pub const MODEL_COUNT: usize = 10;
/// `creatureHealthPointsPerEgg`: the share of the maximum HP an egg heals.
pub const HEALTH_FRACTION: f32 = crate::creature_rules::HEALTH_PER_EGG;
/// `creatureHungerSatisfiedPerEgg`.
pub const HUNGER: f32 = crate::creature_rules::HUNGER_PER_EGG;
/// `creatureEvoPointsPerEgg`.
pub const DNA: f32 = crate::creature_rules::EVO_POINTS_PER_EGG;
/// ADAPTED: seconds between the avatar's hits on an egg (retail attacks each
/// time the creature's attack ability is ready, 0xD854C0: animation bound).
pub const HIT_SECONDS: f32 = 0.8;
/// ADAPTED: how close to the egg the avatar stands (retail: the creature's
/// radius plus the egg's radius, 0xD71A7C).
pub const REACH: f32 = 1.6;
/// ADAPTED: ground radius at which the pointer counts as over an egg.
pub const PICK_RADIUS: f32 = 1.1;
/// ADAPTED: the egg's width in world units per unit of its species' size (the
/// retail scale, 0xC6A320, is the species' size itself, 0xC6D264).
const WIDTH_PER_SIZE: f32 = 0.42;
/// ADAPTED: eggs lie inside the nest bowl, this far from its middle.
const NEST_RING: (f32, f32) = (0.4, 1.5);

/// `cEgg::TakeDamage` (0xC6A880, the hit handler 0xC6B4F0): the crack stage
/// after a hit and whether the egg is now broken. States 3, 4 and 5 are not
/// advanced.
pub fn crack(state: u8) -> (u8, bool) {
    let next = match state {
        state::IDLE | state::LAID => state::CRACKED_ONCE,
        state::CRACKED_ONCE => state::CRACKED_TWICE,
        state::CRACKED_TWICE => state::BROKEN,
        other => return (other, false),
    };
    (next, next == state::BROKEN)
}

/// Whether the avatar can attack (and so eat) an egg in this state: the cursor
/// rule of kEgg (0xD2FFD0) and the eat behaviour's own check (0xD7191F) both
/// refuse the states 3, 4 and 5.
pub fn attackable(state: u8) -> bool {
    !matches!(state, state::HATCHED | state::BROKEN | state::WAITING_TO_HATCH)
}

/// The eggs a herd asks for (0xC6DE5C..0xC6DE98): `min(target - members, wave) -
/// eggs` for a herd of `target` creatures with `members` living ones and
/// `eggs` incubating; `wave` is `MAX_EGGS` (the global 0x1582F34).
pub fn eggs_wanted(target: usize, members: usize, eggs: usize, wave: usize) -> usize {
    target.saturating_sub(members).min(wave).saturating_sub(eggs)
}

/// The egg model a herd uses (0xC6E74E: `RandomInt(10)` at the herd init).
/// ADAPTED: a hash of the nest index and the world seed stands in for the
/// random number.
pub fn herd_egg_index(nest: usize, seed: u64) -> usize {
    (crate::creature_ecology::mix64(seed ^ (nest as u64).wrapping_mul(0x9E37_79B9)) % MODEL_COUNT as u64) as usize
}

/// Which of the three models of an egg's herd an egg state draws: the shell
/// that hatched (`hatchedEggModelKeys`), the broken one
/// (`brokenEggModelKeys`, "breaking_open"), else the whole egg. ADAPTED: the
/// code that picks the model by state was not traced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Base,
    Broken,
    Hatched,
}

pub fn shell_of(state: u8) -> Shell {
    match state {
        state::HATCHED => Shell::Hatched,
        state::BROKEN => Shell::Broken,
        _ => Shell::Base,
    }
}

/// The name of an egg model of the `speciestuning` lists.
pub fn model_name(index: usize, shell: Shell) -> String {
    let kind = match shell {
        Shell::Base => "base",
        Shell::Broken => "breaking_open",
        Shell::Hatched => "hatched",
    };
    format!("cr_egg_{:02}_{kind}", index % MODEL_COUNT + 1)
}

/// What eating an egg gives (0xD71BAC..0xD71C78): HP (a share of the maximum),
/// DNA (only the avatar) and hunger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meal {
    pub health: f32,
    pub dna: f32,
    pub hunger: f32,
}

pub fn meal(max_hp: f32, avatar: bool) -> Meal {
    Meal { health: max_hp.max(0.0) * HEALTH_FRACTION, dna: if avatar { DNA } else { 0.0 }, hunger: HUNGER }
}

/// One egg of a nest.
#[derive(Component, Debug, Clone)]
pub struct NestEgg {
    /// Index into `SpeciesWorld::nests`.
    pub nest: usize,
    /// The herd member the egg hatches (a baby of the nest's species).
    pub member: u32,
    /// Which of the ten egg models the herd uses.
    pub model: usize,
    pub state: u8,
    /// Seconds until it hatches.
    pub hatch_left: f32,
    /// Seconds since it hatched or broke (`mBrokenTimer`).
    pub shell_for: f32,
    /// The model drawn (`shell_of` the state when it was last swapped).
    drawn: Shell,
    /// The uniform scale the egg is drawn at and the ground it lies on.
    scale: f32,
    ground: f32,
}

impl NestEgg {
    pub fn new(nest: usize, member: u32, model: usize, scale: f32, ground: f32) -> Self {
        Self {
            nest,
            member,
            model,
            state: state::LAID,
            hatch_left: HATCH_SECONDS,
            shell_for: 0.0,
            drawn: Shell::Base,
            scale,
            ground,
        }
    }
}

/// The avatar's order to eat an egg (the goal word 0x80 the click handler
/// 0xD33553 posts) and the progress of it.
#[derive(Resource, Debug, Default)]
pub struct EggOrder {
    pub egg: Option<Entity>,
    /// Seconds until the next hit.
    pub timer: f32,
    /// The walk was started (a cleared goal afterwards means the player steered).
    pub walking: bool,
}

impl EggOrder {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// The avatar's order to eat a carcass or a fruit it clicked (the goal words 8
/// and 0x20, 0xD33150 / 0xD335AA) and where the food is.
#[derive(Resource, Debug, Default)]
pub struct FoodOrder {
    pub meat: bool,
    pub at: Option<Vec3>,
    /// The walk was started (a cleared goal before arrival means the player steered).
    pub walking: bool,
}

impl FoodOrder {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// What a click on the world orders (0xD32FD0: the action ids of the cursor
/// table pick the goal): the egg of action 0x15 (goal 0x80), a carcass of 8
/// (goal 8) or a fruit of 0x17 (goal 0x20), each with the object under the
/// pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClickOrder {
    Egg(Entity),
    Food { meat: bool, at: Vec3 },
    None,
}

pub fn click_order(action: i32, egg: Option<Entity>, food: Option<Vec3>) -> ClickOrder {
    use crate::creature_hud::cursor::action as id;
    match (action, egg, food) {
        (id::ATTACK_EGG, Some(egg), _) => ClickOrder::Egg(egg),
        (id::EAT_CARCASS, _, Some(at)) => ClickOrder::Food { meat: true, at },
        (id::EAT_FRUIT, _, Some(at)) => ClickOrder::Food { meat: false, at },
        _ => ClickOrder::None,
    }
}

/// How close to the food the avatar must be before the diet check (the eat
/// behaviours start the clip and check the diet once they have arrived: the
/// carcass update at 0xD7149F..0xD71507, the fruit one at 0xD882C9). ADAPTED:
/// the eating reach of `creature_social::meat` / `creature_graze`.
pub const FOOD_REACH: f32 = 1.9;

/// The diet check of an eat order (0xC0B8E0: the carnivore level of the species
/// profile above 0 for a carcass; 0xC0B900: the herbivore level above 0 for a
/// fruit): false when the creature refuses the food (retail posts the refuse
/// goal 0x20000000, which plays clip 0x931FDCDA and shows `crg-wrongfood`,
/// 0xD70A6A, for the avatar).
pub fn can_eat(carnivore: u8, herbivore: u8, meat: bool) -> bool {
    if meat {
        carnivore > 0
    } else {
        herbivore > 0
    }
}

// ---------------------------------------------------------------------------
// The world side
// ---------------------------------------------------------------------------

/// Where egg `member` of a nest lies: inside the nest bowl.
fn spot(nest: [f32; 3], member: u32, baseline: f32) -> Vec3 {
    let angle = crate::creature_ecology::unit(member as u64 ^ 0xE66) * std::f32::consts::TAU;
    let radius = NEST_RING.0 + (NEST_RING.1 - NEST_RING.0) * crate::creature_ecology::unit(member as u64 ^ 0x66E);
    Vec3::new(nest[0] + angle.cos() * radius, baseline, nest[2] + angle.sin() * radius)
}

fn import_shell(
    originals: &mut Originals,
    index: usize,
    shell: Shell,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<(crate::OriginalModel, crate::world_objects::OriginalWorldObject), String> {
    let name = model_name(index, shell);
    let key = crate::dbpf::ResourceKey {
        group_id: 0,
        instance_id: crate::world_objects::name_id(&name),
        type_id: crate::asset_catalog::PROP,
    };
    let d = crate::world_objects::definition_at(originals, key, &name)?;
    crate::world_objects::import_at(originals, d, meshes, images, materials)
}

/// A model's pose at `position` for a uniform `scale`: the lowest vertex on the
/// baseline (`world_objects::fitted_transform` for a given scale).
fn pose(model: &crate::OriginalModel, meshes: &Assets<Mesh>, position: Vec3, scale: f32) -> Option<Transform> {
    use bevy::mesh::VertexAttributeValues;
    let mesh = meshes.get(&model.handle)?;
    let Some(VertexAttributeValues::Float32x3(vertices)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        return None;
    };
    let min_y = vertices.iter().map(|v| v[1]).fold(f32::INFINITY, f32::min);
    min_y
        .is_finite()
        .then(|| Transform::from_translation(position - Vec3::Y * (min_y * scale)).with_scale(Vec3::splat(scale)))
}

/// The scale that fits a model's wider ground extent to `width`.
fn fit_scale(model: &crate::OriginalModel, meshes: &Assets<Mesh>, width: f32) -> Option<f32> {
    use bevy::mesh::VertexAttributeValues;
    let mesh = meshes.get(&model.handle)?;
    let Some(VertexAttributeValues::Float32x3(vertices)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        return None;
    };
    let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    for v in vertices {
        lo = lo.min(Vec2::new(v[0], v[2]));
        hi = hi.max(Vec2::new(v[0], v[2]));
    }
    let span = (hi.x - lo.x).max(hi.y - lo.y);
    (span.is_finite() && span > 0.0).then_some(width / span)
}

/// Create the entity of one egg lying at `at` (on the ground `baseline`) of
/// nest `nest_index`, drawn with the herd's egg model scaled to `size`.
#[allow(clippy::too_many_arguments)]
fn place_egg(
    commands: &mut Commands,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    nest_index: usize,
    member: u32,
    model: usize,
    size: f32,
    at: Vec3,
) {
    let baseline = at.y;
    let imported = import_shell(originals, model, Shell::Base, meshes, images, materials)
        .ok()
        .and_then(|(m, source)| {
            let scale = fit_scale(&m, meshes, size * WIDTH_PER_SIZE)?;
            let transform = pose(&m, meshes, at, scale)?;
            let material = m.material.clone()?;
            Some((m, source, scale, transform, material))
        });
    let egg = NestEgg::new(nest_index, member, model, imported.as_ref().map_or(1.0, |i| i.2), baseline);
    match imported {
        Some((m, source, _, transform, material)) => {
            commands.spawn((
                StageOwned,
                egg,
                Name::new(format!("Nest egg {member}")),
                source,
                Mesh3d(m.draw_mesh()),
                MeshMaterial3d(material),
                transform,
            ));
        }
        None => {
            // Headless tests have no installation: the egg still exists.
            if !originals.resource_index.records.is_empty() {
                warn!("nest egg {member}: model {} could not be imported", model_name(model, Shell::Base));
            }
            commands.spawn((StageOwned, egg, Name::new(format!("Nest egg {member}")), Transform::from_translation(at)));
        }
    }
}

/// Lay the eggs the casting manager queued (`CastingState::pending_eggs`):
/// one entity per egg at its nest, drawn with the herd's egg model.
#[allow(clippy::too_many_arguments)]
pub fn spawn_laid_eggs(
    mut commands: Commands,
    casting: Option<ResMut<CastingState>>,
    run: Res<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
) {
    let Some(mut casting) = casting else { return };
    if casting.pending_eggs.is_empty() {
        return;
    }
    let laid = std::mem::take(&mut casting.pending_eggs);
    for (nest_index, member) in laid {
        let Some(nest) = run.species_world.nests.get(nest_index) else { continue };
        let model = herd_egg_index(nest_index, run.species_world.seed);
        let size = run.species_world.species(nest.species).map_or(1.0, |s| s.size).max(0.6);
        let baseline = surface.as_deref().map_or(-0.6, |s| s.height(nest.position[0], nest.position[2]));
        let at = spot(nest.position, member, baseline);
        place_egg(&mut commands, &mut originals, &mut meshes, &mut images, &mut materials, nest_index, member, model, size, at);
    }
}

/// Capture / development aid (`OPENSPORE_EGG_DEMO=1`): three eggs of the three
/// models around the avatar's home nest, so the eggs, their cursor and the eat
/// order can be looked at without waiting for a herd to lose a member.
#[allow(clippy::too_many_arguments)]
pub fn demo_eggs(
    mut commands: Commands,
    mut done: Local<bool>,
    run: Res<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
) {
    if *done || std::env::var_os("OPENSPORE_EGG_DEMO").is_none() || originals.resource_index.records.is_empty() {
        return;
    }
    *done = true;
    let home = run.creature_lifecycle.home;
    let baseline = surface.as_deref().map_or(-0.6, |s| s.height(home[0], home[2]));
    for (k, (dx, dz)) in [(-3.0, 6.0), (0.5, 7.5), (3.5, 5.5)].into_iter().enumerate() {
        let at = Vec3::new(home[0] + dx, baseline, home[2] + dz);
        let model = [0, 4, 8][k];
        place_egg(&mut commands, &mut originals, &mut meshes, &mut images, &mut materials, 0, 0xFFFF_0000 + k as u32, model, 1.6, at);
    }
}

/// The herd update's egg tick (0xC6E0E5..0xC6E1A6): the hatch timer runs down,
/// the baby is released at zero, a hatched or broken shell is destroyed after
/// 10 s. The model follows the state.
#[allow(clippy::too_many_arguments)]
pub fn tick_eggs(
    mut commands: Commands,
    time: Res<Time>,
    casting: Option<ResMut<CastingState>>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut order: ResMut<EggOrder>,
    mut eggs: Query<(Entity, &mut NestEgg, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let Some(mut casting) = casting else { return };
    for (entity, mut egg, mut transform) in &mut eggs {
        match egg.state {
            state::HATCHED | state::BROKEN => {
                egg.shell_for += dt;
                if egg.shell_for > SHELL_SECONDS {
                    casting.incubating.remove(&egg.member);
                    if order.egg == Some(entity) {
                        order.clear();
                    }
                    commands.entity(entity).despawn();
                    continue;
                }
            }
            state::WAITING_TO_HATCH => {}
            _ => {
                egg.hatch_left -= dt;
                if egg.hatch_left <= 0.0 {
                    // The baby is created at the egg (0xC6B493): the member the
                    // egg stood for appears with its herd on the next pass.
                    egg.state = state::HATCHED;
                    egg.shell_for = 0.0;
                    casting.incubating.remove(&egg.member);
                }
            }
        }
        let shell = shell_of(egg.state);
        if shell != egg.drawn {
            egg.drawn = shell;
            let at = Vec3::new(transform.translation.x, egg.ground, transform.translation.z);
            let swapped = import_shell(&mut originals, egg.model, shell, &mut meshes, &mut images, &mut materials)
                .ok()
                .and_then(|(m, source)| Some((pose(&m, &meshes, at, egg.scale)?, m, source)));
            if let Some((placed, m, source)) = swapped {
                if let Some(material) = m.material.clone() {
                    commands.entity(entity).insert((source, Mesh3d(m.draw_mesh()), MeshMaterial3d(material)));
                    *transform = placed;
                }
            }
        }
    }
}

/// The avatar eats an egg: walk to it, hit it until it breaks, then eat it.
#[allow(clippy::too_many_arguments)]
pub fn eat_eggs(
    time: Res<Time>,
    mut order: ResMut<EggOrder>,
    mut controls: ResMut<CreatureControls>,
    mut encounter: ResMut<Encounter>,
    mut run: ResMut<RunProgress>,
    casting: Option<ResMut<CastingState>>,
    mut player: Query<(&Transform, &mut Health, &mut Hunger), With<Player>>,
    mut eggs: Query<(&mut NestEgg, &Transform), Without<Player>>,
) {
    let Some(entity) = order.egg else { return };
    let Ok((pt, mut health, mut hunger)) = player.single_mut() else { return };
    let Ok((mut egg, et)) = eggs.get_mut(entity) else {
        order.clear();
        return;
    };
    if !attackable(egg.state) {
        order.clear();
        return;
    }
    let dt = time.delta_secs();
    if dist_xz(pt.translation, et.translation) > REACH {
        // The walk's goal is gone before the avatar arrived: the player steered.
        if order.walking && controls.goal.is_none() {
            order.clear();
            return;
        }
        controls.follow(et.translation, REACH * 0.8);
        order.walking = true;
        order.timer = 0.0;
        return;
    }
    // Arrived: stop walking and hit the egg whenever the attack is ready.
    controls.goal = None;
    order.walking = false;
    order.timer -= dt;
    if order.timer > 0.0 {
        return;
    }
    order.timer = HIT_SECONDS;
    let (next, broken) = crack(egg.state);
    egg.state = next;
    if !broken {
        encounter.message = "You crack the egg".into();
        return;
    }
    // The egg is broken: the meal (0xD71BAC..0xD71C78).
    let meal = meal(health.max, true);
    health.heal(meal.health);
    hunger.eat(meal.hunger);
    run.award_dna(meal.dna, &GameStage::Creature);
    // Eating an egg of a species is an attack on it (retail: the relationship
    // manager's event, 0xBA4A00 with 6, not decoded: ADAPTED to the species
    // becoming aggravated). The egg's member never hatches.
    let member = egg.member;
    run.species_world.record_attack(member);
    if let Some(nest) = run.species_world.nests.get_mut(egg.nest) {
        nest.members.retain(|m| m.id != member);
    }
    if let Some(mut casting) = casting {
        casting.incubating.remove(&member);
    }
    egg.shell_for = 0.0;
    encounter.message = "You ate the egg".into();
    run.request_save();
    order.clear();
}

/// A click on a carcass or a fruit (the cursors 8 and 0x17): the avatar walks
/// there and checks its diet on arrival. A wrong food raises `crg-wrongfood`.
#[allow(clippy::too_many_arguments)]
pub fn eat_food_orders(
    mut order: ResMut<FoodOrder>,
    mut controls: ResMut<CreatureControls>,
    mut encounter: ResMut<Encounter>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    player: Query<&Transform, With<Player>>,
) {
    let Some(at) = order.at else { return };
    let Ok(pt) = player.single() else { return };
    if dist_xz(pt.translation, at) > FOOD_REACH {
        if order.walking && controls.goal.is_none() {
            order.clear();
            return;
        }
        controls.follow(at, FOOD_REACH * 0.8);
        order.walking = true;
        return;
    }
    let abilities = crate::creature_actions::player_abilities(&run, &originals);
    let diet_known = !originals.part_catalog.parts.is_empty();
    if diet_known && !can_eat(abilities.carnivore, abilities.herbivore, order.meat) {
        encounter.fail(AbilityError::WrongFood);
    }
    // The eating itself is the proximity eating of `creature_social::meat` /
    // `creature::creature_graze` (ADAPTED: the retail eat clip and its bites
    // are not ported).
    controls.goal = None;
    order.clear();
}

/// Each stage visit starts without orders or eggs.
fn reset(mut order: ResMut<EggOrder>, mut food: ResMut<FoodOrder>, casting: Option<ResMut<CastingState>>) {
    order.clear();
    food.clear();
    if let Some(mut casting) = casting {
        casting.incubating = HashSet::new();
        casting.pending_eggs.clear();
    }
}

/// An egg laid for `member` of nest `nest` now (the casting manager's birth
/// of an active herd): the member hatches from the egg as a baby.
pub fn lay(casting: &mut CastingState, run: &mut RunProgress, nest: usize, member: u32) {
    casting.incubating.insert(member);
    casting.pending_eggs.push((nest, member));
    if let Some(record) = run.species_world.member_mut(member) {
        record.role = MemberRole::Baby;
        record.baby_age = 0.0;
    }
}

pub struct EggsPlugin;

impl Plugin for EggsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EggOrder>()
            .init_resource::<FoodOrder>()
            .add_systems(OnEnter(GameStage::Creature), reset)
            .add_systems(
                Update,
                (demo_eggs, spawn_laid_eggs, tick_eggs, eat_eggs, eat_food_orders)
                    .chain()
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Creature))
                    .run_if(resource_exists::<Originals>)
                    .run_if(resource_exists::<RunProgress>)
                    .run_if(resource_exists::<Encounter>),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hit_advances_the_crack_stage_until_the_egg_breaks() {
        // 0xC6A880: 0 / 6 -> 1 -> 2 -> 4; 3, 4 and 5 stay.
        assert_eq!(crack(state::IDLE), (state::CRACKED_ONCE, false));
        assert_eq!(crack(state::LAID), (state::CRACKED_ONCE, false));
        assert_eq!(crack(state::CRACKED_ONCE), (state::CRACKED_TWICE, false));
        assert_eq!(crack(state::CRACKED_TWICE), (state::BROKEN, true));
        for s in [state::HATCHED, state::BROKEN, state::WAITING_TO_HATCH] {
            assert_eq!(crack(s), (s, false));
        }
        // Three hits break a freshly laid egg.
        let mut s = state::LAID;
        let mut hits = 0;
        loop {
            let (next, broken) = crack(s);
            s = next;
            hits += 1;
            if broken {
                break;
            }
        }
        assert_eq!(hits, 3);
    }

    #[test]
    fn only_whole_eggs_can_be_attacked() {
        for s in [state::IDLE, state::CRACKED_ONCE, state::CRACKED_TWICE, state::LAID] {
            assert!(attackable(s), "{s}");
        }
        for s in [state::HATCHED, state::BROKEN, state::WAITING_TO_HATCH] {
            assert!(!attackable(s), "{s}");
        }
    }

    #[test]
    fn a_herd_asks_for_at_most_two_eggs_less_the_ones_it_has() {
        assert_eq!(eggs_wanted(4, 4, 0, MAX_EGGS), 0);
        assert_eq!(eggs_wanted(4, 3, 0, MAX_EGGS), 1);
        assert_eq!(eggs_wanted(4, 1, 0, MAX_EGGS), 2, "capped at maxNumEggs");
        assert_eq!(eggs_wanted(4, 1, 1, MAX_EGGS), 1);
        assert_eq!(eggs_wanted(4, 1, 2, MAX_EGGS), 0);
        assert_eq!(eggs_wanted(4, 5, 0, MAX_EGGS), 0);
        assert_eq!(eggs_wanted(4, 1, 0, 1), 1, "the legacy single replacement");
    }

    #[test]
    fn the_retail_numbers_are_the_tuning_values() {
        assert_eq!(HATCH_SECONDS, 180.0);
        assert_eq!(MAX_EGGS, 2);
        assert_eq!(HEALTH_FRACTION, 0.33);
        assert_eq!(HUNGER, 50.0);
        assert_eq!(DNA, 25.0);
        assert_eq!(SHELL_SECONDS, 10.0);
    }

    #[test]
    fn eating_an_egg_heals_a_third_of_the_maximum_and_pays_dna_to_the_avatar_only() {
        let m = meal(60.0, true);
        assert!((m.health - 19.8).abs() < 1e-4);
        assert_eq!((m.dna, m.hunger), (25.0, 50.0));
        assert_eq!(meal(60.0, false).dna, 0.0);
    }

    #[test]
    fn the_egg_models_are_the_ten_numbered_sets() {
        assert_eq!(model_name(0, Shell::Base), "cr_egg_01_base");
        assert_eq!(model_name(9, Shell::Broken), "cr_egg_10_breaking_open");
        assert_eq!(model_name(3, Shell::Hatched), "cr_egg_04_hatched");
        assert_eq!(shell_of(state::LAID), Shell::Base);
        assert_eq!(shell_of(state::BROKEN), Shell::Broken);
        assert_eq!(shell_of(state::HATCHED), Shell::Hatched);
        for nest in 0..30 {
            assert!(herd_egg_index(nest, 7) < MODEL_COUNT);
        }
        // A herd keeps its model.
        assert_eq!(herd_egg_index(3, 7), herd_egg_index(3, 7));
    }

    #[test]
    fn the_diet_check_guards_carcasses_and_fruit_but_not_eggs() {
        // 0xC0B8E0 (carnivore level) for a carcass, 0xC0B900 (herbivore level) for a fruit.
        assert!(can_eat(1, 0, true));
        assert!(!can_eat(0, 3, true));
        assert!(can_eat(0, 1, false));
        assert!(!can_eat(2, 0, false));
        assert!(can_eat(1, 1, true) && can_eat(1, 1, false), "an omnivore eats both");
    }

    #[test]
    fn eggs_lie_inside_the_nest() {
        let nest = [10.0, 0.0, -20.0];
        for member in 300..320 {
            let p = spot(nest, member, -0.6);
            let d = ((p.x - nest[0]).powi(2) + (p.z - nest[2]).powi(2)).sqrt();
            assert!(d >= NEST_RING.0 - 1e-4 && d <= NEST_RING.1 + 1e-4, "{d}");
            assert_eq!(p.y, -0.6);
        }
    }

    fn world() -> App {
        use std::time::Duration;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(200)))
            .init_resource::<EggOrder>()
            .init_resource::<CreatureControls>()
            .init_resource::<Encounter>()
            .init_resource::<RunProgress>()
            .init_resource::<CastingState>()
            .init_resource::<Originals>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, (tick_eggs, eat_eggs).chain());
        app.update();
        app
    }

    fn avatar(app: &mut App) {
        app.world_mut().spawn((
            Player,
            Transform::from_xyz(0.0, 0.0, 0.0),
            Health { hp: 50.0, max: 100.0 },
            Hunger { food: 10.0, max: 100.0, hold: 0.0 },
        ));
    }

    #[test]
    fn the_avatar_cracks_an_egg_three_times_and_eats_it() {
        let mut app = world();
        avatar(&mut app);
        let egg = app.world_mut().spawn((NestEgg::new(0, 301, 0, 1.0, 0.0), Transform::from_xyz(0.5, 0.0, 0.0))).id();
        app.world_mut().resource_mut::<CastingState>().incubating.insert(301);
        app.world_mut().resource_mut::<EggOrder>().egg = Some(egg);
        let dna = app.world().resource::<RunProgress>().dna_this_stage;
        // The first hit lands at once, then one per `HIT_SECONDS`: three hits break it.
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().get::<NestEgg>(egg).unwrap().state, state::CRACKED_ONCE);
        for _ in 0..30 {
            app.update();
        }
        let e = app.world().get::<NestEgg>(egg).expect("the shell stays for a while");
        assert_eq!(e.state, state::BROKEN);
        assert!(app.world().resource::<EggOrder>().egg.is_none(), "the order is done");
        assert!(!app.world().resource::<CastingState>().incubating.contains(&301), "the egg's member never hatches");
        let world = app.world_mut();
        let (health, hunger) = world
            .query_filtered::<(&Health, &Hunger), With<Player>>()
            .single(world)
            .map(|(h, f)| (h.hp, f.food))
            .unwrap();
        assert!((health - 83.0).abs() < 0.01, "50 + 0.33 x 100: {health}");
        assert!((hunger - 60.0).abs() < 0.01, "10 + 50: {hunger}");
        assert!((app.world().resource::<RunProgress>().dna_this_stage - dna - 25.0).abs() < 1e-3);
        // The broken shell is destroyed after 10 s.
        for _ in 0..60 {
            app.update();
        }
        assert!(app.world().get::<NestEgg>(egg).is_none());
    }

    #[test]
    fn a_far_egg_is_walked_to_and_a_steered_avatar_gives_up_the_order() {
        let mut app = world();
        avatar(&mut app);
        let egg = app.world_mut().spawn((NestEgg::new(0, 301, 0, 1.0, 0.0), Transform::from_xyz(12.0, 0.0, 0.0))).id();
        app.world_mut().resource_mut::<EggOrder>().egg = Some(egg);
        app.update();
        let goal = app.world().resource::<CreatureControls>().goal.expect("the avatar walks to the egg");
        assert!((goal.point.x - 12.0).abs() < 1e-4);
        assert_eq!(app.world().get::<NestEgg>(egg).unwrap().state, state::LAID, "nothing is hit from afar");
        // The player steers (the goal is cleared): the order ends.
        app.world_mut().resource_mut::<CreatureControls>().goal = None;
        app.update();
        assert!(app.world().resource::<EggOrder>().egg.is_none());
    }

    #[test]
    fn arriving_at_the_egg_clears_the_walk_but_not_the_order() {
        // `goal_step` clears the goal on arrival: the order must go on to the hits.
        let mut app = world();
        avatar(&mut app);
        let egg = app.world_mut().spawn((NestEgg::new(0, 301, 0, 1.0, 0.0), Transform::from_xyz(6.0, 0.0, 0.0))).id();
        app.world_mut().resource_mut::<EggOrder>().egg = Some(egg);
        app.update();
        assert!(app.world().resource::<CreatureControls>().goal.is_some());
        // The avatar walks up (the controls system is not part of this app) and the goal ends.
        {
            let world = app.world_mut();
            let mut player = world.query_filtered::<&mut Transform, With<Player>>();
            player.single_mut(world).unwrap().translation = Vec3::new(5.0, 0.0, 0.0);
        }
        app.world_mut().resource_mut::<CreatureControls>().goal = None;
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<EggOrder>().egg, Some(egg), "the order survives the arrival");
        assert_eq!(app.world().get::<NestEgg>(egg).unwrap().state, state::CRACKED_ONCE);
    }

    #[test]
    fn an_egg_hatches_after_180_seconds_and_its_member_is_released() {
        let mut app = world();
        let egg = app.world_mut().spawn((NestEgg::new(0, 301, 0, 1.0, 0.0), Transform::default())).id();
        app.world_mut().resource_mut::<CastingState>().incubating.insert(301);
        app.world_mut().get_mut::<NestEgg>(egg).unwrap().hatch_left = 1.0;
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(app.world().get::<NestEgg>(egg).unwrap().state, state::LAID);
        assert!(app.world().resource::<CastingState>().incubating.contains(&301));
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().get::<NestEgg>(egg).unwrap().state, state::HATCHED);
        assert!(!app.world().resource::<CastingState>().incubating.contains(&301), "the baby is released");
    }

    #[test]
    fn a_click_orders_what_the_cursor_showed() {
        use crate::creature_hud::cursor::action as id;
        let at = Vec3::new(1.0, 0.0, 2.0);
        assert_eq!(click_order(id::ATTACK_EGG, Some(Entity::PLACEHOLDER), None), ClickOrder::Egg(Entity::PLACEHOLDER));
        assert_eq!(click_order(id::EAT_CARCASS, None, Some(at)), ClickOrder::Food { meat: true, at });
        assert_eq!(click_order(id::EAT_FRUIT, None, Some(at)), ClickOrder::Food { meat: false, at });
        // No object, or another action: nothing is ordered (the click only walks).
        assert_eq!(click_order(id::ATTACK_EGG, None, None), ClickOrder::None);
        assert_eq!(click_order(id::EAT_FRUIT, None, None), ClickOrder::None);
        assert_eq!(click_order(id::ATTACK, Some(Entity::PLACEHOLDER), Some(at)), ClickOrder::None);
    }

    #[test]
    fn laying_marks_the_member_as_an_incubating_baby() {
        let mut casting = CastingState::default();
        let mut run = RunProgress::default();
        lay(&mut casting, &mut run, 2, 301);
        assert!(casting.incubating.contains(&301));
        assert_eq!(casting.pending_eggs, vec![(2, 301)]);
    }
}
