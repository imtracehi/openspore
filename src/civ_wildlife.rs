//! Civilization wildlife: the eight epic predators the original places on
//! the Civ planet (`CivGameLevelGenerationList` 0x3BCE99E9: 8 x
//! `CVG_NPC_Epic_Predator`) and the casting manager's ambient animals (at most
//! `CastingManager_AnimalsAtMaxDensity` 15, shown inside 150 and culled beyond
//! 200, kept 60 from cities). Distances are retail (the stage is on the
//! planet: `civ_planet`); the lairs and animals stand on land around the level
//! origin and walk over the curved surface.
//!
//! The epics roam their lairs, maul vehicles that come close and can be shot
//! down by military vehicles (`civ_rts` targets them); their fate is saved in
//! `CivCampaign::epics`. The animals are scenery. Visuals are installed
//! creature models (the planet's own epic visuals when the Creature stage made
//! them), a tinted capsule without an install.
//!
//! ADAPTED: epic health, damage, speed and reach (the epic species record is
//! not decoded); retail Civ animals are herds from the planet's species, here
//! single wanderers.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::civ::{self, CivCommander, HOME_INDEX};
use crate::civ_planet::{Globe, Shape, CITY_SCALE, RETAIL_FROM_ISLAND};
use crate::civ_rts::CivUnit;
use crate::civ_rules;
use crate::game::RunProgress;
use crate::stages::{GameStage, StageOwned};

/// ADAPTED: an epic's health in vehicle HP units (a few vehicles need half a
/// minute) and its maul (Creature epics stomp every 3 s).
pub const EPIC_MAX_HP: f32 = 1000.0;
pub const EPIC_DAMAGE: f32 = 20.0;
pub const EPIC_ATTACK_PERIOD: f32 = 3.0;
/// ADAPTED: walk speed, the reach at which it notices a vehicle and the body
/// radius hits are measured from (island values at `CITY_SCALE`, beside the
/// vehicles they fight); its wander range at retail scale.
pub const EPIC_SPEED: f32 = 2.3 * CITY_SCALE;
pub const EPIC_AGGRO: f32 = 5.0 * CITY_SCALE;
pub const EPIC_BODY_RADIUS: f32 = 1.6 * CITY_SCALE;
pub const EPIC_TERRITORY: f32 = 4.0 * RETAIL_FROM_ISLAND;
pub const EPIC_SCALE: f32 = 2.4 * CITY_SCALE;
/// ADAPTED: ambient animals wander this far from their spot; drawn about the
/// size of a Creature-stage adult.
pub const ANIMAL_TERRITORY: f32 = 4.0 * RETAIL_FROM_ISLAND;
pub const ANIMAL_SPEED: f32 = 1.2 * CITY_SCALE;
pub const ANIMAL_SCALE: f32 = 1.2;
/// ADAPTED: where the lairs and the animals are spread (distance from the
/// level origin) and how far apart.
pub const EPIC_RING: (f32, f32) = (100.0, 450.0);
pub const ANIMAL_RING: (f32, f32) = (40.0, 420.0);
pub const EPIC_SPACING: f32 = 60.0;
pub const ANIMAL_SPACING: f32 = 15.0;

/// A saved epic predator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct EpicState {
    pub id: u32,
    pub hp: f32,
    pub alive: bool,
}

/// A live epic predator.
#[derive(Component, Debug, Clone)]
pub struct CivEpic {
    pub id: u32,
    pub hp: f32,
    pub lair: Vec3,
    pub goal: Vec3,
    pub wait: f32,
    pub step: u32,
    pub attack: f32,
    /// Seconds since it was last hit.
    pub hit: f32,
}

/// An ambient animal.
#[derive(Component, Debug, Clone)]
pub struct CivAnimal {
    pub home: Vec3,
    pub goal: Vec3,
    pub wait: f32,
    pub step: u32,
    pub shown: bool,
}

/// Deterministic [0, 1) from two integers.
fn hash01(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h % 10_000) as f32 / 10_000.0
}

/// `CastingManager_CityRadiusBuffer` (60): wildlife keeps this from every
/// city.
pub fn city_buffer() -> f32 {
    civ_rules::world(civ_rules::CITY_RADIUS_BUFFER)
}

/// ADAPTED: an epic's lair also keeps its territory and reach from the
/// cities, so it never mauls a fleet rolling out of a city.
pub fn epic_clearance() -> f32 {
    city_buffer() + EPIC_TERRITORY + EPIC_AGGRO
}

