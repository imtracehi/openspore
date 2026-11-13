//! Player nest presentation and persisted migration. The nest model is the
//! original one of the avatar's brain level (`Creature Nest - Brain Level
//! 0..2`, props 0x87B2ACC2 / 0x87B2ACC3 / 0x87B2ACC0); the migration starts at
//! the avatar's birth (`begin_migration_at_birth`, retail 0xD40FA0) and the
//! destination nest is claimed from 50 m, which swaps the model (kUpgradeNest).
//! Destination selection and travel rules are adapted.
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    asset_catalog::PROP,
    dbpf::ResourceKey,
    game::{Health, Player, RunProgress},
    stages::{GameStage, StageOwned},
    Originals,
};
use bevy::prelude::*;

/// The NPC nest model (`Creature NPC Nest`, `cr_crgnest_npc`): the scenery of
/// the other species' nests, and the stand-in when a player nest model fails
/// to import.
pub const NEST_DEFINITION: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0x2D12268B,
    type_id: PROP,
};

/// The NPC nest model of an extinct herd (`Creature NPC Nest`,
/// `cr_crgnest_npc_abandoned`): the nest update 0xE8DF30 draws it instead of
/// `NEST_DEFINITION` when the herd's `mbExtinction` (+0x190) is set.
pub const ABANDONED_NEST_DEFINITION: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0xFE29_A98E,
    type_id: PROP,
};

/// The model of an NPC nest (0xE8DF30): the abandoned one once its herd is
/// extinct.
pub fn npc_nest_definition(extinct: bool) -> ResourceKey {
    if extinct {
        ABANDONED_NEST_DEFINITION
    } else {
        NEST_DEFINITION
    }
}

/// `Creature Nest - Brain Level 0 / 1 / 2` (PatchData props named by their model,
/// `cr_crgnest_00..02`): the nest the player's species lives in. The nest model
/// update 0xE8DF30 (VERIFIED) switches on a level: 0 -> `0x87B2ACC2`, 1 ->
/// `0x87B2ACC3`, anything else -> `0x87B2ACC0`; the level is the creature game's
/// level (the drama manager singleton 0xD51610, field +0x44; ADAPTED: taken to
/// be the avatar's brain level) for a nest whose herd is owned by the avatar
/// (`mOwnedByAvatar` +0x84) and 0 for any other nest of the species. 0xD40AF0
/// picks the nest whose brain-level range holds the avatar's level
/// (`creature_progression::NestRange`).
pub const PLAYER_NEST_INSTANCES: [u32; 3] = [0x87B2_ACC2, 0x87B2_ACC3, 0x87B2_ACC0];
/// Their `modelBoundingRadius`.
pub const PLAYER_NEST_RADIUS: [f32; 3] = [6.5, 9.0, 12.0];
/// Width the level-0 nest is fitted to (the earlier footprint of the NPC
/// nest); the larger nests scale with their bounding radius. ADAPTED.
const FIT_WIDTH: f32 = 4.8;

/// The nest model of a brain level.
pub fn player_nest_definition(level: u8) -> ResourceKey {
    ResourceKey {
        group_id: 0,
        instance_id: PLAYER_NEST_INSTANCES[(level as usize).min(PLAYER_NEST_INSTANCES.len() - 1)],
        type_id: PROP,
    }
}

/// Width a level's nest is fitted to.
pub fn player_nest_width(level: u8) -> f32 {
    FIT_WIDTH * PLAYER_NEST_RADIUS[(level as usize).min(PLAYER_NEST_RADIUS.len() - 1)] / PLAYER_NEST_RADIUS[0]
}

/// The nest model level a brain level draws (0..=2): the switch of 0xE8DF30.
pub fn nest_model_level(brain_level: u8) -> u8 {
    brain_level.min(PLAYER_NEST_INSTANCES.len() as u8 - 1)
}

/// The model level of a nest of the player's species (0xE8DF30): the avatar's
/// own nest follows the level, any other nest of the species stays at 0.
pub fn nest_model_level_of(brain_level: u8, owned_by_avatar: bool) -> u8 {
    if owned_by_avatar {
        nest_model_level(brain_level)
    } else {
        0
    }
}

#[derive(Component)]
pub struct HomeNest {
    clearance: f32,
    /// The model level drawn (`nest_model_level`).
    pub level: u8,
}

/// `kUpgradeNest` arrived (the end of `CRG_ClaimNest`): redraw the nest for
/// the avatar's brain level.
static UPGRADE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Ask for the nest model of the current brain level (0xD438AC).
pub fn request_upgrade() {
    UPGRADE_REQUESTED.store(true, Ordering::Relaxed);
}

