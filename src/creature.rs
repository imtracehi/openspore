//! Creature stage: hatch on land, eat, befriend or hunt, fill the brain bar.
//! Controls: WASD to walk. E = sing/dance (befriend), F = bite/charge (hunt).
//! Befriend the wild species around their nests or hunt them, keep fed, reach
//! 150 DNA.
//!
//! The animals come from `species::SpeciesWorld`: each species has its own
//! body, its own original parts, and members with persistent ids. Outcomes
//! are recorded once per member, so an alliance or an extinction survives a
//! reload, and a defeated member never comes back.

use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::HashSet;

use crate::game::*;
use crate::species::RecordResult;
use crate::stages::{GameStage, StageOwned};

#[path = "creature_mobility.rs"]
pub mod mobility;

/// Editor part mounted on the creature avatar (original part mesh).
/// `part` indexes into `RunProgress::parts`; `slot_local` is the bind-space
/// offset of the slot bone it rides on.
#[derive(Component)]
pub struct Accessory {
    pub part: usize,
    pub slot_local: Vec3,
}

/// A part baked onto an NPC body at spawn (static child transform).
#[derive(Component)]
pub struct MountedPart;

/// Bind-space offset of a slot bone on the rigged creature, scaled to the
/// creature stage's avatar size. Falls back to the classic head/back anchor.
pub fn slot_offset(originals: &crate::Originals, bone: u32, body_scale: f32) -> Vec3 {
    let fallback = Vec3::new(0.0, 1.9, -0.6) * body_scale;
    let Some(demo) = originals.rigged_demo.as_ref() else {
        return fallback;
    };
    let Some(worlds) = crate::rw4::fk_worlds(&demo.parents, &demo.locals) else {
        return fallback;
    };
    let bi = (bone as usize).min(worlds.len().saturating_sub(1));
    let w = worlds.get(bi).or_else(|| worlds.first());
    match w {
        Some(w) => Vec3::new(
            w[0][3] * body_scale,
            w[1][3] * body_scale,
            w[2][3] * body_scale,
        ),
        None => fallback,
    }
}

/// Members of a species share their authored part transforms. Variation belongs
/// on the actor/body, not individual eyes, mouths or attachment anchors.
pub fn npc_variation(npc_index: usize, part_index: usize) -> (f32, f32) {
    let _ = (npc_index, part_index);
    (1.0, 0.0)
}

/// Mount saved attachments unchanged in parent-local coordinates. A shared
/// species build keeps facial features, mirrored pairs and offsets together.
pub fn mount_saved_parts(
    commands: &mut Commands,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    body: Entity,
    originals: &crate::Originals,
    run: &RunProgress,
    npc_index: usize,
    _body_scale: f32,
) {
    if run.parts.is_empty() && crate::outfitter::worn(run).is_empty() || originals.models.is_empty() {
        return;
    }
    // The body and its attachments share local units. Parent scale applies
    // uniformly to both, including saved placement offsets.
    for (pi, p) in run.parts.iter().enumerate() {
        mount_part(commands, materials, body, originals, run, p, pi, npc_index, false);
    }
    // The stage's outfitter accessories (Tribal / Civ) ride the same body.
    mount_outfit(commands, materials, body, originals, run, npc_index);
}

/// Mount the accessories the stage's members wear (`outfitter`) on `body`.
pub fn mount_outfit(
    commands: &mut Commands,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    body: Entity,
    originals: &crate::Originals,
    run: &RunProgress,
    npc_index: usize,
) {
    let outfit = crate::outfitter::worn(run);
    if outfit.is_empty() || originals.models.is_empty() {
        return;
    }
    for (pi, p) in outfit.iter().enumerate() {
        mount_part(commands, materials, body, originals, run, p, run.parts.len() + pi, npc_index, true);
    }
}

#[allow(clippy::too_many_arguments)]
fn mount_part(
    commands: &mut Commands,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    body: Entity,
    originals: &crate::Originals,
    run: &RunProgress,
    p: &crate::game::PartInstance,
    pi: usize,
    npc_index: usize,
    accessory: bool,
) {
    let Some(model) = crate::editor::model_for_part(originals, p) else {
        return;
    };
    // Deterministic per-creature variation (npc 0 = the player's creature).
    let (jitter, yaw) = npc_variation(npc_index, pi);
    let local = crate::baked_model::attachment_point(originals, &run.body, p.bone)
        + Vec3::from_array(p.offset);
    // Rotate the slot local by the NPC's yaw in the XZ plane.
    let (s, c) = yaw.sin_cos();
    let local = Vec3::new(
        local.x * c - local.z * s,
        local.y,
        local.x * s + local.z * c,
    );
    let child_scale = model.scale * 0.5 * p.scale * jitter;
    commands.entity(body).with_children(|parent| {
        let mut part = parent.spawn((
            StageOwned,
            Mesh3d(model.draw_mesh()),
            MeshMaterial3d(model.material.clone().unwrap_or_else(|| {
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.85, 0.75, 0.6),
                    ..default()
                })
            })),
            Transform::from_translation(local)
                .with_rotation(Quat::from_rotation_y(p.yaw + yaw))
                .with_scale(p.mesh_scale(child_scale)),
        ));
        // Which part it is (the crowd's clip player poses it with its block).
        part.insert(crate::outfit_crowd::MountIndex(pi));
        if accessory {
            // Not `MountedPart`: the authored-model display hides those.
            part.insert(crate::outfitter::OutfitMounted);
        } else {
            part.insert(MountedPart);
        }
    });
}
/// Home of a roaming member: its nest centre and how far it strays.
/// Deliberately not serialized in the checkpoint (persistence only captures a
/// fixed component list), so the stage re-derives it on every entry.
#[derive(Component)]
pub struct NestHome {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
}

/// How many wild members the stage keeps alive at once. Defeated members are
/// never replaced, so a population only ever shrinks.
pub const MAX_LIVE_MEMBERS: usize = 12;

/// Build the world's ecology before the stage spawns anything: reuse a saved
/// world when there is one, otherwise derive a deterministic one from the
/// player's own lineage build, then import every part those species wear.
/// Scheduled immediately before `setup_creature`; calling it twice is safe.
/// The homeworld is generated (or taken from `PlanetCache`) first, because the
/// herds are placed on its land (`terrain::prepare` reuses the cache).
#[allow(clippy::too_many_arguments)]
pub fn prepare_species(
    mut commands: Commands,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cache: Option<Res<crate::terrain::PlanetCache>>,
) {
    let seed = world_seed(&run);
    let generated = crate::creature_planet::ensure_site_cache(&originals.resource_index, cache.as_deref(), seed);
    let land = crate::creature_planet::site_land(
        generated.as_ref().or(cache.as_deref()),
        seed,
        run.species_world.planet_frame,
    );
    if let Some(generated) = generated {
        commands.insert_resource(generated);
    }
    // Legacy saves carry outcomes in the checkpoint with no world at all, so
    // the migration runs before (and creates) the world.
    migrate_checkpoint_outcomes_on(&mut run, &originals.part_catalog, &land);
    let world = ensure_species_world_on(&mut run, &originals.part_catalog, &land);
    import_species_models(
        &world,
        &mut originals,
        &mut meshes,
        &mut images,
        &mut materials,
    );
    remap_checkpoint_species(&mut run);
    let living = world.spawn_list().len();
    info!(
        "species world seed {:#X}: {} species, {} members",
        world.seed,
        world.species.len(),
        living
    );
}

/// Pure: make sure the run holds a species world for the given catalog.
/// Returns the resulting world (cloned, so callers own no borrow of `run`).
/// A world that is already generated is never rebuilt, which is what keeps
/// recorded alliances and extinctions across save/load.
pub fn ensure_species_world(
    run: &mut RunProgress,
    catalog: &crate::asset_catalog::PartCatalog,
) -> crate::species::SpeciesWorld {
    if !run.species_world.generated || run.species_world.species.is_empty() {
        let seed = crate::species::lineage_seed(&run.parts, &run.body);
        run.species_world
            .ensure_generated(seed, catalog, crate::species::DEFAULT_NEST_RADIUS);
    }
    run.species_world.clone()
}

/// `ensure_species_world` on the stage planet: a new world places its herds
/// on the planet's land; an island-scale world saved earlier moves onto it
/// once (`SpeciesWorld::relayout_on`).
pub fn ensure_species_world_on(
    run: &mut RunProgress,
    catalog: &crate::asset_catalog::PartCatalog,
    land: &crate::creature_planet::Land,
) -> crate::species::SpeciesWorld {
    let seed = world_seed(run);
    if run.species_world.generated && !run.species_world.species.is_empty() && run.species_world.relayout_on(land) {
        info!("species world {seed:#X}: moved the island layout onto the planet");
        run.request_save();
    }
    run.species_world.ensure_generated_on(seed, catalog, land);
    run.species_world.clone()
}

/// The species world's seed: the saved world's, or the lineage's for a world
/// still to be generated (the homeworld is chosen from the same seed).
pub fn world_seed(run: &RunProgress) -> u64 {
    if run.species_world.generated && !run.species_world.species.is_empty() {
        run.species_world.seed
    } else {
        crate::species::lineage_seed(&run.parts, &run.body)
    }
}

/// Import the mesh and material of every original part the world's species
/// wear, so mounts can reuse them like the editor does.
fn import_species_models(
    world: &crate::species::SpeciesWorld,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    for species in &world.species {
        if let Some(key) = crate::baked_model::species_key(originals, species.visual()) {
            if let Err(e) = crate::baked_model::ensure(originals, key, meshes, images, materials) {
                warn!("original creature {key:?}: {e}");
            } else if !originals.original_species.contains_key(&key) {
                let native = crate::original_species::resolve(
                    &originals.resource_index,
                    &originals.part_catalog,
                    key,
                );
                if let Err(error) = &native {
                    warn!("original creature anatomy {key:?}: {error}");
                }
                originals.original_species.insert(key, native);
            }
        }
    }
    let mut wanted: Vec<crate::asset_catalog::PartDefinition> = Vec::new();
    for species in &world.species {
        for part in &species.parts {
            let Some(definition) = originals
                .part_catalog
                .by_model(part.instance)
                .filter(|d| !d.is_null)
            else {
                continue;
            };
            if wanted.iter().any(|w| w.model == definition.model) {
                continue;
            }
            wanted.push(definition.clone());
        }
    }
    let mut imported = 0usize;
    for definition in &wanted {
        if crate::editor::ensure_model(originals, definition, meshes, images, materials).is_ok() {
            imported += 1;
        }
    }
    if imported > 0 {
        info!("imported {} species part models", imported);
    }
}

/// Migration for 0.42 checkpoints: those saves record a creature's outcome on
/// `CreatureMind` (allied/defeated) while the species world is still the empty
/// default, because the field did not exist. Without this, a reloaded ally
/// would be neutral in the world and its alliance would be forgotten.
///
/// The outcomes are recorded against their persistent member ids — no DNA is
/// awarded again, because the individual rewards were already paid. A world
/// that already exists is left completely alone: nothing is regenerated and
/// no recorded relationship is ever overwritten.
pub fn migrate_checkpoint_outcomes(
    run: &mut RunProgress,
    catalog: &crate::asset_catalog::PartCatalog,
) -> (u32, u32) {
    migrate_checkpoint_outcomes_on(run, catalog, &crate::creature_planet::Land::flat())
}

/// `migrate_checkpoint_outcomes`, generating a missing world on the planet.
pub fn migrate_checkpoint_outcomes_on(
    run: &mut RunProgress,
    catalog: &crate::asset_catalog::PartCatalog,
    land: &crate::creature_planet::Land,
) -> (u32, u32) {
    if run.species_world.generated && !run.species_world.species.is_empty() {
        // An existing world is the source of truth; never rewrite it.
        return (0, 0);
    }
    ensure_species_world_on(run, catalog, land);
    let Some(checkpoint) = run.checkpoint.clone() else {
        return (0, 0);
    };
    let mut allied = 0;
    let mut defeated = 0;
    for object in &checkpoint.objects {
        let Some(creature) = object.creature.as_ref() else {
            continue;
        };
        if creature.mind.defeated {
            if run.species_world.record_defeat(object.id) == RecordResult::Recorded {
                defeated += 1;
            }
        } else if creature.mind.allied {
            if run.species_world.record_befriend(object.id) == RecordResult::Recorded {
                allied += 1;
            }
        }
    }
    if allied + defeated > 0 {
        info!(
            "migrated {allied} allied and {defeated} defeated creature(s) into the species world"
        );
        run.request_save();
    }
    (allied, defeated)
}

/// Remap saved `CreatureMind::species` values onto ids this world actually
/// has, so a save from an older ecology keeps working after a reload.
fn remap_checkpoint_species(run: &mut RunProgress) {
    let world = &run.species_world;
    if world.species.is_empty() {
        return;
    }
    let Some(checkpoint) = run.checkpoint.as_mut() else {
        return;
    };
    for object in checkpoint.objects.iter_mut() {
        if let Some(creature) = object.creature.as_mut() {
            creature.mind.species = remap_species_id(creature.mind.species, world) as u8;
        }
    }
}

/// Pure: map a saved species id onto one that exists, preferring an
/// approachable species when the saved one is gone.
pub fn remap_species_id(saved: u8, world: &crate::species::SpeciesWorld) -> u32 {
    let saved = saved as u32;
    if world.species(saved).is_some() {
        return saved;
    }
    world
        .species
        .iter()
        .find(|s| !s.hostile)
        .or_else(|| world.species.first())
        .map(|s| s.id)
        .unwrap_or(0)
}

/// Pure: a member's run speed and combat HP from its own build. Authored
/// ranks drive them when a catalog is present (retail speed gears and health
/// table); the fallback is the level-0 health entry (10 HP).
pub fn member_stats(
    species: &crate::species::Species,
    catalog: &crate::asset_catalog::PartCatalog,
) -> (f32, f32) {
    if !catalog.parts.is_empty() {
        let a = species.abilities(catalog);
        return ranked_member_stats(a);
    }
    let base = match species.diet {
        crate::species::Diet::Herbivore => 2.0,
        crate::species::Diet::Omnivore => 2.2,
        crate::species::Diet::Carnivore => 2.6,
    };
    (base * species.size, crate::creature_rules::npc_max_hp(0))
}

/// The displayed GMDL's exact anatomy takes precedence over the generated
/// fallback recipe. Only successfully imported display models qualify.
pub fn original_build<'a>(
    species: &crate::species::Species,
    originals: &'a crate::Originals,
) -> Option<&'a crate::original_species::OriginalSpecies> {
    let key = crate::baked_model::species_key(originals, species.visual())?;
    originals.baked_models.get(&key)?;
    originals.original_species.get(&key)?.as_ref().ok()
}

pub fn species_abilities(
    species: &crate::species::Species,
    originals: &crate::Originals,
) -> crate::asset_catalog::Capabilities {
    original_build(species, originals)
        .map(|native| native.capabilities)
        .unwrap_or_else(|| species.abilities(&originals.part_catalog))
}

fn ranked_member_stats(a: crate::asset_catalog::Capabilities) -> (f32, f32) {
    // Retail (creature_rules C2): the speed gear of the part's speed level and,
    // for a non-player species, the level-0 health entry plus the parts bonus.
    (
        crate::creature_rules::gear_speed(a.speed),
        crate::creature_rules::npc_max_hp(a.health),
    )
}

pub fn displayed_member_stats(
    species: &crate::species::Species,
    originals: &crate::Originals,
) -> (f32, f32) {
    let (speed, hp) = original_build(species, originals)
        .map(|native| ranked_member_stats(native.capabilities))
        .unwrap_or_else(|| member_stats(species, &originals.part_catalog));
    // A retail herd's adults have the archetype's HitpointOverride (C2A).
    match species.archetype {
        Some(archetype) => (
            speed,
            crate::creature_ecology::member_max_hp(archetype, crate::creature_ecology::MemberRole::Adult),
        ),
        None => (speed, hp),
    }
}

/// A member's own maximum HP: a retail herd member's archetype HP with its
/// role (x0.5 baby, x1.25 alpha), a legacy member's build HP. `None` for an
/// unknown id.
pub fn member_max_hp(
    world: &crate::species::SpeciesWorld,
    id: u32,
    originals: &crate::Originals,
) -> Option<f32> {
    let species = world.species_for_member(id)?;
    Some(match species.archetype {
        Some(archetype) => crate::creature_ecology::member_max_hp(archetype, world.member_role(id)),
        None => displayed_member_stats(species, originals).1,
    })
}

