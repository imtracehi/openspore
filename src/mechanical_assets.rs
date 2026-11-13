//! Deterministic original Building/Vehicle definitions + on-demand import.
//!
//! The civ vehicle editor and the space ship loadout editor select through
//! this module. Formerly they cycled `Originals.models` directly: that cache
//! is filled by `main`'s startup scan with up to twelve arbitrary RW4
//! records from every package regardless of catalog group, plus whatever the
//! creature creator imported first, so mechanical stages showed creature
//! parts and preview meshes. Selection is now definition-level:
//!
//! - `eligible`/`first`/`next` walk `PartCatalog` definitions restricted to
//!   one original catalog group (`asset_catalog::BUILDING_PARTS` for city
//!   assembly, `asset_catalog::VEHICLE_PARTS` for mechanical creators),
//!   excluding `is_null` internal helpers and model links that are not RW4 or
//!   do not resolve in the live `ResourceIndex` (the same validity rule
//!   `PartCatalog::from_index` applies at load time, re-checked here for
//!   synthesized or older catalogs).
//! - `import` decodes through the public `editor::import_definition`
//!   importer (complete supported RW4 geometry plus authored diffuse
//!   `0x02424655` and tint `0x02424657` composed through
//!   `texture::skinpaint`) and caches by full PROP definition key;
//!   `cached` hands owning stages the mount handle plus authored name.
//! - `mesh_extent`/`capped_scale` read the imported mesh's real positions to
//!   bound a part to a world-size cap while preserving the authored
//!   orientation and the authored relative scale of everything under the cap.
//!
//! Cache identity: mechanical imports never touch the generic
//! `Originals.models` cache (which keys on model `instance_id` only and
//! stays instance-keyed for Cell/Creature through `editor::ensure_model`;
//! `main::load_original_models` also pushes one entry per extracted sub-mesh,
//! so one instance id can own several mesh handles there). `import` caches
//! per full PROP definition key in `Originals.mechanical_models`, so two
//! definitions or groups sharing one model instance keep their own mesh and
//! authored material. Remaining limitation, inherent to the save schema:
//! `game::VehiclePart` and `game::ShipModule` persist only the model
//! instance id, so variant definitions sharing one model are
//! indistinguishable in a save; selection and `cached` resolve them
//! canonically to the first eligible definition. The remaining
//! instance-keyed aliasing in the shared caches and the installed-package
//! collision survey are recorded in MECHANICAL_ASSETS_PROGRESS.md.

use crate::{
    asset_catalog::{self, PartCatalog, PartDefinition},
    editor, OriginalModel, Originals,
};
use bevy::{prelude::*, render::mesh::VertexAttributeValues};

/// Eligible definitions of `group`, sorted by (definition instance, name).
///
/// Deterministic regardless of catalog sort order, so editor cycling and
/// tests do not depend on `PartCatalog::from_index` ordering.
pub fn eligible<'a>(originals: &'a Originals, group: u32) -> Vec<&'a PartDefinition> {
    let mut out: Vec<&PartDefinition> = originals
        .part_catalog
        .parts
        .iter()
        .filter(|d| usable(originals, group, d))
        .collect();
    out.sort_by(|a, b| {
        a.key
            .instance_id
            .cmp(&b.key.instance_id)
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

fn usable(originals: &Originals, group: u32, d: &PartDefinition) -> bool {
    d.key.group_id == group
        && !d.is_null
        && d.model.type_id == asset_catalog::RW4
        && d.model.instance_id != 0
        && originals.resource_index.records.contains_key(&d.model)
}

/// The first eligible definition of `group` (None when the group has no
/// usable parts).
pub fn first<'a>(originals: &'a Originals, group: u32) -> Option<&'a PartDefinition> {
    eligible(originals, group).first().copied()
}