#[allow(clippy::too_many_arguments)]
fn spawn_nest(
    commands: &mut Commands,
    run: &RunProgress,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    surface: Option<&crate::terrain::LandSurface>,
) {
    // The avatar's own nest (its herd is owned by the avatar).
    let level = nest_model_level_of(run.creature_progression.brain_level, true);
    let home = Vec3::from_array(run.creature_lifecycle.home);
    let baseline = surface.map_or(-0.6, |s| s.height(home.x, home.z));
    let mut import = |originals: &mut Originals, definition: ResourceKey, width: f32| {
        crate::world_objects::definition_at(originals, definition, "Player home nest")
            .and_then(|d| crate::world_objects::import_at(originals, d, meshes, images, materials))
            .and_then(|(model, source)| {
                let pose = crate::world_objects::fitted_transform(
                    &model,
                    meshes,
                    Vec3::new(home.x, baseline, home.z),
                    width,
                )?;
                let material = model.material.clone().ok_or("original nest material missing")?;
                Ok((model, source, pose, material))
            })
    };
    // The brain level's own nest, else the NPC nest model.
    let native = import(originals, player_nest_definition(level), player_nest_width(level)).or_else(
        |error: String| {
            if !originals.resource_index.records.is_empty() {
                warn!("Player nest of brain level {level}: {error}; using the NPC nest model");
            }
            import(originals, NEST_DEFINITION, FIT_WIDTH)
        },
    );
    match native {
        Ok((model, source, pose, material)) => {
            commands.spawn((
                StageOwned,
                HomeNest {
                    clearance: pose.translation.y - baseline,
                    level,
                },
                Name::new("Player home nest"),
                source,
                Mesh3d(model.draw_mesh()),
                MeshMaterial3d(material),
                pose,
            ));
        }
        Err(error) => {
            if !originals.resource_index.records.is_empty() {
                warn!("Player home nest: {error}");
            }
            commands.spawn((
                StageOwned,
                HomeNest { clearance: 0.08, level },
                Name::new("Player home nest fallback"),
                Mesh3d(meshes.add(Cylinder::new(2.4, 0.16))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.28, 0.20, 0.10),
                    ..default()
                })),
                Transform::from_xyz(home.x, baseline + 0.08, home.z),
            ));
        }
    }
}

pub fn setup(
    mut commands: Commands,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
) {
    let dna = run.dna_this_stage;
    run.creature_progression.observe_dna(dna);
    spawn_nest(
        &mut commands,
        &run,
        &mut originals,
        &mut meshes,
        &mut images,
        &mut materials,
        surface.as_deref(),
    );
}

/// `kUpgradeNest` (0xD438AC, sent by `CRG_ClaimNest`): the claimed nest is drawn
/// with the model of the avatar's brain level. Runs only when the message has
/// arrived and the model differs.
#[allow(clippy::too_many_arguments)]
pub fn upgrade_model(
    mut commands: Commands,
    state: Res<State<GameStage>>,
    run: Res<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    nests: Query<(Entity, &HomeNest)>,
) {
    if *state.get() != GameStage::Creature {
        UPGRADE_REQUESTED.store(false, Ordering::Relaxed);
        return;
    }
    if !UPGRADE_REQUESTED.swap(false, Ordering::Relaxed) {
        return;
    }
    let wanted = nest_model_level(run.creature_progression.brain_level);
    if nests.iter().all(|(_, nest)| nest.level == wanted) {
        return;
    }
    for (entity, _) in &nests {
        commands.entity(entity).despawn();
    }
    spawn_nest(
        &mut commands,
        &run,
        &mut originals,
        &mut meshes,
        &mut images,
        &mut materials,
        surface.as_deref(),
    );
}

