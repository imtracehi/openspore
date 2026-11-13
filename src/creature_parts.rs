//! Creature-stage part discovery. Spore keeps its better creature parts out
//! of the editor until they are found: skeletons of long-dead creatures lie
//! around the island, and picking one over reveals a new part. Defeating or
//! befriending a species can also yield one of the parts it carries.
//!
//! RETAIL (VERIFIED in the installed props, `notes\creature-bones-notes.md`):
//! * The piles are the four `SkeletonPartUnlockColumn1..4` nouns
//!   (`InteractiveOrnament`, type 2, pickup disabled): PROPs
//!   `CRG_Part_Unlock_Skeleton` / `2` / `3` / `4` with the models `bone_pile`,
//!   `bone_pile_02`, `bone_pile_03`, `bone_pile_huge` at `modelScale` 1.15 /
//!   1.0 / 1.25 / 1.5 and LOD distances 20 / 60 / 175 (the huge pile 20 /
//!   150 / 175). Each carries one `modelEffect` (four effects, one per pile)
//!   within `modelEffectRange` 60: two Swarm particle emitters (a slow
//!   billboard glow, a flash sparkle) and a ground decal, scaled 0.825 / 1.25
//!   / 1.25 / 1.9. There is no sphere, shell or volume.
//! * A gameplay trigger (type 6, `Radius` 0 = the object, `UsageCount` 1,
//!   `AvatarOnly`, event 0xA07DDE8F, `Amount` = the column 1..4) fires when
//!   the avatar touches the pile; the pile is then shown as its
//!   `destructedModelStates` model (`CRG_Skeleton_Destructed_01..04`,
//!   `bone_pile_pillaged_*`, no effect).
//! * Placement (`CreatureGameLevelGenerationList!forestNouns`, the four
//!   `noun_placement_markers` 0x2C4332AE / 0xA81BDB1B / 0x21444574 /
//!   0xBC00CB11): for every forest of the planet within 200 m of the start
//!   (the same forests the epics use) column 1 is placed with 100 % at 16 to
//!   24 m from the forest, column 2 with 30 % at 12 to 20 m, column 3 with
//!   20 % at 8 to 20 m, column 4 with 10 % at 3 to 10 m (slope 0 to 30
//!   degrees, `useOrigin` = the forest).
//!
//! ADAPTED (marked at the code): the slope test and the exclusive areas of
//! the placement are a land test plus a minimum spacing; the models show only
//! LOD0; the effect range is gated with a hysteresis; the pickup reach is the
//! pile's size (retail `Radius` 0 asks the object's own bounds); which part a
//! pile yields and what its `Amount` does (0xD47E50 passes the column to the
//! row picker 0xD3BF50, not decoded) stay OpenSpore's rule (the lowest locked
//! column of a palette row, rows the creature uses first). Without forests
//! (no planet) the earlier scatter of `PILE_COUNT` piles is kept.

use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::{
    asset_catalog::{PartDefinition, CREATURE_PARTS, PROP},
    creature_ecology::{mix64, unit},
    creature_planet::Land,
    dbpf::ResourceKey,
    game::*,
    stages::StageOwned,
};

/// Parts at this unlock level and above must be found before buying them.
pub const FIRST_FOUND_LEVEL: u32 = 2;
/// Bone piles of the earlier scatter (no planet, headless tests).
pub const PILE_COUNT: u32 = 9;
/// Distance at which the player picks a pile over (at least; ADAPTED: grows
/// with the pile's size, retail asks the object's own bounds).
const PICKUP_REACH: f32 = 2.4;
/// Share of a pile's width that counts as touching it.
const REACH_PER_WIDTH: f32 = 0.4;
/// `modelEffectRange` of the pile PROPs: the glow and sparkle play within it.
pub const EFFECT_RANGE: f32 = 60.0;
/// ADAPTED: the effects stop this much beyond the range (no flicker at the
/// edge).
const EFFECT_HYSTERESIS: f32 = 6.0;
/// The grounding baseline stage objects are spawned at (`terrain::BASE_Y`).
const GROUND_BASELINE: f32 = -0.6;
/// Ids of piles placed at forests start here (the earlier scatter used 0..9,
/// so saved pickups of an old world never hide a new pile).
pub const FOREST_ID_BASE: u32 = 100;
/// Tries to find dry land for one pile.
const SITE_TRIES: u64 = 16;
/// ADAPTED: two piles are never closer than this (retail: exclusive areas of
/// radius 1 or 2 per noun).
const MIN_SPACING: f32 = 3.0;