/// Pure: a wander target inside `radius` of a nest home, clamped to the stage
/// bounds, so neutral members roam their own neighbourhood.
pub fn nest_wander_target(hx: f32, hz: f32, radius: f32, roll: f32, bounds: f32) -> (f32, f32) {
    let angle = roll.rem_euclid(1.0) * std::f32::consts::TAU;
    let distance = radius * (0.3 + 0.7 * (roll * 7.0).fract().abs());
    let x = (hx + angle.cos() * distance).clamp(-bounds, bounds);
    let z = (hz + angle.sin() * distance).clamp(-bounds, bounds);
    (x, z)
}

/// Mount a species' own parts unchanged in its body's local coordinates.
pub fn mount_species_parts(
    commands: &mut Commands,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    body: Entity,
    originals: &crate::Originals,
    species: &crate::species::Species,
    member: usize,
) {
    if species.parts.is_empty() || originals.models.is_empty() {
        return;
    }
    for (pi, p) in species.parts.iter().enumerate() {
        let Some(model) = originals.models.iter().find(|m| m.instance == p.instance) else {
            continue;
        };
        // Deterministic per-member variation (the writer's own recipe, kept
        // identical so nested mounts still look alive).
        let (jitter, yaw) = npc_variation(member, pi);
        let local = species.body.slot(p.bone) + Vec3::from_array(p.offset);
        let (s, c) = yaw.sin_cos();
        let local = Vec3::new(
            local.x * c - local.z * s,
            local.y,
            local.x * s + local.z * c,
        );
        let child_scale = model.scale * 0.5 * p.scale * jitter;
        commands.entity(body).with_children(|parent| {
            parent.spawn((
                StageOwned,
                MountedPart,
                Mesh3d(model.draw_mesh()),
                MeshMaterial3d(model.material.clone().unwrap_or_else(|| {
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(0.85, 0.75, 0.6),
                        ..default()
                    })
                })),
                Transform::from_translation(local)
                    .with_rotation(Quat::from_rotation_y(p.yaw + yaw))
                    .with_scale(p.mesh_scale(child_scale)),
            ));
        });
    }
}

/// Spawn one persistent member of a species: its own body, its own parts,
/// its own home. `member` is the stable id used to record outcomes once.
pub fn spawn_species_member(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    originals: &crate::Originals,
    world: &crate::species::SpeciesWorld,
    pack: &crate::creature_pack::CreaturePack,
    member: u32,
    index: usize,
) -> Option<Entity> {
    let spawn = world
        .spawn_list()
        .into_iter()
        .find(|s| s.member == member)?;
    let species = world.species(spawn.species)?;
    let nest_index = world.nest_index_for_member(member)?;
    let nest = &world.nests[nest_index];
    let (speed, legacy_hp) = displayed_member_stats(species, originals);
    let hp = member_max_hp(world, member, originals).unwrap_or(legacy_hp);
    // A recorded membership follows the member: an ally that wanders off (or
    // is missing after a reload) rejoins as an ally, not as a stranger. A
    // retail herd the player is best friends with is friendly as a whole.
    let allied = world.member_friendly(member)
        && (!world.species_aggravated(species.id) || pack.members.contains(&member));
    let record = world.member(member).cloned()?;
    let (radius, scale, rounds) = match species.archetype {
        Some(archetype) => (
            crate::creature_ecology::territory_radius(archetype, record.guard, record.role),
            crate::creature_ecology::member_scale(
                species.size,
                record.role,
                crate::creature_ecology::unit(member as u64 ^ 0x5CA1E),
            ),
            crate::creature_ecology::social_rounds(archetype, record.role),
        ),
        None => (NEST_ROAM_RADIUS, species.size, 1),
    };
    let (tx, tz) = nest_wander_target(
        nest.position[0],
        nest.position[2],
        radius,
        (member as f32) * 0.37,
        f32::INFINITY,
    );
    let body_mesh = meshes.add(crate::body::mesh(&species.body));
    let name = format!("{}{}-{}", record.role.prefix(), species.name, member);
    let entity = commands
        .spawn((
            StageOwned,
            crate::persistence::WorldId(member),
            Visibility::Inherited,
            crate::creature_actions::CreatureMind {
                species: species.id as u8,
                allied,
                rounds,
                ..default()
            },
            Npc {
                speed,
                // Attack on sight is the species relationship's (0xC0B9C0): the
                // herd's initial one (`species.hostile`) until the player's
                // befriends or kills move it out of hated / hostile.
                hostile: ((species.hostile
                    && crate::creature_actions::species_relationship_of(world, species.id)
                        .is_none_or(crate::creature_rules::attacks_on_sight))
                    || world.species_aggravated(species.id))
                    && !allied,
                hp,
            },
            Wander { tx, tz, wait: 0.0 },
            NestHome {
                x: nest.position[0],
                z: nest.position[2],
                radius,
            },
            Mesh3d(body_mesh),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(species.tint[0], species.tint[1], species.tint[2]),
                perceptual_roughness: 0.6,
                ..default()
            })),
            Transform::from_xyz(spawn.position[0], 0.4, spawn.position[2])
                .with_scale(Vec3::splat(scale)),
            Name::new(name),
        ))
        .id();
    if let Some(archetype) = species.archetype {
        commands.entity(entity).insert(crate::creature_ecology::HerdMember {
            species: species.id,
            nest: nest_index,
            archetype,
            role: record.role,
            guard: record.guard,
        });
    }
    if crate::baked_model::spawn_species_visual(commands, originals, meshes, entity, species.visual()) {
        commands
            .entity(entity)
            .remove::<(Mesh3d, MeshMaterial3d<StandardMaterial>)>();
    } else {
        mount_species_parts(
            commands,
            materials,
            entity,
            originals,
            species,
            index.max(1),
        );
    }
    Some(entity)
}

/// How far a neutral member wanders from its nest centre.
const NEST_ROAM_RADIUS: f32 = 3.0;
/// Spawn every saved part on the avatar, each on its own slot bone.
pub fn spawn_creature_parts(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    player: Query<&Transform, With<Player>>,
) {
    let Ok(pt) = player.single() else {
        return;
    };
    for (i, p) in run.parts.iter().enumerate() {
        let Some(part) = originals.models.iter().find(|m| m.instance == p.instance) else {
            continue;
        };
        let slot = crate::baked_model::attachment_point(&originals, &run.body, p.bone);
        commands.spawn((
            StageOwned,
            Accessory {
                part: i,
                slot_local: slot,
            },
            Mesh3d(part.handle.clone()),
            MeshMaterial3d(part.material.clone().unwrap_or_else(|| {
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.85, 0.75, 0.6),
                    perceptual_roughness: 0.6,
                    ..default()
                })
            })),
            Transform::from_translation(
                pt.translation + pt.rotation * ((slot + Vec3::from_array(p.offset)) * pt.scale),
            )
            .with_rotation(pt.rotation * p.rotation())
            .with_scale(p.mesh_scale(part.scale * 0.5 * p.scale) * pt.scale),
        ));
    }
    if !run.parts.is_empty() {
        info!("creature carries {} original part(s)", run.parts.len());
    }
}

/// Each part rides its slot bone on the avatar, honoring editor tweaks.
pub fn accessory_follow(
    player: Query<&Transform, With<Player>>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    mut acc: Query<(&Accessory, &mut Transform), Without<Player>>,
) {
    let Ok(pt) = player.single() else {
        return;
    };
    for (a, mut t) in &mut acc {
        let Some(p) = run.parts.get(a.part) else {
            continue;
        };
        t.translation = pt.translation
            + pt.rotation
                * ((crate::baked_model::attachment_point(&originals, &run.body, p.bone)
                    + Vec3::from_array(p.offset))
                    * pt.scale);
        t.rotation = pt.rotation * p.rotation();
        if let Some(model) = originals.models.iter().find(|m| m.instance == p.instance) {
            t.scale = p.mesh_scale(model.scale * 0.5 * p.scale) * pt.scale;
        }
    }
}

/// Creature-stage part editor over the saved part list:
/// `[`/`]` cycle the focused part's instance, `,`/`.` its slot bone,
/// `Tab` focus the next part, `N` add a part, `Backspace` remove it,
/// `R` rotate, `-`/`=` scale. Everything persists.
pub fn part_editor(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut run: ResMut<RunProgress>,
    originals: Res<crate::Originals>,
    acc: Query<Entity, With<Accessory>>,
    player: Query<&Transform, With<Player>>,
) {
    let n_models = originals.models.len() as u32;
    if !originals.part_catalog.parts.is_empty() {
        return;
    }
    let n_bones = originals
        .rigged_demo
        .as_ref()
        .map(|d| d.bones as u32)
        .unwrap_or(0);
    let mut changed = false;

    // Focus movement / add / remove.
    if keys.just_pressed(KeyCode::Tab) && !run.parts.is_empty() {
        run.focused_part = (run.focused_part + 1) % run.parts.len() as u32;
        changed = true;
    }
    if keys.just_pressed(KeyCode::KeyN) && n_models > 0 {
        // Mount a part that isn't carried yet, on a slot matching its family.
        let mounted: Vec<u32> = run.parts.iter().map(|p| p.instance).collect();
        let pick = originals
            .models
            .iter()
            .find(|m| !mounted.contains(&m.instance))
            .or_else(|| originals.models.first())
            .expect("models non-empty");
        let bone = crate::default_slot_for_part(&originals, pick.rig_bones, SlotKind::Limb)
            .unwrap_or(run.accessory_bone as usize) as u32;
        run.part_add(pick.instance, bone);
        changed = true;
    }
    if keys.just_pressed(KeyCode::Backspace) && !run.parts.is_empty() {
        run.part_remove_focused();
        changed = true;
    }

    // Mirror the focused part onto its sagittal pair (M) — before the
    // focused-edit borrow below.
    if keys.just_pressed(KeyCode::KeyM) {
        let focus = run.focused_part;
        if let Some(demo) = originals.rigged_demo.as_ref() {
            if let Some(worlds) = crate::rw4::fk_worlds(&demo.parents, &demo.locals) {
                let map = crate::rw4::mirror_map(&worlds, 0.2);
                if part_add_mirrored(&mut run.parts, focus, &map) {
                    changed = true;
                }
            }
        }
    }

    // Focused-part edits.
    let fi = focus_index(run.focused_part, run.parts.len()) as usize;
    if fi < run.parts.len() {
        let part = &mut run.parts[fi];
        if keys.just_pressed(KeyCode::BracketLeft) || keys.just_pressed(KeyCode::BracketRight) {
            if n_models > 0 {
                let dir = if keys.just_pressed(KeyCode::BracketRight) {
                    1
                } else {
                    -1
                };
                let cur = originals
                    .models
                    .iter()
                    .position(|m| m.instance == part.instance)
                    .map(|i| i as i32 + 1)
                    .unwrap_or(0);
                let next = cycle_index(cur, dir, n_models as i32);
                part.instance = if next == 0 {
                    0
                } else {
                    originals.models[next as usize - 1].instance
                };
                changed = true;
            }
        }
        if keys.just_pressed(KeyCode::Comma) || keys.just_pressed(KeyCode::Period) {
            if n_bones > 0 {
                let dir = if keys.just_pressed(KeyCode::Period) {
                    1
                } else {
                    -1
                };
                part.bone = cycle_bone(part.bone, dir, n_bones);
                changed = true;
            }
        }
        if keys.just_pressed(KeyCode::KeyR) {
            part.yaw = wrap_yaw(part.yaw + std::f32::consts::FRAC_PI_4);
            changed = true;
        }
        if keys.just_pressed(KeyCode::Minus) {
            part.scale = scaled(part.scale, 0.9);
            changed = true;
        }
        if keys.just_pressed(KeyCode::Equal) {
            part.scale = scaled(part.scale, 1.1);
            changed = true;
        }
        // Slide the part on its slot: I/K x, J/L z, U/O y.
        if apply_slide_keys(&keys, part) {
            changed = true;
        }
    }
    if !changed {
        return;
    }
    // Re-sync legacy mirror + respawn the mounted visuals.
    run.sync_legacy_accessory();
    run.request_save();
    for e in &acc {
        commands.entity(e).despawn();
    }
    let at = player.single().map(|t| t.translation).unwrap_or(Vec3::ZERO);
    for (i, p) in run.parts.iter().enumerate() {
        let Some(model) = crate::editor::model_for_part(&originals, p) else {
            continue;
        };
        let slot = crate::baked_model::attachment_point(&originals, &run.body, p.bone);
        commands.spawn((
            StageOwned,
            Accessory {
                part: i,
                slot_local: slot,
            },
            Mesh3d(model.draw_mesh()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.85, 0.75, 0.6),
                perceptual_roughness: 0.6,
                ..default()
            })),
            Transform::from_translation(at + Vec3::new(slot.x, slot.y, slot.z))
                .with_scale(p.mesh_scale(model.scale * 0.5 * p.scale)),
        ));
    }
    if let Some(p) = run.parts.get(fi) {
        info!(
            "part[{fi}] {:#X} slot {} yaw {:.0}° scale {:.2} ({} parts)",
            p.instance,
            p.bone,
            p.yaw.to_degrees(),
            p.scale,
            run.parts.len()
        );
    }
}

/// Fruit on the planet's groves. The fruit manager (cGamePlantManager, message
/// 0x030FC4EC) gets one message per grove of the theme's fruit distribute
/// effect, the same groves `planet_flora` draws and the level generation marks
/// as forests; the theme (`planetTerrainThemes~!AllPlanets`, PROP
/// 0x42840100!0x6E660BEB) caps the fruit of a grove by tree class:
/// `maxLargeTreeFruitPerGrove` 30 (0x035494E3), `maxMediumTreeFruitPerGrove` 36
/// (0x035494E6), small trees 0 (0x035494E9). Index = the flora class (large,
/// medium).
pub const GROVE_FRUIT: [u32; 2] = [30, 36];
/// Groves farther than this from the home nest hold no fruit objects (the
/// level generator's forest radius, prop 0xF64F11BE, default 200). The fruit
/// bushes' world ids 100.. stay below the tribe's 200..
pub const FRUIT_RADIUS: f32 = 200.0;
const MAX_FRUIT_TREES: usize = 99;

/// Legacy island layout (no planet, or no grove in range): this many bushes
/// around the home nest, `BUSHES_PER_NEST` in the territory of every
/// plant-eating herd, at most `MAX_BUSHES`. ADAPTED: invented, kept only where
/// the planet's groves cannot be used.
const HOME_BUSHES: usize = 10;
const BUSHES_PER_NEST: usize = 2;
const MAX_BUSHES: usize = 60;

/// Pure: the fruit piles of the planet's groves near `home` as (chart point,
/// food in hunger units), nearest grove first, at most `MAX_FRUIT_TREES`. A
/// grove's fruit (`GROVE_FRUIT` per tree class) is shared by its trees of that
/// class; each fruit is `creatureHungerSatisfiedPerFruit` (25) of hunger.
/// ADAPTED: the fruit of a tree lies as one pile under its crown, 3 x the
/// tree size from the trunk at a golden-angle heading (the retail fruit
/// objects' positions on the trees were not traced).
pub fn fruit_spots(
    land: &crate::creature_planet::Land,
    home: Vec2,
    pads: &[([f32; 3], f32)],
) -> Vec<(Vec2, f32)> {
    fruit_spots_of(land, &land.fruit_groves(home, FRUIT_RADIUS, pads))
}

/// [`fruit_spots`] for the given groves.
pub fn fruit_spots_of(
    land: &crate::creature_planet::Land,
    groves: &[Vec<crate::creature_planet::FruitTree>],
) -> Vec<(Vec2, f32)> {
    let mut out = Vec::new();
    'groves: for grove in groves {
        for class in 0..GROVE_FRUIT.len() {
            let trees: Vec<_> = grove.iter().filter(|t| t.class == class).collect();
            if trees.is_empty() {
                continue;
            }
            let total = GROVE_FRUIT[class];
            let count = trees.len() as u32;
            for (i, tree) in trees.iter().enumerate() {
                let fruit = total / count + u32::from((i as u32) < total % count);
                if fruit == 0 {
                    continue;
                }
                if out.len() >= MAX_FRUIT_TREES {
                    break 'groves;
                }
                let angle = (i as f32 + 0.5 * class as f32) * 2.399_963;
                let at = land.toward(tree.at, 3.0 * tree.scale.max(0.5), angle);
                if land.is_land(at) {
                    out.push((at, fruit as f32 * crate::creature_rules::HUNGER_PER_FRUIT));
                }
            }
        }
    }
    out
}

