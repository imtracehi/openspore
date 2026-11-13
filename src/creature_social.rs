//! Creature-stage world play around the encounter rules: Spore-style award
//! banners, meat dropped by kills, NPC ecology (carnivores hunting
//! herbivores, migrating herds, knocked-out creatures returning to their
//! nests), healing at allied nests, and the world-anchored encounter cues
//! (the performer's action bubble, the friendship bar, health bars and names
//! above creatures).
//!
//! Ecology timings, hunting damage and the overlay layout are OpenSpore
//! adaptations; the action icons are the installed original HUD textures.
//! NPC movement here is a thin steering layer: every creature this module
//! moves carries its intent in `Ecology::steer`, so a locomotion system can
//! animate the same goal (see `STEER_FALLBACK`).
use crate::{
    creature_actions::{self, CreatureMind, Encounter, EncounterMode},
    epic_creatures::RoamerActor,
    game::*,
    persistence::WorldId,
    species::{Diet, NestStatus},
    stages::{GameStage, StageOwned},
};
use bevy::prelude::*;
use std::collections::VecDeque;

/// Seconds a knocked-out creature (beaten by a hunter or a giant, never by
/// the player, whose kills are permanent) stays away before it returns to
/// its nest with full health. Spore's nests keep their wild herds stocked.
pub const DOWNED_SECONDS: f32 = 40.0;
/// Food in the meat a kill leaves behind.
pub const MEAT_FOOD: f32 = 30.0;
/// Meat rots away after this long.
const MEAT_LIFETIME: f32 = 75.0;
const MEAT_REACH: f32 = 1.8;
/// A friendly nest heals the player standing in it, as in Spore: within
/// `creatureHealingRadius` (5) at `creatureHealingHealthRecovery` (7.5% of the
/// maximum HP per second); see `creature_rules::healing_per_second`.
pub const NEST_HEAL_RADIUS: f32 = crate::creature_rules::HEALING_RADIUS;
/// ADAPTED: seconds of carcass eating a successful hunt feeds a hunter (heals
/// it `creatureHealthPointsPerCarcassPerSec` x this).
const HUNTER_MEAL_SECONDS: f32 = 4.0;
/// Seconds an award banner stays on screen.
const BANNER_SECONDS: f32 = 3.6;
/// ADAPTED: how far a herbivore herd walks off to graze (`herd migration`
/// below; 9 units on the island at a tenth of the scale).
const HERD_TRIP: f32 = 30.0;
/// Hunting tuning.
/// (`creatureSightRadius`, speciestuning).
const HUNT_SIGHT: f32 = crate::creature_rules::SIGHT_RADIUS;
const HUNT_REACH: f32 = 1.9;
const HUNT_SECONDS: f32 = 14.0;
const HUNT_REST: f32 = 25.0;
/// Herd migration tuning.
const HERD_FIRST: f32 = 45.0;
const HERD_INTERVAL: f32 = 75.0;
const HERD_SECONDS: f32 = 28.0;
/// While `true`, this module moves the creatures it steers itself. A
/// locomotion system that walks NPCs toward `Ecology::steer` (with gait and
/// facing) makes this redundant; switch it off then to avoid double speed.
pub const STEER_FALLBACK: bool = true;

/// Big Spore-style award messages ("Befriended a species", "Extinction",
/// brain upgrades, new parts). Shown one at a time across the screen.
#[derive(Resource, Default, Debug)]
pub struct CreatureAwards {
    pub queue: VecDeque<String>,
    pub showing: Option<(String, f32)>,
    /// Recent awards, newest last (bounded; used by tests and diagnostics).
    pub history: Vec<String>,
}

impl CreatureAwards {
    pub fn push(&mut self, text: impl Into<String>) {
        let text = text.into();
        if self.history.len() >= 32 {
            self.history.remove(0);
        }
        self.history.push(text.clone());
        // A burst of awards (nest + species + part) plays in order, but a
        // flood never stalls newer news for long.
        if self.queue.len() >= 4 {
            self.queue.pop_front();
        }
        self.queue.push_back(text);
    }

    /// Advance the banner clock; returns the shown text and its opacity.
    pub fn tick(&mut self, dt: f32) -> Option<(String, f32)> {
        if let Some((_, age)) = self.showing.as_mut() {
            *age += dt.max(0.0);
        }
        if self
            .showing
            .as_ref()
            .is_none_or(|(_, age)| *age >= BANNER_SECONDS)
        {
            self.showing = self.queue.pop_front().map(|text| (text, 0.0));
        }
        self.showing.as_ref().map(|(text, age)| {
            let fade_in = (age / 0.25).min(1.0);
            let fade_out = ((BANNER_SECONDS - age) / 0.6).clamp(0.0, 1.0);
            (text.clone(), fade_in.min(fade_out))
        })
    }
}

/// Meat left by a kill. Carnivores and omnivores eat it by walking over it.
#[derive(Component, Debug, Clone, Copy)]
pub struct Meat {
    pub food: f32,
    pub age: f32,
}

/// Queue a piece of meat at a kill site. The dressing system gives it a mesh.
pub fn spawn_meat(commands: &mut Commands, position: Vec3, food: f32) {
    // Spawned at clearance above the base terrain height, so the grounding
    // pass keeps it resting on the local surface.
    commands.spawn((
        StageOwned,
        Meat { food, age: 0.0 },
        Name::new("Creature meat"),
        Transform::from_xyz(position.x, -0.6 + 0.3, position.z)
            .with_rotation(Quat::from_rotation_y(position.x * 1.7 + position.z)),
        Visibility::Inherited,
    ));
}

/// Knock a creature out (it is not killed; the persistent outcome stays).
pub fn knock_out(npc: &mut Npc, mind: &mut CreatureMind, visibility: &mut Visibility) {
    npc.hp = 0.0;
    mind.downed = DOWNED_SECONDS;
    mind.response_time = 0.0;
    *visibility = Visibility::Hidden;
}

/// Per-creature ecology state (transient; rebuilt on stage entry).
#[derive(Component, Debug, Clone, Default)]
pub struct Ecology {
    /// Persistent id of the prey this carnivore is chasing.
    pub hunting: Option<u32>,
    pub hunt_time: f32,
    /// Seconds before this hunter looks for prey again.
    pub rest: f32,
    /// Seconds since this creature last lost health.
    pub since_hurt: f32,
    pub last_hp: f32,
    /// Where ecology wants this creature to go, and at what multiple of its
    /// walk speed. Locomotion can animate toward it (see `STEER_FALLBACK`).
    pub steer: Option<(Vec3, f32)>,
}

#[derive(Default)]
pub struct HerdState {
    timer: Option<f32>,
    cursor: usize,
    active: Option<(usize, Vec3, f32)>,
    /// The last floating-origin turn this state follows.
    reanchor: u32,
}

pub struct CreatureGameplayPlugin;

impl Plugin for CreatureGameplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreatureAwards>()
            .add_plugins(crate::creature_events::CreatureEventsPlugin)
            .add_systems(
                OnEnter(GameStage::Creature),
                (reset_awards, crate::epic_creatures::setup)
                    .after(crate::creature::setup_creature)
                    .before(crate::persistence::restore),
            )
            .add_systems(
                Update,
                (
                    crate::creature_ecology::advance_day_clock,
                    crate::epic_creatures::roam,
                    crate::epic_creatures::respawn,
                    ecology,
                    meat,
                    nest_healing,
                    crate::creature_support::grow_babies,
                    crate::creature_support::sea_monster,
                    crate::creature_group::social_groups,
                )
                    .chain()
                    .after(creature_actions::update)
                    .before(crate::creature::creature_respawn_nests)
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Creature)),
            )
            .add_systems(
                Update,
                crate::creature_support::swim_float
                    .after(crate::GameSystems::Grounding)
                    .before(crate::GameSystems::Presentation)
                    .run_if(in_state(GameStage::Creature)),
            )
            .add_systems(
                Update,
                (
                    crate::creature_support::swim_test,
                    crate::creature_alpha::rules4_test,
                    crate::creature_group::group_test,
                    crate::epic_creatures::epic_test,
                )
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Creature)),
            )
            .add_systems(
                Update,
                (dress_meat, overlay.after(crate::camera_follow), crate::creature_alpha::paint_alphas)
                    .in_set(crate::GameSystems::Presentation)
                    .run_if(in_state(GameStage::Creature)),
            )
            .init_resource::<crate::creature_alpha::AlphaPaintCache>();
    }
}

