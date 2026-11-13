//! Creation assets: mount original BLD/VCL/UFO creation assemblies as Bevy
//! geometry presets.
//!
//! Source of truth: the installed game packages (BLD/VCL/UFO records parsed
//! by `creation_records`). This module is a renderer only — it reads parsed
//! `CreationRecord` data and spawns meshes via `mechanical_assets::import`.
//!
//! Coordinate convention: XML transform data is Z-up with Y longitudinal
//! (same as raw RW4). The importer already applies C = Rx(-π/2) to convert
//! to Y-up. The renderer conjugates XML transforms: pos_world = C·(pos·5),
//! rot_world = C·R·C^T. Bounds use full transformed mesh positions. Global
//! fit only — no per-block capping.

use bevy::mesh::{Mesh, VertexAttributeValues};
use bevy::prelude::*;

use crate::creation_records::{CreationPaint, CreationRecord};
use crate::dbpf::ResourceKey;
use crate::mechanical_assets::{self, ImportedMechanical};
use crate::stages::StageOwned;
use crate::{OriginalModel, Originals};

// ---------------------------------------------------------------------------
// Public renderer contract
// ---------------------------------------------------------------------------

/// Which kind of preset to mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetKind {
    Building,
    LandVehicle,
    SeaVehicle,
    AirVehicle,
    Spaceship,
}

/// Component on the normalized assembly root.
#[derive(Component, Debug, Clone, Copy)]
pub struct OriginalCreation {
    pub key: ResourceKey,
}

/// Component on each part mesh child.
#[derive(Component, Debug, Clone, Copy)]
pub struct OriginalCreationPart {
    pub definition: ResourceKey,
}

/// Cached imported creation: the fully resolved assembly ready to spawn.
/// Contains the per-block imported mechanical models and the assembly's
/// un-scaled local bounds so the caller can globally fit to `max_extent`.
#[derive(Clone)]
pub struct ImportedCreation {
    pub key: ResourceKey,
    pub blocks: Vec<ImportedBlock>,
    /// Local-space AABB of the full assembly BEFORE global fitting.
    pub local_min: Vec3,
    pub local_max: Vec3,
}

#[derive(Clone)]
pub struct ImportedBlock {
    pub definition: ResourceKey,
    pub mechanical: ImportedMechanical,
    pub scale: f32,
    pub position: Vec3,
    pub orientation: Mat3,
    pub paints: Vec<CreationPaint>,
    pub handles: Vec<(u32, f32)>,
    pub morph: crate::morph::MorphApplication,
}

/// Geometry identity includes every assigned handle. A shared PROP model
/// must not make differently shaped blocks share one vertex buffer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MorphMeshKey {
    pub definition: ResourceKey,
    pub weights: Vec<(u32, u32)>,
}
fn morph_mesh_key(definition: ResourceKey, handles: &[(u32, f32)]) -> Result<MorphMeshKey, String> {
    if handles.iter().any(|(_, w)| !w.is_finite()) {
        return Err("non-finite morph handle".into());
    }
    let mut weights: Vec<_> = handles
        .iter()
        .map(|(id, w)| {
            (
                *id,
                if *w <= 0. {
                    0f32.to_bits()
                } else {
                    w.min(1.).to_bits()
                },
            )
        })
        .collect();
    weights.sort_unstable_by_key(|w| w.0);
    if weights.windows(2).any(|w| w[0].0 == w[1].0) {
        return Err("duplicate morph handle".into());
    }
    Ok(MorphMeshKey {
        definition,
        weights,
    })
}

// ---------------------------------------------------------------------------
// Coordinate basis conversion
// ---------------------------------------------------------------------------

