//! Installed Cell-stage species, body skin and food models. The scene renderer
//! is still a Bevy preview; authored rules are decoded without calling them
//! a verified reimplementation of the native Cell simulation.
use crate::{
    asset_catalog::{Capabilities, PartDefinition, CELL_PARTS, RW4},
    dbpf::ResourceKey,
    stages::StageOwned,
    OriginalModel, Originals,
};
use bevy::{mesh::VertexAttributeValues, prelude::*};

pub const CELL_FILE: u32 = 0xdfad9f51;
pub const CELL_STRUCTURE: u32 = 0x4b9ef6dc;
pub const CELL_MODEL_GROUP: u32 = 0x40617100;
const CELL_ANATOMY_GROUP: u32 = 0x40616200;
const CELL_GLOBALS: u32 = 0x2a3ce5b7;

#[derive(Clone, Copy, Debug)]
pub struct NativeCellAi {
    pub awareness: f32,
    pub awareness_food: f32,
    pub awareness_predator: f32,
    pub movement: u32,
    pub speed: f32,
    pub chase: f32,
    pub wander: f32,
    pub flee: f32,
    pub turn: f32,
    pub flocking: bool,
    pub fears_weapons: bool,
    pub fears_poison: bool,
    pub ignores_food: bool,
}
#[derive(Clone, Debug)]
pub struct CellRule {
    pub key: ResourceKey,
    pub structure: u32,
    pub name: String,
    pub ai_type: i32,
    pub ai: NativeCellAi,
    pub size: [f32; 2],
    pub food_value: i32,
    pub hp_value: i32,
}
fn word(bytes: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or("truncated Cell field")?
            .try_into()
            .unwrap(),
    ))
}
fn scalar(bytes: &[u8], at: usize) -> Result<f32, String> {
    let f = f32::from_bits(word(bytes, at)?);
    if f.is_finite() {
        Ok(f)
    } else {
        Err("non-finite Cell field".into())
    }
}
/// The 0x31c on-disk layout follows CellFile.read in SporeModder-FX. These
/// are file offsets, not the SDK's pointers after resource deserialization.
pub fn parse_rule(key: ResourceKey, bytes: &[u8]) -> Result<CellRule, String> {
    if bytes.len() != 0x31c {
        return Err(format!("unsupported Cell byte count {}", bytes.len()));
    }
    let name = String::from_utf16_lossy(
        &bytes[4..164]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .take_while(|c| *c != 0)
            .collect::<Vec<_>>(),
    );
    let b = 0xe0;
    Ok(CellRule {
        key,
        structure: word(bytes, 0)?,
        name,
        ai_type: word(bytes, b)? as i32,
        ai: NativeCellAi {
            awareness: scalar(bytes, b + 4)?,
            awareness_food: scalar(bytes, b + 8)?,
            awareness_predator: scalar(bytes, b + 12)?,
            movement: word(bytes, b + 16)?,
            flocking: bytes[b + 20] != 0,
            speed: scalar(bytes, b + 24)?,
            chase: scalar(bytes, b + 28)?,
            wander: scalar(bytes, b + 32)?,
            flee: scalar(bytes, b + 36)?,
            turn: scalar(bytes, b + 64)?,
            fears_weapons: bytes[b + 142] != 0,
            fears_poison: bytes[b + 144] != 0,
            ignores_food: bytes[b + 146] != 0,
        },
        size: [scalar(bytes, 0x304)?, scalar(bytes, 0x308)?],
        food_value: word(bytes, 0x30c)? as i32,
        hp_value: word(bytes, 0x310)? as i32,
    })
}
pub fn structure_creature(bytes: &[u8]) -> Result<Option<u32>, String> {
    if bytes.len() < 28 || word(bytes, 20)? != 28 {
        return Err("invalid Cell structure header".into());
    }
    let count = word(bytes, 24)? as usize;
    if count > 256 || bytes.len() != 28 + count * 40 {
        return Err("invalid Cell structure attachment count".into());
    }
    Ok((0..count).find_map(|i| {
        let at = 28 + i * 40;
        (word(bytes, at + 4).ok()? == 3).then(|| word(bytes, at + 16).unwrap())
    }))
}