/// Pure: where the fruit bushes grow (world x, z), on land, from their own
/// seed so the layout does not shift with the ecology's random values.
pub fn bush_spots(
    world: &crate::species::SpeciesWorld,
    land: &crate::creature_planet::Land,
    home: [f32; 3],
) -> Vec<Vec2> {
    let mut rng = ChaCha8Rng::seed_from_u64(0xB005_5EED);
    let mut out = Vec::new();
    let mut grow = |centre: Vec2, near: f32, far: f32, count: usize, out: &mut Vec<Vec2>| {
        let mut placed = 0;
        for _ in 0..count * 8 {
            if placed == count || out.len() >= MAX_BUSHES {
                break;
            }
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            let at = land.toward(centre, rng.random_range(near..far.max(near + 1.0)), angle);
            if land.is_land(at) && !land.water_within(at, 2.0) {
                out.push(at);
                placed += 1;
            }
        }
    };
    grow(Vec2::new(home[0], home[2]), 6.0, 40.0, HOME_BUSHES, &mut out);
    for nest in &world.nests {
        let plants = world.species(nest.species).is_some_and(|s| {
            matches!(s.diet, crate::species::Diet::Herbivore | crate::species::Diet::Omnivore)
                && s.archetype != Some(crate::creature_ecology::Archetype::NightPredator)
        });
        if plants {
            let territory = world
                .archetype_of(nest.species)
                .map_or(NEST_ROAM_RADIUS, |a| {
                    crate::creature_ecology::territory_radius(a, false, crate::creature_ecology::MemberRole::Adult)
                });
            grow(Vec2::new(nest.position[0], nest.position[2]), 4.0, territory, BUSHES_PER_NEST, &mut out);
        }
    }
    out
}

/// Facing turn tuning (adapted, not recovered Spore values). Rates are the
/// exponential approach per second; speeds cap the angular velocity (rad/s).
const PLAYER_TURN_RATE: f32 = 12.0;
const PLAYER_MAX_TURN_SPEED: f32 = 10.0;
const NPC_TURN_RATE: f32 = 7.0;
const NPC_MAX_TURN_SPEED: f32 = 5.0;
/// Idle members turn toward something they watch more lazily.
const NPC_GLANCE_RATE: f32 = 2.5;
/// A non-hostile idle member turns to watch the player within this range.
/// (`creatureWaryDistance`, tuning).
const NPC_CURIOUS_RANGE: f32 = crate::creature_rules::WARY_DISTANCE;

/// Pure: ease `yaw` toward `target` along the shorter arc. `rate` is an
/// exponential approach per second and `max_speed` caps the angular velocity,
/// so a reversal is a visible turn rather than a snap.
pub fn turn_toward(yaw: f32, target: f32, rate: f32, max_speed: f32, dt: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    if !(yaw.is_finite() && target.is_finite() && dt.is_finite()) || dt <= 0.0 {
        return if yaw.is_finite() { yaw } else { target };
    }
    let error = (target - yaw + PI).rem_euclid(TAU) - PI;
    let step = (error * (1.0 - (-rate * dt).exp())).clamp(-max_speed * dt, max_speed * dt);
    (yaw + step + PI).rem_euclid(TAU) - PI
}

/// Pure: yaw whose local -Z (the creature front) points along `direction`.
pub fn facing_yaw(direction: Vec3) -> f32 {
    (-direction.x).atan2(-direction.z)
}

/// Pure: how much of its travel speed a member uses while still turning
/// toward `direction`. Facing it walks at full pace; a large heading error
/// turns nearly in place instead of sliding sideways or backwards.
pub fn stride_alignment(facing: Vec3, direction: Vec3) -> f32 {
    let facing = Vec3::new(facing.x, 0., facing.z).normalize_or_zero();
    let direction = Vec3::new(direction.x, 0., direction.z).normalize_or_zero();
    if facing == Vec3::ZERO || direction == Vec3::ZERO {
        return 1.0;
    }
    let t = ((facing.dot(direction) + 0.2) / 1.0).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Wander state for neutral NPCs (idle pauses + random strolls).
#[derive(Component)]
pub struct Wander {
    pub tx: f32,
    pub tz: f32,
    pub wait: f32,
}

/// Pure wander step on the XZ plane. Returns (new_x, new_z, arrived).
/// When `wait > 0` the NPC idles and the timer ticks down (arrived=false).
/// Otherwise it walks toward the target at `speed`.
pub fn wander_advance(
    x: f32,
    z: f32,
    tx: f32,
    tz: f32,
    wait: f32,
    dt: f32,
    speed: f32,
) -> (f32, f32, f32, bool) {
    if wait > 0.0 {
        return (x, z, (wait - dt).max(0.0), false);
    }
    let dx = tx - x;
    let dz = tz - z;
    let d = (dx * dx + dz * dz).sqrt();
    if d < 0.6 {
        return (x, z, 0.0, true);
    }
    let step = (speed * dt).min(d);
    (x + dx / d * step, z + dz / d * step, 0.0, step >= d - 1e-6)
}

#[derive(Resource)]
pub struct CreatureSpawnTimer(pub Timer);
impl Default for CreatureSpawnTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(4.0, TimerMode::Repeating))
    }
}

/// The current creature's derived stats (set on stage entry).
#[derive(Resource, Default, Clone, Copy)]
pub struct CreatureStatsRes(pub CreatureStats);

/// Pure helper: the assembled creature's stats from the saved part list.
pub fn stats_for(run: &RunProgress, originals: &crate::Originals) -> CreatureStats {
    if !originals.part_catalog.parts.is_empty() {
        let a = originals
            .part_catalog
            .abilities_for_parts_in(crate::asset_catalog::CREATURE_PARTS, &run.parts);
        // These world units still use prototype tuning. Authored ranks drive
        // the values instead of rewarding arbitrary mesh sizes/part count.
        return CreatureStats {
            speed: 8.0 + a.speed as f32 * 0.6,
            max_hp: 120.0 + a.health as f32 * 20.0,
            social: 1.0 + a.sing.max(a.dance).max(a.charm).max(a.pose) as f32 * 0.12,
            size: 1.0,
        };
    }
    creature_stats(&run.parts, |instance| {
        originals
            .models
            .iter()
            .find(|m| m.instance == instance)
            .map(|m| m.scale)
    })
}

/// The Creature-stage avatar's stats under the retail rules (creature_rules
/// C1/C2): base HP from the brain level's health-table entry plus the parts
/// bonus, the speed gear of its speed level, and the lineage size grown to the
/// brain level's size factor. `stats_for` stays the lineage summary that later
/// stages inherit (it is saved in `RunProgress::stats`).
pub fn avatar_stats(run: &RunProgress, originals: &crate::Originals) -> CreatureStats {
    let lineage = stats_for(run, originals);
    let abilities = crate::creature_actions::player_abilities(run, originals);
    let level = crate::creature_rules::brain_level_from_total(run.earned_creature_dna());
    CreatureStats {
        speed: crate::creature_rules::avatar_speed(abilities.speed),
        max_hp: crate::creature_rules::avatar_max_hp(level, abilities.health),
        social: lineage.social,
        size: crate::creature_rules::avatar_scale(lineage.size, level),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn setup_creature(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut clear: ResMut<ClearColor>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    cache: Option<Res<crate::terrain::PlanetCache>>,
) {
    // The planet the herds, bushes and scenery stand on (flat without one).
    let land = crate::creature_planet::site_land(cache.as_deref(), world_seed(&run), run.species_world.planet_frame);
    run.enter_stage(GameStage::Creature);
    commands.insert_resource(mobility::CreatureMobility::default());
    commands.insert_resource(CreatureSpawnTimer::default());
    clear.0 = GameStage::Creature.accent();
    commands.spawn((
        StageOwned,
        Mesh3d(meshes.add(Cuboid::new(52.0, 1.0, 52.0))),
        crate::terrain::WorldGround,
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.32, 0.12),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -1.1, 0.0),
    ));
    // The editable spine surface carries the original authored parts.
    let stats = stats_for(&run, &originals);
    let avatar = avatar_stats(&run, &originals);
    commands.insert_resource(CreatureStatsRes(stats));
    // Persist so later stages scale to the creature you built.
    if run.stats != Some(stats) {
        run.stats = Some(stats);
        run.request_save();
    }
    let original_body = crate::baked_model::prepare_body(
        &mut originals,
        &run.body,
        &mut meshes,
        &mut images,
        &mut materials,
    )
    .map_err(|e| {
        if !originals.resource_index.records.is_empty() {
            warn!("original player body: {e}")
        }
        e
    })
    .ok();
    let body_mesh = original_body
        .as_ref()
        .map(|m| m.draw_mesh())
        .unwrap_or_else(|| meshes.add(crate::body::mesh(&run.body)));
    let body_material = original_body
        .as_ref()
        .and_then(|m| m.material.clone())
        .unwrap_or_else(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.43, 0.68, 0.33),
                perceptual_roughness: 0.6,
                ..default()
            })
        });
    let player = commands
        .spawn((
            StageOwned,
            Player,
            Mover {
                speed: avatar.speed,
            },
            Health::new(avatar.max_hp),
            avatar_hunger(&mut run),
            Mesh3d(body_mesh),
            MeshMaterial3d(body_material),
            Transform::from_xyz(0.0, 0.7 * avatar.size, 8.0).with_scale(Vec3::splat(avatar.size)),
        ))
        .id();
    if let Some(m) = &original_body {
        commands
            .entity(player)
            .insert(crate::baked_model::source(m.source_definition.unwrap()));
    }
    // Species world: reuse a saved ecology, otherwise derive one from the
    // lineage, and import every original part those species wear. This
    // duplicates `prepare_species` on purpose so the stage is also correct
    // when only this system runs (headless checks).
    migrate_checkpoint_outcomes_on(&mut run, &originals.part_catalog, &land);
    let world = ensure_species_world_on(&mut run, &originals.part_catalog, &land);
    import_species_models(
        &world,
        &mut originals,
        &mut meshes,
        &mut images,
        &mut materials,
    );
    remap_checkpoint_species(&mut run);
    commands.insert_resource(crate::creature_actions::Encounter::default());
    // A nest prop per home, tinted by its species. The night predators have
    // no nest of their own on the island: they come at night (0xD4F290).
    for (nest_index, nest) in world.nests.iter().enumerate() {
        let Some(species) = world.species(nest.species) else {
            continue;
        };
        if species.archetype == Some(crate::creature_ecology::Archetype::NightPredator) {
            continue;
        }
        // 0xE8DF30: the nest of an extinct herd is drawn abandoned.
        let extinct = world.nest_status(nest_index) == crate::species::NestStatus::Destroyed;
        if crate::world_objects::spawn_at(
            &mut commands,
            &mut originals,
            crate::creature_home::npc_nest_definition(extinct),
            "Creature NPC Nest",
            &mut meshes,
            &mut images,
            &mut materials,
            Vec3::new(nest.position[0], -0.6, nest.position[2]),
            4.8,
        )
        .is_some()
        {
            continue;
        }
        commands.spawn((
            StageOwned,
            Mesh3d(meshes.add(Cylinder::new(2.4, 0.5))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(species.tint[0], species.tint[1], species.tint[2]),
                perceptual_roughness: 0.8,
                ..default()
            })),
            Transform::from_xyz(nest.position[0], -0.7, nest.position[2]),
            Name::new(format!("nest-{}", nest.species)),
        ));
    }
    // Retail herds: the casting manager's first pass activates every herd
    // within the activation distance of where the player starts (the home
    // nest, or the checkpoint's position), all at once as at load time.
    let mut spawned = 0usize;
    if world.is_retail() {
        let start = land.world_point(start_position(&run));
        let night = crate::creature_ecology::night_predators_out(world.time_of_day);
        let mut casting = crate::creature_ecology::CastingState::default();
        let mut existing = HashSet::new();
        for index in 0..world.nests.len() {
            if !herd_wanted(&world, index, start, night, false) {
                continue;
            }
            casting.active.insert(index);
            spawned += activate_herd(
                &mut commands,
                &mut meshes,
                &mut materials,
                &originals,
                &world,
                &run.creature_pack,
                index,
                &mut existing,
                spawned,
            );
        }
        casting.night = night;
        commands.insert_resource(casting);
        // The generation list's Carcass: a meal near home for a fresh stage.
        if let Some(at) = world.carcass.filter(|_| run.earned_creature_dna() < 1.0) {
            crate::creature_social::spawn_meat(
                &mut commands,
                Vec3::from_array(at),
                crate::creature_ecology::CARCASS.food_value,
            );
        }
    }
    // Legacy worlds: living members, in the world's deterministic order.
    // Defeated members are already recorded as such, so they never come back.
    for entry in world.spawn_list() {
        if world.is_retail() || spawned >= MAX_LIVE_MEMBERS {
            break;
        }
        if !world
            .relationship(entry.member)
            .is_some_and(|r| r.is_alive())
        {
            continue;
        }
        if spawn_species_member(
            &mut commands,
            &mut meshes,
            &mut materials,
            &originals,
            &world,
            &run.creature_pack,
            entry.member,
            spawned,
        )
        .is_some()
        {
            spawned += 1;
        }
    }
    // Fruit stands in the planet's groves; the invented island layout (its own
    // seed, so it does not shift when the species ecology changes how many
    // random values the world consumes) only where no grove is in range.
    let home = run.creature_lifecycle.home;
    // The pads the planet flora keeps free of plants (`planet_flora::clearings`).
    let pads: Vec<([f32; 3], f32)> = std::iter::once((home, 8.0))
        .chain(world.nests.iter().map(|n| (n.position, 6.0)))
        .collect();
    let mut spots = fruit_spots(&land, Vec2::new(home[0], home[2]), &pads);
    if spots.is_empty() {
        spots = bush_spots(&world, &land, home).into_iter().map(|at| (at, 60.0)).collect();
    } else {
        let nearest = spots
            .iter()
            .map(|(at, _)| land.chord(Vec2::new(home[0], home[2]), *at))
            .fold(f32::INFINITY, f32::min);
        info!(
            "creature fruit: {} piles ({:.0} hunger) in the planet's groves, nearest {nearest:.0} from the home nest",
            spots.len(),
            spots.iter().map(|(_, food)| food).sum::<f32>()
        );
    }
    for (i, (spot, food)) in spots.into_iter().enumerate() {
        let i = i as u32;
        let position = Vec3::new(spot.x, -0.6, spot.y);
        if let Some(e) = crate::world_objects::spawn_at(
            &mut commands,
            &mut originals,
            crate::dbpf::ResourceKey {
                group_id: 0,
                instance_id: 0x154FBFB5,
                type_id: crate::asset_catalog::PROP,
            },
            "Fruit_Normal_01",
            &mut meshes,
            &mut images,
            &mut materials,
            position,
            1.4,
        ) {
            commands.entity(e).insert((
                crate::persistence::WorldId(100 + i),
                Bush { food_left: food },
                Edible {
                    nutrition: 0.,
                    dna: 0.,
                    meat: false,
                },
            ));
            continue;
        }
        commands.spawn((
            StageOwned,
            crate::persistence::WorldId(100 + i),
            Bush { food_left: food },
            Edible {
                nutrition: 0.0,
                dna: 0.0,
                meat: false,
            },
            Mesh3d(meshes.add(Sphere::new(0.9))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.1, 0.5, 0.15),
                perceptual_roughness: 0.6,
                ..default()
            })),
            Transform::from_translation(position + Vec3::Y * 0.6),
        ));
    }
    // Scenery only on land and off the home nest's pad; ADAPTED spread x2
    // (the planet's flora from terrainDistribute is not placed yet).
    let home = Vec2::new(run.creature_lifecycle.home[0], run.creature_lifecycle.home[2]);
    crate::baked_model::scenery_where(
        &mut commands,
        &mut originals,
        &mut meshes,
        &mut images,
        &mut materials,
        world.seed,
        if land.on_planet() { 2.0 } else { 1.0 },
        &|p: Vec3| land.is_land(p.xz()) && p.xz().distance(home) > 6.0,
    );
    info!(
        "creature stage: {} species, {} living members",
        world.species.len(),
        spawned
    );
}