/// One column of the part-unlock skeletons (`SkeletonPartUnlockColumn1..4`).
#[derive(Debug, Clone, Copy)]
pub struct PileColumn {
    /// The pile PROP (`CRG_Part_Unlock_Skeleton*`) and its instance id.
    pub name: &'static str,
    pub prop: u32,
    /// The picked-over model (`destructedModelStates`).
    pub spent_name: &'static str,
    pub spent_prop: u32,
    /// `forestNouns` placement: chance per forest and distance band (m).
    pub chance: f32,
    pub band: (f32, f32),
}

/// Columns 1 to 4 (PROP instance ids are the FNV-1 of the names).
pub const COLUMNS: [PileColumn; 4] = [
    PileColumn {
        name: "CRG_Part_Unlock_Skeleton",
        prop: 0x61C0_F798,
        spent_name: "CRG_Skeleton_Destructed_01",
        spent_prop: 0x4773_3115,
        chance: 1.0,
        band: (16.0, 24.0),
    },
    PileColumn {
        name: "CRG_Part_Unlock_Skeleton2",
        prop: 0x7AC5_C47A,
        spent_name: "CRG_Skeleton_Destructed_02",
        spent_prop: 0x4773_3116,
        chance: 0.3,
        band: (12.0, 20.0),
    },
    PileColumn {
        name: "CRG_Part_Unlock_Skeleton3",
        prop: 0x7AC5_C47B,
        spent_name: "CRG_Skeleton_Destructed_03",
        spent_prop: 0x4773_3117,
        chance: 0.2,
        band: (8.0, 20.0),
    },
    PileColumn {
        name: "CRG_Part_Unlock_Skeleton4",
        prop: 0x7AC5_C47C,
        spent_name: "CRG_Skeleton_Destructed_04",
        spent_prop: 0x4773_3110,
        chance: 0.1,
        band: (3.0, 10.0),
    },
];

fn prop_key(instance: u32) -> ResourceKey {
    ResourceKey {
        group_id: 0,
        instance_id: instance,
        type_id: PROP,
    }
}

/// An unclaimed bone pile, identified across saves by its id.
#[derive(Component, Debug, Clone, Copy)]
pub struct BonePile {
    pub id: u32,
    /// 0..4: `SkeletonPartUnlockColumn` minus one.
    pub column: usize,
    /// How close the avatar must come (world units).
    pub reach: f32,
}

/// The pile's model effects (`modelEffect`): played while the avatar is within
/// `modelEffectRange`.
#[derive(Component, Debug, Clone, Copy)]
pub struct PileEffects {
    pub key: ResourceKey,
    pub running: bool,
}

/// A pile that was picked over: the `destructedModelStates` model.
#[derive(Component, Debug, Clone, Copy)]
pub struct BonePileSpent {
    pub id: u32,
}

/// Where one pile lies: a stable id, its column and the chart point (x, z).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PileSite {
    pub id: u32,
    pub column: usize,
    pub at: Vec3,
}

impl PileSite {
    pub fn chart(&self) -> Vec2 {
        Vec2::new(self.at.x, self.at.z)
    }
}

fn lerp((lo, hi): (f32, f32), t: f32) -> f32 {
    lo + (hi - lo) * t
}