fn reset_awards(mut awards: ResMut<CreatureAwards>) {
    *awards = CreatureAwards::default();
}

/// Whether a creature takes part in the simulation: within the casting
/// manager's deactivation distance of the avatar (150, a chord; the island
/// used its square edge). Without an avatar every creature does.
fn in_range(position: Vec3, player: Option<Vec3>) -> bool {
    player.is_none_or(|p| p.distance(position) <= crate::creature_ecology::casting_thresholds().1)
}

/// Step `position` toward `goal` on the ground plane, stopping `stop` short.
fn step_toward(position: Vec3, goal: Vec3, distance: f32, stop: f32) -> Vec3 {
    let delta = Vec3::new(goal.x - position.x, 0.0, goal.z - position.z);
    let length = delta.length();
    if length <= stop || length < 1e-4 {
        return Vec3::ZERO;
    }
    delta / length * distance.min(length - stop)
}

fn face(transform: &mut Transform, direction: Vec3) {
    let flat = Vec3::new(direction.x, 0.0, direction.z);
    if flat.length_squared() > 1e-6 {
        let flat = flat.normalize();
        transform.rotation = Quat::from_rotation_y((-flat.x).atan2(-flat.z));
    }
}

#[derive(Clone, Copy)]
struct Snapshot {
    entity: Entity,
    id: u32,
    position: Vec3,
    diet: Option<Diet>,
    species: u32,
    speed: f32,
    wild: bool,
    alive: bool,
    hp: f32,
}