#[allow(clippy::too_many_arguments)]
/// The avatar's hunger when it is created. VERIFIED (`cCreatureAnimal::Create`
/// 0xC09B40, tail 0xC0ACFD..0xC0ADC5): hunger = RandomFloat in
/// [`creatureStartingMotive_HungerMin`, `..Max`] (0.5, 1.0) x the maximum for
/// every creature it creates, the avatar included. The roll is kept with the
/// lineage (`CreatureProgression::avatar_hunger`), so re-entering the stage after
/// the editor or a load continues the saved hunger instead of rolling again.
fn avatar_hunger(run: &mut RunProgress) -> Hunger {
    use crate::creature_rules::MOTIVE_MAX;
    let fraction = *run
        .creature_progression
        .avatar_hunger
        .get_or_insert_with(|| crate::creature_progression::start_hunger_fraction(rand::random::<f32>()));
    Hunger {
        food: (fraction.clamp(0.0, 1.0)) * MOTIVE_MAX,
        max: MOTIVE_MAX,
        hold: 0.0,
    }
}

pub fn creature_walk(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut controls: ResMut<crate::creature_controls::CreatureControls>,
    originals: Res<crate::Originals>,
    mut mobility: ResMut<mobility::CreatureMobility>,
    mut run: ResMut<RunProgress>,
    mut encounter: ResMut<crate::creature_actions::Encounter>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    clip_drive: Option<Res<crate::creature_clips::ClipDrive>>,
    mut player: Query<
        (
            &mut Transform,
            &Mover,
            &mut Hunger,
            &mut Health,
            Option<&crate::locomotion_preview::AdaptedWalk>,
        ),
        With<Player>,
    >,
) {
    let Ok((mut t, mover, mut hunger, mut health, walk)) = player.single_mut() else {
        return;
    };
    let abilities = crate::creature_actions::player_abilities(&run, &originals);
    let attack_pressed = encounter.mode == crate::creature_actions::EncounterMode::Combat
        && (keys.just_pressed(KeyCode::Digit1)
            || keys.just_pressed(KeyCode::Digit2)
            || keys.just_pressed(KeyCode::Digit3)
            || keys.just_pressed(KeyCode::Digit4));
    mobility.update(&keys, abilities, time.delta_secs());
    if attack_pressed {
        // After `update`, so this frame's attack still counts as a sneak attack.
        mobility.on_attack();
    }
    let before = t.translation;
    let previous_yaw = t.rotation.to_euler(EulerRot::YXZ).0;
    // Retail: the gear speed, plus the additive sprint boost while sprinting.
    let speed = (mover.speed + mobility.sprint_speed_bonus()) * mobility.sneak_speed_multiplier();
    // A held glide moves at the original's creatureGlideSpeed.
    let speed = if mobility.gliding {
        crate::creature_controls::GLIDE_SPEED
    } else {
        speed
    };
    // A charge in flight (creature_actions::Rush) runs the avatar at its target at
    // the rushing speed and ignores the steering until it ends.
    let rushing = encounter.rush.is_some();
    let direct_yaw = if let Some(mut rush) = encounter.rush {
        controls.goal = None;
        controls.autorun = false;
        let next = crate::creature_actions::rush_step(&mut rush, t.translation, time.delta_secs());
        let heading = Vec3::new(rush.goal.x - t.translation.x, 0.0, rush.goal.z - t.translation.z);
        t.translation = crate::creature_planet::Land::flat().step(t.translation, next);
        if heading.length_squared() > 1.0e-6 {
            t.rotation = Quat::from_rotation_y(facing_yaw(heading.normalize()));
        }
        // A rush whose range ran out without a hit is over; one that arrived
        // is landed by `creature_actions::update` on its next pass.
        encounter.rush = Some(rush);
        if rush.missed {
            encounter.rush = None;
            encounter.message = "The charge missed".into();
        }
        true
    } else {
        // Retail steering (creature_controls, C6B): W/S walk along the facing, A/D
        // turn it, Q/E strafe, R autoruns, a click walks to a point.
        crate::creature_controls::steer(
            &keys,
            &mut controls,
            &mut t,
            speed,
            time.delta_secs(),
            // Retail creatures swim (creature_support): the avatar is not held at
            // the waterline; the sea monster bounds it instead.
            &crate::creature_planet::Land::flat(),
        )
    };
    let speed = if rushing {
        crate::creature_rules::CHARGE_RUSH_SPEED
    } else {
        speed
    };
    let _ = &surface;
    let moved = t.translation - before;
    // Procedural gait for the original body: vertical bob proportional to
    // ground speed, plus a slight lean into the motion. A skinned walk cycle
    // drives the bob from its own stride phase so steps and bob stay in sync.
    let ground = (moved.x * moved.x + moved.z * moved.z).sqrt() / time.delta_secs().max(1e-4);
    let gait = (ground / speed.max(1e-4)).clamp(0.0, 1.0);
    let bob = walk
        .map(|walk| walk.bob_phase())
        .unwrap_or(time.elapsed_secs() * 11.0);
    // Clips pose the skinned body (`creature_clips`): no procedural bob/lean.
    let gait = if crate::creature_clips::drives_body(clip_drive.as_deref()) { 0.0 } else { gait };
    let bob_offset = if mobility.height <= 0.0 {
        bob.sin() * 0.06 * gait
    } else {
        0.0
    };
    t.translation.y = 0.7 * t.scale.x + mobility.height + bob_offset;
    // A click goal names the yaw to face: turn toward it. The keys turn the
    // creature at their own rate, so their yaw is final.
    let target_yaw = t.rotation.to_euler(EulerRot::YXZ).0;
    let yaw = if direct_yaw {
        target_yaw
    } else {
        turn_toward(
            previous_yaw,
            target_yaw,
            PLAYER_TURN_RATE,
            PLAYER_MAX_TURN_SPEED,
            time.delta_secs(),
        )
    };
    let lean = (bob.cos() * 0.05 * gait).clamp(-0.08, 0.08);
    t.rotation = Quat::from_euler(EulerRot::YXZ, yaw, -lean, 0.0);
    // Retail hunger (creature_rules C2B): held full for the decay delay, then
    // drained per second; below hunger 1 starvation costs HP per second.
    let difficulty = crate::creature_rules::Difficulty::from_index(run.cell.difficulty);
    hunger.tick_retail(
        time.delta_secs(),
        crate::creature_rules::hunger_drain(difficulty),
        crate::creature_rules::hunger_decay_delay(difficulty),
    );
    health.hurt(crate::creature_rules::starvation_damage(
        hunger.food,
        time.delta_secs(),
    ));
    // The avatar's hunger is saved with the lineage (a stage entry continues it
    // instead of drawing the start value again, `creature_progression`).
    run.creature_progression.avatar_hunger = Some(hunger.frac());
    let threatened = encounter.mode == crate::creature_actions::EncounterMode::Combat
        && encounter.target.is_some();
    let restored = run.creature_lifecycle.tick(
        time.delta_secs(),
        t.translation.to_array(),
        gait > 0.05,
        threatened,
    );
    if !health.dead() {
        // `restored` is a fraction of the maximum HP (retail healing rate).
        let healed = restored * health.max;
        health.heal(healed);
    }
    if health.dead() {
        // Retail (0xD38E60) plays `CRG_AvatarDied*` where the avatar fell and
        // respawns it at the nest on the cinematic's kRestartAtNest.
        let cause = if crate::creature_support::take_sea_monster() {
            crate::cinematics::DeathCause::SeaMonster
        } else if hunger.food < 1.0 {
            crate::cinematics::DeathCause::Starvation
        } else {
            crate::cinematics::DeathCause::Combat
        };
        if crate::cinematics::hold_death(cause) {
            return;
        }
        health.hp = health.max;
        hunger.food = hunger.max;
        hunger.hold = 0.0;
        encounter.rush = None;
        // Death loses no whole DNA (avatarDeathEvoPointsKeptPercentage 1.0,
        // avatarDeathMaxProgressEvoPointsRemoved 0.0: 0xD401E0 only drops the
        // fraction of the stage total).
        let (spendable, total) =
            crate::creature_rules::dna_after_death(run.editor_dna, run.dna_this_stage);
        run.editor_dna = spendable;
        run.dna_this_stage = total;
        t.translation = Vec3::from_array(run.creature_lifecycle.respawn());
        mobility.reset_for_respawn(abilities);
        controls.reset_motion();
        encounter.target = None;
        encounter.message = "You hatched again at your home nest".into();
        run.request_save();
    }
}

pub fn creature_graze(
    time: Res<Time>,
    originals: Res<crate::Originals>,
    mut run: ResMut<RunProgress>,
    mobility: Option<Res<mobility::CreatureMobility>>,
    player: Query<&Transform, With<Player>>,
    mut bushes: Query<(&Transform, &mut Bush)>,
    mut hunger: Query<(&mut Hunger, Option<&mut Health>), With<Player>>,
) {
    let Ok(pt) = player.single() else {
        return;
    };
    let Ok((mut hunger, mut health)) = hunger.single_mut() else {
        return;
    };
    // Food is a ground-level interaction; jumping/gliding over a bush cannot
    // trigger collection solely because the XZ projection overlaps it.
    if mobility
        .as_deref()
        .is_some_and(|mobility| mobility.height > 0.15)
    {
        return;
    }
    let abilities = originals.part_catalog.abilities_in(
        crate::asset_catalog::CREATURE_PARTS,
        run.parts.iter().map(|p| p.instance),
    );
    if !originals.part_catalog.parts.is_empty() && abilities.herbivore == 0 {
        return;
    }
    for (bt, mut bush) in &mut bushes {
        if bush.food_left > 0. && dist_xz(pt.translation, bt.translation) < 1.6 {
            // Retail: a fruit satisfies 25 hunger and heals 0.25 HP. The bush
            // counts its food in hunger units (60 = 2.4 fruits) and is eaten at
            // one fruit per second (ADAPTED: the eating rate is not traced).
            let bite = (crate::creature_rules::HUNGER_PER_FRUIT * time.delta_secs())
                .min(bush.food_left);
            bush.food_left -= bite;
            hunger.eat(bite);
            if let Some(health) = health.as_mut() {
                health.heal(
                    bite * crate::creature_rules::HEALTH_PER_FRUIT
                        / crate::creature_rules::HUNGER_PER_FRUIT,
                );
            }
            run.diet_herbivore += bite * 0.1;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn creature_wander(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    run: Res<RunProgress>,
    originals: Option<Res<crate::Originals>>,
    mobility: Option<Res<mobility::CreatureMobility>>,
    encounter: Res<crate::creature_actions::Encounter>,
    player: Query<&Transform, With<Player>>,
    bushes: Query<&Transform, (With<Bush>, Without<Npc>, Without<Player>)>,
    mut npcs: Query<
        (
            &mut Transform,
            &Npc,
            Option<&mut Wander>,
            Option<&NestHome>,
            Option<&crate::creature_actions::CreatureMind>,
            &crate::persistence::WorldId,
            Option<&crate::creature_social::Ecology>,
            Option<&crate::creature_ecology::HerdMember>,
        ),
        (Without<Player>, Without<Bush>),
    >,
    surface: Option<Res<crate::terrain::LandSurface>>,
) {
    let land = crate::creature_planet::Land::from_surface(surface.as_deref());
    let bush_spots: Vec<Vec3> = bushes.iter().map(|t| t.translation).collect();
    let player_pose = player.single().copied().unwrap_or_default();
    let pt = player_pose.translation;
    let dt = time.delta_secs();
    let sneak_rank = originals.as_deref().map_or(0, |originals| {
        crate::creature_actions::player_abilities(&run, originals).sneak
    });
    // Creatures notice the player within `creatureBasePerception` (10), less
    // while stealthed.
    let perception = crate::creature_rules::BASE_PERCEPTION;
    let acquisition_radius = mobility.as_deref().map_or(perception, |mobility| {
        mobility.npc_acquisition_radius(perception, sneak_rank)
    });
    // Fright: attacking neutrals scatter them. An attack verb (1-4 in the
    // attack stance) is the attack; F only targets now.
    let scare = encounter.mode == crate::creature_actions::EncounterMode::Combat
        && keys.any_just_pressed([
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
        ]);
    use rand::{Rng, SeedableRng};
    // Deterministic-per-frame rng for retargeting (visual only).
    let mut rng = ChaCha8Rng::seed_from_u64((time.elapsed_secs() * 60.0) as u64 + 9137);
    let combat_target = if encounter.mode == crate::creature_actions::EncounterMode::Combat {
        encounter.target.and_then(|target| {
            npcs.iter()
                .find(|(_, npc, _, _, mind, id, _, _)| {
                    id.0 == target && npc.hp > 0. && mind.is_some_and(|m| !m.allied && !m.defeated)
                })
                .map(|(t, _, _, _, _, _, _, _)| t.translation)
        })
    } else {
        None
    };
    // The creature that hit the avatar, for the pack members defending it
    // (creature_support, retail 0xD8F1C0).
    let defend_target = encounter
        .defend
        .filter(|_| combat_target.is_none() && encounter.defend_time > 0.0)
        .and_then(|target| {
            npcs.iter()
                .find(|(_, npc, _, _, mind, id, _, _)| {
                    id.0 == target && npc.hp > 0. && mind.is_some_and(|m| !m.allied && !m.defeated)
                })
                .map(|(t, _, _, _, _, _, _, _)| t.translation)
        });
    let player_forward = player_pose.rotation * Vec3::NEG_Z;
    for (mut t, npc, wander, home, mind, id, ecology, herd) in &mut npcs {
        if mind.is_some_and(|m| m.defeated || m.downed > 0.0 || m.stunned > 0.0)
            || ecology.is_some_and(|ecology| ecology.steer.is_some())
        {
            continue;
        }
        // The herd's daily schedule (notes C4C): asleep members stay home and
        // do not notice the player, eaters go to food, drinkers to the shore.
        let activity = crate::creature_ecology::member_activity(&run.species_world, herd);
        let asleep = activity == crate::creature_ecology::Activity::Sleep;
        let facing = t.rotation * Vec3::NEG_Z;
        // Each branch moves the member and reports where it wants to face:
        // its travel direction, or something it is attending to, with the
        // turn rate to use. Travel speed waits for the body to come round.
        let heading: Option<(Vec3, f32)> = 'step: {
            if let Some(slot) = run
                .creature_pack
                .members
                .iter()
                .take(crate::creature_pack::capacity(run.earned_creature_dna()))
                .position(|member| *member == id.0)
                .filter(|_| mind.is_some_and(|m| m.allied) && npc.hp > 0.)
            {
                let offset = match slot {
                    0 => Vec3::new(-1.7, 0., 3.),
                    1 => Vec3::new(1.7, 0., 3.),
                    _ => Vec3::new(0., 0., 4.8),
                };
                let combat_target =
                    combat_target.or(defend_target.filter(|_| encounter.defenders.contains(&id.0)));
                let target = match run.creature_pack.stance {
                    crate::creature_pack::PackStance::Stay => {
                        let base = run.creature_pack.stay_position.map(Vec3::from_array).unwrap_or(pt);
                        base + offset
                    }
                    crate::creature_pack::PackStance::Attack => {
                        combat_target.unwrap_or(pt + player_pose.rotation * offset)
                    }
                    crate::creature_pack::PackStance::Follow => {
                        combat_target.unwrap_or(pt + player_pose.rotation * offset)
                    }
                };
                let delta = Vec3::new(target.x - t.translation.x, 0., target.z - t.translation.z);
                let distance = delta.length();
                if distance > 0.4 {
                    let direction = delta / distance;
                    let pace = stride_alignment(facing, direction);
                    let to = t.translation + direction * (npc.speed * pace * dt).min(distance - 0.4);
                    t.translation = land.step(t.translation, to);
                    break 'step Some((direction, NPC_TURN_RATE));
                }
                // In formation: face the fight, or the way the leader faces (or outwards if staying).
                let look = if run.creature_pack.stance == crate::creature_pack::PackStance::Stay {
                    offset
                } else {
                    combat_target.map_or(player_forward, |c| c - t.translation)
                };
                break 'step Some((look, NPC_GLANCE_RATE));
            }
            let to_p = Vec3::new(pt.x - t.translation.x, 0., pt.z - t.translation.z);
            if let Some(slot) = mind.and_then(|m| m.slot_at) {
                // A herd mate answering a group social (creature_group,
                // 0xDA3AF0): walk to its slot behind the proposer at standard
                // speed, then face the player with it.
                let delta = Vec3::new(slot[0] - t.translation.x, 0., slot[1] - t.translation.z);
                let distance = delta.length();
                if distance > crate::creature_rules::SOCIAL_GROUP_SLOT_STOP {
                    let direction = delta / distance;
                    let pace = stride_alignment(facing, direction);
                    let to = t.translation
                        + direction
                            * (npc.speed * pace * dt).min(distance - crate::creature_rules::SOCIAL_GROUP_SLOT_STOP * 0.5);
                    t.translation = land.step(t.translation, to);
                    break 'step Some((direction, NPC_TURN_RATE));
                }
                break 'step Some((to_p, NPC_TURN_RATE));
            }
            if mind.is_some_and(|m| m.response_time > 0. || m.gather > 0.) {
                // Mid-encounter (or calling its herd for a group social):
                // hold still and watch the player.
                break 'step Some((to_p, NPC_TURN_RATE));
            }
            if npc.hostile && !asleep && to_p.length() < acquisition_radius {
                let direction = to_p.normalize_or_zero();
                let pace = stride_alignment(facing, direction);
                let to = t.translation + direction * (npc.speed * 1.4 * pace * dt).min(to_p.length());
                t.translation = land.step(t.translation, to);
                break 'step Some((to_p, NPC_TURN_RATE));
            }
            let Some(mut w) = wander else {
                // Idle bob for brainless NPCs.
                t.translation.y = 0.4 + (time.elapsed_secs() * 2.0 + t.translation.x).sin() * 0.08;
                break 'step None;
            };
            // Attacking nearby neutrals spooks them into fleeing.
            if scare && !npc.hostile && !mind.is_some_and(|m| m.allied) {
                if let Some((fx, fz)) = flee_target(t.translation, pt, f32::INFINITY) {
                    let away = land.land_toward(t.translation.xz(), Vec2::new(fx, fz));
                    w.tx = away.x;
                    w.tz = away.y;
                    w.wait = 0.0;
                }
            }
            let toward = Vec3::new(w.tx - t.translation.x, 0., w.tz - t.translation.z);
            let walking = w.wait <= 0.0 && toward.length() >= 0.6;
            let pace = if walking {
                stride_alignment(facing, toward)
            } else {
                1.0
            };
            let (nx, nz, wait, arrived) = wander_advance(
                t.translation.x,
                t.translation.z,
                w.tx,
                w.tz,
                w.wait,
                dt,
                npc.speed * 0.6 * pace,
            );
            // Herds stay on land (the island clamped them to its square); a
            // goal the shore blocks is given up.
            let next = land.step(t.translation, Vec3::new(nx, 0.4, nz));
            if walking && next.xz() == t.translation.xz() && Vec2::new(nx, nz) != t.translation.xz() {
                w.tx = t.translation.x;
                w.tz = t.translation.z;
            }
            t.translation = next;
            t.translation.y = 0.4;
            w.wait = wait;
            if arrived {
                // Retarget inside this member's own nest, so a species stays near
                // the home it belongs to instead of roaming the whole map.
                match home {
                    Some(home) if herd.is_some_and(|h| h.guard) && activity == crate::creature_ecology::Activity::Idle => {
                        // Guard posts (creature_support::guard_post).
                        let herd = herd.unwrap();
                        let (post, wait) = crate::creature_support::guard_post(
                            herd.archetype,
                            Vec2::new(home.x, home.z),
                            herd.nest,
                            [rng.random_range(0.0..1.0), rng.random_range(0.0..1.0), rng.random_range(0.0..1.0)],
                        );
                        let goal = land.land_toward(Vec2::new(home.x, home.z), post);
                        w.tx = goal.x;
                        w.tz = goal.y;
                        w.wait = wait;
                        break 'step if !npc.hostile && to_p.length() < NPC_CURIOUS_RANGE {
                            Some((to_p, NPC_GLANCE_RATE))
                        } else {
                            None
                        };
                    }
                    Some(home) if herd.is_some() => {
                        let (tx, tz, wait) = schedule_goal(
                            activity,
                            home,
                            herd.is_some_and(|h| h.archetype.record().diet != crate::species::Diet::Carnivore),
                            &bush_spots,
                            rng.random_range(0.0..1.0),
                            &land,
                        );
                        w.tx = tx;
                        w.tz = tz;
                        w.wait = wait;
                        break 'step if !npc.hostile && !asleep && to_p.length() < NPC_CURIOUS_RANGE {
                            Some((to_p, NPC_GLANCE_RATE))
                        } else {
                            None
                        };
                    }
                    Some(home) => {
                        let (tx, tz) = nest_wander_target(
                            home.x,
                            home.z,
                            home.radius,
                            rng.random_range(0.0..1.0),
                            f32::INFINITY,
                        );
                        let goal = land.land_toward(Vec2::new(home.x, home.z), Vec2::new(tx, tz));
                        w.tx = goal.x;
                        w.tz = goal.y;
                    }
                    None => {
                        // ADAPTED: a member without a nest strolls up to
                        // `NEST_ROAM_RADIUS` from where it stands.
                        let here = t.translation.xz();
                        let angle = rng.random_range(0.0..std::f32::consts::TAU);
                        let reach = rng.random_range(2.0..NEST_ROAM_RADIUS.max(3.0));
                        let goal = land.land_toward(here, here + Vec2::new(angle.cos(), angle.sin()) * reach);
                        w.tx = goal.x;
                        w.tz = goal.y;
                    }
                }
                w.wait = rng.random_range(1.0..3.0);
            }
            if walking {
                Some((toward, NPC_TURN_RATE))
            } else if !npc.hostile && !asleep && to_p.length() < NPC_CURIOUS_RANGE {
                // A resting member keeps an eye on a nearby player.
                Some((to_p, NPC_GLANCE_RATE))
            } else {
                None
            }
        };
        if let Some((direction, rate)) = heading {
            if Vec3::new(direction.x, 0., direction.z).length_squared() > 1e-6 {
                let yaw = t.rotation.to_euler(EulerRot::YXZ).0;
                let yaw = turn_toward(yaw, facing_yaw(direction), rate, NPC_MAX_TURN_SPEED, dt);
                t.rotation = Quat::from_rotation_y(yaw);
            }
        }
    }
}