/// 0xE8DF30 redraws an NPC nest whose herd went extinct with the abandoned
/// model. The nests are drawn once at the stage start (with the abandoned
/// model for a herd that is extinct already, `creature::spawn_stage`); this
/// swaps the scenery of a herd that goes extinct while the stage runs (checked
/// twice a second). ADAPTED: the model is swapped in place, the fitted
/// transform stays (the abandoned model has the same 6.5 bounding radius).
#[allow(clippy::too_many_arguments)]
pub fn abandon_nests(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<GameStage>>,
    run: Res<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut clock: Local<f32>,
    scenery: Query<(Entity, &crate::world_objects::OriginalWorldObject, &Transform)>,
) {
    if *state.get() != GameStage::Creature || originals.resource_index.records.is_empty() {
        return;
    }
    *clock += time.delta_secs();
    if *clock < 0.5 {
        return;
    }
    *clock = 0.0;
    let extinct: Vec<Vec3> = (0..run.species_world.nests.len())
        .filter(|&i| run.species_world.nest_status(i) == crate::species::NestStatus::Destroyed)
        .map(|i| Vec3::from_array(run.species_world.nests[i].position))
        .collect();
    if extinct.is_empty() {
        return;
    }
    for (entity, source, pose) in &scenery {
        if source.definition != NEST_DEFINITION
            || source.name != "Creature NPC Nest"
            || !extinct.iter().any(|at| crate::game::dist_xz(*at, pose.translation) < 0.5)
        {
            continue;
        }
        let swapped = crate::world_objects::definition_at(&originals, npc_nest_definition(true), "Creature NPC Nest")
            .and_then(|d| crate::world_objects::import_at(&mut originals, d, &mut meshes, &mut images, &mut materials))
            .and_then(|(model, source)| {
                let material = model.material.clone().ok_or("abandoned nest has no material")?;
                Ok((model, source, material))
            });
        match swapped {
            Ok((model, source, material)) => {
                commands
                    .entity(entity)
                    .insert((source, Mesh3d(model.draw_mesh()), MeshMaterial3d(material)));
            }
            Err(error) => {
                // Keep the live nest model; do not try this site again every check.
                warn!("Abandoned nest model: {error}");
                commands.entity(entity).insert(crate::world_objects::OriginalWorldObject {
                    definition: ABANDONED_NEST_DEFINITION,
                    model: source.model,
                    name: source.name.clone(),
                });
            }
        }
    }
}