/// Carnivores hunt herbivores, beaten creatures recover at their nest,
/// herds migrate, and calm wounded creatures slowly heal.
pub fn ecology(
    mut commands: Commands,
    time: Res<Time>,
    originals: Res<crate::Originals>,
    run: Res<RunProgress>,
    encounter: Res<Encounter>,
    mobility: Option<Res<crate::creature::mobility::CreatureMobility>>,
    player: Query<&Transform, With<Player>>,
    mut npcs: Query<
        (
            Entity,
            &WorldId,
            &mut Transform,
            &mut Npc,
            &mut CreatureMind,
            &mut Visibility,
            Option<&mut Ecology>,
        ),
        (Without<Player>, Without<RoamerActor>),
    >,
    herds: Query<&crate::creature_ecology::HerdMember>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    reanchored: Option<Res<crate::creature_planet::Reanchored>>,
    mut herd: Local<HerdState>,
) {
    // The grazing trip's goal turns with the planet (floating origin).
    if let Some(turn) = reanchored.as_deref().filter(|r| r.generation != herd.reanchor) {
        herd.reanchor = turn.generation;
        if let Some((_, destination, _)) = herd.active.as_mut() {
            *destination = turn.apply(*destination);
        }
    }
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let land = crate::creature_planet::Land::from_surface(surface.as_deref());
    let player_position = player.single().map(|t| t.translation).ok();
    let perception = crate::creature_rules::BASE_PERCEPTION;
    let notice_radius = mobility.as_deref().map_or(perception, |mobility| {
        let abilities = crate::creature_actions::player_abilities(&run, &originals);
        mobility.npc_acquisition_radius(perception, abilities.sneak)
    });
    let world = &run.species_world;
    // Recovery first: hidden knocked-out creatures tick back to their nest.
    for (entity, id, mut t, mut npc, mut mind, mut visibility, eco) in &mut npcs {
        if eco.is_none() {
            commands.entity(entity).insert(Ecology {
                last_hp: npc.hp,
                ..default()
            });
        }
        if mind.defeated {
            continue;
        }
        // A reload can restore a beaten (0 HP) creature that was mid-recovery.
        if npc.hp <= 0.0 && mind.downed <= 0.0 {
            mind.downed = 3.0;
        }
        if mind.downed > 0.0 {
            *visibility = Visibility::Hidden;
            mind.downed = (mind.downed - dt).max(0.0);
            if mind.downed <= 0.0 {
                npc.hp = crate::creature::member_max_hp(world, id.0, &originals)
                    .unwrap_or_else(|| crate::creature_rules::npc_max_hp(0));
                if let Some(home) = world.member_position(id.0) {
                    t.translation.x = home[0];
                    t.translation.z = home[2];
                }
                mind.response_time = 0.0;
                mind.attack_timer = 0.0;
                *visibility = Visibility::Inherited;
            }
        }
    }
    let snapshots: Vec<Snapshot> = npcs
        .iter()
        .map(|(entity, id, t, npc, mind, _, _)| {
            let species = world.species_for_member(id.0);
            Snapshot {
                entity,
                id: id.0,
                position: t.translation,
                diet: species.map(|s| s.diet),
                species: species.map_or(u32::MAX, |s| s.id),
                speed: npc.speed,
                wild: !mind.allied
                    && !run.creature_pack.members.contains(&id.0)
                    && encounter.target != Some(id.0)
                    && mind.response_time <= 0.0,
                alive: !mind.defeated && mind.downed <= 0.0 && npc.hp > 0.0,
                hp: npc.hp,
            }
        })
        .collect();
    let can_prey = |s: &Snapshot| {
        s.alive
            && s.wild
            && in_range(s.position, player_position)
            && matches!(s.diet, Some(Diet::Herbivore | Diet::Omnivore))
    };
    // Movement and damage are planned from the snapshot, then applied.
    let mut moves: Vec<(Entity, Vec3, Vec3)> = Vec::new();
    let mut damage: Vec<(Entity, f32)> = Vec::new();
    let mut hunt_updates: Vec<(Entity, Option<u32>, f32, f32)> = Vec::new();
    // Steering intent is rebuilt every frame: nothing steers by default.
    let mut steers: Vec<(Entity, Option<(Vec3, f32)>)> =
        snapshots.iter().map(|s| (s.entity, None)).collect();
    let mut fed: Vec<(Entity, u32)> = Vec::new();
    let mut attack_timers: Vec<(Entity, f32)> = Vec::new();
    for hunter in &snapshots {
        let Ok((_, _, _, npc, mind, _, Some(eco))) = npcs.get(hunter.entity) else {
            continue;
        };
        let hostile_engaged = npc.hostile
            && player_position.is_some_and(|p| dist_xz(p, hunter.position) < notice_radius);
        // A retail herd hunts by its schedule: never asleep or drinking (C4C).
        let off_duty = herds.get(hunter.entity).is_ok_and(|member| {
            matches!(
                crate::creature_ecology::member_activity(world, Some(member)),
                crate::creature_ecology::Activity::Sleep | crate::creature_ecology::Activity::Drink
            )
        });
        if hunter.diet != Some(Diet::Carnivore)
            || !hunter.alive
            || !hunter.wild
            || !in_range(hunter.position, player_position)
            || hostile_engaged
            || off_duty
        {
            if eco.hunting.is_some() {
                hunt_updates.push((hunter.entity, None, 0.0, eco.rest.max(4.0)));
            }
            continue;
        }
        let mut rest = (eco.rest - dt).max(0.0);
        let mut hunt_time = (eco.hunt_time - dt).max(0.0);
        let mut prey = eco
            .hunting
            .and_then(|id| snapshots.iter().find(|s| s.id == id))
            .filter(|p| {
                can_prey(p)
                    && p.species != hunter.species
                    && dist_xz(p.position, hunter.position) < 16.0
            })
            .copied();
        if eco.hunting.is_some() && (prey.is_none() || hunt_time <= 0.0) {
            // The prey escaped (or was claimed by someone else).
            prey = None;
            rest = HUNT_REST * 0.6;
        }
        if prey.is_none() && eco.hunting.is_none() && rest <= 0.0 {
            prey = snapshots
                .iter()
                .filter(|p| can_prey(p) && p.species != hunter.species)
                .map(|p| (dist_xz(p.position, hunter.position), p))
                .filter(|(d, _)| *d <= HUNT_SIGHT)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, p)| *p);
            if prey.is_some() {
                hunt_time = HUNT_SECONDS;
            } else {
                rest = 3.0;
            }
        }
        let Some(prey) = prey else {
            hunt_updates.push((hunter.entity, None, 0.0, rest));
            continue;
        };
        hunt_updates.push((hunter.entity, Some(prey.id), hunt_time, rest));
        let distance = dist_xz(prey.position, hunter.position);
        steers.push((hunter.entity, Some((prey.position, 1.25))));
        steers.push((
            prey.entity,
            Some((prey.position * 2.0 - hunter.position, 1.1)),
        ));
        if STEER_FALLBACK {
            let chase = step_toward(
                hunter.position,
                prey.position,
                hunter.speed * 1.25 * dt,
                1.4,
            );
            moves.push((hunter.entity, chase, prey.position - hunter.position));
            let away = (prey.position - hunter.position)
                .with_y(0.0)
                .normalize_or_zero();
            let flee = away * prey.speed * 1.1 * dt;
            moves.push((prey.entity, flee, away));
        }
        if distance <= HUNT_REACH + 0.6 && mind.attack_timer <= 0.0 {
            let ranks = creature_actions::levels(
                creature_actions::member_abilities(&run, &originals, hunter.id),
                EncounterMode::Combat,
            );
            let ranks = if ranks.iter().all(|r| *r == 0) {
                [1, 0, 0, 0]
            } else {
                ranks
            };
            if let Some((_, ability)) = creature_actions::pack_attack(
                &originals.creature_abilities,
                ranks,
                distance.min(HUNT_REACH),
            ) {
                // Every NPC hit carries its herd's DamageMultiplier (C2D).
                let ability = creature_actions::CombatAbility {
                    damage: ability.damage * creature_actions::attacker_multiplier(&run, hunter.id),
                    ..ability
                };
                damage.push((prey.entity, ability.damage));
                attack_timers.push((hunter.entity, ability.recharge.max(1.0)));
                if prey.hp - ability.damage <= 0.0 {
                    fed.push((hunter.entity, hunter.id));
                }
                // Cornered prey with weapons bites back.
                let prey_ranks = creature_actions::levels(
                    creature_actions::member_abilities(&run, &originals, prey.id),
                    EncounterMode::Combat,
                );
                if let Some((_, counter)) = creature_actions::pack_attack(
                    &originals.creature_abilities,
                    prey_ranks,
                    distance.min(HUNT_REACH),
                ) {
                    damage.push((
                        hunter.entity,
                        counter.damage * creature_actions::attacker_multiplier(&run, prey.id),
                    ));
                }
            }
        }
    }
    // Herd migration: one herbivore nest at a time wanders off to graze.
    let herd = &mut *herd;
    let timer = herd.timer.get_or_insert(HERD_FIRST);
    *timer -= dt;
    if herd.active.is_none() && *timer <= 0.0 {
        *timer = HERD_INTERVAL;
        let candidates: Vec<usize> = (0..world.nests.len())
            .filter(|&i| {
                let nest = &world.nests[i];
                world
                    .species(nest.species)
                    .is_some_and(|s| s.diet == Diet::Herbivore)
                    && world.nest_status(i) == NestStatus::Wild
                    && !world.species_aggravated(nest.species)
            })
            .collect();
        if !candidates.is_empty() {
            let index = candidates[herd.cursor % candidates.len()];
            herd.cursor += 1;
            let home = Vec3::from_array(world.nests[index].position);
            let angle = herd.cursor as f32 * 2.399 + home.x * 0.1;
            // The trip ends on land (the island clamped it to its square).
            let goal = land.land_toward(home.xz(), land.toward(home.xz(), HERD_TRIP, angle));
            let destination = Vec3::new(goal.x, home.y, goal.y);
            let near_lair = world
                .roamers
                .iter()
                .any(|r| dist_xz(Vec3::from_array(r.lair), destination) < HERD_TRIP);
            if !near_lair {
                herd.active = Some((index, destination, HERD_SECONDS));
            }
        }
    }
    if let Some((index, destination, remaining)) = herd.active {
        let remaining = remaining - dt;
        herd.active = (remaining > 0.0 && index < world.nests.len()).then_some((
            index,
            destination,
            remaining,
        ));
        if let Some(nest) = world.nests.get(index).filter(|_| herd.active.is_some()) {
            let members: Vec<&Snapshot> = snapshots
                .iter()
                .filter(|s| {
                    s.alive && s.wild && in_range(s.position, player_position) && nest.member(s.id).is_some()
                })
                .collect();
            let count = members.len().max(1);
            for (slot, member) in members.into_iter().enumerate() {
                if steers
                    .iter()
                    .any(|(e, s)| *e == member.entity && s.is_some())
                {
                    continue; // fleeing a hunter comes first
                }
                let angle = slot as f32 / count as f32 * std::f32::consts::TAU;
                let goal = destination + Vec3::new(angle.cos(), 0.0, angle.sin()) * 1.6;
                steers.push((member.entity, Some((goal, 1.0))));
                if STEER_FALLBACK {
                    let step = step_toward(member.position, goal, member.speed * 1.1 * dt, 0.8);
                    moves.push((member.entity, step, step));
                }
            }
        }
    }
    // Apply the plan.
    for (entity, delta, facing) in moves {
        if let Ok((_, _, mut t, _, _, _, _)) = npcs.get_mut(entity) {
            // Herds stay on land (the island clamped them to its square).
            t.translation = land.step(t.translation, t.translation + delta);
            face(&mut t, facing);
        }
    }
    for (entity, timer) in attack_timers {
        if let Ok((_, _, _, _, mut mind, _, _)) = npcs.get_mut(entity) {
            mind.attack_timer = timer;
        }
    }
    for (entity, hunting, hunt_time, rest) in hunt_updates {
        if let Ok((_, _, _, _, _, _, Some(mut eco))) = npcs.get_mut(entity) {
            eco.hunting = hunting;
            eco.hunt_time = hunt_time;
            eco.rest = rest;
        }
    }
    for (entity, steer) in steers {
        if let Ok((_, _, _, _, _, _, Some(mut eco))) = npcs.get_mut(entity) {
            eco.steer = steer;
        }
    }
    for (entity, amount) in damage {
        if let Ok((_, _, t, mut npc, mut mind, mut visibility, _)) = npcs.get_mut(entity) {
            if npc.hp <= 0.0 || mind.defeated {
                continue;
            }
            npc.hp = (npc.hp - amount).max(0.0);
            if npc.hp <= 0.0 {
                let at = t.translation;
                knock_out(&mut npc, &mut mind, &mut visibility);
                spawn_meat(&mut commands, at, MEAT_FOOD * 0.7);
            }
        }
    }
    let difficulty = crate::creature_rules::Difficulty::from_index(run.cell.difficulty);
    for (entity, id) in fed {
        let max = crate::creature::member_max_hp(world, id, &originals)
            .unwrap_or(crate::creature_rules::npc_max_hp(0));
        if let Ok((_, _, _, mut npc, _, _, Some(mut eco))) = npcs.get_mut(entity) {
            // A successful hunt feeds the hunter (never past its build): it
            // eats the carcass for a few seconds at the retail healing rate.
            eco.hunting = None;
            eco.rest = HUNT_REST;
            let heal = crate::creature_rules::health_per_carcass_per_sec(difficulty)
                * HUNTER_MEAL_SECONDS;
            npc.hp = (npc.hp + heal).min(max).max(npc.hp);
        }
    }
    // Calm creatures slowly recover from wounds (never mid-fight).
    for (_, id, _, mut npc, mind, _, eco) in &mut npcs {
        let Some(mut eco) = eco else {
            continue;
        };
        if npc.hp < eco.last_hp - 1e-4 {
            eco.since_hurt = 0.0;
        } else {
            eco.since_hurt += dt;
        }
        if !npc.hostile
            && !mind.defeated
            && mind.downed <= 0.0
            && npc.hp > 0.0
            && eco.since_hurt > 8.0
        {
            let max = crate::creature::member_max_hp(world, id.0, &originals)
                .unwrap_or(crate::creature_rules::npc_max_hp(0));
            if npc.hp < max {
                // ADAPTED: calm wounded creatures heal at the retail healing
                // rate (a fraction of their maximum HP per second).
                npc.hp = (npc.hp + crate::creature_rules::healing_per_second(max) * dt).min(max);
            }
        }
        eco.last_hp = npc.hp;
    }
}