/// ADAPTED: how far from its nest a herd looks for water to drink.
const DRINK_REACH: f32 = 120.0;

/// Pure: a herd member's next wander goal for its schedule's activity, and how
/// long it then waits there (notes C4C; the goals are ADAPTED, the retail
/// planner was not traced): sleepers settle at the nest, herbivores eat at the
/// nearest fruit bush of their territory, drinkers walk to the planet's
/// nearest shore (within `DRINK_REACH`), and idle members roam the territory.
/// Every goal is on land.
pub fn schedule_goal(
    activity: crate::creature_ecology::Activity,
    home: &NestHome,
    herbivore: bool,
    bushes: &[Vec3],
    roll: f32,
    land: &crate::creature_planet::Land,
) -> (f32, f32, f32) {
    let (x, z, wait) = schedule_target(activity, home, herbivore, bushes, roll, land);
    let at = land.land_toward(Vec2::new(home.x, home.z), Vec2::new(x, z));
    (at.x, at.y, wait)
}

fn schedule_target(
    activity: crate::creature_ecology::Activity,
    home: &NestHome,
    herbivore: bool,
    bushes: &[Vec3],
    roll: f32,
    land: &crate::creature_planet::Land,
) -> (f32, f32, f32) {
    use crate::creature_ecology::Activity;
    // No island edge on the planet: goals stay ashore via `schedule_goal`.
    const UNBOUNDED: f32 = f32::INFINITY;
    let roll = roll.rem_euclid(1.0);
    match activity {
        Activity::Sleep => {
            let (x, z) = nest_wander_target(home.x, home.z, home.radius * 0.25, roll, UNBOUNDED);
            (x, z, 8.0 + 4.0 * roll)
        }
        Activity::Eat if herbivore => {
            let nest = Vec3::new(home.x, 0.0, home.z);
            match bushes
                .iter()
                .filter(|b| dist_xz(**b, nest) <= home.radius * 1.5)
                .min_by(|a, b| dist_xz(**a, nest).total_cmp(&dist_xz(**b, nest)))
            {
                Some(bush) => {
                    let angle = roll * std::f32::consts::TAU;
                    (
                        bush.x + angle.cos() * 1.2,
                        bush.z + angle.sin() * 1.2,
                        3.0 + 2.0 * roll,
                    )
                }
                None => {
                    let (x, z) = nest_wander_target(home.x, home.z, home.radius, roll, UNBOUNDED);
                    (x, z, 1.0 + 2.0 * roll)
                }
            }
        }
        Activity::Drink => match land.shore(Vec2::new(home.x, home.z), DRINK_REACH, roll) {
            // At the waterline of the nearest shore.
            Some(shore) => (shore.x, shore.y, 3.0 + 2.0 * roll),
            None => {
                let (x, z) = nest_wander_target(home.x, home.z, home.radius, roll, UNBOUNDED);
                (x, z, 1.0 + 2.0 * roll)
            }
        },
        _ => {
            let (x, z) = nest_wander_target(home.x, home.z, home.radius, roll, UNBOUNDED);
            (x, z, 1.0 + 2.0 * roll)
        }
    }
}

/// Where the player starts this visit: the checkpoint's position when it was
/// saved in the Creature stage, else the home nest.
fn start_position(run: &RunProgress) -> Vec2 {
    run.checkpoint
        .as_ref()
        .filter(|c| c.stage == run.stage_order_index)
        .map(|c| Vec2::new(c.player.position[0], c.player.position[2]))
        .filter(|p| p.is_finite())
        .unwrap_or_else(|| {
            let home = run.creature_lifecycle.home;
            Vec2::new(home[0], home[2])
        })
}

/// Whether the casting manager wants herd `index` active for a player at
/// `player`: within the activation distance (or, already active, within the
/// deactivation distance); the night predators only at night; an extinct
/// species never activates again.
fn herd_wanted(
    world: &crate::species::SpeciesWorld,
    index: usize,
    player: Vec3,
    night: bool,
    active: bool,
) -> bool {
    let Some(nest) = world.nests.get(index) else {
        return false;
    };
    let night_predator = world.archetype_of(nest.species)
        == Some(crate::creature_ecology::Archetype::NightPredator);
    if night_predator {
        return night && !world.species_extinct(nest.species);
    }
    let (near, far) = crate::creature_ecology::casting_thresholds();
    // Retail distance: the chord between the avatar and the nest.
    let distance = player.distance(Vec3::from_array(nest.position));
    if active {
        distance <= far
    } else {
        distance <= near && !world.species_extinct(nest.species)
    }
}

/// Spawn the living members of nest `index` that are not present yet, while
/// the animal pool (`CastingManager_MaxAnimalPoolSize`) has room. Returns how
/// many were spawned.
#[allow(clippy::too_many_arguments)]
fn activate_herd(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    originals: &crate::Originals,
    world: &crate::species::SpeciesWorld,
    pack: &crate::creature_pack::CreaturePack,
    index: usize,
    existing: &mut HashSet<u32>,
    living: usize,
) -> usize {
    let Some(nest) = world.nests.get(index) else {
        return 0;
    };
    if world.species_extinct(nest.species) {
        return 0;
    }
    let mut spawned = 0;
    for member in &nest.members {
        if living + spawned >= crate::creature_ecology::CASTING_MAX_ANIMAL_POOL {
            break;
        }
        if !member.relationship.is_alive() || existing.contains(&member.id) {
            continue;
        }
        if spawn_species_member(commands, meshes, materials, originals, world, pack, member.id, living + spawned)
            .is_some()
        {
            existing.insert(member.id);
            spawned += 1;
        }
    }
    spawned
}

/// NPC herd evolution (notes C4B, `dnaPointsEvolutionThresholds`): when the
/// player's stage total passes a threshold of a herd's archetype (starter
/// herds 50 / 200, small 150 / 450, medium 400 / 650; large herds, night
/// predators and roamers never), the species evolves. ADAPTED: retail swaps in
/// an evolved species profile (`mEvolvedSpeciesProfileKeys`); OpenSpore shows
/// another installed creature of the pool (`Species::visual`), with that
/// creature's native abilities, for members spawned from then on.
fn evolve_herds(
    run: &mut RunProgress,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    let total = run.earned_creature_dna();
    let mut changed = false;
    for species in run.species_world.species.iter_mut() {
        let Some(archetype) = species.archetype else {
            continue;
        };
        let stage = crate::creature_ecology::evolution_stage(archetype, total);
        if stage <= species.evolution {
            continue;
        }
        species.evolution = stage;
        changed = true;
        let Some(key) = crate::baked_model::species_key(originals, species.visual()) else {
            continue;
        };
        if let Err(e) = crate::baked_model::ensure(originals, key, meshes, images, materials) {
            warn!("evolved creature {key:?}: {e}");
        } else if !originals.original_species.contains_key(&key) {
            let native =
                crate::original_species::resolve(&originals.resource_index, &originals.part_catalog, key);
            originals.original_species.insert(key, native);
        }
    }
    if changed {
        info!("NPC herds evolved at {total:.0} DNA");
        run.request_save();
    }
}

/// The casting manager for retail herds (CreatureGameTuning
/// `CastingManager_*`): every `UpdateTime` (1 s) herds within the activation
/// distance of the player spawn (at most two per interval, nearest first) and
/// herds beyond the deactivation distance leave (their idle wild members are
/// despawned; pack members, the current target and creatures mid-performance
/// stay). Lost members hatch again after the archetype's `RespawnRate`, the
/// night predators come at dusk near the player and leave at dawn.
#[allow(clippy::too_many_arguments)]
fn cast_retail_herds(
    dt: f32,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    npcs: &Query<
        (
            Entity,
            &crate::persistence::WorldId,
            &crate::creature_actions::CreatureMind,
        ),
        With<Npc>,
    >,
    run: &mut RunProgress,
    originals: &crate::Originals,
    player: Option<Vec3>,
    target: Option<u32>,
    casting: &mut crate::creature_ecology::CastingState,
    land: &crate::creature_planet::Land,
) {
    casting.timer += dt;
    if casting.timer < crate::creature_ecology::CASTING_UPDATE_TIME {
        return;
    }
    let interval = casting.timer;
    casting.timer = 0.0;
    let Some(player) = player else {
        return;
    };
    // The NightPredators event (21:00 to 03:00): the herd gathers near the player (0xD4F290).
    let night = crate::creature_ecology::night_predators_out(run.species_world.time_of_day);
    if night && !casting.night {
        run.species_world.nights += 1;
        let seed = run.species_world.seed ^ run.species_world.nights as u64;
        let at = crate::creature_ecology::ambush_point([player.x, player.z], seed, land);
        let at = land.world_point(Vec2::from(at)).to_array();
        let world = &mut run.species_world;
        let predators: Vec<u32> = world
            .species
            .iter()
            .filter(|s| s.archetype == Some(crate::creature_ecology::Archetype::NightPredator))
            .map(|s| s.id)
            .collect();
        for nest in world.nests.iter_mut().filter(|n| predators.contains(&n.species)) {
            nest.position = at;
        }
    }
    casting.night = night;
    // Retail herds replace lost members by eggs: the herd update (0xC6DE0E) asks
    // for `min(shortfall, maxNumEggs) - eggs` of them once `RespawnRate` has
    // run, so members still in an egg (`creature_eggs`) count against the wave.
    let mut eggs_by_nest: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for id in &casting.incubating {
        if let Some(index) = run.species_world.nest_index_for_member(*id) {
            *eggs_by_nest.entry(index).or_default() += 1;
        }
    }
    let born = run.species_world.tick_respawn_eggs(
        interval,
        |index| eggs_by_nest.get(&index).copied().unwrap_or(0),
        crate::creature_eggs::MAX_EGGS,
    );
    let world = run.species_world.clone();
    let pack = run.creature_pack.clone();
    let mut existing: HashSet<u32> = HashSet::new();
    // A member that is still in its egg does not exist as a creature yet.
    existing.extend(casting.incubating.iter().copied());
    let mut living = 0usize;
    for (_, id, mind) in npcs.iter() {
        existing.insert(id.0);
        if !mind.defeated {
            living += 1;
        }
    }
    // Deactivate, then activate (nearest first, two per interval).
    let mut entries: Vec<(usize, bool, f32)> = Vec::new();
    for index in 0..world.nests.len() {
        let active = casting.active.contains(&index);
        let wanted = herd_wanted(&world, index, player, night, active);
        let nest = &world.nests[index];
        let distance = player.distance(Vec3::from_array(nest.position));
        entries.push((index, active, if wanted { distance } else { f32::INFINITY }));
    }
    for (index, active, distance) in &entries {
        if !*active || distance.is_finite() {
            continue;
        }
        casting.active.remove(index);
        let nest = &world.nests[*index];
        for (entity, id, mind) in npcs.iter() {
            if nest.member(id.0).is_none()
                || pack.members.contains(&id.0)
                || target == Some(id.0)
                || mind.response_time > 0.0
                || mind.gather > 0.0
                || mind.joining.is_some()
            {
                continue;
            }
            commands.entity(entity).despawn();
            existing.remove(&id.0);
            if !mind.defeated {
                living = living.saturating_sub(1);
            }
        }
    }
    let mut candidates: Vec<(usize, f32)> = entries
        .iter()
        .filter(|(_, active, distance)| !*active && distance.is_finite())
        .map(|(index, _, distance)| (*index, *distance))
        .collect();
    candidates.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    for (index, _) in candidates
        .into_iter()
        .take(crate::creature_ecology::CASTING_MAX_HERDS_PER_INTERVAL)
    {
        casting.active.insert(index);
        living += activate_herd(commands, meshes, materials, originals, &world, &pack, index, &mut existing, living);
    }
    // The replacements of active herds are laid as eggs (0xD6CFE3 `CreateEgg`)
    // and hatch into babies after `egg_hatch_time`; those of herds that are
    // not active appear with their herd (ADAPTED: retail does not update them).
    for id in born {
        let Some(index) = world.nest_index_for_member(id) else {
            continue;
        };
        if casting.active.contains(&index) {
            crate::creature_eggs::lay(casting, run, index, id);
            existing.insert(id);
        }
    }
    // Active herds refill members that are missing (a reload, a despawned
    // straggler) the way the legacy respawn did.
    for index in casting.active.iter().copied().collect::<Vec<_>>() {
        living += activate_herd(commands, meshes, materials, originals, &world, &pack, index, &mut existing, living);
    }
}