/// XML is Z-up with Y longitudinal (like RW4). The importer already applied
/// C = rotation_x(-PI/2) to convert to Y-up. So we must conjugate the XML
/// transform: pos_world = C * (pos_xml * 5.0) and rot_world = C * R_xml * C^T.
fn xml_basis() -> Mat3 {
    Mat3::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

fn conjugate(rotation: Mat3) -> Mat3 {
    let c = xml_basis();
    c * rotation * c.transpose()
}

fn xml_position_to_world(p: [f32; 3]) -> Vec3 {
    xml_basis() * (Vec3::new(p[0], p[1], p[2]) * 5.0)
}

// ---------------------------------------------------------------------------
// Mesh bounds from full transformed positions
// ---------------------------------------------------------------------------

/// Read the mesh's vertex positions in model units.
pub(crate) fn mesh_positions(meshes: &Assets<Mesh>, model: &OriginalModel) -> Option<Vec<Vec3>> {
    let mesh = meshes.get(&model.handle)?;
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    Some(
        positions
            .iter()
            .map(|p| Vec3::new(p[0], p[1], p[2]))
            .collect(),
    )
}

/// Compute the AABB of a mesh after applying translation, rotation, and uniform
/// scale. Uses every vertex position, not just the max unrotated axis.
fn transformed_bounds(
    positions: &[Vec3],
    translation: Vec3,
    rotation: Mat3,
    scale: f32,
) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for v in positions {
        let p = translation + rotation * (v * scale);
        min = min.min(p);
        max = max.max(p);
    }
    (min, max)
}

/// Check that a 3x3 matrix is a proper rotation: positive determinant near +1
/// AND orthonormal columns (R^T·R ≈ I).
fn is_rotation(mat: &Mat3) -> bool {
    let det = mat.determinant();
    if det < 0.99 || det > 1.01 {
        return false;
    }
    // Check orthonormality: columns should be unit length and mutually orthogonal
    let c0 = mat.col(0);
    let c1 = mat.col(1);
    let c2 = mat.col(2);
    let unit = |v: Vec3| (v.length() - 1.0).abs() < 0.01;
    let ortho = |a: Vec3, b: Vec3| a.dot(b).abs() < 0.01;
    unit(c0) && unit(c1) && unit(c2) && ortho(c0, c1) && ortho(c0, c2) && ortho(c1, c2)
}

// ---------------------------------------------------------------------------
// Preset selection
// ---------------------------------------------------------------------------

const TYPE_BLD: u32 = 0x2399BE55;
const TYPE_VCL: u32 = 0x24682294;
const TYPE_UFO: u32 = 0x476A98C7;