/// Meat rots, and a meat-eating player eats it by walking over it.
pub fn meat(
    mut commands: Commands,
    time: Res<Time>,
    originals: Res<crate::Originals>,
    mut run: ResMut<RunProgress>,
    mut encounter: ResMut<Encounter>,
    mut player: Query<(&Transform, &mut Hunger, Option<&mut Health>), With<Player>>,
    mut meat: Query<(Entity, &Transform, &mut Meat), Without<Player>>,
) {
    let dt = time.delta_secs();
    let difficulty = crate::creature_rules::Difficulty::from_index(run.cell.difficulty);
    let player_abilities = creature_actions::player_abilities(&run, &originals);
    let can_eat =
        originals.part_catalog.parts.is_empty() || crate::editor::can_eat(player_abilities, true);
    let mut player = player.single_mut().ok();
    for (entity, t, mut piece) in &mut meat {
        piece.age += dt;
        if piece.age >= MEAT_LIFETIME {
            commands.entity(entity).despawn();
            continue;
        }
        let Some((pt, hunger, health)) = player.as_mut() else {
            continue;
        };
        let wounded = health.as_ref().is_some_and(|h| h.hp < h.max);
        if can_eat
            && dist_xz(pt.translation, t.translation) <= MEAT_REACH
            && (hunger.food < hunger.max || wounded)
        {
            // Retail carcass eating (0xD70A80): 15 hunger per second out of the
            // carcass and 1.75 HP per second, until the carcass is gone.
            let (gain, heal) = crate::creature_rules::eat_carcass(piece.food, dt, difficulty);
            piece.food -= gain;
            hunger.eat(gain);
            if let Some(health) = health.as_mut() {
                health.heal(heal);
            }
            run.diet_carnivore += gain / MEAT_FOOD;
            encounter.message = "You ate the meat".into();
            if piece.food <= 0.0 {
                commands.entity(entity).despawn();
            }
        }
    }
}

/// Standing in an allied species' nest heals the player.
pub fn nest_healing(
    time: Res<Time>,
    run: Res<RunProgress>,
    mut encounter: ResMut<Encounter>,
    mut player: Query<(&Transform, &mut Health), With<Player>>,
    mut inside: Local<Option<usize>>,
) {
    let Ok((t, mut health)) = player.single_mut() else {
        return;
    };
    let threatened = encounter.mode == EncounterMode::Combat && encounter.target.is_some();
    let nest = run
        .species_world
        .allied_nest_near(t.translation.to_array(), NEST_HEAL_RADIUS);
    if let Some(index) = nest {
        if *inside != Some(index) {
            let name = run
                .species_world
                .species(run.species_world.nests[index].species)
                .map(|s| s.name.clone())
                .unwrap_or_default();
            encounter.message = format!("The allied {name} nest shelters you: rest here to heal");
        }
        if !threatened && !health.dead() {
            let rate = crate::creature_rules::healing_per_second(health.max);
            health.heal(rate * time.delta_secs());
        }
    }
    *inside = nest;
}

#[derive(Resource)]
struct MeatAssets {
    mesh: Handle<Mesh>,
    bone: Handle<Mesh>,
    flesh: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
}

/// Give queued meat a simple drumstick (no original carcass is wired yet).
fn dress_meat(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Option<Res<MeatAssets>>,
    added: Query<Entity, Added<Meat>>,
) {
    if added.is_empty() {
        return;
    }
    let assets = match assets {
        Some(assets) => MeatAssets {
            mesh: assets.mesh.clone(),
            bone: assets.bone.clone(),
            flesh: assets.flesh.clone(),
            white: assets.white.clone(),
        },
        None => {
            let created = MeatAssets {
                mesh: meshes.add(Sphere::new(0.32).mesh().uv(16, 10)),
                bone: meshes.add(Capsule3d::new(0.07, 0.5)),
                flesh: materials.add(StandardMaterial {
                    base_color: Color::srgb(0.62, 0.16, 0.12),
                    perceptual_roughness: 0.55,
                    ..default()
                }),
                white: materials.add(StandardMaterial {
                    base_color: Color::srgb(0.93, 0.9, 0.82),
                    perceptual_roughness: 0.7,
                    ..default()
                }),
            };
            let copy = MeatAssets {
                mesh: created.mesh.clone(),
                bone: created.bone.clone(),
                flesh: created.flesh.clone(),
                white: created.white.clone(),
            };
            commands.insert_resource(created);
            copy
        }
    };
    for entity in &added {
        commands
            .entity(entity)
            .insert((
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(assets.flesh.clone()),
            ))
            .with_children(|meat| {
                meat.spawn((
                    StageOwned,
                    Mesh3d(assets.bone.clone()),
                    MeshMaterial3d(assets.white.clone()),
                    Transform::from_xyz(0.32, 0.05, 0.0)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));
            });
    }
}

// ------------------------------------------------------------ overlay

#[derive(Component)]
struct OverlayRoot;
#[derive(Component)]
struct BannerRoot;
#[derive(Component)]
struct BannerText;
#[derive(Component)]
struct OverlayPart;

#[derive(Component, Clone, Copy)]
struct OverlayWidget {
    actor: Entity,
    label: Entity,
    bubble: Entity,
    icon: Entity,
    icon_text: Entity,
    friend_track: Entity,
    friend_fill: Entity,
    timer_fill: Entity,
    health_track: Entity,
    health_fill: Entity,
}

/// What one creature's cue shows this frame.
struct Cue {
    actor: Entity,
    anchor: Vec3,
    label: String,
    label_color: Color,
    bubble: Option<usize>,
    friendship: Option<(f32, f32)>,
    health: Option<(f32, Color)>,
    distance: f32,
}

const WIDGET_WIDTH: f32 = 150.0;
const MAX_CUES: usize = 10;

fn ui_text(value: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(value),
        TextFont {
            font_size: bevy::text::FontSize::Px(size),
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.97, 1.0)),
        TextShadow::default(),
    )
}