/// The campaign's cities and geysers (world points).
fn sites(campaign: &civ::CivCampaign) -> (Vec<Vec3>, Vec<Vec3>) {
    (
        campaign.cities.iter().map(|c| c.center()).collect(),
        (0..civ::GEYSER_COUNT).map(|g| campaign.geyser_at(g)).collect(),
    )
}

/// The epics' lairs (one per `EPIC_PREDATOR_COUNT`), on land.
pub fn epic_lairs(globe: &Globe, campaign: &civ::CivCampaign) -> Vec<Vec3> {
    let (cities, geysers) = sites(campaign);
    crate::civ_planet::wild_spots(
        globe,
        &cities,
        &geysers,
        civ_rules::EPIC_PREDATOR_COUNT,
        0xE91C,
        EPIC_SPACING,
        epic_clearance(),
        EPIC_RING,
    )
}

/// The ambient animals' spots (at most `AnimalsAtMaxDensity`), on land.
pub fn animal_spots(globe: &Globe, campaign: &civ::CivCampaign) -> Vec<Vec3> {
    let (cities, geysers) = sites(campaign);
    crate::civ_planet::wild_spots(
        globe,
        &cities,
        &geysers,
        civ_rules::ANIMALS_AT_MAX_DENSITY,
        0xA417,
        ANIMAL_SPACING,
        city_buffer() + 1.0,
        ANIMAL_RING,
    )
}

fn wander_goal(shape: Shape, home: Vec3, territory: f32, id: u32, step: u32) -> Vec3 {
    let angle = hash01(id, step) * std::f32::consts::TAU;
    let r = territory * (0.3 + 0.7 * hash01(id ^ 0x77, step));
    shape.toward(home, shape.bearing(home, angle), r)
}

/// One walking step toward `goal` over the surface (land only on the planet:
/// a step into the sea is not taken). Returns the new position and the
/// facing, `None` when it stays.
fn walk(globe: &Globe, at: Vec3, goal: Vec3, distance: f32) -> Option<(Vec3, Quat)> {
    let shape = globe.shape();
    let d = shape.distance(at, goal);
    if d <= 1e-3 || distance <= 0.0 {
        return None;
    }
    let dir = shape.from_tangent(at, shape.to_tangent(at, goal - at).normalize_or_zero());
    let next = shape.advance(at, dir * distance.min(d));
    if globe.on_planet() && !globe.is_land(next) && globe.is_land(at) {
        return None;
    }
    Some((next, shape.heading(next, shape.yaw_of(next, dir))))
}

#[allow(clippy::too_many_arguments)]
fn spawn_body(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    visual: u32,
    at: Vec3,
    scale: f32,
    tint: Color,
) -> Entity {
    let native = crate::baked_model::species_key(originals, visual).is_some_and(|key| {
        crate::baked_model::ensure(originals, key, meshes, images, materials).is_ok()
    });
    let entity = commands
        .spawn((
            StageOwned,
            Visibility::Inherited,
            Transform::from_translation(at).with_scale(Vec3::splat(scale)),
            crate::terrain::Radial,
            crate::terrain::Grounded::new(0.0),
        ))
        .id();
    if !(native && crate::baked_model::spawn_species_visual(commands, originals, meshes, entity, visual)) {
        commands.entity(entity).insert((
            Mesh3d(meshes.add(Capsule3d::new(0.45, 0.9))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: tint,
                perceptual_roughness: 0.8,
                ..default()
            })),
        ));
    }
    entity
}