/// Select the best creation record for `kind`.
/// Priority: explicit name match (gencar/gensea/genair/genufo/first BLD),
/// skip PaintSwatch records.
pub fn select_creation<'a>(
    creations: &'a [CreationRecord],
    kind: PresetKind,
) -> Option<&'a CreationRecord> {
    let candidates: Vec<_> = creations
        .iter()
        .filter(|c| {
            let is_paint_swatch = c.name.contains("PaintSwatch");
            if is_paint_swatch {
                return false;
            }
            match kind {
                PresetKind::Building => c.key.type_id == TYPE_BLD,
                PresetKind::LandVehicle => c.key.type_id == TYPE_VCL && c.name == "gencar",
                PresetKind::SeaVehicle => c.key.type_id == TYPE_VCL && c.name == "gensea",
                PresetKind::AirVehicle => c.key.type_id == TYPE_VCL && c.name == "genair",
                PresetKind::Spaceship => c.key.type_id == TYPE_UFO && c.name == "genufo",
            }
        })
        .collect();

    candidates.first().copied()
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Import all blocks of a creation record, caching each PROP definition
/// through `mechanical_assets::import`. Returns the assembled creation
/// or an error if any block's definition is missing or fails to import.
/// All GPU allocations happen before the caller spawns any ECS children.
pub fn import_creation(
    originals: &mut Originals,
    record: &CreationRecord,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<ImportedCreation, String> {
    if record.blocks.is_empty() {
        return Err("creation has no blocks".into());
    }
    let mut imported_blocks = Vec::new();
    let mut local_min = Vec3::splat(f32::INFINITY);
    let mut local_max = Vec3::splat(f32::NEG_INFINITY);

    for block in &record.blocks {
        if !block.scale.is_finite()
            || block.scale <= 0.0
            || !block.position.iter().all(|value| value.is_finite())
        {
            return Err("invalid creation block scale or position".into());
        }
        // Reject non-finite or non-rotation orientations BEFORE Quat conversion.
        // is_rotation checks: positive determinant near +1 AND orthonormal columns.
        for row in &block.orientation {
            if !row.iter().all(|&v| v.is_finite()) {
                return Err(format!(
                    "non-finite orientation in block {:08X}:{:08X}",
                    block.definition.group_id, block.definition.instance_id
                ));
            }
        }
        // The XML rows are the block's local axes (row-vector convention,
        // `v * M`, as `creature_skin` / `creature_limbs` read them): as a
        // column-math rotation the rows are the columns. (Was the transpose,
        // i.e. the inverse rotation: the rim parts of `genufo` stood at
        // random tilts; helper maxis-edit round 2.)
        let r_xml = Mat3::from_cols(
            Vec3::from_array(block.orientation[0]),
            Vec3::from_array(block.orientation[1]),
            Vec3::from_array(block.orientation[2]),
        );
        if !is_rotation(&r_xml) {
            return Err(format!(
                "orientation is not a proper rotation (reflection or non-orthonormal) in block {:08X}:{:08X}",
                block.definition.group_id, block.definition.instance_id
            ));
        }

        // Resolve the PROP definition from the part catalog.
        // Clone to release the immutable borrow before mechanical_assets::import
        // takes &mut Originals.
        let part_def = originals
            .part_catalog
            .parts
            .iter()
            .find(|d| d.key == block.definition)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "missing PROP definition {:08X}:{:08X}",
                    block.definition.group_id, block.definition.instance_id
                )
            })?;

        // Import through mechanical_assets (exact PROP key caching)
        let mut imported = mechanical_assets::import(
            originals, &part_def, meshes, images, materials,
        )
        .map_err(|e| format!("import failed for {:08X}: {}", part_def.key.instance_id, e))?;
        let key = morph_mesh_key(block.definition, &block.handles)?;
        let (handle, surface_handles, morph) =
            if let Some(cached) = originals.creation_part_meshes.get(&key) {
                cached.clone()
            } else {
                let (mesh, surfaces, applied) =
                    crate::editor::creation_mesh(originals, &part_def, &block.handles)
                        .map_err(|e| format!("morph {:08X}: {e}", part_def.key.instance_id))?;
                let entry = (
                    meshes.add(mesh),
                    surfaces
                        .into_iter()
                        .map(|(section, m)| (section, meshes.add(m)))
                        .collect(),
                    applied,
                );
                originals.creation_part_meshes.insert(key, entry.clone());
                entry
            };
        imported.model.handle = handle;
        for surface in &mut imported.model.surfaces {
            surface.handle = surface_handles
                .iter()
                .find(|(section, _)| *section == surface.source_section)
                .map(|(_, handle)| handle.clone())
                .ok_or("morphed surface has no matching source mesh")?;
        }

        // Compute this block's local bounds in world space using full transformed positions.
        // Reject empty or non-finite geometry rather than silently producing infinite bounds.
        let positions = mesh_positions(meshes, &imported.model).ok_or_else(|| {
            format!(
                "no vertex positions for block {:08X}:{:08X}",
                block.definition.group_id, block.definition.instance_id
            )
        })?;
        if positions.is_empty() || !positions.iter().all(|p| p.is_finite()) {
            return Err(format!(
                "empty or non-finite vertex positions for block {:08X}:{:08X}",
                block.definition.group_id, block.definition.instance_id
            ));
        }

        // Convert XML position (Z-up, ~1/5 world units) to world space (Y-up, world units).
        let pos_world = xml_position_to_world(block.position);

        // Convert XML orientation (row-major 3x3, Z-up) to world space (Y-up) via conjugation.
        let orient_world = conjugate(r_xml);

        // Compute this block's local bounds in world space using full transformed positions.
        let block_scale = block.scale * imported.model.scale;
        let (bmin, bmax) = transformed_bounds(&positions, pos_world, orient_world, block_scale);
        local_min = local_min.min(bmin);
        local_max = local_max.max(bmax);

        imported_blocks.push(ImportedBlock {
            definition: block.definition,
            mechanical: imported,
            scale: block.scale,
            position: pos_world,
            orientation: orient_world,
            paints: block.paints.clone(),
            handles: block.handles.clone(),
            morph,
        });
    }

    let extent = (local_max - local_min).max_element();
    if !local_min.is_finite() || !local_max.is_finite() || !extent.is_finite() || extent <= 0.0 {
        return Err("creation has invalid transformed bounds".into());
    }
    Ok(ImportedCreation {
        key: record.key,
        blocks: imported_blocks,
        local_min,
        local_max,
    })
}