/// VERIFIED (`forestNouns`, see the module notes): the piles round every
/// forest, in the forest list's order: column `c` of forest `f` is placed
/// with probability `COLUMNS[c].chance`, in the column's distance band from
/// the forest, on dry land (ADAPTED: the slope test is a land test) and not
/// closer than `MIN_SPACING` to another pile or the home nest.
pub fn forest_sites(seed: u64, forests: &[Vec2], land: &Land) -> Vec<PileSite> {
    let home = Vec2::from(crate::creature_ecology::HOME);
    let mut out: Vec<PileSite> = Vec::new();
    for (f, forest) in forests.iter().enumerate() {
        for (c, column) in COLUMNS.iter().enumerate() {
            let base = mix64(seed ^ 0x5CE1_E70A ^ ((f as u64) << 16) ^ ((c as u64) << 8));
            if unit(base) >= column.chance {
                continue;
            }
            for tries in 0..SITE_TRIES {
                let k = mix64(base ^ (tries + 1).wrapping_mul(0x9E37_79B9));
                let angle = unit(k) * std::f32::consts::TAU;
                let p = *forest + Vec2::new(angle.cos(), angle.sin()) * lerp(column.band, unit(k ^ 0xA5));
                if !land.is_land(p)
                    || p.distance(home) < PICKUP_REACH + MIN_SPACING
                    || out.iter().any(|s| s.chart().distance(p) < MIN_SPACING)
                {
                    continue;
                }
                out.push(PileSite {
                    id: FOREST_ID_BASE + (f * COLUMNS.len() + c) as u32,
                    column: c,
                    at: Vec3::new(p.x, 0.0, p.y),
                });
                break;
            }
        }
    }
    out
}

/// The earlier scatter: `PILE_COUNT` piles on the island, columns in turn.
pub fn scatter_sites(seed: u64) -> Vec<PileSite> {
    (0..PILE_COUNT)
        .map(|id| PileSite {
            id,
            column: id as usize % COLUMNS.len(),
            at: pile_position(seed, id),
        })
        .collect()
}

/// Every pile of a world: at the forests within 200 m of the start (retail),
/// else (no planet, or no forest near) the earlier scatter.
pub fn sites(seed: u64, land: &Land) -> Vec<PileSite> {
    let forests = land.forests(
        Vec2::from(crate::creature_ecology::HOME),
        crate::creature_ecology::FOREST_RADIUS * crate::creature_ecology::CREATURE_WORLD_SCALE,
    );
    let placed = forest_sites(seed, &forests, land);
    if placed.is_empty() {
        scatter_sites(seed)
    } else {
        placed
    }
}

/// Pure: where pile `id` lies on the island for a species-world seed. Piles
/// keep clear of the home nest and the island centre.
pub fn pile_position(seed: u64, id: u32) -> Vec3 {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0xB0E5_u64.wrapping_mul(id as u64 + 1));
    loop {
        let at = Vec3::new(
            rng.random_range(-20.0..20.0),
            0.0,
            rng.random_range(-20.0..20.0),
        );
        let home = Vec3::new(0.0, 0.0, 8.0);
        if at.length() > 5.0 && dist_xz(at, home) > 5.0 {
            return at;
        }
    }
}

/// Pure: the next part a discovery reveals — the lowest locked unlock level
/// first, chosen deterministically from how many parts were already found.
pub fn next_part<'a>(run: &RunProgress, parts: &'a [PartDefinition]) -> Option<&'a PartDefinition> {
    let locked: Vec<_> = parts
        .iter()
        .filter(|p| {
            p.key.group_id == CREATURE_PARTS
                && !p.is_null
                && p.unlock_level >= FIRST_FOUND_LEVEL
                && !run.creature_found_parts.contains(&p.key.instance_id)
        })
        .collect();
    let level = locked.iter().map(|p| p.unlock_level).min()?;
    let tier: Vec<_> = locked
        .into_iter()
        .filter(|p| p.unlock_level == level)
        .collect();
    let pick = (run.creature_found_parts.len() * 7919 + run.creature_bone_piles_taken.len() * 31)
        % tier.len();
    Some(tier[pick])
}

/// Record a found part and return its display name.
pub fn reveal(run: &mut RunProgress, definition: &PartDefinition) -> String {
    if !run
        .creature_found_parts
        .contains(&definition.key.instance_id)
    {
        run.creature_found_parts.push(definition.key.instance_id);
        run.request_save();
    }
    definition.name.trim().to_string()
}