/// Deterministic next definition relative to a saved model instance
/// (`VehiclePart::instance` / `ShipModule::instance`), wrapping with `dir`.
/// Stages persist only a model instance id, so cycling walks the unique
/// model identities in deterministic order (first definition per identity);
/// duplicate definitions sharing one model never trap the cycle, and
/// `eligible` still lists every definition for reporting. Returns the first
/// eligible definition of the target identity. No `current`, or an
/// unknown/stale instance, yields the deterministic first.
pub fn next<'a>(
    originals: &'a Originals,
    group: u32,
    current: Option<u32>,
    dir: i32,
) -> Option<&'a PartDefinition> {
    let list = eligible(originals, group);
    if list.is_empty() {
        return None;
    }
    let mut identities: Vec<u32> = Vec::with_capacity(list.len());
    for d in &list {
        if !identities.contains(&d.model.instance_id) {
            identities.push(d.model.instance_id);
        }
    }
    let n = identities.len() as i64;
    let pos = match current.and_then(|c| identities.iter().position(|&i| i == c)) {
        // Signed wrap that stays safe for any `dir`.
        Some(p) => ((p as i64 + dir as i64).rem_euclid(n)) as usize,
        None => 0,
    };
    let target = identities[pos];
    list.iter().find(|d| d.model.instance_id == target).copied()
}

/// An imported mechanical model: the cached `Originals.mechanical_models`
/// entry for one full PROP definition key (mesh handle, uniform `scale`,
/// authored material, root matrix) plus the authored part identity, so the
/// owning stage can mount it with its own material instead of the authored
/// one. Entries are per definition key, so definitions or groups sharing one
/// model instance never receive another definition's mesh or texture.
// `OriginalModel` is not Debug, so this stays Clone-only.
#[derive(Clone)]
pub struct ImportedMechanical {
    pub model: OriginalModel,
    /// Locale-resolved authored part name (`PartDefinition::name`).
    pub name: String,
    /// Catalog group the definition was selected from.
    pub group: u32,
    /// The definition's PROP instance (distinct from the model instance when
    /// several definitions share one RW4 link).
    pub definition_instance: u32,
}

/// Import the definition's authored RW4 geometry and diffuse/tint material on
/// demand through `editor::import_definition`, cached per full PROP key in
/// `Originals.mechanical_models` (repeat calls return the exact same entry).
/// The generic `Originals.models` cache is never touched. On failure nothing
/// is allocated: `import_definition` decodes geometry before adding any GPU
/// asset, so failed imports can be retried without growth.
///
/// Note: `first`/`next`/`eligible` borrow the catalog inside `Originals`;
/// clone the chosen `PartDefinition` (it is `Clone`) to free the borrow
/// before calling this with `&mut Originals`.
pub fn import(
    originals: &mut Originals,
    definition: &PartDefinition,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<ImportedMechanical, String> {
    if !originals.mechanical_models.contains_key(&definition.key) {
        let model = editor::import_definition(originals, definition, meshes, images, materials)?;
        originals.mechanical_models.insert(definition.key, model);
    }
    // Hand back exactly what is cached for this definition key.
    let model = originals
        .mechanical_models
        .get(&definition.key)
        .cloned()
        .expect("definition cached above");
    Ok(ImportedMechanical {
        model,
        name: definition.name.clone(),
        group: definition.key.group_id,
        definition_instance: definition.key.instance_id,
    })
}

/// The cached mechanical model a stage should mount for a saved instance id:
/// resolves the instance to the same deterministic first eligible definition
/// `first`/`next` use, then looks up its full PROP key in
/// `Originals.mechanical_models`. None when the group has no such part or it
/// has not been imported yet (call `import` for the selected definition).
pub fn cached<'a>(
    originals: &'a Originals,
    group: u32,
    instance: u32,
) -> Option<&'a OriginalModel> {
    let definition = eligible(originals, group)
        .into_iter()
        .find(|d| d.model.instance_id == instance)?;
    originals.mechanical_models.get(&definition.key)
}