pub fn update(
    state: Res<State<GameStage>>,
    editor: Res<crate::editor::EditorState>,
    mut run: ResMut<RunProgress>,
    mut encounter: ResMut<crate::creature_actions::Encounter>,
    mut awards: ResMut<crate::creature_social::CreatureAwards>,
    player: Query<(&Transform, &Health), (With<Player>, Without<HomeNest>)>,
    mut nests: Query<(&mut Transform, &HomeNest), Without<Player>>,
    mut scenery: Query<
        (
            &crate::world_objects::OriginalWorldObject,
            &Transform,
            &mut Visibility,
        ),
        Without<HomeNest>,
    >,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut previous_rank: Local<Option<u8>>,
    mut previous_birth: Local<Option<(u32, u32)>>,
) {
    if *state.get() != GameStage::Creature {
        *previous_rank = None;
        *previous_birth = None;
        return;
    }
    if editor.open {
        return;
    }
    let Ok((pose, health)) = player.single() else {
        return;
    };
    if health.dead() {
        return;
    }
    let dna = run.dna_this_stage;
    run.creature_progression.observe_dna(dna);
    let rank = run.creature_progression.brain_level;
    if previous_rank.is_some_and(|previous| rank > previous) {
        let pack = crate::creature_pack::capacity(run.earned_creature_dna());
        encounter.message =
            format!("Brain evolved to {rank}/4; return home to mate and choose new parts");
        // Spore celebrates each brain upgrade with what it unlocked.
        awards.push(if rank >= crate::creature_progression::MAX_BRAIN_LEVEL {
            format!("Brain upgrade! Level {rank}: your species is ready for the tribe")
        } else {
            format!("Brain upgrade! Level {rank}: room for {pack} in your pack")
        });
        run.request_save();
    }
    *previous_rank = Some(rank);
    let threatened = encounter.mode == crate::creature_actions::EncounterMode::Combat
        && encounter.target.is_some();
    // Retail has no migrate key: 0xD40FA0 (the avatar's birth at its nest, after a
    // death's kRestartAtNest and on the return from the editor = a hatch) reads
    // `MigrationDNALevels` against the last migration's DNA and starts the
    // migration itself. Here the births are the lifecycle's death and
    // generation counters.
    let birth = (run.creature_lifecycle.deaths, run.creature_lifecycle.generation);
    let born = previous_birth.is_some_and(|before| before != birth);
    *previous_birth = Some(birth);
    if born {
        begin_migration_at_birth(&mut run, &mut encounter, &mut awards);
    }
    if !threatened && run.creature_progression.pending_migration.is_some() {
        // 0xD41510: the destination nest is claimed from 50 m (not the home
        // radius); `CRG_ClaimNest` plays (cinematics::moments).
        if let Ok(home) = run.creature_progression.complete_migration(
            pose.translation.to_array(),
            crate::creature_progression::CLAIM_RADIUS,
        ) {
            run.creature_lifecycle.home = home;
            run.creature_lifecycle.resting = false;
            run.request_save();
            let count = run.creature_progression.migrations_completed;
            awards.push(format!("Migration {count} complete! New home nest established"));
            encounter.message = format!(
                "Migration {count} complete; this is your new home nest",
            );
        }
    }
    let home = run.creature_lifecycle.home;
    let height = surface
        .as_ref()
        .map_or(-0.6, |s| s.height(home[0], home[2]));
    // A home beyond the planet's limb (the floating origin turned it there)
    // keeps the nest where the turn left it.
    let beyond = surface
        .as_ref()
        .and_then(|s| s.planet())
        .is_some_and(|p| home[1] < p.frame.centre.y);
    for (mut t, nest) in &mut nests {
        if !beyond {
            t.translation = Vec3::new(home[0], height + nest.clearance, home[2]);
        }
    }
    for (source, pose, mut visibility) in &mut scenery {
        if (source.definition == NEST_DEFINITION || source.definition == ABANDONED_NEST_DEFINITION)
            && source.name == "Creature NPC Nest"
        {
            // Reuse the site's appearance without drawing two identical nests
            // on the same ground after migration. Restore it when home moves.
            *visibility = if crate::game::dist_xz(pose.translation, Vec3::from_array(home)) < 0.5 {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
    }
}

/// What 0xD40FA0 does at the avatar's birth when `MigrationDNALevels` is
/// crossed: the species leaves its nest (the old nest is retired) for another
/// nest. Retail picks a nest of the player's species valid for the new brain
/// level (0xD40AF0); ADAPTED: OpenSpore picks the nearest nest whose
/// occupants are gone or allied (the generated world has no spare nests of the
/// player's species). With no such nest nothing is recorded and the next birth
/// tries again. `CRG_SpeciesMigrate` (cinematics::moments) plays instead of the
/// egg fade when this starts a migration.
pub fn begin_migration_at_birth(
    run: &mut RunProgress,
    encounter: &mut crate::creature_actions::Encounter,
    awards: &mut crate::creature_social::CreatureAwards,
) -> bool {
    if run.creature_progression.pending_migration.is_some() {
        return false;
    }
    // Only a cleared nest or an allied species' nest is safe to adopt.
    let destinations: Vec<_> = run
        .species_world
        .nests
        .iter()
        .filter(|n| {
            n.surviving() == 0
                || (run.species_world.species_allied(n.species) && !run.species_world.species_aggravated(n.species))
        })
        .map(|n| n.position)
        .collect();
    let total = run.dna_this_stage.max(run.creature_progression.observed_dna);
    let home = run.creature_lifecycle.home;
    match run.creature_progression.begin_migration(total, home, &destinations) {
        Ok(_) => {
            run.creature_lifecycle.resting = false;
            run.request_save();
            awards.push("Species Migration Begun! Follow path to your new home nest");
            encounter.message = "Your species is migrating; follow MOVE on the map to the new nest".into();
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nest_model_rule_is_the_one_of_the_nest_update() {
        // 0xE8DF30: the avatar's own nest switches on the level (0, 1, else 2);
        // another nest of the species stays at level 0; an extinct NPC herd's
        // nest is the abandoned model.
        assert_eq!((0..5).map(|l| nest_model_level_of(l, true)).collect::<Vec<_>>(), [0, 1, 2, 2, 2]);
        assert!((0..5).all(|l| nest_model_level_of(l, false) == 0));
        assert_eq!(npc_nest_definition(false), NEST_DEFINITION);
        assert_eq!(npc_nest_definition(true).instance_id, 0xFE29_A98E);
        assert_ne!(npc_nest_definition(true), npc_nest_definition(false));
    }

    #[test]
    fn the_nest_model_follows_the_brain_level() {
        // `Creature Nest - Brain Level 0 / 1 / 2` (PatchData props named by their
        // model); the higher levels keep the last model.
        let instances: Vec<u32> = (0..5).map(|l| player_nest_definition(nest_model_level(l)).instance_id).collect();
        assert_eq!(instances, [0x87B2_ACC2, 0x87B2_ACC3, 0x87B2_ACC0, 0x87B2_ACC0, 0x87B2_ACC0]);
        assert_eq!(crate::world_objects::name_id("cr_crgnest_00"), 0x87B2_ACC2);
        assert_eq!(crate::world_objects::name_id("cr_crgnest_01"), 0x87B2_ACC3);
        assert_eq!(crate::world_objects::name_id("cr_crgnest_02"), 0x87B2_ACC0);
        assert_eq!(crate::world_objects::name_id("cr_crgnest_npc"), NEST_DEFINITION.instance_id);
        // The bigger nests grow with their `modelBoundingRadius` 6.5 / 9 / 12.
        assert_eq!(player_nest_width(0), 4.8);
        assert!((player_nest_width(1) - 4.8 * 9.0 / 6.5).abs() < 1e-5);
        assert!((player_nest_width(2) - 4.8 * 12.0 / 6.5).abs() < 1e-5);
        assert_eq!(player_nest_width(4), player_nest_width(2));
    }
}