#[derive(Clone)]
pub struct InstalledCell {
    pub rule: CellRule,
    pub model_key: ResourceKey,
    pub model: OriginalModel,
    pub abilities: Capabilities,
    pub local: Transform,
}
#[derive(Resource, Default)]
pub struct CellVisualAssets {
    pub species: Vec<InstalledCell>,
    pub body: Option<OriginalModel>,
    /// Bevy units per baked unit of the body mesh (the fit of `player_body`):
    /// the body in the skin units its paint scripts are written in
    /// (`cell_skinpaint`).
    pub body_scale: f32,
    /// The fit `player_body` gave the baked body (translation, no turn: head
    /// first, scale): the frame an edited body (`cell_body`) is meshed in.
    pub body_fit: Option<Transform>,
    /// The baked body's mesh, kept when `cell_body` replaces the shared mesh
    /// asset in place with an edited body (and restores it for the neutral
    /// chain).
    pub baked_mesh: Option<Mesh>,
    /// Bumped whenever `cell_body` rewrites the body mesh (the paint, the
    /// Creator's picking shape and the HUD portrait follow it).
    pub body_revision: u32,
    /// The dominant vertebra of every vertex of the edited body mesh (`None`:
    /// the baked body, whose own partition `cell_paint::body_bones` reads).
    pub body_edit: Option<Vec<u8>>,
    pub plant: Option<(OriginalModel, Transform)>,
    pub meat: Option<(OriginalModel, Transform)>,
    pub flora: Vec<(OriginalModel, Transform)>,
    /// Blue crystal meteor rocks of the smallest Cell level.
    pub rocks: Vec<(OriginalModel, Transform)>,
    pub decoded_rules: usize,
    pub decoded_structures: usize,
}
#[derive(Component, Clone, Debug)]
pub struct OriginalCellVisual {
    pub rule: ResourceKey,
    pub model: ResourceKey,
}
#[derive(Component, Clone, Copy, Debug)]
pub struct CellNativeBehavior {
    pub ai: NativeCellAi,
    pub abilities: Capabilities,
}