/// Largest axis extent of the imported mesh's AABB in authored model units
/// (after the importer's Z-up to Y-up rotation and authored offset). None
/// when the handle is dead or the positions are missing, empty or all
/// non-finite. Non-finite individual vertices are ignored.
pub fn mesh_extent(meshes: &Assets<Mesh>, model: &OriginalModel) -> Option<f32> {
    let mesh = meshes.get(&model.handle)?;
    let VertexAttributeValues::Float32x3(positions) =
        mesh.try_attribute(Mesh::ATTRIBUTE_POSITION).ok()?
    else {
        return None;
    };
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for (lo, v) in min.iter_mut().zip(p.iter()) {
            if v.is_finite() {
                *lo = (*lo).min(*v);
            }
        }
        for (hi, v) in max.iter_mut().zip(p.iter()) {
            if v.is_finite() {
                *hi = (*hi).max(*v);
            }
        }
    }
    let extent = (max[0] - min[0]).max(max[1] - min[1]).max(max[2] - min[2]);
    (extent.is_finite() && extent > 0.).then_some(extent)
}

/// A uniform world scale that keeps the part's largest axis within `cap`
/// world units, preserving the imported orientation and material. Parts under
/// the cap keep `model.scale` exactly (authored relative scale); oversized
/// parts clamp to `cap` on their largest axis. The imported mesh already
/// carries the authored rotation/offset (`editor::assembled_mesh`), so only
/// the uniform scale changes. `model.scale` already includes the authored
/// `model_scale` (`editor::import_definition`), so stages still multiply
/// their own slot and per-part multipliers on top; pick `cap` with that
/// headroom. Returns `model.scale` unchanged for a non-positive/non-finite
/// `cap`, a dead handle, missing bounds, or a degenerate/non-finite world
/// extent.
pub fn capped_scale(meshes: &Assets<Mesh>, model: &OriginalModel, cap: f32) -> f32 {
    if !cap.is_finite() || cap <= 0. {
        return model.scale;
    }
    let Some(extent) = mesh_extent(meshes, model) else {
        return model.scale;
    };
    let world = extent * model.scale;
    if !world.is_finite() || world <= cap {
        model.scale
    } else {
        cap / extent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_catalog::Capabilities;
    use crate::dbpf::{DbpfEntry, ResourceKey};
    use std::path::PathBuf;

    fn definition(group: u32, instance: u32, model_instance: u32, name: &str) -> PartDefinition {
        PartDefinition {
            key: ResourceKey {
                type_id: asset_catalog::PROP,
                group_id: group,
                instance_id: instance,
            },
            model: ResourceKey {
                type_id: asset_catalog::RW4,
                group_id: group,
                instance_id: model_instance,
            },
            name: name.into(),
            is_null: false,
            price: 10,
            unlock_level: 1,
            min_scale: 0.5,
            max_scale: 2.,
            default_scale: 1.,
            model_scale: 1.,
            model_offset: [0.; 3],
            model_rotation: [0.; 3],
            diffuse: None,
            tint: None,
            spec_bump: None,
            abilities: Capabilities::default(),
        }
    }
    /// Catalog plus an index where every definition's model key resolves to a
    /// (never opened) fixture record, mirroring `PartCatalog::from_index`
    /// output: selection must still re-check validity defensively.
    fn originals_with(defs: Vec<PartDefinition>) -> Originals {
        let mut originals = Originals::default();
        for d in &defs {
            originals.resource_index.records.insert(
                d.model,
                (
                    PathBuf::from("mechanical_fixture.package"),
                    DbpfEntry {
                        key: d.model,
                        offset: 0,
                        disk_size: 0,
                        mem_size: 0,
                        is_compressed: false,
                    },
                ),
            );
        }
        originals.part_catalog.parts = defs;
        originals
    }
    fn names<'a>(list: &[&'a PartDefinition]) -> Vec<&'a str> {
        list.iter().map(|d| d.name.as_str()).collect()
    }

    #[test]
    fn selection_is_group_isolated_and_skips_null_helpers() {
        let mut buildings = definition(asset_catalog::BUILDING_PARTS, 30, 0x300, "Hut");
        let later = definition(asset_catalog::BUILDING_PARTS, 40, 0x400, "Tower");
        let mut null = definition(asset_catalog::BUILDING_PARTS, 20, 0x200, "NullHelper");
        null.is_null = true;
        let vehicle = definition(asset_catalog::VEHICLE_PARTS, 10, 0x100, "Wheels");
        buildings.price = 5; // Selection must ignore the abilities/price catalog sort.
        let originals = originals_with(vec![later.clone(), null, vehicle, buildings.clone()]);
        let list = eligible(&originals, asset_catalog::BUILDING_PARTS);
        // Deterministic order: definition instance, not catalog sort order.
        assert_eq!(names(&list), ["Hut", "Tower"]);
        assert!(list
            .iter()
            .all(|d| d.key.group_id == asset_catalog::BUILDING_PARTS));
        let vehicles = eligible(&originals, asset_catalog::VEHICLE_PARTS);
        assert_eq!(names(&vehicles), ["Wheels"]);
        assert_eq!(
            first(&originals, asset_catalog::VEHICLE_PARTS)
                .unwrap()
                .name,
            "Wheels"
        );
        // An unrelated group stays empty instead of leaking neighbors.
        assert!(eligible(&originals, asset_catalog::CREATURE_PARTS).is_empty());
        assert!(first(&originals, asset_catalog::CREATURE_PARTS).is_none());
    }

    #[test]
    fn invalid_model_links_are_excluded() {
        let mut wrong_type = definition(asset_catalog::VEHICLE_PARTS, 11, 0x110, "BadType");
        wrong_type.model.type_id = asset_catalog::PROP;
        let mut zero = definition(asset_catalog::VEHICLE_PARTS, 12, 0, "ZeroInstance");
        zero.model.instance_id = 0;
        // Present in the catalog, but the index has no record behind the link.
        let unresolved = definition(asset_catalog::VEHICLE_PARTS, 13, 0x130, "Unresolved");
        let good = definition(asset_catalog::VEHICLE_PARTS, 14, 0x140, "Good");
        let mut originals = originals_with(vec![good.clone(), wrong_type, zero]);
        originals.part_catalog.parts.push(unresolved);
        let list = eligible(&originals, asset_catalog::VEHICLE_PARTS);
        assert_eq!(names(&list), ["Good"]);
        // A stale saved instance falls back to the deterministic first.
        assert_eq!(
            next(&originals, asset_catalog::VEHICLE_PARTS, Some(0x130), 1)
                .unwrap()
                .name,
            "Good"
        );
    }

    #[test]
    fn duplicate_definitions_cycle_deterministically_with_wrap() {
        // Two building definitions legitimately share one RW4 model link.
        let a = definition(asset_catalog::BUILDING_PARTS, 1, 0xaaa, "A1");
        let b = definition(asset_catalog::BUILDING_PARTS, 2, 0xaaa, "A2");
        let c = definition(asset_catalog::BUILDING_PARTS, 3, 0xbbb, "B");
        let originals = originals_with(vec![c, b.clone(), a.clone()]);
        // Reporting keeps every definition, deterministically ordered.
        let list = eligible(&originals, asset_catalog::BUILDING_PARTS);
        assert_eq!(names(&list), ["A1", "A2", "B"]);
        // No saved instance yet: the first, in both directions.
        assert_eq!(
            next(&originals, asset_catalog::BUILDING_PARTS, None, 1)
                .unwrap()
                .name,
            "A1"
        );
        assert_eq!(
            next(&originals, asset_catalog::BUILDING_PARTS, None, -1)
                .unwrap()
                .name,
            "A1"
        );
        // A duplicated instance anchors on its first definition and steps to
        // the next *unique* model identity; repeated calls are stable.
        assert_eq!(
            next(&originals, asset_catalog::BUILDING_PARTS, Some(0xaaa), 1)
                .unwrap()
                .name,
            "B"
        );
        assert_eq!(
            next(&originals, asset_catalog::BUILDING_PARTS, Some(0xaaa), 1)
                .unwrap()
                .name,
            "B"
        );
        assert_eq!(
            next(&originals, asset_catalog::BUILDING_PARTS, Some(0xbbb), -1)
                .unwrap()
                .name,
            "A1"
        );
        // Large and negative offsets wrap over unique identities without
        // signed overflow (i32::MIN would panic on a usize/i32 add).
        assert_eq!(
            next(&originals, asset_catalog::BUILDING_PARTS, Some(0xaaa), 5)
                .unwrap()
                .name,
            "B"
        );
        assert_eq!(
            next(&originals, asset_catalog::BUILDING_PARTS, Some(0xbbb), -3)
                .unwrap()
                .name,
            "A1"
        );
        assert_eq!(
            next(
                &originals,
                asset_catalog::BUILDING_PARTS,
                Some(0xaaa),
                i32::MIN
            )
            .unwrap()
            .name,
            "A1"
        );
        // Repeated cycling walks every unique model identity and wraps,
        // instead of trapping on the duplicate definition of 0xaaa.
        let mut cur = Some(a.model.instance_id);
        let mut visited = Vec::new();
        for _ in 0..3 {
            let d = next(&originals, asset_catalog::BUILDING_PARTS, cur, 1).unwrap();
            visited.push(d.name.clone());
            cur = Some(d.model.instance_id);
        }
        // From A1's identity: B, wrap back to A1 (0xaaa), then B again.
        assert_eq!(visited, ["B", "A1", "B"]);
        assert!(!visited.contains(&"A2".to_string()));
        // Backward cycling wraps the same way.
        let mut visited_back = Vec::new();
        cur = Some(a.model.instance_id);
        for _ in 0..3 {
            let d = next(&originals, asset_catalog::BUILDING_PARTS, cur, -1).unwrap();
            visited_back.push(d.name.clone());
            cur = Some(d.model.instance_id);
        }
        assert_eq!(visited_back, ["B", "A1", "B"]);
        assert_eq!(b.model.instance_id, a.model.instance_id);
    }

    #[test]
    fn import_failure_leaves_gpu_asset_stores_unchanged() {
        // Record present but its package file can never open: `import` must
        // fail during the geometry read, before allocating any GPU asset or
        // inserting into the mechanical cache.
        let mut originals = originals_with(vec![]);
        let d = definition(asset_catalog::VEHICLE_PARTS, 1, 0xdead, "Turret");
        originals.resource_index.records.insert(
            d.model,
            (
                PathBuf::from("this_package_does_not_exist.pkg"),
                DbpfEntry {
                    key: d.model,
                    offset: 0,
                    disk_size: 8,
                    mem_size: 8,
                    is_compressed: false,
                },
            ),
        );
        originals.part_catalog.parts.push(d.clone());
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        assert!(import(&mut originals, &d, &mut meshes, &mut images, &mut materials).is_err());
        assert_eq!((meshes.len(), images.len(), materials.len()), (0, 0, 0));
        assert!(originals.mechanical_models.is_empty());
        assert!(originals.models.is_empty());
        // A link with no record at all fails the same way, retry included.
        let missing = definition(asset_catalog::BUILDING_PARTS, 2, 0xbeef, "Clinic");
        for _ in 0..2 {
            assert!(import(
                &mut originals,
                &missing,
                &mut meshes,
                &mut images,
                &mut materials
            )
            .is_err());
            assert_eq!((meshes.len(), images.len(), materials.len()), (0, 0, 0));
            assert!(originals.mechanical_models.is_empty());
            assert!(originals.models.is_empty());
        }
        // Selection must not surface the failed definitions either: their
        // model links never resolved into the resource index.
        assert!(cached(&originals, asset_catalog::VEHICLE_PARTS, 0xdead).is_none());
        assert!(cached(&originals, asset_catalog::BUILDING_PARTS, 0xbeef).is_none());
    }

    #[test]
    fn cache_is_per_definition_key_and_reuses_exact_entry() {
        // Two definitions in different groups share one RW4 model instance,
        // exactly the aliasing that trapped the instance-keyed shared cache.
        let building = definition(asset_catalog::BUILDING_PARTS, 1, 0xaaa, "Depot");
        let vehicle = definition(asset_catalog::VEHICLE_PARTS, 5, 0xaaa, "Hauler");
        let mut originals = originals_with(vec![building.clone(), vehicle.clone()]);
        // Seed both mechanical cache entries with distinct meshes/materials,
        // as two exact-key imports would have produced.
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let red = materials.add(StandardMaterial {
            base_color: Color::srgb(1., 0., 0.),
            ..Default::default()
        });
        let blue = materials.add(StandardMaterial {
            base_color: Color::srgb(0., 0., 1.),
            ..Default::default()
        });
        let mut entry = |handle_i: u32, scale: f32, material| OriginalModel {
            handle: meshes.add(Sphere::new(handle_i as f32)),
            scale,
            instance: 0xaaa,
            material: Some(material),
            ..Default::default()
        };
        let a = entry(1, 1., red.clone());
        let b = entry(2, 2., blue.clone());
        let (a_handle, b_handle) = (a.handle.clone(), b.handle.clone());
        originals.mechanical_models.insert(building.key, a);
        originals.mechanical_models.insert(vehicle.key, b);
        // Each group resolves the shared instance to its own full-key entry:
        // distinct mesh and material, never the other definition's.
        let ga = cached(&originals, asset_catalog::BUILDING_PARTS, 0xaaa).unwrap();
        let gb = cached(&originals, asset_catalog::VEHICLE_PARTS, 0xaaa).unwrap();
        assert_eq!(ga.handle, a_handle);
        assert_eq!(gb.handle, b_handle);
        assert_ne!(ga.handle, gb.handle);
        assert_eq!(ga.material.as_ref(), Some(&red));
        assert_eq!(gb.material.as_ref(), Some(&blue));
        assert_eq!((ga.scale, gb.scale), (1., 2.));
        // Other groups see nothing.
        assert!(cached(&originals, asset_catalog::CREATURE_PARTS, 0xaaa).is_none());
        // Repeat import of a cached definition is an exact cache hit: the
        // fixture package cannot open, so any real decode attempt would fail;
        // the hit path must return the seeded entry without allocating.
        let counts = (meshes.len(), images.len(), materials.len());
        let got = import(
            &mut originals,
            &building,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .expect("cache hit needs no decode");
        assert_eq!(got.model.handle, a_handle);
        assert_eq!(got.model.material.as_ref(), Some(&red));
        assert_eq!(got.model.scale, 1.);
        assert_eq!(got.name, "Depot");
        assert_eq!(got.group, asset_catalog::BUILDING_PARTS);
        assert_eq!(got.definition_instance, 1);
        assert_eq!((meshes.len(), images.len(), materials.len()), counts);
        assert_eq!(originals.mechanical_models.len(), 2);
        // The generic instance-keyed cache stays untouched by mechanical use.
        assert!(originals.models.is_empty());
    }

    #[test]
    fn capped_scale_bounds_oversized_models_and_keeps_finite_checks() {
        let mut meshes = Assets::<Mesh>::default();
        let mut mesh = Mesh::new(
            bevy::render::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0., 0., 0.], [2., 0., 0.], [0., 1., 0.]],
        );
        let handle = meshes.add(mesh);
        let model = OriginalModel {
            handle,
            scale: 3.,
            ..Default::default()
        };
        assert_eq!(mesh_extent(&meshes, &model), Some(2.));
        // 2 * 3 = 6 > cap 4: clamp the largest axis to the cap.
        assert_eq!(capped_scale(&meshes, &model, 4.), 2.);
        // Under the cap the authored relative scale is preserved exactly.
        assert_eq!(capped_scale(&meshes, &model, 10.), 3.);
        // Invalid caps and dead handles leave the scale untouched.
        assert_eq!(capped_scale(&meshes, &model, 0.), 3.);
        assert_eq!(capped_scale(&meshes, &model, f32::NAN), 3.);
        let dead = OriginalModel {
            handle: Default::default(),
            scale: 7.,
            ..Default::default()
        };
        assert_eq!(mesh_extent(&meshes, &dead), None);
        assert_eq!(capped_scale(&meshes, &dead, 1.), 7.);
        // Non-finite vertices are ignored; all-non-finite bounds preserve scale.
        let mut bad = Mesh::new(
            bevy::render::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        bad.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0., 0., 0.], [f32::NAN, 0., 0.], [1., 0., 0.]],
        );
        let model = OriginalModel {
            handle: meshes.add(bad),
            scale: 2.,
            ..Default::default()
        };
        assert_eq!(mesh_extent(&meshes, &model), Some(1.));
        let mut all_bad = Mesh::new(
            bevy::render::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        all_bad.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[f32::NAN; 3], [f32::INFINITY; 3]],
        );
        let model = OriginalModel {
            handle: meshes.add(all_bad),
            scale: 5.,
            ..Default::default()
        };
        assert_eq!(mesh_extent(&meshes, &model), None);
        assert_eq!(capped_scale(&meshes, &model, 1.), 5.);
    }

    #[test]
    #[ignore = "requires original Spore packages"]
    fn retail_building_and_vehicle_geometry_and_material() {
        let install = crate::spore_install::discover().unwrap();
        let index = asset_catalog::ResourceIndex::load(&install.packages);
        let catalog = asset_catalog::PartCatalog::from_index(&index);
        let mut originals = Originals {
            part_catalog: catalog,
            resource_index: index,
            ..Default::default()
        };
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        for group in [asset_catalog::BUILDING_PARTS, asset_catalog::VEHICLE_PARTS] {
            // Owned snapshot: cloning frees the catalog borrow so `import`
            // can take `&mut originals` (stages clone the chosen definition
            // the same way before importing).
            let list: Vec<PartDefinition> =
                eligible(&originals, group).into_iter().cloned().collect();
            assert!(
                !list.is_empty(),
                "retail group {group:08x} has eligible parts"
            );
            // Deterministic unique model identities, in selection order.
            let mut identities: Vec<u32> = Vec::new();
            for d in &list {
                if !identities.contains(&d.model.instance_id) {
                    identities.push(d.model.instance_id);
                }
            }
            // Full forward and backward passes each visit every unique
            // identity once and wrap back to the start.
            let n = identities.len();
            let mut cur = Some(identities[0]);
            for step in 1..=n {
                let d = next(&originals, group, cur, 1).unwrap();
                assert_eq!(
                    d.model.instance_id,
                    identities[step % n],
                    "forward step {step}"
                );
                cur = Some(d.model.instance_id);
            }
            assert_eq!(cur, Some(identities[0]), "forward cycle wraps");
            for step in 1..=n {
                let d = next(&originals, group, cur, -1).unwrap();
                let expect = identities[(n - step % n) % n];
                assert_eq!(d.model.instance_id, expect, "backward step {step}");
                cur = Some(d.model.instance_id);
            }
            assert_eq!(cur, Some(identities[0]), "backward cycle wraps");
            // Some authored parts use RW4 geometry our importer skips; the
            // first importable definition must carry real geometry + material.
            let mut winner: Option<(PartDefinition, ImportedMechanical)> = None;
            for d in &list {
                if let Ok(m) = import(&mut originals, d, &mut meshes, &mut images, &mut materials) {
                    winner = Some((d.clone(), m));
                    break;
                }
            }
            let (winning, got) = winner.unwrap_or_else(|| panic!("group {group:08x} imports"));
            assert_eq!(got.definition_instance, winning.key.instance_id);
            assert_eq!(got.group, group);
            assert!(!got.name.is_empty());
            assert!(got.model.scale.is_finite() && got.model.scale > 0.);
            assert!(
                got.model.material.is_some(),
                "authored diffuse/tint material"
            );
            let extent = mesh_extent(&meshes, &got.model).expect("positions readable");
            assert!(extent.is_finite() && extent > 0.);
            // `cached` resolves a saved instance through the first eligible
            // definition of that identity; it must agree with the cache that
            // the winning import produced (or report the canonical entry as
            // still unimported when a later duplicate won).
            let canonical = list
                .iter()
                .find(|d| d.model.instance_id == winning.model.instance_id)
                .unwrap();
            if canonical.key == winning.key {
                assert_eq!(
                    cached(&originals, group, winning.model.instance_id).map(|m| m.handle.clone()),
                    Some(got.model.handle.clone())
                );
            } else {
                assert!(cached(&originals, group, winning.model.instance_id).is_none());
            }
            // Repeat import of the exact winning definition reuses the cache
            // entry: same mesh/material handles, no mesh growth.
            let before = meshes.len();
            let again = import(
                &mut originals,
                &winning,
                &mut meshes,
                &mut images,
                &mut materials,
            )
            .expect("cached repeat import");
            assert_eq!(again.model.handle, got.model.handle);
            assert_eq!(again.model.material, got.model.material);
            assert_eq!(meshes.len(), before);
        }
    }
}