/// Spawn the epics still alive and the ambient animals (on entering Civ).
#[allow(clippy::too_many_arguments)]
pub fn spawn_wildlife(
    mut commands: Commands,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    site: Option<Res<crate::civ_planet::CivSite>>,
) {
    // The stage planet as setup laid the campaign out on it (the stage
    // surface is built later in the same transition).
    let empty = crate::civ_planet::CivSite::default();
    let globe = site.as_deref().unwrap_or(&empty).globe();
    let shape = run.civ_campaign.shape();
    let lairs = epic_lairs(&globe, &run.civ_campaign);
    if run.civ_campaign.epics.len() != lairs.len() {
        run.civ_campaign.epics = (0..lairs.len())
            .map(|i| EpicState {
                id: i as u32,
                hp: EPIC_MAX_HP,
                alive: true,
            })
            .collect();
    }
    // The planet's own giants when the Creature stage generated them.
    let visuals: Vec<u32> = run
        .species_world
        .roamers
        .iter()
        .filter(|r| r.kind == crate::species::RoamerKind::Epic)
        .map(|r| r.visual)
        .collect();
    for (state, lair) in run.civ_campaign.epics.clone().iter().zip(&lairs) {
        if !state.alive {
            continue;
        }
        let visual = visuals
            .get(state.id as usize % visuals.len().max(1))
            .copied()
            .unwrap_or(40 + state.id * 7);
        let at = shape.place_on(*lair, 0.0);
        let entity = spawn_body(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            visual,
            at,
            EPIC_SCALE,
            Color::srgb(0.42, 0.16, 0.30),
        );
        commands.entity(entity).insert((
            Name::new(format!("civ epic predator {}", state.id)),
            CivEpic {
                id: state.id,
                hp: state.hp.min(EPIC_MAX_HP),
                lair: at,
                goal: at,
                wait: 1.0,
                step: 0,
                attack: EPIC_ATTACK_PERIOD,
                hit: 10.0,
            },
        ));
    }
    // Ambient animals from the planet's species (at most 15 at full density).
    let species: Vec<u32> = run.species_world.species.iter().map(|s| s.id).collect();
    for (i, spot) in animal_spots(&globe, &run.civ_campaign).into_iter().enumerate() {
        let visual = species.get(i % species.len().max(1)).copied().unwrap_or(470 + i as u32 * 3);
        let at = shape.place_on(spot, 0.0);
        let entity = spawn_body(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            visual,
            at,
            ANIMAL_SCALE,
            Color::srgb(0.55, 0.5, 0.35),
        );
        commands.entity(entity).insert((
            Name::new(format!("civ animal {i}")),
            CivAnimal {
                home: at,
                goal: at,
                wait: hash01(i as u32, 1) * 3.0,
                step: 0,
                shown: true,
            },
        ));
    }
}

/// Epics wander their lair, maul the nearest vehicle in reach (yours are
/// spared while Invulnerability lasts) and die when shot down.
pub fn epic_update(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut epics: Query<(Entity, &mut CivEpic, &mut Transform), Without<CivUnit>>,
    mut units: Query<(&mut CivUnit, &Transform), Without<CivEpic>>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let globe = Globe::from_surface(
        surface
            .as_deref()
            .filter(|s| s.stage == GameStage::Civilization),
    );
    let shape = run.civ_campaign.shape();
    let dist_xz = |a: Vec3, b: Vec3| shape.distance(a, b);
    let invulnerable = run.civ_campaign.invulnerability_timer > 0.0;
    for (entity, mut epic, mut t) in &mut epics {
        epic.hit += dt;
        if epic.hp <= 0.0 {
            if let Some(state) = run.civ_campaign.epics.iter_mut().find(|s| s.id == epic.id) {
                state.alive = false;
                state.hp = 0.0;
            }
            run.civ_campaign.message = "An epic predator was brought down".into();
            run.request_save();
            commands.entity(entity).despawn();
            continue;
        }
        if let Some(state) = run.civ_campaign.epics.iter_mut().find(|s| s.id == epic.id) {
            state.hp = epic.hp;
        }
        let at = t.translation;
        // Prey: the nearest vehicle within reach of its territory.
        let prey = units
            .iter()
            .filter(|(u, ut)| {
                u.hp > 0.0
                    && dist_xz(ut.translation, at) <= EPIC_AGGRO
                    && dist_xz(ut.translation, epic.lair) <= EPIC_TERRITORY + EPIC_AGGRO
            })
            .min_by(|a, b| dist_xz(a.1.translation, at).total_cmp(&dist_xz(b.1.translation, at)))
            .map(|(u, ut)| (u.id, ut.translation));
        let goal = if let Some((id, p)) = prey {
            epic.attack -= dt;
            if dist_xz(p, at) <= EPIC_BODY_RADIUS + 1.6 * CITY_SCALE && epic.attack <= 0.0 {
                epic.attack = EPIC_ATTACK_PERIOD;
                if let Some((mut unit, _)) = units.iter_mut().find(|(u, _)| u.id == id) {
                    if !(unit.nation == HOME_INDEX && invulnerable) {
                        unit.hp -= EPIC_DAMAGE;
                        unit.hit = 0.0;
                    }
                }
            }
            p
        } else {
            epic.attack = epic.attack.max(0.5);
            epic.wait -= dt;
            if epic.wait <= 0.0 || dist_xz(epic.goal, at) < 0.4 * CITY_SCALE {
                epic.step = epic.step.wrapping_add(1);
                epic.goal = wander_goal(shape, epic.lair, EPIC_TERRITORY, epic.id, epic.step);
                epic.wait = 4.0 + 4.0 * hash01(epic.id, epic.step);
            }
            epic.goal
        };
        if dist_xz(goal, at) > EPIC_BODY_RADIUS {
            if let Some((next, facing)) = walk(&globe, at, goal, EPIC_SPEED * dt) {
                t.translation = if shape.on_sphere() {
                    next
                } else {
                    Vec3::new(next.x, t.translation.y, next.z)
                };
                t.rotation = facing;
            }
        }
    }
}