pub(crate) fn centered_fit(
    model: &OriginalModel,
    meshes: &Assets<Mesh>,
    radius: f32,
) -> Result<Transform, String> {
    let vertices = match meshes
        .get(&model.handle)
        .and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION))
    {
        Some(VertexAttributeValues::Float32x3(v)) => v,
        _ => return Err("Cell model has no positions".into()),
    };
    let min = vertices.iter().fold(Vec3::splat(f32::INFINITY), |a, p| {
        a.min(Vec3::from_array(*p))
    });
    let max = vertices
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |a, p| {
            a.max(Vec3::from_array(*p))
        });
    let center = (min + max) * 0.5;
    let span = (max.x - min.x).max(max.z - min.z);
    if !span.is_finite() || span <= 0. {
        return Err("empty Cell model footprint".into());
    }
    let scale = radius * 2. / span;
    // GMDL Y longitudinal becomes Bevy Z; face the same -Z as mounted parts.
    let rotation = Quat::from_rotation_y(std::f32::consts::PI);
    Ok(Transform::from_translation(rotation * (-center * scale))
        .with_rotation(rotation)
        .with_scale(Vec3::splat(scale)))
}
/// A fit of `centered_fit` without its half turn about Y (the NPC cells of
/// `cell_engine` do the same: GMDL front (-Y) is Bevy -Z, the travel direction).
pub(crate) fn head_first(fit: Transform) -> Transform {
    let undo = Quat::from_rotation_y(std::f32::consts::PI).inverse();
    Transform { translation: undo * fit.translation, rotation: undo * fit.rotation, scale: fit.scale }
}
fn abilities(originals: &Originals, creature: u32) -> Result<Capabilities, String> {
    let key = ResourceKey {
        group_id: CELL_ANATOMY_GROUP,
        instance_id: creature,
        type_id: crate::creature_data::TYPE,
    };
    let data = crate::creature_data::parse(&originals.resource_index.read(key)?)?;
    Ok(originals.part_catalog.abilities_in(
        CELL_PARTS,
        data.rigblocks
            .iter()
            .filter(|b| b.group_id() == CELL_PARTS)
            .map(|b| b.instance_id()),
    ))
}
fn player_body(
    originals: &mut Originals,
    creature: u32,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<(OriginalModel, Transform), String> {
    let key = ResourceKey {
        group_id: CELL_MODEL_GROUP,
        instance_id: creature,
        type_id: crate::decode::TYPE_GMDL,
    };
    let raw = crate::baked_model::parse(&originals.resource_index.read(key)?)?;
    if raw.root_bones == 0
        || raw.root_bones == u32::MAX
        || raw.bone_ranges.len() != raw.pieces.len()
    {
        return Err("Cell body bone partition absent".into());
    }
    let imported = crate::baked_model::ensure(originals, key, meshes, images, materials)?;
    let mut body = imported.clone();
    body.surfaces.clear();
    for (i, p) in raw.pieces.iter().enumerate() {
        if p.bones.len() != p.positions.len() || p.weights.len() != p.positions.len() {
            return Err("Cell body has no skin weights".into());
        }
        let base = raw.bone_ranges[i][0];
        let mut piece = p.clone();
        piece.indices = p
            .indices
            .chunks_exact(3)
            .filter(|tri| {
                tri.iter().all(|&v| {
                    (0..4).all(|j| {
                        p.weights[v as usize][j] == 0.
                            || base + p.bones[v as usize][j] as u32 / 3 < raw.root_bones
                    })
                })
            })
            .flatten()
            .copied()
            .collect();
        if piece.indices.is_empty() {
            continue;
        }
        // Compact before fitting; discarded appendages must not affect bounds.
        let mut remap = std::collections::HashMap::new();
        let mut compact = crate::baked_model::Piece {
            positions: vec![],
            normals: vec![],
            uvs: vec![],
            indices: vec![],
            bones: vec![],
            weights: vec![],
            material_id: piece.material_id,
        };
        for old in piece.indices {
            let next = compact.positions.len() as u32;
            let index = *remap.entry(old).or_insert_with(|| {
                compact.positions.push(p.positions[old as usize]);
                compact.normals.push(p.normals[old as usize]);
                compact.uvs.push(p.uvs[old as usize]);
                next
            });
            compact.indices.push(index);
        }
        let mut surface = imported.surfaces[i].clone();
        surface.handle = meshes.add(crate::baked_model::mesh(&compact));
        body.surfaces.push(surface);
    }
    if body.surfaces.len() != 1 {
        return Err("Cell player body needs one material surface".into());
    }
    body.handle = body.surfaces[0].handle.clone();
    body.material = Some(body.surfaces[0].material.clone());
    // Head first: the GMDL's front end (-Y: vertebra 0, the narrower end, where the
    // anatomy puts the mouth and the eyes) already maps to Bevy -Z, where the Creator
    // and the stage mount the mouth (`editor::cell_slot(0)`) and the cell travels; the
    // half turn of `centered_fit` left the body's mouth end at +Z (`head_first`).
    let fit = head_first(centered_fit(&body, meshes, crate::cell::CELL_BODY_RADIUS)?);
    let mut mesh = meshes.get_mut(&body.handle).unwrap();
    if let Some(VertexAttributeValues::Float32x3(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in v {
            *p = (fit.translation + fit.rotation * (Vec3::from_array(*p) * fit.scale)).to_array();
        }
    }
    if let Some(VertexAttributeValues::Float32x3(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL) {
        for n in v {
            *n = (fit.rotation * Vec3::from_array(*n)).to_array();
        }
    }
    let _ = mesh.generate_tangents();
    body.scale = 1.;
    // A distinct cached body mesh lets source inspection find its native atlas
    // while the complete starting species remains in baked_models unchanged.
    originals.world_models.insert(
        ResourceKey {
            type_id: CELL_GLOBALS,
            group_id: 0,
            instance_id: creature,
        },
        body.clone(),
    );
    Ok((body, fit))
}
pub(crate) fn static_model(
    originals: &mut Originals,
    name: &str,
    radius: f32,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<(OriginalModel, Transform), String> {
    let key = ResourceKey {
        type_id: RW4,
        group_id: 0,
        instance_id: crate::world_objects::name_id(name),
    };
    let mut d = PartDefinition::raw_model(key);
    d.name = name.into();
    let model = crate::editor::import_definition(originals, &d, meshes, images, materials)?;
    let fit = centered_fit(&model, meshes, radius)?;
    originals.world_models.insert(key, model.clone());
    Ok((model, fit))
}
pub fn prepare(
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> CellVisualAssets {
    let mut out = CellVisualAssets::default();
    if originals.resource_index.records.is_empty() {
        return out;
    }
    let mut keys = originals
        .resource_index
        .records
        .keys()
        .copied()
        .filter(|k| k.type_id == CELL_FILE)
        .collect::<Vec<_>>();
    keys.sort_by_key(|k| (k.group_id, k.instance_id));
    let mut models = std::collections::HashSet::new();
    for key in keys {
        let result = (|| {
            let rule = parse_rule(key, &originals.resource_index.read(key)?)?;
            out.decoded_rules += 1;
            if std::env::var_os("OPENSPORE_DUMP_CELL").is_some() {
                eprintln!(
                    "CELLRULE {:08X}!{:08X} {:?} ai={:#x} size={:?} food={} hp={} {:?}",
                    key.group_id,
                    key.instance_id,
                    rule.name,
                    rule.ai_type,
                    rule.size,
                    rule.food_value,
                    rule.hp_value,
                    rule.ai
                );
            }
            if rule.ai_type != 0x1006 {
                return Ok(());
            }
            let structure_key = ResourceKey {
                type_id: CELL_STRUCTURE,
                group_id: key.group_id,
                instance_id: rule.structure,
            };
            let Some(creature) =
                structure_creature(&originals.resource_index.read(structure_key)?)?
            else {
                return Ok(());
            };
            out.decoded_structures += 1;
            if !models.insert(creature) {
                return Ok(());
            }
            let model_key = ResourceKey {
                type_id: crate::decode::TYPE_GMDL,
                group_id: CELL_MODEL_GROUP,
                instance_id: creature,
            };
            let abilities = abilities(originals, creature)?;
            if abilities.herbivore == 0 && abilities.carnivore == 0 {
                return Ok(());
            }
            let model =
                crate::baked_model::ensure(originals, model_key, meshes, images, materials)?;
            let local = centered_fit(&model, meshes, crate::cell::CELL_BODY_RADIUS)?;
            out.species.push(InstalledCell {
                rule,
                model_key,
                model,
                abilities,
                local,
            });
            Ok::<_, String>(())
        })();
        if let Err(e) = result {
            warn!("Cell species {key:?}: {e}");
        }
    }
    let global_key = originals
        .resource_index
        .records
        .keys()
        .copied()
        .filter(|k| k.type_id == CELL_GLOBALS)
        .min_by_key(|k| (k.group_id, k.instance_id));
    if let Some(key) = global_key {
        let result = (|| {
            let bytes = originals.resource_index.read(key)?;
            let creature = word(&bytes, 0x38)?;
            player_body(originals, creature, meshes, images, materials)
        })();
        match result {
            Ok((body, fit)) => {
                out.body = Some(body);
                out.body_scale = fit.scale.x;
                out.body_fit = Some(fit);
            }
            Err(e) => warn!("original Cell body: {e}"),
        }
    }
    for (name, target) in [
        ("CG_liquidFood_L1", &mut out.plant),
        ("CG_solidFood_1_L1", &mut out.meat),
    ] {
        match static_model(originals, name, 0.3, meshes, images, materials) {
            Ok(model) => *target = Some(model),
            Err(e) => warn!("original Cell food {name}: {e}"),
        }
    }
    for name in [
        "cell_flora_clover_L0",
        "cell_flora_clover2_L0",
        "cell_flora_clover3_l0",
    ] {
        match static_model(originals, name, 1.1, meshes, images, materials) {
            Ok(model) => out.flora.push(model),
            Err(e) => warn!("original Cell flora {name}: {e}"),
        }
    }
    for name in ["CG_rock1_L1", "CG_rock2_L1", "CG_rock3_L1", "CG_rock4_L1"] {
        match static_model(originals, name, 1.0, meshes, images, materials) {
            Ok(model) => out.rocks.push(model),
            Err(e) => warn!("original Cell meteor rock {name}: {e}"),
        }
    }
    info!(
        "Cell installed assets: {} rules, {} resolved species, original body {}, food {}",
        out.decoded_rules,
        out.species.len(),
        out.body.is_some(),
        out.plant.is_some() && out.meat.is_some()
    );
    out
}
pub fn spawn_model(
    commands: &mut Commands,
    actor: Entity,
    model: &OriginalModel,
    transform: Transform,
) {
    commands.entity(actor).with_children(|p| {
        p.spawn((
            StageOwned,
            Mesh3d(model.draw_mesh()),
            MeshMaterial3d(model.material.clone().unwrap()),
            transform,
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_first_removes_the_half_turn_of_the_fit() {
        let turn = Quat::from_rotation_y(std::f32::consts::PI);
        let fit = Transform::from_translation(turn * Vec3::new(-0.1, 0.2, 0.3)).with_rotation(turn).with_scale(Vec3::splat(2.0));
        let head = head_first(fit);
        assert!(head.rotation.angle_between(Quat::IDENTITY) < 1e-6);
        assert_eq!(head.scale, fit.scale);
        // Every point lands where the turned fit puts it, turned back.
        let p = Vec3::new(0.3, 0.4, 0.5);
        assert!((turn * head.transform_point(p) - fit.transform_point(p)).length() < 1e-5);
        // The model's front end (-Y of the GMDL, Bevy -Z before the fit) stays at -Z.
        assert!(head.transform_point(Vec3::new(0.0, 0.0, -1.0)).z < head.transform_point(Vec3::ZERO).z);
    }
    fn key() -> ResourceKey {
        ResourceKey {
            type_id: CELL_FILE,
            group_id: 0,
            instance_id: 3,
        }
    }
    #[test]
    fn exact_cell_file_offsets_preserve_species_ai_food_and_size() {
        let mut bytes = vec![0u8; 0x31c];
        bytes[0..4].copy_from_slice(&77u32.to_le_bytes());
        bytes[4..6].copy_from_slice(&(b'C' as u16).to_le_bytes());
        bytes[0xe0..0xe4].copy_from_slice(&0x1006u32.to_le_bytes());
        for (at, f) in [
            (0xe4, 19f32),
            (0xf8, 4.),
            (0xfc, 11.),
            (0x120, 0.6),
            (0x304, 0.2),
            (0x308, 0.7),
        ] {
            bytes[at..at + 4].copy_from_slice(&f.to_le_bytes());
        }
        bytes[0x30c..0x310].copy_from_slice(&6i32.to_le_bytes());
        bytes[0x310..0x314].copy_from_slice(&2i32.to_le_bytes());
        let rule = parse_rule(key(), &bytes).unwrap();
        assert_eq!(
            (rule.structure, rule.name.as_str(), rule.ai_type),
            (77, "C", 0x1006)
        );
        assert_eq!(
            (
                rule.ai.awareness,
                rule.ai.speed,
                rule.ai.chase,
                rule.ai.turn
            ),
            (19., 4., 11., 0.6)
        );
        assert_eq!(
            (rule.size, rule.food_value, rule.hp_value),
            ([0.2, 0.7], 6, 2)
        );
        bytes[0xe4..0xe8].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(parse_rule(key(), &bytes).is_err());
        assert!(parse_rule(key(), &bytes[..0x318]).is_err());
    }
    #[test]
    fn bounded_structure_parser_follows_creature_attachment() {
        let mut bytes = vec![0u8; 108];
        bytes[20..24].copy_from_slice(&28u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&2u32.to_le_bytes());
        bytes[72..76].copy_from_slice(&3u32.to_le_bytes());
        bytes[84..88].copy_from_slice(&91u32.to_le_bytes());
        assert_eq!(structure_creature(&bytes).unwrap(), Some(91));
        bytes[24..28].copy_from_slice(&100_000u32.to_le_bytes());
        assert!(structure_creature(&bytes).is_err());
    }
}