// ---------------------------------------------------------------------------
// Public renderer contract
// ---------------------------------------------------------------------------

/// Mounts the selected preset as children of `parent`. Returns `Ok(false)` if no
/// catalog/preset is available; `Err` if a preset was selected but failed to
/// import (missing defs, decode failure). On success, spawns exactly one
/// `OriginalCreation` root with `OriginalCreationPart` children, all
/// `StageOwned`, and returns `Ok(true)`.
///
/// All import work (GPU allocations, bounds computation) completes before any
/// ECS child is spawned. On failure, nothing is spawned.
pub fn mount_preset(
    commands: &mut Commands,
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    parent: Entity,
    kind: PresetKind,
    max_extent: f32,
) -> Result<bool, String> {
    let record = match select_creation(&originals.creation_catalog.creations, kind) {
        Some(r) => r.clone(),
        None => return Ok(false),
    };

    // Reject empty creation records before any work.
    if record.blocks.is_empty() {
        return Err(format!("creation {} has no blocks", record.name));
    }

    // Validate max_extent before any GPU allocation or ECS spawn.
    if !max_extent.is_finite() || max_extent <= 0.0 {
        return Err(format!("invalid max_extent: {max_extent}"));
    }

    // Check cache first
    if !originals.creation_models.contains_key(&record.key) {
        let imported = import_creation(originals, &record, meshes, images, materials)?;
        originals.creation_models.insert(record.key, imported);
    }
    let imported = originals.creation_models.get(&record.key).unwrap();

    // Validate assembled bounds before any ECS spawn.
    if imported.blocks.is_empty()
        || !imported.local_min.is_finite()
        || !imported.local_max.is_finite()
    {
        return Err(format!("creation {} has invalid bounds", record.name));
    }

    // Global fit: compute scale to fit the full assembly into max_extent.
    // imported bounds are already in world space (Y-up, world units).
    let local_size = imported.local_max - imported.local_min;
    let max_dim = local_size.max_element();
    if !max_dim.is_finite() || max_dim <= 0.0 {
        return Err("creation has zero or invalid extent".into());
    }
    let global_scale = max_extent / max_dim;
    if !global_scale.is_finite() || global_scale <= 0.0 {
        return Err("creation fit scale exceeds finite range".into());
    }

    // Center offset: vehicles and UFOs center at origin; buildings ground at minY.
    let center = if kind == PresetKind::Building {
        Vec3::new(
            (imported.local_min.x + imported.local_max.x) * 0.5,
            imported.local_min.y,
            (imported.local_min.z + imported.local_max.z) * 0.5,
        )
    } else {
        (imported.local_min + imported.local_max) * 0.5
    };

    // Spawn the assembly root.
    let root = commands
        .spawn((
            StageOwned,
            OriginalCreation { key: imported.key },
            Transform::default(),
        ))
        .id();

    // Spawn each block as a child.
    for block in &imported.blocks {
        let mesh_handle = block.mechanical.model.draw_mesh();

        // Material: every block gets an independent clone so player C recolor
        // never mutates the shared cache. Painted blocks also get color1 tint.
        let material = {
            let base = block
                .mechanical
                .model
                .material
                .as_ref()
                .and_then(|h| materials.get(h))
                .cloned()
                .unwrap_or_else(|| StandardMaterial {
                    base_color: Color::srgb(0.6, 0.7, 0.85),
                    perceptual_roughness: 0.4,
                    ..default()
                });
            let mut mat = base;
            if let Some(paint) = block.paints.first() {
                mat.base_color = Color::srgb(paint.color1[0], paint.color1[1], paint.color1[2]);
            }
            materials.add(mat)
        };

        // Build the block's transform: positions already in world space.
        let local_pos = (block.position - center) * global_scale;
        let transform = Transform::from_translation(local_pos)
            .with_rotation(Quat::from_mat3(&block.orientation).normalize())
            .with_scale(Vec3::splat(
                block.scale * block.mechanical.model.scale * global_scale,
            ));

        let child = commands
            .spawn((
                StageOwned,
                OriginalCreationPart {
                    definition: block.definition,
                },
                Mesh3d(mesh_handle),
                MeshMaterial3d(material),
                transform,
            ))
            .id();

        commands.entity(root).add_child(child);
    }

    commands.entity(parent).add_child(root);
    Ok(true)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creation_records::CreationBlock;
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{PrimitiveTopology, VertexAttributeValues};

    #[test]
    fn xml_z_maps_to_world_y() {
        // XML Z (up) should map to world Y (up)
        let z = Vec3::new(0.0, 0.0, 1.0);
        let result = xml_basis() * z;
        assert!((result - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn xml_y_maps_to_world_neg_z() {
        // XML Y (longitudinal) should map to world -Z
        let y = Vec3::new(0.0, 1.0, 0.0);
        let result = xml_basis() * y;
        assert!((result - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-5);
    }

    #[test]
    fn conjugated_identity_is_identity() {
        let identity = Mat3::IDENTITY;
        let result = conjugate(identity);
        assert!(result.abs_diff_eq(Mat3::IDENTITY, 1e-5));
    }

    #[test]
    fn conjugated_z_rotation_maps_to_y_rotation() {
        // A 90-degree rotation about XML Z should become a 90-degree rotation about world Y
        let r_xml = Mat3::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let result = conjugate(r_xml);
        let expected = Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
        assert!(result.abs_diff_eq(expected, 1e-5));
    }

    #[test]
    fn is_rotation_accepts_proper_rotation() {
        let r = Mat3::from_rotation_y(0.5);
        assert!(is_rotation(&r));
    }

    #[test]
    fn is_rotation_rejects_reflection() {
        let r = Mat3::from_cols(-Vec3::X, Vec3::Y, Vec3::Z);
        assert!(!is_rotation(&r));
    }

    #[test]
    fn transformed_bounds_covers_all_vertices() {
        let positions = vec![
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(0.0, 0.0, -3.0),
        ];
        let translation = Vec3::new(10.0, 20.0, 30.0);
        let rotation = Mat3::IDENTITY;
        let scale = 2.0;
        let (min, max) = transformed_bounds(&positions, translation, rotation, scale);
        assert_eq!(min, Vec3::new(8.0, 20.0, 24.0));
        assert_eq!(max, Vec3::new(12.0, 24.0, 30.0));
    }

    #[test]
    fn transformed_bounds_with_rotation() {
        let positions = vec![Vec3::new(1.0, 0.0, 0.0)];
        let translation = Vec3::ZERO;
        let rotation = Mat3::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let scale = 1.0;
        let (min, max) = transformed_bounds(&positions, translation, rotation, scale);
        assert!(min.abs_diff_eq(Vec3::Y, 1e-5));
        assert!((max.x - 0.0).abs() < 1e-5 && (max.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn select_creation_returns_none_for_empty_catalog() {
        let empty: Vec<CreationRecord> = vec![];
        assert!(select_creation(&empty, PresetKind::Spaceship).is_none());
        assert!(select_creation(&empty, PresetKind::Building).is_none());
        assert!(select_creation(&empty, PresetKind::LandVehicle).is_none());
    }

    #[test]
    fn import_creation_fails_on_missing_definition() {
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut originals = Originals::default();
        originals.creation_catalog = crate::creation_records::CreationCatalog::default();

        let record = CreationRecord {
            key: ResourceKey {
                type_id: TYPE_UFO,
                group_id: 0x40656200,
                instance_id: 0x6ABF210B,
            },
            format_version: 15,
            name: "genufo".to_string(),
            model_type: 0x98E03C0D,
            blocks: vec![CreationBlock {
                definition: ResourceKey {
                    type_id: 0x00B1B104,
                    group_id: 0x40646000,
                    instance_id: 0x8D53A9DE,
                },
                scale: 1.0,
                position: [0.0, 0.0, 0.0],
                orientation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                paints: vec![],
                handles: vec![],
            }],
        };

        let result = import_creation(
            &mut originals,
            &record,
            &mut meshes,
            &mut images,
            &mut materials,
        );
        let err = result
            .err()
            .expect("expected Err for missing PROP definition");
        assert!(err.contains("missing PROP definition"));
    }

    #[test]
    fn select_creation_prefers_genufo_over_paint_swatch() {
        let genufo = CreationRecord {
            key: ResourceKey {
                type_id: TYPE_UFO,
                group_id: 0x40656200,
                instance_id: 0x6ABF210B,
            },
            format_version: 15,
            name: "genufo".to_string(),
            model_type: 0x98E03C0D,
            blocks: vec![],
        };
        let paint_swatch = CreationRecord {
            key: ResourceKey {
                type_id: TYPE_UFO,
                group_id: 0x40656200,
                instance_id: 0xB11D3AA9,
            },
            format_version: 15,
            name: "UFOPaintSwatch".to_string(),
            model_type: 0x98E03C0D,
            blocks: vec![],
        };
        let creations = vec![paint_swatch.clone(), genufo.clone()];
        let selected = select_creation(&creations, PresetKind::Spaceship);
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().name, "genufo");
    }

    #[test]
    fn mesh_positions_extracts_vertex_data() {
        let mut meshes = Assets::<Mesh>::default();
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        let mut mesh = mesh;
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            VertexAttributeValues::Float32x3(vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            ]),
        );
        let handle = meshes.add(mesh);
        let model = OriginalModel {
            source_definition: None,
            handle,
            surfaces: vec![],
            scale: 1.0,
            instance: 0,
            material: None,
            root: None,
            rig_bones: 0,
        };
        let positions = mesh_positions(&meshes, &model).unwrap();
        assert_eq!(positions.len(), 3);
        assert_eq!(positions[0], Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(positions[1], Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(positions[2], Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn geometry_cache_separates_weights_and_definition_variants() {
        let key = ResourceKey {
            group_id: 1,
            instance_id: 2,
            type_id: 3,
        };
        let a = morph_mesh_key(key, &[(9, 0.25), (7, -0.0)]).unwrap();
        assert_eq!(a, morph_mesh_key(key, &[(7, -4.), (9, 0.25)]).unwrap());
        assert_ne!(a, morph_mesh_key(key, &[(9, 0.75), (7, 0.)]).unwrap());
        assert_ne!(
            a,
            morph_mesh_key(ResourceKey { group_id: 4, ..key }, &[(9, 0.25), (7, 0.)]).unwrap()
        );
        assert_eq!(
            morph_mesh_key(key, &[(7, 1.)]).unwrap(),
            morph_mesh_key(key, &[(7, 9.)]).unwrap()
        );
        assert!(morph_mesh_key(key, &[(7, 0.), (7, 1.)]).is_err());
        assert!(morph_mesh_key(key, &[(7, f32::NAN)]).is_err());
    }
}