/// Animals wander, and the casting manager shows them inside 150 of the
/// camera focus and hides them beyond 200 (scaled).
pub fn animal_update(
    time: Res<Time>,
    run: Res<RunProgress>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    focus: Query<&Transform, (With<CivCommander>, Without<CivAnimal>)>,
    mut animals: Query<(&mut CivAnimal, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let globe = Globe::from_surface(
        surface
            .as_deref()
            .filter(|s| s.stage == GameStage::Civilization),
    );
    let shape = run.civ_campaign.shape();
    let camera = focus.single().map(|t| t.translation).unwrap_or(Vec3::ZERO);
    for (k, (mut animal, mut t, mut visibility)) in animals.iter_mut().enumerate() {
        animal.wait -= dt;
        let at = t.translation;
        if animal.wait <= 0.0 {
            animal.step = animal.step.wrapping_add(1);
            animal.goal =
                wander_goal(shape, animal.home, ANIMAL_TERRITORY, 0xA000 + k as u32, animal.step);
            animal.wait = 3.0 + 5.0 * hash01(k as u32, animal.step);
        }
        if shape.distance(animal.goal, at) > 0.2 {
            if let Some((next, facing)) = walk(&globe, at, animal.goal, ANIMAL_SPEED * dt) {
                t.translation = if shape.on_sphere() {
                    next
                } else {
                    Vec3::new(next.x, t.translation.y, next.z)
                };
                t.rotation = facing;
            }
        }
        let shown = civ_rules::animal_visible(shape.distance(t.translation, camera), animal.shown);
        if shown != animal.shown {
            animal.shown = shown;
            *visibility = if shown { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

/// Registers the Civ wildlife systems.
pub struct CivWildlifePlugin;

impl Plugin for CivWildlifePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(GameStage::Civilization),
            spawn_wildlife.after(civ::setup_civ),
        )
        .add_systems(
            Update,
            (
                epic_update
                    .after(crate::civ_rts::rts_move)
                    .before(crate::civ_rts::rts_cleanup),
                animal_update,
            )
                .in_set(crate::GameSystems::Simulation)
                .run_if(in_state(GameStage::Civilization)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_epic_lairs_and_fifteen_animals_keep_clear_of_the_cities() {
        let planet = crate::civ_planet::tests::continent(450.0);
        for globe in [Globe::flat(), Globe::on(&planet)] {
            let mut campaign = civ::CivCampaign::default();
            crate::civ_planet::lay_out(&mut campaign, &globe, 11);
            let shape = campaign.shape();
            let lairs = epic_lairs(&globe, &campaign);
            assert_eq!(lairs.len(), civ_rules::EPIC_PREDATOR_COUNT);
            let animals = animal_spots(&globe, &campaign);
            assert_eq!(animals.len(), civ_rules::ANIMALS_AT_MAX_DENSITY);
            let buffer = civ_rules::world(civ_rules::CITY_RADIUS_BUFFER);
            for p in lairs.iter().chain(&animals) {
                assert!(globe.is_land(*p), "{p:?} is at sea");
                for city in &campaign.cities {
                    assert!(shape.distance(*p, city.center()) >= buffer, "{p:?} crowds a city");
                }
            }
            for p in &lairs {
                for city in &campaign.cities {
                    assert!(shape.distance(*p, city.center()) >= epic_clearance());
                }
            }
            for (i, a) in lairs.iter().enumerate() {
                for b in &lairs[i + 1..] {
                    assert!(shape.distance(*a, *b) >= EPIC_SPACING);
                }
            }
            assert_eq!(epic_lairs(&globe, &campaign), lairs, "deterministic");
        }
    }
}