/// Unlock one locked part among a defeated or befriended creature's own
/// parts, if it carries any the player has not found.
pub fn take_species_part(
    run: &mut RunProgress,
    parts: &[PartDefinition],
    carried: impl IntoIterator<Item = u32>,
) -> Option<String> {
    let carried: Vec<u32> = carried.into_iter().collect();
    let definition = parts.iter().find(|p| {
        p.key.group_id == CREATURE_PARTS
            && !p.is_null
            && p.unlock_level >= FIRST_FOUND_LEVEL
            && carried.contains(&p.key.instance_id)
            && !run.creature_found_parts.contains(&p.key.instance_id)
    })?;
    Some(reveal(run, definition))
}

/// The pile's width in world units: its model's span times the PROP's
/// `modelScale` (the retail size), `None` without an installation.
fn native_width(
    originals: &mut crate::Originals,
    key: ResourceKey,
    name: &str,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<f32> {
    if originals.resource_index.records.is_empty() {
        return None;
    }
    let d = crate::world_objects::definition_at(originals, key, name).ok()?;
    let (model, _) = crate::world_objects::import_at(originals, d.clone(), meshes, images, materials).ok()?;
    let unit = crate::world_objects::fitted_transform(&model, meshes, Vec3::ZERO, 1.0).ok()?;
    (unit.scale.x > 0.0).then(|| d.model_scale / unit.scale.x)
}

/// Spawn one pile model at its native size (`modelScale`). Returns the root
/// and the model's width; an empty root stands in without an installation.
#[allow(clippy::too_many_arguments)]
fn spawn_model(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    instance: u32,
    name: &str,
    at: Vec3,
) -> (Entity, f32) {
    let key = prop_key(instance);
    let width = native_width(originals, key, name, meshes, images, materials);
    let spawned = width.and_then(|w| {
        crate::world_objects::spawn_at(commands, originals, key, name, meshes, images, materials, at, w).map(|e| (e, w))
    });
    spawned.unwrap_or_else(|| {
        (
            commands
                .spawn((StageOwned, Transform::from_translation(at), Visibility::default()))
                .id(),
            0.0,
        )
    })
}

pub fn setup(
    mut commands: Commands,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    run: Res<RunProgress>,
    cache: Option<Res<crate::terrain::PlanetCache>>,
) {
    let seed = crate::creature::world_seed(&run);
    let land = crate::creature_planet::site_land(cache.as_deref(), seed, run.species_world.planet_frame);
    let started = std::time::Instant::now();
    let list = sites(seed, &land);
    info!(
        "part piles: {} placed ({}) in {} ms",
        list.len(),
        if list.iter().any(|s| s.id >= FOREST_ID_BASE) { "at the forests within 200 m" } else { "scattered, no forest near" },
        started.elapsed().as_millis()
    );
    for site in list {
        let column = &COLUMNS[site.column];
        // Stage objects are spawned on the grounding baseline; `terrain::ground`
        // lifts them onto the surface (the planet's heights included).
        let at = site.at.with_y(GROUND_BASELINE);
        if run.creature_bone_piles_taken.contains(&site.id) {
            // Picked over earlier: the destructed model stays.
            let (spent, _) = spawn_model(
                &mut commands,
                &mut originals,
                &mut meshes,
                &mut images,
                &mut materials,
                column.spent_prop,
                column.spent_name,
                at,
            );
            commands.entity(spent).insert(BonePileSpent { id: site.id });
            continue;
        }
        let (entity, width) = spawn_model(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            column.prop,
            column.name,
            at,
        );
        if width > 0.0 {
            info!("part pile {} (column {}): {} wide at ({:.0}, {:.0})", site.id, site.column + 1, width, at.x, at.z);
        }
        commands.entity(entity).insert((
            BonePile {
                id: site.id,
                column: site.column,
                reach: PICKUP_REACH.max(REACH_PER_WIDTH * width),
            },
            PileEffects {
                key: prop_key(column.prop),
                running: false,
            },
        ));
    }
}

/// `OPENSPORE_PILE_DEMO` (metres), captures only.
fn pile_demo_gap() -> Option<f32> {
    static GAP: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *GAP.get_or_init(|| std::env::var("OPENSPORE_PILE_DEMO").ok().and_then(|v| v.parse().ok()))
}

/// Walk onto a pile to pick it over and reveal a part.
#[allow(clippy::too_many_arguments)]
pub fn collect(
    mut commands: Commands,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut encounter: ResMut<crate::creature_actions::Encounter>,
    mut awards: ResMut<crate::creature_social::CreatureAwards>,
    mut editor_data: Option<ResMut<crate::creature_editor_data::CreatureEditorData>>,
    mut stops: MessageWriter<crate::swarm_fx::SwarmStop>,
    mut player: Query<&mut Transform, With<Player>>,
    mut piles: Query<(Entity, &Transform, &BonePile, &mut PileEffects), Without<Player>>,
    mut frame: Local<u32>,
) {
    let Ok(mut pose) = player.single_mut() else {
        return;
    };
    // Capture hook: `OPENSPORE_PILE_DEMO=<metres>` puts the avatar that far
    // from the biggest pile (nearest of the highest column), on the nest's side, facing it (frame 40).
    *frame = frame.saturating_add(1);
    if *frame == 40 {
        if let Some(gap) = pile_demo_gap() {
            let from = pose.translation;
            // The biggest pile (highest column), the nearest of those.
            let top = piles.iter().map(|p| p.2.column).max().unwrap_or(0);
            let nearest = piles
                .iter()
                .filter(|p| p.2.column == top)
                .map(|p| p.1.translation)
                .min_by(|a, b| dist_xz(*a, from).total_cmp(&dist_xz(*b, from)));
            if let Some(near) = nearest {
                let away = (from - near).with_y(0.0).normalize_or(Vec3::Z);
                pose.translation = near + away * gap;
                pose.rotation = Quat::from_rotation_y(away.x.atan2(away.z));
            }
        }
    }
    let pt = *pose;
    let mut taken = Vec::new();
    for (entity, t, pile, mut fx) in &mut piles {
        let distance = dist_xz(t.translation, pt.translation);
        if distance < pile.reach {
            commands.entity(entity).despawn();
            taken.push((pile.id, pile.column, t.translation));
            continue;
        }
        // `modelEffectRange`: the glow and sparkle play within 60 m only.
        if !fx.running && distance < EFFECT_RANGE {
            let started = crate::tribal_village::attach_model_effects(&mut commands, &originals, fx.key, entity);
            if started > 0 {
                info!("part pile {}: {started} model effect(s) started at {distance:.0} m", pile.id);
                fx.running = true;
            }
        } else if fx.running && distance > EFFECT_RANGE + EFFECT_HYSTERESIS {
            stops.write(crate::swarm_fx::SwarmStop {
                effect: 0,
                attach_to: Some(entity),
                tag: 0,
                hard: false,
            });
            fx.running = false;
        }
    }
    taken.sort_unstable_by_key(|t| t.0);
    taken.dedup_by_key(|t| t.0);
    for (id, column, translation) in taken {
        if run.creature_bone_piles_taken.contains(&id) {
            continue;
        }
        run.creature_bone_piles_taken.push(id);
        run.request_save();
        // `destructedModelStates`: the pile is now the picked-over model.
        let spent = &COLUMNS[column.min(COLUMNS.len() - 1)];
        let (entity, _) = spawn_model(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            spent.spent_prop,
            spent.spent_name,
            translation.with_y(GROUND_BASELINE),
        );
        commands.entity(entity).insert(BonePileSpent { id });
        // Retail (`cCollectableItems`): the next column of a palette row,
        // rows the creature uses first (`creature_editor_edit::unlocks`).
        if let Some(data) = editor_data.as_deref_mut() {
            if !data.loaded && !originals.resource_index.records.is_empty() {
                *data = crate::creature_editor_data::CreatureEditorData::load(&originals.resource_index);
            }
            let roll = (run.creature_found_parts.len() as u64).wrapping_mul(7919) ^ (id as u64).wrapping_mul(31);
            let unlock = data
                .parts_palette(crate::creature_editor_data::SetupKind::Creature)
                .and_then(|p| crate::creature_editor_edit::unlocks::next_unlock(p, data, &run, roll));
            if let Some(part) = unlock {
                if !run.creature_found_parts.contains(&part) {
                    run.creature_found_parts.push(part);
                }
                let name = data.parts.get(&part).map(|i| i.name.clone()).unwrap_or_default();
                awards.push(format!("New part unlocked: {name}"));
                encounter.message = format!("You found a new part in the bones: {name}! Mate at your nest (M) to use it.");
                continue;
            }
        }
        encounter.message = match next_part(&run, &originals.part_catalog.parts).cloned() {
            Some(definition) => {
                let name = reveal(&mut run, &definition);
                awards.push(format!("New part unlocked: {name}"));
                format!(
                    "You found a new part in the bones: {name}! Mate at your nest (M) to use it."
                )
            }
            None => "These old bones hold nothing new.".into(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_catalog::Capabilities;

    fn part(instance: u32, level: u32) -> PartDefinition {
        let key = ResourceKey {
            type_id: PROP,
            group_id: CREATURE_PARTS,
            instance_id: instance,
        };
        PartDefinition {
            key,
            model: ResourceKey {
                type_id: crate::asset_catalog::RW4,
                ..key
            },
            name: format!("Part {instance}"),
            is_null: false,
            price: 10,
            unlock_level: level,
            min_scale: 0.5,
            max_scale: 1.5,
            default_scale: 1.0,
            model_scale: 1.0,
            model_offset: [0.0; 3],
            model_rotation: [0.0; 3],
            diffuse: None,
            tint: None,
            spec_bump: None,
            abilities: Capabilities::default(),
        }
    }

    #[test]
    fn discoveries_climb_unlock_levels_without_repeats() {
        let parts = vec![part(1, 1), part(2, 2), part(3, 2), part(4, 3)];
        let mut run = RunProgress::default();
        let mut seen = Vec::new();
        while let Some(definition) = next_part(&run, &parts).cloned() {
            assert!(definition.unlock_level >= FIRST_FOUND_LEVEL);
            assert!(!seen.contains(&definition.key.instance_id));
            if let Some(&last) = seen.last() {
                let previous = parts.iter().find(|p| p.key.instance_id == last).unwrap();
                assert!(definition.unlock_level >= previous.unlock_level);
            }
            seen.push(definition.key.instance_id);
            reveal(&mut run, &definition);
        }
        assert_eq!(seen.len(), 3, "every locked part is eventually found");
        assert!(take_species_part(&mut run, &parts, [4]).is_none());
    }

    #[test]
    fn piles_avoid_home_and_are_stable_per_world() {
        for id in 0..PILE_COUNT {
            let a = pile_position(42, id);
            assert_eq!(a, pile_position(42, id));
            assert!(a.length() > 5.0 && dist_xz(a, Vec3::new(0.0, 0.0, 8.0)) > 5.0);
            assert!(a.x.abs() <= 20.0 && a.z.abs() <= 20.0);
        }
    }

    #[test]
    fn walking_onto_a_pile_reveals_a_part_and_remembers_it() {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut()
            .resource_mut::<crate::Originals>()
            .part_catalog
            .parts
            .extend([part(10, 2), part(11, 3)]);
        app.world_mut()
            .resource_mut::<NextState<crate::stages::GameStage>>()
            .set(crate::stages::GameStage::Creature);
        app.update();
        app.update();
        let at = pile_position(app.world().resource::<RunProgress>().species_world.seed, 0);
        {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&mut Transform, With<Player>>();
            q.single_mut(world).unwrap().translation = at;
        }
        app.update();
        app.update();
        let run = app.world().resource::<RunProgress>();
        assert!(run.creature_bone_piles_taken.contains(&0));
        assert_eq!(run.creature_found_parts, vec![10]);
        let message = &app
            .world()
            .resource::<crate::creature_actions::Encounter>()
            .message;
        assert!(message.contains("Part 10"), "{message}");
    }

    /// Diagnostic (ignored): dump the model effects of the pile PROPs.
    #[test]
    #[ignore = "requires the installed game"]
    fn dump_pile_effects() {
        use crate::swarm_fx::{format::kind, SwarmLibrary};
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        fn walk(lib: &SwarmLibrary, e: crate::swarm_fx::EffectRef, depth: usize, out: &mut Vec<String>) {
            let dir = lib.dir(e.dir);
            let def = &dir.effects[e.index as usize];
            let pad = "  ".repeat(depth);
            out.push(format!(
                "{pad}effect d{} i{} flags {:x} appmask {:x} screen {:?} lod {:?}",
                e.dir, e.index, def.flags, def.app_flags_mask, def.screen_size_range, def.lod_distances
            ));
            for b in &def.blocks {
                out.push(format!(
                    "{pad} block kind {:#x} idx {} xf scale {} off {:?} lod {}..{} app {:x}/{:x} emit {:?} size {:?} alpha {:?} flags {:x}",
                    b.kind, b.index, b.transform.scale, b.transform.offset, b.lod_begin, b.lod_end, b.app_flags, b.app_flags_mask, b.emit_scale, b.size_scale, b.alpha_scale, b.flags
                ));
                let local = b.index >= 0 && b.index & crate::swarm_fx::format::IMPORT_MASK == 0;
                match b.kind {
                    kind::EFFECT => {
                        if let Some(n) = lib.resolve_effect(e.dir, b.index) {
                            walk(lib, n, depth + 1, out);
                        }
                    }
                    kind::PARTICLES if local => {
                        let p = &dir.particles[b.index as usize];
                        out.push(format!(
                            "{pad}   particles life {:?} rate {:?} size {:?} alpha {:?} color {:?} vol {:?} dir {:?} speed {:?} grav {} wind {} tex {:?} tiles {:?} align {} flags {:x}",
                            p.life, p.rate, p.size, p.alpha, p.color, p.emit_volume, p.emit_direction, p.emit_speed, p.gravity_strength, p.wind_strength, p.texture, p.tile_count, p.align_mode, p.flags
                        ));
                    }
                    kind::META_PARTICLES if local => {
                        let m = &dir.metas[b.index as usize];
                        out.push(format!(
                            "{pad}   meta life {:?} rate {:?} size {:?} alpha {:?} comp {:?} vol {:?} speed {:?}",
                            m.life, m.rate, m.size, m.alpha, m.component, m.emit_volume, m.emit_speed
                        ));
                        if m.component.1 == 0 {
                            if let Some(n) = lib.resolve_effect(e.dir, m.component.0) {
                                walk(lib, n, depth + 2, out);
                            }
                        }
                    }
                    kind::MODEL if local => {
                        let m = &dir.models[b.index as usize];
                        out.push(format!("{pad}   model {:08x}!{:08x} size {} color {:?} alpha {} mat {:?}", m.resource.group, m.resource.instance, m.size, m.color, m.alpha, m.material));
                    }
                    kind::SEQUENCE if local => {
                        for (t, ie) in &dir.sequences[b.index as usize].instances {
                            out.push(format!("{pad}   seq {:?}", t));
                            if let Some(n) = lib.resolve_effect(e.dir, *ie) {
                                walk(lib, n, depth + 2, out);
                            }
                        }
                    }
                    kind::VOLUME if local => {
                        out.push(format!("{pad}   volume {:?}", dir.volumes[b.index as usize].material));
                    }
                    _ => {}
                }
            }
        }
        {
            use bevy::mesh::VertexAttributeValues;
            let mut originals = crate::Originals::default();
            originals.resource_index = index.clone();
            let mut meshes = Assets::<Mesh>::default();
            let mut images = Assets::<Image>::default();
            let mut materials = Assets::<StandardMaterial>::default();
            for (name, instance) in COLUMNS.iter().map(|c| (c.name, c.prop)) {
                let key = ResourceKey { group_id: 0, instance_id: instance, type_id: PROP };
                let d = crate::world_objects::definition_at(&originals, key, name).unwrap();
                let (model, _) = crate::world_objects::import_at(&mut originals, d.clone(), &mut meshes, &mut images, &mut materials).unwrap();
                let mesh = meshes.get(&model.handle).unwrap();
                let Some(VertexAttributeValues::Float32x3(v)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
                let mut min = Vec3::splat(f32::INFINITY);
                let mut max = Vec3::splat(f32::NEG_INFINITY);
                for p in v { let p = Vec3::from_array(*p); min = min.min(p); max = max.max(p); }
                println!("{name}: model_scale {} offset {:?} rot {:?} bounds {:?} .. {:?} size {:?}", d.model_scale, d.model_offset, d.model_rotation, min, max, max - min);
            }
        }
        for id in [0x8B61750Fu32, 0x2BE8DA98, 0x7598FAE1, 0x96475C0A] {
            println!("==== effect {id:08X}");
            match lib.find(id) {
                Some(e) => {
                    let mut out = Vec::new();
                    walk(&lib, e, 0, &mut out);
                    for l in out {
                        println!("{l}");
                    }
                }
                None => println!("not found"),
            }
        }
    }

    #[test]
    fn columns_follow_the_installed_props() {
        // Instance ids are the FNV-1 of the PROP names (SkeletonPartUnlockColumn1..4
        // use CRG_Part_Unlock_Skeleton, 2, 3, 4; destructedModelStates 01..04).
        for c in COLUMNS {
            assert_eq!(crate::world_objects::name_id(c.name), c.prop, "{}", c.name);
            assert_eq!(crate::world_objects::name_id(c.spent_name), c.spent_prop, "{}", c.spent_name);
        }
        // forestNouns: 100 %, 30 %, 20 %, 10 % at 16-24, 12-20, 8-20, 3-10 m.
        let chances: Vec<f32> = COLUMNS.iter().map(|c| c.chance).collect();
        assert_eq!(chances, [1.0, 0.3, 0.2, 0.1]);
        assert_eq!(COLUMNS[0].band, (16.0, 24.0));
        assert_eq!(COLUMNS[3].band, (3.0, 10.0));
    }

    #[test]
    fn forest_piles_follow_the_retail_rule() {
        let land = Land::flat();
        let forests: Vec<Vec2> = (0..40).map(|i| Vec2::new(40.0 + i as f32 * 30.0, -90.0 + (i % 5) as f32 * 40.0)).collect();
        let sites = forest_sites(5, &forests, &land);
        // Same world, same piles.
        assert_eq!(sites, forest_sites(5, &forests, &land));
        // Every forest gets its column 1 (100 %); the rarer columns follow
        // their chances over the 40 forests.
        let per_column = |c: usize| sites.iter().filter(|s| s.column == c).count();
        assert_eq!(per_column(0), forests.len());
        assert!(per_column(1) > 3 && per_column(1) < 25, "{}", per_column(1));
        assert!(per_column(3) < per_column(1), "{} {}", per_column(3), per_column(1));
        // Each pile lies in its band round its forest.
        for s in &sites {
            let f = (s.id - FOREST_ID_BASE) as usize / COLUMNS.len();
            let band = COLUMNS[s.column].band;
            let d = s.chart().distance(forests[f]);
            assert!(d >= band.0 - 1e-3 && d <= band.1 + 1e-3, "{d} {band:?}");
        }
        // Ids are unique and above the earlier scatter's.
        let mut ids: Vec<u32> = sites.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), sites.len());
        assert!(ids.iter().all(|id| *id >= FOREST_ID_BASE));
        // No planet: the earlier scatter (9 piles, columns in turn).
        let flat = sites_for(7, &land);
        assert_eq!(flat.len(), PILE_COUNT as usize);
        assert_eq!(flat[5].column, 1);
    }

    fn sites_for(seed: u64, land: &Land) -> Vec<PileSite> {
        sites(seed, land)
    }
}