/// Replace only what is missing: a defeated member stays gone, and a species
/// that is extinct never returns. Wild members that wandered out of range or
/// were never spawned are recreated from their saved build. Retail worlds use
/// the casting manager instead (`cast_retail_herds`).
#[allow(clippy::too_many_arguments)]
pub fn creature_respawn_nests(
    time: Res<Time>,
    mut timer: ResMut<CreatureSpawnTimer>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    all_npcs: Query<
        (
            Entity,
            &crate::persistence::WorldId,
            &crate::creature_actions::CreatureMind,
        ),
        With<Npc>,
    >,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut images: ResMut<Assets<Image>>,
    player: Query<&Transform, With<Player>>,
    encounter: Option<Res<crate::creature_actions::Encounter>>,
    casting: Option<ResMut<crate::creature_ecology::CastingState>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
) {
    if run.species_world.is_retail() {
        let Some(mut casting) = casting else {
            commands.insert_resource(crate::creature_ecology::CastingState::default());
            return;
        };
        evolve_herds(&mut run, &mut originals, &mut meshes, &mut images, &mut materials);
        let player = player.single().ok().map(|t| t.translation);
        let land = crate::creature_planet::Land::from_surface(surface.as_deref());
        cast_retail_herds(
            time.delta_secs(),
            &mut commands,
            &mut meshes,
            &mut materials,
            &all_npcs,
            &mut run,
            &originals,
            player,
            encounter.and_then(|e| e.target),
            &mut casting,
            &land,
        );
        return;
    }
    timer.0.tick(time.delta());
    if !timer.0.just_finished() {
        return;
    }
    let world = &run.species_world;
    // Hidden defeated NPCs remain as persistence tombstones. Track every
    // present id to prevent duplicate entities, but count only unique living
    // ids against the active population budget.
    let mut existing_ids = HashSet::new();
    let mut living_ids = HashSet::new();
    for (_, id, mind) in &all_npcs {
        existing_ids.insert(id.0);
        if !mind.defeated {
            living_ids.insert(id.0);
        }
    }
    if living_ids.len() >= MAX_LIVE_MEMBERS {
        return;
    }
    for entry in world.spawn_list() {
        if living_ids.len() >= MAX_LIVE_MEMBERS {
            return;
        }
        if existing_ids.contains(&entry.member) {
            continue;
        }
        // A recorded outcome is final: no dead or extinct member respawns.
        if !world
            .relationship(entry.member)
            .is_some_and(|r| r.is_alive())
        {
            continue;
        }
        if world.species_extinct(entry.species) {
            continue;
        }
        if spawn_species_member(
            &mut commands,
            &mut meshes,
            &mut materials,
            &originals,
            world,
            &run.creature_pack,
            entry.member,
            living_ids.len(),
        )
        .is_some()
        {
            existing_ids.insert(entry.member);
            living_ids.insert(entry.member);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stage-level app: the real schedule, no window, no install.
    fn stage_app(run: RunProgress) -> App {
        crate::runtime::headless_app(run)
    }

    fn enter(app: &mut App, stage: GameStage) {
        app.world_mut()
            .resource_mut::<RunProgress>()
            .enter_stage(stage.clone());
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(stage);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }

    #[test]
    fn grazing_refills_hunger_but_cannot_earn_dna_or_skip_creature_stage() {
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(0.5));
        let mut run = RunProgress::default();
        run.stage_order_index = 1;
        run.dna_this_stage = crate::creature_rules::STAGE_TOTAL_DNA - 1.0;
        run.editor_dna = run.dna_this_stage;
        let mut app = App::new();
        app.insert_resource(time)
            .insert_resource(crate::Originals::default())
            .insert_resource(run)
            .add_systems(Update, creature_graze);
        app.world_mut().spawn((
            Player,
            Transform::default(),
            Hunger {
                food: 0.0,
                max: 100.0,
                hold: 0.0,
            },
            Health { hp: 5.0, max: 10.0 },
        ));
        let bush = app
            .world_mut()
            .spawn((Transform::default(), Bush { food_left: 60.0 }))
            .id();

        app.update();

        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.dna_this_stage, 999.0);
        assert_eq!(run.dna_banked_total, 0.0);
        assert_eq!(run.editor_dna, 999.0);
        assert_eq!(run.stage_order_index, 1, "fruit cannot advance the stage");
        assert!(
            run.diet_herbivore > 0.0,
            "diet history still records grazing"
        );
        let player = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(app.world())
            .unwrap();
        // Retail fruit: 25 hunger and 0.25 HP per fruit; half a second of
        // eating at one fruit per second is half a fruit.
        let hunger = app.world().get::<Hunger>(player).unwrap().food;
        assert!((hunger - 12.5).abs() < 1e-3, "{hunger}");
        let hp = app.world().get::<Health>(player).unwrap().hp;
        assert!((hp - 5.125).abs() < 1e-3, "{hp}");
        assert!(app.world().get::<Bush>(bush).unwrap().food_left < 60.0);
    }

    #[test]
    fn the_avatar_follows_the_retail_health_speed_and_size_tables() {
        let originals = crate::Originals::default();
        let mut run = RunProgress::default();
        // Brain level 0: the level-0 health entry, speed gear 0, base size.
        let level0 = avatar_stats(&run, &originals);
        assert_eq!(level0.max_hp, 10.0);
        assert_eq!(level0.speed, 2.5);
        assert!((level0.size - stats_for(&run, &originals).size).abs() < 1e-6);
        // The stage total picks the level: 600 is level 3 (HP 40, size x1.45).
        run.dna_this_stage = 600.0;
        let level3 = avatar_stats(&run, &originals);
        assert_eq!(level3.max_hp, 40.0);
        assert!((level3.size / level0.size - 1.45).abs() < 1e-5);
        run.dna_this_stage = 1000.0;
        assert_eq!(avatar_stats(&run, &originals).max_hp, 100.0);
        // The lineage summary that later stages inherit is untouched.
        assert!(stats_for(&run, &originals).max_hp >= 120.0);
    }

    #[test]
    fn creature_grazing_requires_near_ground_contact() {
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(0.5));
        let mut app = App::new();
        app.insert_resource(time)
            .insert_resource(crate::Originals::default())
            .insert_resource(RunProgress::default())
            .insert_resource(mobility::CreatureMobility {
                height: 0.5,
                ..default()
            })
            .add_systems(Update, creature_graze);
        app.world_mut().spawn((
            Player,
            Transform::default(),
            Hunger {
                food: 0.0,
                max: 100.0,
                hold: 0.0,
            },
        ));
        let bush = app
            .world_mut()
            .spawn((Transform::default(), Bush { food_left: 60.0 }))
            .id();

        app.update();

        assert_eq!(app.world().get::<Bush>(bush).unwrap().food_left, 60.0);
        let player = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(app.world())
            .unwrap();
        assert_eq!(app.world().get::<Hunger>(player).unwrap().food, 0.0);
    }

    #[test]
    fn legacy_wander_skips_downed_and_socially_steered_members() {
        for (downed, steer) in [(0.0, Some((Vec3::X, 1.0))), (1.0, None)] {
            let mut app = App::new();
            let mut time = Time::<()>::default();
            time.advance_by(std::time::Duration::from_secs_f32(0.1));
            app.insert_resource(time)
                .insert_resource(ButtonInput::<KeyCode>::default())
                .insert_resource(RunProgress::default())
                .insert_resource(crate::creature_actions::Encounter::default())
                .add_systems(Update, creature_wander);
            let start = Transform::from_xyz(2.0, 7.0, 3.0);
            let member = app
                .world_mut()
                .spawn((
                    start,
                    Npc {
                        speed: 5.0,
                        hostile: false,
                        hp: 50.0,
                    },
                    Wander {
                        tx: 10.0,
                        tz: 10.0,
                        wait: 0.0,
                    },
                    crate::creature_actions::CreatureMind {
                        downed,
                        ..default()
                    },
                    crate::persistence::WorldId(300),
                    crate::creature_social::Ecology { steer, ..default() },
                ))
                .id();

            app.update();
            assert_eq!(
                app.world().get::<Transform>(member).unwrap(),
                &start,
                "downed or ecology-steered NPC must not be changed by legacy wander"
            );
        }
    }

    fn creature_members(app: &mut App) -> Vec<u32> {
        let mut query = app.world_mut().query::<&crate::persistence::WorldId>();
        let mut out: Vec<u32> = query
            .iter(app.world())
            .map(|id| id.0)
            .filter(|id| (crate::species::MEMBER_ID_BASE..1200).contains(id))
            .collect();
        out.sort_unstable();
        out
    }

    /// Updates needed for the respawn timer (a few seconds) to elapse.
    const RESPAWN_TICKS: usize = 300;

    /// Ids of members with a live (not defeated) mind.
    fn living_members(app: &mut App) -> Vec<u32> {
        let mut query = app.world_mut().query::<(
            &crate::persistence::WorldId,
            &crate::creature_actions::CreatureMind,
        )>();
        let mut out: Vec<u32> = query
            .iter(app.world())
            .filter(|(_, m)| !m.defeated)
            .map(|(id, _)| id.0)
            .filter(|id| (crate::species::MEMBER_ID_BASE..1200).contains(id))
            .collect();
        out.sort_unstable();
        out
    }
    #[test]
    fn creature_stage_spawns_species_with_stable_member_ids() {
        let mut app = stage_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        // Read the ecology out before touching the world again for ids.
        let (generated, species_count, bodies, approachable) = {
            let world = app.world().resource::<RunProgress>();
            (
                world.species_world.generated,
                world.species_world.species.len(),
                world
                    .species_world
                    .species
                    .iter()
                    .map(|s| s.body.clone())
                    .collect::<Vec<_>>(),
                world.species_world.species.iter().any(|s| !s.hostile),
            )
        };
        assert!(generated);
        assert!(species_count >= 3);
        // Every species body is its own shape, independent of the player's
        // build, so no two species (nor the player) share a silhouette.
        for i in 0..bodies.len() {
            for j in (i + 1)..bodies.len() {
                assert_ne!(bodies[i], bodies[j], "species {} and {} share a body", i, j);
            }
            assert_ne!(bodies[i], RunProgress::default().body);
        }
        // Members keep their persistent ids, starting at 300.
        let members = creature_members(&mut app);
        assert!(!members.is_empty());
        assert!(members.contains(&crate::species::MEMBER_ID_BASE));
        // There is at least one approachable (non-hostile) species.
        assert!(approachable);
    }

    #[test]
    fn headless_members_start_at_the_level_zero_health_entry() {
        let mut app = stage_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        let mut ids = app
            .world_mut()
            .query::<(&crate::persistence::WorldId, &Npc)>();
        let run = app.world().resource::<RunProgress>().clone();
        let originals = crate::Originals::default();
        for (id, npc) in ids.iter(app.world()) {
            if (crate::species::MEMBER_ID_BASE..crate::species::ROAMER_ID_BASE).contains(&id.0) {
                // Retail herds: the archetype's HitpointOverride with the role
                // (the starter herds' adults have the health table's 10 HP).
                let max = member_max_hp(&run.species_world, id.0, &originals).unwrap();
                assert_eq!(npc.hp, max, "member {} starts at full HP", id.0);
                if run.species_world.archetype_for(id.0) == Some(crate::creature_ecology::Archetype::Social)
                    && run.species_world.member_role(id.0) == crate::creature_ecology::MemberRole::Adult
                {
                    assert_eq!(npc.hp, 10.0);
                }
            }
        }
    }

    #[test]
    fn defeated_members_are_never_respawned() {
        let mut app = stage_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        let victim = crate::species::MEMBER_ID_BASE;
        app.world_mut()
            .resource_mut::<RunProgress>()
            .species_world
            .record_defeat(victim);
        // Drop the entity, the way a reload after a defeat leaves the world.
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<Npc>>()
            .iter(app.world())
            .find(|e| {
                app.world()
                    .entity(*e)
                    .get::<crate::persistence::WorldId>()
                    .is_some_and(|id| id.0 == victim)
            })
            .expect("member entity");
        app.world_mut().entity_mut(entity).despawn();
        // Several respawn ticks, which must all skip the recorded kill.
        for _ in 0..RESPAWN_TICKS {
            app.update();
        }
        assert!(
            !creature_members(&mut app).contains(&victim),
            "a defeated member must never come back"
        );
    }

    #[test]
    fn a_befriended_member_that_is_missing_comes_back_as_an_ally() {
        let mut app = stage_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        let victim = crate::species::MEMBER_ID_BASE;
        // Record the membership the way an encounter would: exactly one call.
        let world = app.world_mut();
        world
            .resource_mut::<RunProgress>()
            .species_world
            .record_befriend(victim);
        assert_eq!(
            world
                .resource::<RunProgress>()
                .species_world
                .relationship(victim),
            Some(crate::species::Relationship::Allied)
        );
        // A second record must not change anything (exactly once).
        assert_eq!(
            world
                .resource_mut::<RunProgress>()
                .species_world
                .record_befriend(victim),
            RecordResult::Already
        );
        let entity = world
            .query_filtered::<Entity, With<Npc>>()
            .iter(&world)
            .find(|e| {
                world
                    .entity(*e)
                    .get::<crate::persistence::WorldId>()
                    .is_some_and(|id| id.0 == victim)
            })
            .expect("member entity");
        world.entity_mut(entity).despawn();
        // The respawn timer needs a few seconds of game time to elapse.
        for _ in 0..RESPAWN_TICKS {
            app.update();
        }
        assert!(
            creature_members(&mut app).contains(&victim),
            "a living ally must respawn"
        );
        // The respawned member is still an ally, not a stranger.
        let mut minds = app.world_mut().query::<(
            &crate::persistence::WorldId,
            &crate::creature_actions::CreatureMind,
            &Npc,
        )>();
        let (mind, npc) = minds
            .iter(app.world())
            .find(|(id, _, _)| id.0 == victim)
            .map(|(_, m, n)| (m.clone(), n.hp))
            .expect("respawned ally");
        assert!(mind.allied, "the ally must still be allied");
        assert!(!mind.defeated);
        let max = member_max_hp(
            &app.world().resource::<RunProgress>().species_world,
            victim,
            &crate::Originals::default(),
        )
        .unwrap();
        assert_eq!(npc, max, "a respawned ally keeps full HP");
        assert_eq!(
            app.world()
                .resource::<RunProgress>()
                .species_world
                .relationship(victim),
            Some(crate::species::Relationship::Allied)
        );
    }

    #[test]
    fn a_defeated_member_that_is_missing_never_returns_hidden() {
        // Mirrors the hidden-defeated checkpoint path: the dead member's
        // entity may still exist (hidden), and its mind must stay defeated.
        let mut app = stage_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        let victim = crate::species::MEMBER_ID_BASE;
        app.world_mut()
            .resource_mut::<RunProgress>()
            .species_world
            .record_defeat(victim);
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<Npc>>()
            .iter(app.world())
            .find(|e| {
                app.world()
                    .entity(*e)
                    .get::<crate::persistence::WorldId>()
                    .is_some_and(|id| id.0 == victim)
            })
            .expect("member entity");
        {
            let mut e = app.world_mut().entity_mut(entity);
            e.get_mut::<crate::creature_actions::CreatureMind>()
                .unwrap()
                .defeated = true;
            e.get_mut::<Visibility>().unwrap();
        }
        for _ in 0..RESPAWN_TICKS {
            app.update();
        }
        // A dead member is either a hidden tombstone or absent; either way it
        // never comes back as a living one.
        assert!(
            !living_members(&mut app).contains(&victim),
            "a defeated member must never respawn"
        );
        if let Some((mind, visibility)) = app
            .world_mut()
            .query::<(
                &crate::persistence::WorldId,
                &crate::creature_actions::CreatureMind,
                &Visibility,
            )>()
            .iter(app.world())
            .find(|(id, _, _)| id.0 == victim)
            .map(|(_, m, v)| (m.clone(), *v))
        {
            assert!(mind.defeated, "a tombstone keeps its defeated mind");
            assert_eq!(visibility, Visibility::Hidden);
        }
        assert_eq!(
            app.world()
                .resource::<RunProgress>()
                .species_world
                .relationship(victim),
            Some(crate::species::Relationship::Defeated)
        );
    }

    #[test]
    fn hidden_defeated_npc_does_not_consume_a_living_spawn_slot() {
        // The legacy population cap (worlds saved before the retail ecology).
        let mut app = stage_app(legacy_run());
        enter(&mut app, GameStage::Creature);
        let initial_ids = creature_members(&mut app);
        assert!(initial_ids.len() >= MAX_LIVE_MEMBERS);

        let victim = initial_ids[0];
        let reserve = {
            let run = app.world().resource::<RunProgress>();
            let present: HashSet<_> = initial_ids.iter().copied().collect();
            run.species_world
                .spawn_list()
                .into_iter()
                .find(|entry| {
                    !present.contains(&entry.member)
                        && run
                            .species_world
                            .relationship(entry.member)
                            .is_some_and(|relationship| relationship.is_alive())
                        && !run.species_world.species_extinct(entry.species)
                })
                .map(|entry| entry.member)
                .expect("a living reserve member exists beyond the initial population cap")
        };

        app.world_mut()
            .resource_mut::<RunProgress>()
            .species_world
            .record_defeat(victim);
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<Npc>>()
            .iter(app.world())
            .find(|entity| {
                app.world()
                    .entity(*entity)
                    .get::<crate::persistence::WorldId>()
                    .is_some_and(|id| id.0 == victim)
            })
            .expect("defeated NPC entity remains as a tombstone");
        {
            let mut entity = app.world_mut().entity_mut(entity);
            entity
                .get_mut::<crate::creature_actions::CreatureMind>()
                .expect("creature mind")
                .defeated = true;
            *entity.get_mut::<Visibility>().expect("visibility") = Visibility::Hidden;
        }
        assert_eq!(living_members(&mut app).len(), initial_ids.len() - 1);

        for _ in 0..RESPAWN_TICKS {
            app.update();
        }

        let living = living_members(&mut app);
        assert_eq!(living.len(), initial_ids.len());
        let all_member_ids = creature_members(&mut app);
        assert_eq!(
            all_member_ids.iter().collect::<HashSet<_>>().len(),
            all_member_ids.len(),
            "respawning must not create a duplicate persistent NPC id"
        );
        assert!(
            living.contains(&reserve),
            "a reserve member fills the freed slot"
        );
        assert!(
            !living.contains(&victim),
            "the defeated tombstone stays defeated"
        );
        assert_eq!(
            app.world()
                .resource::<RunProgress>()
                .species_world
                .relationship(victim),
            Some(crate::species::Relationship::Defeated)
        );
        let defeated_visibility = app
            .world_mut()
            .query::<(
                &crate::persistence::WorldId,
                &crate::creature_actions::CreatureMind,
                &Visibility,
            )>()
            .iter(app.world())
            .find(|(id, _, _)| id.0 == victim)
            .map(|(_, mind, visibility)| (mind.defeated, *visibility));
        assert_eq!(defeated_visibility, Some((true, Visibility::Hidden)));
    }

    /// A run holding a legacy (pre-archetype) species world.
    fn legacy_run() -> RunProgress {
        let mut run = RunProgress::default();
        run.species_world = crate::species::SpeciesWorld::generate_n(
            crate::species::lineage_seed(&run.parts, &run.body),
            &crate::asset_catalog::PartCatalog::default(),
            crate::species::DEFAULT_NEST_RADIUS,
            crate::species::DEFAULT_SPECIES,
        );
        run
    }

    fn place_player(app: &mut App, at: Vec3) {
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Transform, With<Player>>();
        players.single_mut(world).unwrap().translation = at;
    }

    /// Nest indices of the herds with a spawned living member.
    fn spawned_nests(app: &mut App) -> HashSet<usize> {
        let world = app.world().resource::<RunProgress>().species_world.clone();
        living_members(app)
            .into_iter()
            .filter_map(|id| world.nest_index_for_member(id))
            .collect()
    }

    #[test]
    fn retail_herds_activate_near_the_player_and_leave_when_far() {
        let mut app = stage_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        let world = app.world().resource::<RunProgress>().species_world.clone();
        assert!(world.is_retail());
        let (near, far) = crate::creature_ecology::casting_thresholds();
        let home = Vec3::new(crate::creature_ecology::HOME[0], 0.4, crate::creature_ecology::HOME[1]);
        let distance = |index: usize| home.distance(Vec3::from_array(world.nests[index].position));
        let active = spawned_nests(&mut app);
        assert!(active.len() >= 4, "the starter herds around home are active: {active:?}");
        for index in &active {
            assert!(distance(*index) <= near + 1e-3, "nest {index} at {}", distance(*index));
        }
        let expected: HashSet<usize> = (0..world.nests.len())
            .filter(|i| {
                distance(*i) <= near
                    && world.archetype_of(world.nests[*i].species)
                        != Some(crate::creature_ecology::Archetype::NightPredator)
            })
            .collect();
        assert_eq!(active, expected, "every herd within the activation distance spawns");
        assert!(living_members(&mut app).len() <= crate::creature_ecology::CASTING_MAX_ANIMAL_POOL);
        // Walk to the far side: the home herds leave, the far herds come.
        let target = (0..world.nests.len())
            .max_by(|a, b| distance(*a).total_cmp(&distance(*b)))
            .unwrap();
        let p = world.nests[target].position;
        place_player(&mut app, Vec3::new(p[0], 0.7, p[2]));
        for _ in 0..(60 * 8) {
            place_player(&mut app, Vec3::new(p[0], 0.7, p[2]));
            app.update();
        }
        let now = spawned_nests(&mut app);
        assert!(now.contains(&target), "the herd at the player activates");
        let here = Vec2::new(p[0], p[2]);
        for index in &now {
            let q = world.nests[*index].position;
            assert!(here.distance(Vec2::new(q[0], q[2])) <= far + 1e-3, "nest {index} stayed active");
        }
        assert!(now.is_disjoint(&active), "the home herds left");
    }

    #[test]
    fn herd_members_spawn_with_their_archetype_role_stats() {
        let mut app = stage_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        let run = app.world().resource::<RunProgress>().clone();
        let mut query = app.world_mut().query::<(
            &crate::persistence::WorldId,
            &Npc,
            &Transform,
            &crate::creature_actions::CreatureMind,
            &crate::creature_ecology::HerdMember,
        )>();
        let mut alphas = 0;
        for (id, npc, t, mind, herd) in query.iter(app.world()) {
            let role = run.species_world.member_role(id.0);
            assert_eq!(herd.role, role);
            assert_eq!(npc.hp, crate::creature_ecology::member_max_hp(herd.archetype, role));
            assert_eq!(mind.rounds, crate::creature_ecology::social_rounds(herd.archetype, role));
            assert_eq!(npc.hostile, herd.archetype.hostile());
            let base = crate::creature_ecology::species_scale(herd.archetype);
            assert!(t.scale.x >= crate::creature_ecology::member_scale(base, role, 0.0) - 1e-4);
            assert!(t.scale.x <= crate::creature_ecology::member_scale(base, role, 1.0) + 1e-4);
            if role == crate::creature_ecology::MemberRole::Alpha {
                alphas += 1;
                assert_eq!(npc.hp, herd.archetype.record().hit_points * 1.25);
            }
        }
        assert!(alphas > 0, "the starter herds have alphas (0.25 of 4)");
        // The Carcass lies near home as a meal.
        let meat = app
            .world_mut()
            .query::<&crate::creature_social::Meat>()
            .iter(app.world())
            .map(|m| m.food)
            .fold(0.0, f32::max);
        assert_eq!(meat, crate::creature_ecology::CARCASS.food_value);
    }

    #[test]
    fn night_predators_come_at_dusk_and_leave_at_dawn() {
        let mut app = stage_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        let (species, members) = {
            let world = &app.world().resource::<RunProgress>().species_world;
            let species = world
                .species
                .iter()
                .find(|s| s.archetype == Some(crate::creature_ecology::Archetype::NightPredator))
                .unwrap()
                .id;
            let members: Vec<u32> = world.nests_of_species(species)[0].members.iter().map(|m| m.id).collect();
            (species, members)
        };
        assert!(living_members(&mut app).iter().all(|id| !members.contains(id)), "no predators by day");
        app.world_mut().resource_mut::<RunProgress>().species_world.time_of_day =
            crate::creature_ecology::NIGHT_EVENT_START_HOUR - 0.001;
        for _ in 0..150 {
            app.update();
        }
        let living = living_members(&mut app);
        assert!(members.iter().all(|id| living.contains(id)), "the herd arrives at dusk");
        let (nights, nest) = {
            let world = &app.world().resource::<RunProgress>().species_world;
            (world.nights, world.nests_of_species(species)[0].position)
        };
        assert_eq!(nights, 1);
        let home = crate::creature_ecology::HOME;
        let d = Vec2::new(nest[0], nest[2]).distance(Vec2::new(home[0], home[1]));
        assert!(
            d <= crate::creature_ecology::NIGHT_PREDATOR_AMBUSH_DISTANCE * crate::creature_ecology::CREATURE_WORLD_SCALE + 0.5,
            "they gather near the player: {d}"
        );
        app.world_mut().resource_mut::<RunProgress>().species_world.time_of_day =
            crate::creature_ecology::NIGHT_EVENT_END_HOUR + 0.5;
        for _ in 0..150 {
            app.update();
        }
        let living = living_members(&mut app);
        assert!(members.iter().all(|id| !living.contains(id)), "they leave at dawn");
    }

    #[test]
    fn npc_herds_evolve_when_the_player_passes_their_dna_thresholds() {
        let mut app = stage_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 210.0;
        for _ in 0..90 {
            app.update();
        }
        let world = &app.world().resource::<RunProgress>().species_world;
        for species in &world.species {
            let expected = crate::creature_ecology::evolution_stage(species.archetype.unwrap(), 210.0);
            assert_eq!(species.evolution, expected, "{:?}", species.archetype);
        }
        let social = world
            .species
            .iter()
            .find(|s| s.archetype == Some(crate::creature_ecology::Archetype::Social))
            .unwrap();
        assert_eq!(social.evolution, 2, "starter herds evolve at 50 and 200");
        assert_ne!(social.visual(), social.id, "an evolved herd shows another creature");
    }

    #[test]
    fn schedules_send_members_home_to_food_and_to_the_shore() {
        use crate::creature_ecology::Activity;
        let home = NestHome { x: 10.0, z: 2.0, radius: 6.0 };
        let flat = crate::creature_planet::Land::flat();
        let (x, z, wait) = schedule_goal(Activity::Sleep, &home, true, &[], 0.3, &flat);
        assert!(((x - 10.0).powi(2) + (z - 2.0).powi(2)).sqrt() <= 1.5 + 1e-4);
        assert!(wait >= 8.0, "sleepers settle");
        let bush = Vec3::new(14.0, 0.0, 2.0);
        let (x, z, _) = schedule_goal(Activity::Eat, &home, true, &[bush, Vec3::new(-20.0, 0.0, 0.0)], 0.0, &flat);
        assert!(((x - bush.x).powi(2) + (z - bush.z).powi(2)).sqrt() <= 1.3, "herbivores eat at the bush");
        // Drinkers walk to the planet's nearest shore (here east of the nest).
        let land = crate::creature_planet::tests::island_planet(200.0);
        let coast = NestHome { x: 150.0, z: 2.0, radius: 6.0 };
        let (x, z, _) = schedule_goal(Activity::Drink, &coast, true, &[], 0.5, &land);
        assert!(x > 175.0 && land.is_land(Vec2::new(x, z)), "drinkers walk to the nearest shore: {x}");
        let (x, z, wait) = schedule_goal(Activity::Idle, &home, false, &[], 0.7, &flat);
        assert!(((x - 10.0).powi(2) + (z - 2.0).powi(2)).sqrt() <= 6.0 + 1e-3);
        assert!(wait < 3.1);
    }

    #[test]
    fn fruit_stands_in_the_planets_groves_by_the_theme_counts() {
        // No planet: no groves, the island layout stands in.
        assert!(fruit_spots(&crate::creature_planet::Land::flat(), Vec2::ZERO, &[]).is_empty());
        let land = crate::creature_planet::tests::island_planet(450.0);
        let home = Vec2::new(0.0, 8.0);
        let pads = [([0.0, 0.7, 8.0], 8.0)];
        let groves = land.fruit_groves(home, FRUIT_RADIUS, &pads);
        assert!(!groves.is_empty(), "the planet has groves within 200 units of its pole");
        // Only large and medium trees bear fruit (maxSmallTreeFruitPerGrove 0).
        assert!(groves.iter().flatten().all(|t| t.class < 2));
        let spots = fruit_spots(&land, home, &pads);
        assert!(!spots.is_empty() && spots.len() <= MAX_FRUIT_TREES);
        assert!(spots.iter().all(|(at, food)| land.is_land(*at) && *food > 0.0 && food % 25.0 == 0.0));
        // The nearest grove's fruit is exactly its theme caps: 30 on the large
        // trees and 36 on the medium ones, whole fruit of 25 hunger.
        let first = &groves[0];
        let large = first.iter().filter(|t| t.class == 0).count();
        let medium = first.iter().filter(|t| t.class == 1).count();
        let one = fruit_spots_of(&land, std::slice::from_ref(first));
        let total: f32 = one.iter().map(|(_, food)| food).sum();
        let expected = (if large > 0 { 30.0 } else { 0.0 } + if medium > 0 { 36.0 } else { 0.0 }) * 25.0;
        assert!((total - expected).abs() < 1e-3, "{total} vs {expected} ({large} large, {medium} medium)");
        assert_eq!(spots, fruit_spots(&land, home, &pads), "pure");
        // The nest pad is plant-free: no fruit tree stands within 8 units of the home nest.
        assert!(groves.iter().flatten().all(|t| land.chord(home, t.at) > 7.0));
    }

    #[test]
    fn prepare_species_reuses_a_saved_world() {
        let mut run = RunProgress::default();
        let catalog = crate::asset_catalog::PartCatalog::default();
        let world = crate::creature::ensure_species_world(&mut run, &catalog);
        assert!(run.species_world.generated);
        let first = world.spawn_list()[0].member;
        run.species_world.record_befriend(first);
        // A second call must not regenerate behind recorded outcomes.
        let again = crate::creature::ensure_species_world(&mut run, &catalog);
        assert_eq!(again, run.species_world);
        assert_eq!(
            again.relationship(first),
            Some(crate::species::Relationship::Allied)
        );
    }

    #[test]
    fn saved_checkpoint_outcomes_migrate_into_a_generated_world() {
        // A 0.42 save has mind.allied/defeated creatures but an empty species
        // world: those outcomes must survive the new ecology.
        let mut run = RunProgress::default();
        run.checkpoint = Some(crate::persistence::Checkpoint {
            encounter: None,
            stage: 1,
            player: crate::persistence::PlayerCheckpoint {
                position: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                hp: 100.0,
                food: None,
            },
            objects: vec![
                crate::persistence::ObjectCheckpoint {
                    id: 300,
                    position: [0.0, 0.4, 0.0],
                    rotation: None,
                    wander: None,
                    creature: Some(crate::persistence::CreatureCheckpoint {
                        hp: 50.0,
                        hostile: false,
                        mind: crate::creature_actions::CreatureMind {
                            species: 0,
                            allied: true,
                            defeated: false,
                            ..default()
                        },
                    }),
                    food_left: None,
                    spice_left: None,
                    planet: None,
                    hp: None,
                },
                crate::persistence::ObjectCheckpoint {
                    id: 301,
                    position: [0.0, 0.4, 0.0],
                    rotation: None,
                    wander: None,
                    creature: Some(crate::persistence::CreatureCheckpoint {
                        hp: 0.0,
                        hostile: true,
                        mind: crate::creature_actions::CreatureMind {
                            species: 0,
                            allied: false,
                            defeated: true,
                            ..default()
                        },
                    }),
                    food_left: None,
                    spice_left: None,
                    planet: None,
                    hp: None,
                },
            ],
        });
        let catalog = crate::asset_catalog::PartCatalog::default();
        let (allied, defeated) = migrate_checkpoint_outcomes(&mut run, &catalog);
        assert_eq!((allied, defeated), (1, 1));
        // The outcomes are recorded against the member ids, not the DNA.
        let world = &run.species_world;
        assert_eq!(
            world.relationship(300),
            Some(crate::species::Relationship::Allied)
        );
        assert_eq!(
            world.relationship(301),
            Some(crate::species::Relationship::Defeated)
        );
        // Running twice is idempotent and never awards DNA again.
        let again = migrate_checkpoint_outcomes(&mut run, &catalog);
        assert_eq!(again, (0, 0));
        assert_eq!(run.dna_this_stage, 0.0);
    }

    #[test]
    fn saved_mind_species_are_remapped_onto_living_species() {
        let mut run = RunProgress::default();
        let catalog = crate::asset_catalog::PartCatalog::default();
        crate::creature::ensure_species_world(&mut run, &catalog);
        let world = run.species_world.clone();
        // An old ecology references a species id this world does not have; it
        // remaps onto an approachable one so social play still works.
        let remapped = crate::creature::remap_species_id(99, &world);
        assert!(world.species(remapped).is_some());
        assert!(!world.species(remapped).unwrap().hostile);
    }

    #[test]
    fn species_members_roam_their_own_nests() {
        let mut app = stage_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        let world = app.world().resource::<RunProgress>().species_world.clone();
        for _ in 0..40 {
            app.update();
        }
        let mut checked = 0;
        let mut query = app
            .world_mut()
            .query::<(&crate::persistence::WorldId, &Transform, &NestHome)>();
        for (id, transform, home) in query.iter(app.world()) {
            let nest = world.nest_for_member(id.0).expect("nest for member");
            if world.species(nest.species).is_some_and(|s| s.hostile) {
                continue; // carnivore herds chase the player by design
            }
            assert_eq!((home.x, home.z), (nest.position[0], nest.position[2]));
            let distance = ((transform.translation.x - home.x).powi(2)
                + (transform.translation.z - home.z).powi(2))
            .sqrt();
            assert!(
                distance <= home.radius + 1.5,
                "member {} left its nest",
                id.0
            );
            checked += 1;
        }
        assert!(checked > 0, "members must carry a nest home");
    }

    #[test]
    fn wander_targets_stay_near_the_nest_and_inside_bounds() {
        let (x, z) = nest_wander_target(5.0, -3.0, 3.0, 0.25, 6.0);
        assert!((x - 5.0).powi(2) + (z + 3.0).powi(2) < 3.1 * 3.1);
        assert!(x.abs() <= 6.0 && z.abs() <= 6.0);
        let (x, z) = nest_wander_target(100.0, 100.0, 3.0, 0.75, 22.0);
        assert!(x <= 22.0 && z <= 22.0);
        for roll in 0..10 {
            let (x, z) = nest_wander_target(0.0, 0.0, 3.0, roll as f32 / 10.0, 22.0);
            assert!(x.is_finite() && z.is_finite());
        }
    }

    #[test]
    fn member_stats_use_the_build_or_the_tuned_fallback() {
        let mut species = crate::species::Species {
            id: 0,
            name: "test".into(),
            diet: crate::species::Diet::Herbivore,
            tier: 1,
            size: 1.0,
            hostile: false,
            tint: [0.0; 3],
            body: crate::body::BodyProfile::default(),
            parts: Vec::new(),
            archetype: None,
            schedule: 0,
            evolution: 0,
        };
        let empty = crate::asset_catalog::PartCatalog::default();
        let (speed, hp) = member_stats(&species, &empty);
        assert_eq!(hp, 10.0, "the fallback is the level-0 health entry");
        species.size = 1.25;
        species.diet = crate::species::Diet::Carnivore;
        let (faster, _) = member_stats(&species, &empty);
        assert!(faster > speed);
        // A catalog turns the authored ranks into the values. Instance ids
        // refer to the part's *model* record, so use the same id the catalog
        // resolves through `by_model`.
        let mut catalog = catalog();
        catalog.parts.push(ranked_part(9));
        let model = ranked_part(9).model.instance_id;
        species.parts.push(PartInstance::new(model, 0));
        let (ranked_speed, ranked_hp) = member_stats(&species, &catalog);
        assert!(ranked_speed >= 2.5, "authored speed picks a retail gear");
        assert_eq!(
            ranked_hp,
            10.0 + 3.0 * crate::creature_rules::HEALTH_INCREMENT,
            "authored health adds the parts bonus to the level-0 entry"
        );
    }

    /// A catalog part carrying authored combat/social/health ranks.
    fn ranked_part(instance: u32) -> crate::asset_catalog::PartDefinition {
        crate::asset_catalog::PartDefinition {
            key: crate::dbpf::ResourceKey {
                type_id: crate::asset_catalog::PROP,
                group_id: crate::asset_catalog::CREATURE_PARTS,
                instance_id: instance,
            },
            model: crate::dbpf::ResourceKey {
                type_id: crate::asset_catalog::RW4,
                group_id: crate::asset_catalog::CREATURE_PARTS,
                instance_id: instance + 5000,
            },
            name: format!("part{instance}"),
            is_null: false,
            price: 5,
            unlock_level: 1,
            min_scale: 0.5,
            max_scale: 2.0,
            default_scale: 1.0,
            model_scale: 1.0,
            model_offset: [0.0; 3],
            model_rotation: [0.0; 3],
            diffuse: None,
            tint: None,
            spec_bump: None,
            abilities: crate::asset_catalog::Capabilities {
                health: 3,
                speed: 2,
                sing: 4,
                ..default()
            },
        }
    }

    fn catalog() -> crate::asset_catalog::PartCatalog {
        crate::asset_catalog::PartCatalog {
            parts: Vec::new(),
            errors: Vec::new(),
        }
    }

    #[test]
    fn members_mount_their_own_species_parts() {
        let species = crate::species::Species {
            id: 0,
            name: "mounts".into(),
            diet: crate::species::Diet::Herbivore,
            tier: 1,
            size: 1.0,
            hostile: false,
            tint: [0.0; 3],
            body: crate::body::BodyProfile::default(),
            parts: vec![PartInstance::new(7, 1)],
            archetype: None,
            schedule: 0,
            evolution: 0,
        };
        let mut originals = crate::Originals::default();
        originals.models.push(crate::OriginalModel {
            source_definition: None,
            surfaces: vec![],
            instance: 7,
            handle: default(),
            scale: 2.0,
            root: None,
            rig_bones: 0,
            material: None,
        });
        // The player's own build stays empty: whatever mounts must come from
        // the species, not from a save this test never populates.
        let mut run = RunProgress::default();
        assert!(run.parts.is_empty());
        let mounted = species.clone();
        let _ = &mut run;
        let mut app = App::new();
        app.insert_resource(originals)
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(
                Update,
                move |mut commands: Commands,
                      mut materials: ResMut<Assets<StandardMaterial>>,
                      originals: Res<crate::Originals>| {
                    let body = commands.spawn((Player, Transform::default())).id();
                    mount_species_parts(
                        &mut commands,
                        &mut materials,
                        body,
                        &originals,
                        &mounted,
                        3,
                    );
                },
            );
        app.update();
        let mut query = app.world_mut().query::<&MountedPart>();
        let mounts = query.iter(app.world()).count();
        assert_eq!(mounts, 1, "the species' own part mounts, not the player's");
        // The instance and slot both come from the species' own build.
        assert_eq!(species.parts[0].instance, 7);
        assert_eq!(species.parts[0].bone, 1);
    }

    #[test]
    fn attachments_follow_the_full_body_transform_and_mesh_reflection() {
        let mut part = PartInstance::new(123, 1);
        part.mirrored = true;
        part.yaw = 0.4;
        part.offset = [0.1, 0.2, 0.3];
        let mut run = RunProgress::default();
        run.parts.push(part.clone());
        let slot = run.body.slot(part.bone);
        let mut originals = crate::Originals::default();
        originals.models.push(crate::OriginalModel {
            source_definition: None,
            surfaces: vec![],
            instance: 123,
            handle: default(),
            scale: 2.,
            root: None,
            rig_bones: 0,
            material: None,
        });
        let mut app = App::new();
        app.insert_resource(run)
            .insert_resource(originals)
            .add_systems(Update, accessory_follow);
        let pt = Transform::from_xyz(3., 4., 5.)
            .with_rotation(Quat::from_euler(EulerRot::XYZ, 0.2, 0.7, 0.3))
            .with_scale(Vec3::splat(2.));
        app.world_mut().spawn((Player, pt));
        let accessory = app
            .world_mut()
            .spawn((
                Accessory {
                    part: 0,
                    slot_local: Vec3::ZERO, // stale cached slots must be ignored
                },
                Transform::default(),
            ))
            .id();
        app.update();
        let t = app.world().entity(accessory).get::<Transform>().unwrap();
        let expected =
            pt.translation + pt.rotation * ((slot + Vec3::from_array(part.offset)) * pt.scale);
        assert!(t.translation.distance(expected) < 1e-6);
        assert!(
            t.rotation
                .angle_between(pt.rotation * Quat::from_rotation_y(part.yaw))
                < 1e-4
        );
        assert_eq!(t.scale, Vec3::new(-2., 2., 2.));
    }
    #[test]
    fn species_members_keep_matching_attachment_transforms() {
        for member in 0..12 {
            for part in 0..8 {
                assert_eq!(npc_variation(member, part), (1.0, 0.0));
            }
        }
    }

    #[test]
    fn slot_offset_falls_back_without_rig() {
        // No rigged creature in Originals -> classic head/back anchor.
        let o = crate::Originals::default();
        assert_eq!(slot_offset(&o, 0, 1.0), Vec3::new(0.0, 1.9, -0.6));
        // Scale applies to the fallback anchor constants.
        assert_eq!(slot_offset(&o, 0, 0.5), Vec3::new(0.0, 0.95, -0.3));
    }

    #[test]
    fn wander_idles_then_walks() {
        // Waiting ticks down without moving.
        let (x, z, wait, arrived) = wander_advance(0.0, 0.0, 10.0, 0.0, 1.0, 0.4, 5.0);
        assert_eq!((x, z), (0.0, 0.0));
        assert!((wait - 0.6).abs() < 1e-5);
        assert!(!arrived);
        // Walking covers speed*dt toward the target.
        let (x, _, _, arrived) = wander_advance(0.0, 0.0, 10.0, 0.0, 0.0, 0.4, 5.0);
        assert!((x - 2.0).abs() < 1e-5);
        assert!(!arrived);
        // Already there counts as arrived.
        let (_, _, _, arrived) = wander_advance(9.9, 0.0, 10.0, 0.0, 0.0, 0.4, 5.0);
        assert!(arrived);
    }

    #[test]
    fn turning_eases_along_the_short_arc_at_a_capped_rate() {
        use std::f32::consts::PI;
        let dt = 1.0 / 60.0;
        // One frame of a reversal is a visible turn, not a snap.
        let yaw = turn_toward(0.0, PI, NPC_TURN_RATE, NPC_MAX_TURN_SPEED, dt);
        // Exactly PI has no unique short direction; rem_euclid picks -PI.
        // Check that the tie moves without exceeding the per-frame cap.
        assert!(yaw.abs() > 0.0 && yaw.abs() <= NPC_MAX_TURN_SPEED * dt + 1e-6);
        // The short way round crosses the +-PI seam.
        let yaw = turn_toward(3.0, -3.0, NPC_TURN_RATE, NPC_MAX_TURN_SPEED, dt);
        assert!(yaw > 3.0 || yaw < -3.0, "{yaw}");
        // Holding a target converges onto it.
        let mut yaw = -2.5;
        for _ in 0..120 {
            yaw = turn_toward(yaw, 1.0, PLAYER_TURN_RATE, PLAYER_MAX_TURN_SPEED, dt);
        }
        assert!((yaw - 1.0).abs() < 1e-3);
        assert_eq!(turn_toward(0.4, 1.0, 5.0, 5.0, 0.0), 0.4);
        assert_eq!(turn_toward(f32::NAN, 1.0, 5.0, 5.0, dt), 1.0);
    }

    #[test]
    fn facing_matches_the_minus_z_creature_front() {
        for direction in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::new(1.0, 0.0, -1.0)] {
            let front = Quat::from_rotation_y(facing_yaw(direction)) * Vec3::NEG_Z;
            assert!(
                front.abs_diff_eq(direction.normalize(), 1e-5),
                "{direction}"
            );
        }
    }

    #[test]
    fn members_turn_before_walking_off_sideways() {
        assert_eq!(stride_alignment(Vec3::NEG_Z, Vec3::NEG_Z), 1.0);
        assert_eq!(stride_alignment(Vec3::NEG_Z, Vec3::Z), 0.0);
        let side = stride_alignment(Vec3::NEG_Z, Vec3::X);
        assert!(side > 0.0 && side < 0.2, "{side}");
        let near = stride_alignment(Vec3::NEG_Z, Vec3::new(0.3, 0.0, -1.0));
        assert!(near > 0.95, "{near}");
        assert_eq!(stride_alignment(Vec3::ZERO, Vec3::X), 1.0);
    }

    #[test]
    fn wandering_members_face_where_they_walk() {
        let mut app = stage_app(RunProgress::default());
        enter(&mut app, GameStage::Creature);
        // Keep the herds where they are while the player is parked away.
        app.world_mut()
            .resource_mut::<crate::creature_ecology::CastingState>()
            .timer = f32::NEG_INFINITY;
        // Park the player far away so nothing chases or watches it.
        let player = {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<Player>>()
                .single(world)
                .unwrap()
        };
        app.world_mut()
            .get_mut::<Transform>(player)
            .unwrap()
            .translation = Vec3::new(-1000.0, 0.7, -1000.0);
        let member = {
            let world = app.world_mut();
            world
                .query_filtered::<(Entity, &Npc), With<Wander>>()
                .iter(world)
                .find(|(_, npc)| !npc.hostile)
                .map(|(e, _)| e)
                .unwrap()
        };
        let start = app.world().get::<Transform>(member).unwrap().translation;
        // Head toward the middle of the map so the bounds never block it.
        let heading = if start.x > 0.0 { Vec3::NEG_X } else { Vec3::X };
        let target = start + heading * 4.0;
        for _ in 0..90 {
            let mut wander = app.world_mut().get_mut::<Wander>(member).unwrap();
            wander.tx = target.x;
            wander.tz = target.z;
            wander.wait = 0.0;
            app.update();
        }
        let t = app.world().get::<Transform>(member).unwrap();
        let front = t.rotation * Vec3::NEG_Z;
        assert!(front.dot(heading) > 0.95, "faces travel: {front}");
        assert!(
            (t.translation - start).dot(heading) > 0.5,
            "walked after turning"
        );
    }
}