fn spawn_widget(commands: &mut Commands, root: Entity, actor: Entity) -> OverlayWidget {
    let label = commands
        .spawn((OverlayPart, ui_text("", 12.0), Node::default()))
        .id();
    let icon = commands
        .spawn((
            OverlayPart,
            ImageNode::default(),
            Node {
                width: Val::Px(34.0),
                height: Val::Px(34.0),
                ..default()
            },
        ))
        .id();
    let icon_text = commands
        .spawn((OverlayPart, ui_text("", 11.0), Node::default()))
        .id();
    let bubble = commands
        .spawn((
            OverlayPart,
            Node {
                width: Val::Px(46.0),
                height: Val::Px(46.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(23.0)),
                margin: UiRect::vertical(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.98, 0.97, 0.90, 0.92)),
            BorderColor::all(Color::srgba(0.25, 0.55, 0.85, 0.95)),
        ))
        .id();
    commands.entity(bubble).add_children(&[icon, icon_text]);
    let bar = |commands: &mut Commands, height: f32, color: Color| {
        let fill = commands
            .spawn((
                OverlayPart,
                Node {
                    width: Val::Percent(0.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(color),
            ))
            .id();
        let track = commands
            .spawn((
                OverlayPart,
                Node {
                    width: Val::Px(110.0),
                    height: Val::Px(height),
                    margin: UiRect::top(Val::Px(3.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.06, 0.78)),
                BorderColor::all(Color::srgba(0.9, 0.95, 1.0, 0.55)),
            ))
            .id();
        commands.entity(track).add_child(fill);
        (track, fill)
    };
    let (friend_track, friend_fill) = bar(commands, 9.0, Color::srgb(0.25, 0.75, 1.0));
    let timer_fill = commands
        .spawn((
            OverlayPart,
            Node {
                width: Val::Px(110.0),
                height: Val::Px(2.0),
                margin: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 0.92, 0.55, 0.85)),
        ))
        .id();
    let (health_track, health_fill) = bar(commands, 7.0, Color::srgb(0.9, 0.2, 0.15));
    let widget = OverlayWidget {
        actor,
        label,
        bubble,
        icon,
        icon_text,
        friend_track,
        friend_fill,
        timer_fill,
        health_track,
        health_fill,
    };
    let root_widget = commands
        .spawn((
            OverlayPart,
            widget,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(WIDGET_WIDTH),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .id();
    commands.entity(root_widget).add_children(&[
        label,
        bubble,
        friend_track,
        timer_fill,
        health_track,
    ]);
    commands.entity(root).add_child(root_widget);
    widget
}

/// World-anchored encounter cues: the performer's action bubble (original
/// HUD icon), friendship and response-time bars, health bars and names.
#[allow(clippy::too_many_arguments)]
fn overlay(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    encounter: Res<Encounter>,
    editor: Res<crate::editor::EditorState>,
    mut awards: ResMut<CreatureAwards>,
    camera: Query<(Entity, &Camera, &Transform), With<crate::GameCamera>>,
    player: Query<&Transform, With<Player>>,
    actors: Query<
        (
            Entity,
            &WorldId,
            &Transform,
            &Npc,
            &CreatureMind,
            &Visibility,
        ),
        Without<RoamerActor>,
    >,
    roamers: Query<
        (
            Entity,
            &WorldId,
            &Transform,
            &RoamerActor,
            &Health,
            &CreatureMind,
            &Visibility,
        ),
        Without<Npc>,
    >,
    roots: Query<Entity, With<OverlayRoot>>,
    banner_roots: Query<Entity, With<BannerRoot>>,
    widgets: Query<(Entity, &OverlayWidget)>,
    (mut nodes, mut texts, mut banner, mut images, mut colors): (
        Query<&mut Node, With<OverlayPart>>,
        Query<(&mut Text, &mut TextColor), Without<BannerText>>,
        Query<(&mut Text, &mut TextColor, &mut Node), (With<BannerText>, Without<OverlayPart>)>,
        Query<&mut ImageNode, With<OverlayPart>>,
        Query<&mut BackgroundColor, With<OverlayPart>>,
    ),
) {
    let dt = time.delta_secs();
    let shown_award = awards.tick(dt);
    let Ok((camera_entity, camera, camera_pose)) = camera.single() else {
        return;
    };
    let root = match roots.iter().next() {
        Some(root) => root,
        None => {
            let root = commands
                .spawn((
                    StageOwned,
                    OverlayRoot,
                    Name::new("Creature encounter cues"),
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    GlobalZIndex(-1),
                    UiTargetCamera(camera_entity),
                ))
                .id();
            // Filled from the next frame, once the camera link exists.
            let _ = root;
            return;
        }
    };
    if banner_roots.is_empty() {
        let text = commands
            .spawn((
                BannerText,
                Text::new(""),
                TextFont {
                    font_size: bevy::text::FontSize::Px(30.0),
                    ..default()
                },
                TextColor(Color::srgba(1.0, 0.93, 0.55, 0.0)),
                TextShadow::default(),
                Node::default(),
            ))
            .id();
        let banner_root = commands
            .spawn((
                StageOwned,
                BannerRoot,
                Name::new("Creature award banner"),
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    top: Val::Percent(17.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                GlobalZIndex(30),
                UiTargetCamera(camera_entity),
            ))
            .id();
        commands.entity(banner_root).add_child(text);
    }
    for (mut text, mut color, mut node) in &mut banner {
        match &shown_award {
            Some((award, alpha)) if !editor.open => {
                if text.0 != *award {
                    text.0 = award.clone();
                }
                color.0 = Color::srgba(1.0, 0.93, 0.55, *alpha);
                node.display = Display::Flex;
            }
            _ => node.display = Display::None,
        }
    }
    if let Ok(mut node) = nodes.get_mut(root) {
        node.display = if editor.open {
            Display::None
        } else {
            Display::Flex
        };
    }
    let player_position = player.single().map(|t| t.translation).unwrap_or(Vec3::ZERO);
    let world = &run.species_world;
    let social = encounter.mode == EncounterMode::Social;
    let mut cues: Vec<Cue> = Vec::new();
    for (entity, id, t, npc, mind, visibility) in &actors {
        if mind.defeated || mind.downed > 0.0 || *visibility == Visibility::Hidden {
            continue;
        }
        let Some(species) = world.species_for_member(id.0) else {
            continue;
        };
        let distance = dist_xz(t.translation, player_position);
        let targeted = encounter.target == Some(id.0);
        let _ = species;
        let max = crate::creature::member_max_hp(world, id.0, &originals).unwrap_or(npc.hp);
        let hurt = npc.hp < max - 0.5;
        let pack = run.creature_pack.members.contains(&id.0);
        let close_fight = hurt && distance < 14.0 && (npc.hostile || mind.allied);
        if !targeted && !close_fight && !(pack && hurt) {
            continue;
        }
        let performing = targeted && social && !mind.allied && mind.response_time > 0.0;
        let label = if targeted {
            if mind.allied {
                format!("{} · ally", species.name)
            } else if mind.bored > 0.0 && social {
                format!("{} · not interested", species.name)
            } else {
                species.name.clone()
            }
        } else {
            String::new()
        };
        let health_color = if mind.allied {
            Color::srgb(0.3, 0.85, 0.3)
        } else if npc.hostile {
            Color::srgb(0.92, 0.2, 0.14)
        } else {
            Color::srgb(0.95, 0.75, 0.2)
        };
        cues.push(Cue {
            actor: entity,
            anchor: t.translation + Vec3::Y * (1.15 * t.scale.y + 0.55),
            label,
            label_color: if npc.hostile && !mind.allied {
                Color::srgb(1.0, 0.6, 0.55)
            } else {
                Color::srgb(0.95, 0.97, 1.0)
            },
            bubble: performing.then_some(mind.requested.min(3) as usize),
            friendship: (targeted
                && social
                && !mind.allied
                && (performing || mind.impression > 0.0))
                .then(|| {
                    (
                        (mind.impression / 100.0).clamp(0.0, 1.0),
                        (mind.response_time / creature_actions::SOCIAL_RESPONSE_WINDOW)
                            .clamp(0.0, 1.0),
                    )
                }),
            health: (hurt || (targeted && !social))
                .then(|| ((npc.hp / max.max(1.0)).clamp(0.0, 1.0), health_color)),
            distance,
        });
    }
    for (entity, id, t, roamer, health, mind, visibility) in &roamers {
        if mind.defeated || *visibility == Visibility::Hidden {
            continue;
        }
        let distance = dist_xz(t.translation, player_position);
        let targeted = encounter.target == Some(id.0);
        if !targeted && distance > 26.0 {
            continue;
        }
        let performing = targeted && social && !mind.allied && mind.response_time > 0.0;
        let name = world
            .roamer(id.0)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "Epic creature".into());
        cues.push(Cue {
            actor: entity,
            anchor: t.translation + Vec3::Y * (1.15 * t.scale.y + 0.55),
            label: if mind.allied {
                format!("{name} · ally")
            } else if roamer.aggressive {
                format!("{name} · enraged")
            } else {
                name
            },
            label_color: if mind.allied {
                Color::srgb(0.6, 1.0, 0.6)
            } else {
                Color::srgb(1.0, 0.55, 0.4)
            },
            bubble: performing.then_some(mind.requested.min(3) as usize),
            friendship: (targeted
                && social
                && !mind.allied
                && (performing || mind.impression > 0.0))
                .then(|| {
                    (
                        (mind.impression / 100.0).clamp(0.0, 1.0),
                        (mind.response_time / creature_actions::SOCIAL_RESPONSE_WINDOW)
                            .clamp(0.0, 1.0),
                    )
                }),
            health: Some((
                health.frac(),
                if mind.allied {
                    Color::srgb(0.3, 0.85, 0.3)
                } else {
                    Color::srgb(0.85, 0.12, 0.35)
                },
            )),
            distance: distance - 6.0,
        });
    }
    cues.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    cues.truncate(MAX_CUES);
    // Reconcile widgets with this frame's cues.
    let mut existing: Vec<(Entity, OverlayWidget)> = widgets.iter().map(|(e, w)| (e, *w)).collect();
    existing.retain(|(entity, widget)| {
        let keep = cues.iter().any(|c| c.actor == widget.actor);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
    let camera_global = GlobalTransform::from(*camera_pose);
    let bob = (time.elapsed_secs() * 5.0).sin() * 3.0;
    for cue in cues {
        let Some((entity, widget)) = existing.iter().find(|(_, w)| w.actor == cue.actor).copied()
        else {
            spawn_widget(&mut commands, root, cue.actor);
            continue;
        };
        let projected = camera.world_to_viewport(&camera_global, cue.anchor).ok();
        let show = |nodes: &mut Query<&mut Node, With<OverlayPart>>, part: Entity, on: bool| {
            if let Ok(mut node) = nodes.get_mut(part) {
                let display = if on { Display::Flex } else { Display::None };
                if node.display != display {
                    node.display = display;
                }
            }
        };
        let Some(screen) = projected else {
            show(&mut nodes, entity, false);
            continue;
        };
        show(&mut nodes, entity, true);
        show(&mut nodes, widget.label, !cue.label.is_empty());
        show(&mut nodes, widget.bubble, cue.bubble.is_some());
        show(&mut nodes, widget.friend_track, cue.friendship.is_some());
        show(&mut nodes, widget.timer_fill, cue.friendship.is_some());
        show(&mut nodes, widget.health_track, cue.health.is_some());
        let height = if cue.label.is_empty() { 0.0 } else { 16.0 }
            + if cue.bubble.is_some() { 50.0 } else { 0.0 }
            + if cue.friendship.is_some() { 15.0 } else { 0.0 }
            + if cue.health.is_some() { 10.0 } else { 0.0 };
        if let Ok(mut node) = nodes.get_mut(entity) {
            node.left = Val::Px(screen.x - WIDGET_WIDTH * 0.5);
            node.top = Val::Px(screen.y - height);
        }
        if let Ok((mut text, mut color)) = texts.get_mut(widget.label) {
            if text.0 != cue.label {
                text.0 = cue.label.clone();
            }
            color.0 = cue.label_color;
        }
        if let Some(action) = cue.bubble {
            let name = creature_actions::SOCIAL[action];
            let icon = originals
                .ui_art
                .action_icons
                .iter()
                .find(|icon| icon.action.eq_ignore_ascii_case(name));
            show(&mut nodes, widget.icon, icon.is_some());
            show(&mut nodes, widget.icon_text, icon.is_none());
            if let (Some(icon), Ok(mut image)) = (icon, images.get_mut(widget.icon)) {
                if image.image != icon.image {
                    image.image = icon.image.clone();
                }
            }
            if let Ok((mut text, _)) = texts.get_mut(widget.icon_text) {
                if text.0 != name {
                    text.0 = name.to_string();
                }
            }
            if let Ok(mut node) = nodes.get_mut(widget.bubble) {
                node.margin = UiRect::new(
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Px(2.0 + bob),
                    Val::Px(2.0 - bob),
                );
            }
        }
        if let Some((fill, timer)) = cue.friendship {
            if let Ok(mut node) = nodes.get_mut(widget.friend_fill) {
                node.width = Val::Percent(fill * 100.0);
            }
            if let Ok(mut node) = nodes.get_mut(widget.timer_fill) {
                node.width = Val::Px(110.0 * timer);
            }
        }
        if let Some((fill, color)) = cue.health {
            if let Ok(mut node) = nodes.get_mut(widget.health_fill) {
                node.width = Val::Percent(fill * 100.0);
            }
            if let Ok(mut background) = colors.get_mut(widget.health_fill) {
                background.0 = color;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn award_banners_play_in_order_fade_and_stay_bounded() {
        let mut awards = CreatureAwards::default();
        assert!(awards.tick(0.1).is_none());
        awards.push("Nest allied");
        awards.push("Befriended a species");
        let (first, alpha) = awards.tick(0.0).unwrap();
        assert_eq!(first, "Nest allied");
        assert_eq!(alpha, 0.0, "a banner fades in");
        let (_, alpha) = awards.tick(1.0).unwrap();
        assert_eq!(alpha, 1.0);
        let (second, _) = awards.tick(BANNER_SECONDS).unwrap();
        assert_eq!(second, "Befriended a species");
        assert!(awards.tick(BANNER_SECONDS + 0.1).is_none());
        for i in 0..40 {
            awards.push(format!("award {i}"));
        }
        assert!(awards.history.len() <= 32);
        assert!(awards.queue.len() <= 4);
        assert_eq!(awards.history.last().unwrap(), "award 39");
    }

    #[test]
    fn steering_stops_short_and_never_overshoots() {
        let step = step_toward(Vec3::ZERO, Vec3::new(10.0, 3.0, 0.0), 2.0, 1.0);
        assert!((step.length() - 2.0).abs() < 1e-5);
        assert_eq!(step.y, 0.0);
        let near = step_toward(Vec3::ZERO, Vec3::new(1.5, 0.0, 0.0), 2.0, 1.0);
        assert!((near.x - 0.5).abs() < 1e-5, "stops at the requested gap");
        assert_eq!(
            step_toward(Vec3::ZERO, Vec3::new(0.5, 0.0, 0.0), 2.0, 1.0),
            Vec3::ZERO
        );
        assert!(in_range(Vec3::new(100.0, 0.0, 0.0), Some(Vec3::ZERO)));
        assert!(!in_range(Vec3::new(1000.0, 0.0, 1000.0), Some(Vec3::ZERO)));
        assert!(in_range(Vec3::new(1000.0, 0.0, 1000.0), None));
    }

    fn creature_app(run: RunProgress) -> App {
        let mut app = crate::runtime::headless_app(run);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        app.update();
        app.update();
        app
    }

    fn steps(app: &mut App, count: usize) {
        for _ in 0..count {
            app.update();
        }
    }

    fn place_player(app: &mut App, at: Vec3) {
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Transform, With<Player>>();
        players.single_mut(world).unwrap().translation = at;
    }

    fn press(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.release(key);
        input.clear();
    }

    /// Move every nest member except `keep` off the island (dormant), and
    /// stop the casting manager so no herd activates or leaves meanwhile.
    fn park_members_except(app: &mut App, keep: &[u32]) {
        if let Some(mut casting) = app
            .world_mut()
            .get_resource_mut::<crate::creature_ecology::CastingState>()
        {
            casting.timer = f32::NEG_INFINITY;
        }
        let world = app.world_mut();
        for (id, mut t) in world
            .query_filtered::<(&WorldId, &mut Transform), With<Npc>>()
            .iter_mut(world)
        {
            if !keep.contains(&id.0) {
                t.translation = Vec3::new(1000.0, 0.4, 1000.0);
            }
        }
    }

    fn roamer_entities(app: &mut App) -> Vec<(Entity, u32, Vec3)> {
        let world = app.world_mut();
        world
            .query::<(Entity, &WorldId, &Transform, &RoamerActor)>()
            .iter(world)
            .map(|(e, id, t, _)| (e, id.0, t.translation))
            .collect()
    }

    fn player_health(app: &mut App) -> (f32, f32) {
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Health, With<Player>>();
        let health = q.single(world).unwrap();
        (health.hp, health.max)
    }

    #[test]
    fn roamers_spawn_from_the_world_and_old_saves_gain_them_once() {
        let mut app = creature_app(RunProgress::default());
        let roamers = roamer_entities(&mut app);
        assert_eq!(
            roamers.len(),
            crate::species::EPIC_COUNT + crate::species::ROGUE_COUNT
        );
        let home = Vec3::new(crate::creature_ecology::HOME[0], 0.0, crate::creature_ecology::HOME[1]);
        for (_, id, at) in &roamers {
            assert!(*id >= crate::species::ROAMER_ID_BASE);
            // Retail lairs: the archetype bands, clear of the hatching nest.
            assert!(dist_xz(*at, home) >= crate::creature_ecology::HOME_CLEARANCE, "lair at {at}");
        }
        // A save from before roamers existed gains them on the next entry.
        let mut saved =
            serde_json::to_value(app.world().resource::<RunProgress>().clone()).unwrap();
        let world = saved["species_world"].as_object_mut().unwrap();
        world.remove("roamers");
        world.remove("roamers_generated");
        saved["checkpoint"] = serde_json::Value::Null;
        let old: RunProgress = serde_json::from_value(saved).unwrap();
        assert!(old.species_world.roamers.is_empty());
        let mut resumed = creature_app(old);
        assert_eq!(roamer_entities(&mut resumed).len(), roamers.len());
        // A slain giant never comes back.
        let first = roamers[0].1;
        resumed
            .world_mut()
            .resource_mut::<RunProgress>()
            .species_world
            .record_roamer(first, crate::species::Relationship::Defeated);
        let mut run = resumed.world().resource::<RunProgress>().clone();
        run.checkpoint = None;
        let mut again = creature_app(run);
        assert!(roamer_entities(&mut again)
            .iter()
            .all(|(_, id, _)| *id != first));
    }

    #[test]
    fn an_epic_creature_charges_and_mauls_a_player_who_comes_close() {
        let mut app = creature_app(RunProgress::default());
        park_members_except(&mut app, &[]);
        let epics: Vec<u32> = app
            .world()
            .resource::<RunProgress>()
            .species_world
            .roamers
            .iter()
            .filter(|r| r.kind == crate::species::RoamerKind::Epic)
            .map(|r| r.id)
            .collect();
        let (entity, _, at) = roamer_entities(&mut app)
            .into_iter()
            .find(|(_, id, _)| epics.contains(id))
            .unwrap();
        place_player(&mut app, Vec3::new(at.x, 0.7, at.z + 6.0));
        let (_, full) = player_health(&mut app);
        // A retail avatar has 10-ish HP: watch for the first wound, because a
        // second hit can kill it and it respawns at full health.
        let mut hurt = false;
        for _ in 0..240 {
            app.update();
            if player_health(&mut app).0 < full {
                hurt = true;
                break;
            }
        }
        assert!(app.world().get::<RoamerActor>(entity).unwrap().aggressive);
        assert!(hurt, "the giant must hurt the player (full {full})");
    }

    #[test]
    fn ignoring_a_performance_makes_the_creature_lose_interest() {
        let mut app = creature_app(RunProgress::default());
        let target = crate::species::MEMBER_ID_BASE;
        park_members_except(&mut app, &[target]);
        // Roamers too: a rogue's territory can reach the island centre.
        {
            let world = app.world_mut();
            for mut t in world
                .query_filtered::<&mut Transform, With<RoamerActor>>()
                .iter_mut(world)
            {
                t.translation = Vec3::new(100.0, 0.0, 100.0);
            }
        }
        {
            let world = app.world_mut();
            for (id, mut t, mut npc) in world
                .query::<(&WorldId, &mut Transform, &mut Npc)>()
                .iter_mut(world)
            {
                if id.0 == target {
                    t.translation = Vec3::new(0.0, 0.4, -2.0);
                    npc.hostile = false;
                }
            }
        }
        place_player(&mut app, Vec3::new(0.0, 0.7, 0.0));
        press(&mut app, KeyCode::KeyF);
        assert_eq!(
            app.world().resource::<Encounter>().target,
            Some(target),
            "{}",
            app.world().resource::<Encounter>().message
        );
        steps(
            &mut app,
            (creature_actions::SOCIAL_RESPONSE_WINDOW * 60.0) as usize + 30,
        );
        let encounter = app.world().resource::<Encounter>();
        assert_eq!(encounter.target, None);
        assert!(
            encounter.message.contains("lost interest"),
            "{}",
            encounter.message
        );
        let world = app.world_mut();
        let mind = world
            .query::<(&WorldId, &CreatureMind)>()
            .iter(world)
            .find(|(id, _)| id.0 == target)
            .map(|(_, m)| m.clone())
            .unwrap();
        assert!(mind.bored > 0.0);
        // While bored it refuses another performance.
        press(&mut app, KeyCode::KeyF);
        assert!(app
            .world()
            .resource::<Encounter>()
            .message
            .contains("not interested"));
    }

    #[test]
    fn carnivores_hunt_and_knocked_out_prey_returns_to_its_nest() {
        let mut app = creature_app(RunProgress::default());
        // Pick two spawned members of different species and make one a
        // hunter (the headless world's carnivores may all be in reserve).
        let (hunter, prey) = {
            let world = app.world_mut();
            let members: Vec<u32> = world
                .query_filtered::<&WorldId, With<Npc>>()
                .iter(world)
                .map(|id| id.0)
                .collect();
            let run = world.resource::<RunProgress>();
            let species = |id: u32| run.species_world.species_for_member(id).unwrap().id;
            let hunter = members[0];
            let prey = *members
                .iter()
                .find(|id| species(**id) != species(hunter))
                .unwrap();
            (hunter, prey)
        };
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            let hunter_species = run.species_world.species_for_member(hunter).unwrap().id;
            let prey_species = run.species_world.species_for_member(prey).unwrap().id;
            for s in &mut run.species_world.species {
                if s.id == hunter_species {
                    s.diet = Diet::Carnivore;
                } else if s.id == prey_species {
                    s.diet = Diet::Herbivore;
                }
            }
        }
        park_members_except(&mut app, &[hunter, prey]);
        place_player(&mut app, Vec3::new(-18.0, 0.7, 18.0));
        {
            let world = app.world_mut();
            for (id, mut t, mut npc) in world
                .query::<(&WorldId, &mut Transform, &mut Npc)>()
                .iter_mut(world)
            {
                if id.0 == hunter {
                    t.translation = Vec3::new(6.0, 0.4, 2.0);
                    npc.hostile = false;
                } else if id.0 == prey {
                    t.translation = Vec3::new(7.5, 0.4, 2.0);
                    npc.hp = 4.0;
                }
            }
        }
        let prey_state = |app: &mut App| {
            let world = app.world_mut();
            world
                .query::<(&WorldId, &CreatureMind, &Npc, &Visibility, &Transform)>()
                .iter(world)
                .find(|(id, ..)| id.0 == prey)
                .map(|(_, m, n, v, t)| (m.clone(), n.hp, *v, t.translation))
                .unwrap()
        };
        let mut knocked = false;
        for _ in 0..900 {
            app.update();
            if prey_state(&mut app).0.downed > 0.0 {
                knocked = true;
                break;
            }
        }
        assert!(knocked, "the hunter must catch and beat its prey");
        let (mind, hp, visibility, _) = prey_state(&mut app);
        assert!(!mind.defeated, "a hunt never records a player outcome");
        assert_eq!(hp, 0.0);
        assert_eq!(visibility, Visibility::Hidden);
        assert_eq!(
            app.world()
                .resource::<RunProgress>()
                .species_world
                .relationship(prey),
            Some(crate::species::Relationship::Neutral)
        );
        let meat = {
            let world = app.world_mut();
            world.query::<&Meat>().iter(world).count()
        };
        assert!(meat > 0, "the kill leaves meat");
        steps(&mut app, (DOWNED_SECONDS * 60.0) as usize + 30);
        let (mind, hp, visibility, at) = prey_state(&mut app);
        assert_eq!(mind.downed, 0.0);
        assert!(hp > 0.0);
        assert_eq!(visibility, Visibility::Inherited);
        let home = app
            .world()
            .resource::<RunProgress>()
            .species_world
            .member_position(prey)
            .unwrap();
        assert!(
            dist_xz(at, Vec3::from_array(home)) < 8.0,
            "it recovers at its nest"
        );
    }

    fn player_hunger(app: &mut App) -> f32 {
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Hunger, With<Player>>();
        q.single(world).unwrap().food
    }

    fn clear_bushes(app: &mut App) {
        let world = app.world_mut();
        let bushes: Vec<Entity> = world
            .query_filtered::<Entity, With<Bush>>()
            .iter(world)
            .collect();
        for bush in bushes {
            world.despawn(bush);
        }
    }

    #[test]
    fn hunger_holds_then_drains_and_starvation_costs_a_twentieth_hp_per_second() {
        let mut app = creature_app(RunProgress::default());
        park_members_except(&mut app, &[]);
        clear_bushes(&mut app);
        // Full hunger is held for hungerDecayDelay (10 s) before it drains.
        {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&mut Hunger, With<Player>>();
            let mut hunger = q.single_mut(world).unwrap();
            hunger.food = hunger.max;
            hunger.hold = 0.0;
        }
        steps(&mut app, 9 * 60);
        assert_eq!(player_hunger(&mut app), 100.0, "held at the maximum for 9 s");
        steps(&mut app, 3 * 60);
        let food = player_hunger(&mut app);
        assert!(food < 100.0 && food > 96.0, "drained for about 2 s: {food}");
        // Below hunger 1 the avatar loses creatureStarvationPain per second.
        {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&mut Hunger, With<Player>>();
            q.single_mut(world).unwrap().food = 0.0;
        }
        let (before, _) = player_health(&mut app);
        steps(&mut app, 120);
        let (after, _) = player_health(&mut app);
        assert!(
            (before - after - 0.1).abs() < 0.011,
            "lost {} HP in 2 s",
            before - after
        );
    }

    #[test]
    fn meat_is_eaten_over_time_at_the_retail_rate_and_heals() {
        let mut app = creature_app(RunProgress::default());
        park_members_except(&mut app, &[]);
        clear_bushes(&mut app);
        place_player(&mut app, Vec3::new(0.0, 0.7, 8.0));
        {
            let world = app.world_mut();
            let mut q = world.query_filtered::<(&mut Hunger, &mut Health), With<Player>>();
            let (mut hunger, mut health) = q.single_mut(world).unwrap();
            hunger.food = 40.0;
            health.hp = health.max * 0.5;
        }
        let (hp0, max) = player_health(&mut app);
        let piece = app
            .world_mut()
            .spawn((
                Meat {
                    food: 30.0,
                    age: 0.0,
                },
                Transform::from_xyz(0.0, 0.3, 8.0),
            ))
            .id();
        steps(&mut app, 60);
        // 15 hunger per second out of the carcass (minus 1.1 of drain), 1.75 HP per second.
        let food = player_hunger(&mut app);
        assert!((food - (40.0 + 15.0 - 1.111)).abs() < 1.0, "hunger {food}");
        let (hp, _) = player_health(&mut app);
        assert!((hp - (hp0 + 1.75).min(max)).abs() < 0.1, "hp {hp} from {hp0}");
        let left = app.world().get::<Meat>(piece).unwrap().food;
        assert!((left - 15.0).abs() < 0.5, "half the carcass is left: {left}");
        steps(&mut app, 90);
        assert!(
            app.world().get::<Meat>(piece).is_none(),
            "an eaten carcass is gone"
        );
    }

    #[test]
    fn the_avatar_walks_at_the_retail_gear_speed() {
        let mut app = creature_app(RunProgress::default());
        park_members_except(&mut app, &[]);
        clear_bushes(&mut app);
        place_player(&mut app, Vec3::new(0.0, 0.7, 12.0));
        let at = |app: &mut App| {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&Transform, With<Player>>();
            q.single(world).unwrap().translation
        };
        let start = at(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        steps(&mut app, 60);
        let moved = dist_xz(start, at(&mut app));
        // The headless avatar has no speed parts: gear 0 = 2.5 units per second.
        assert!((moved - 2.5).abs() < 0.3, "walked {moved} in one second");
    }

    #[test]
    fn standing_in_an_allied_nest_heals_the_player() {
        let mut app = creature_app(RunProgress::default());
        let members: Vec<u32> = app.world().resource::<RunProgress>().species_world.nests[0]
            .members
            .iter()
            .map(|m| m.id)
            .collect();
        let position = {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            for id in &members {
                run.species_world.record_befriend(*id);
            }
            assert_eq!(
                run.species_world.nest_status(0),
                crate::species::NestStatus::Allied
            );
            Vec3::from_array(run.species_world.nests[0].position)
        };
        park_members_except(&mut app, &[]);
        place_player(&mut app, Vec3::new(position.x, 0.7, position.z));
        {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&mut Health, With<Player>>();
            let mut health = q.single_mut(world).unwrap();
            health.hp = health.max * 0.5;
        }
        steps(&mut app, 60);
        // 7.5% of the maximum HP per second (creatureHealingHealthRecovery).
        let (hp, max) = player_health(&mut app);
        assert!(hp > max * 0.5 + max * 0.06, "healed to {hp}/{max}");
        assert!(hp < max * 0.5 + max * 0.09, "healed too fast: {hp}/{max}");
        assert!(app
            .world()
            .resource::<Encounter>()
            .message
            .contains("shelters you"));
    }
}
