//! Installed scenery definitions. These are world objects, not editor parts;
//! the restricted part catalog intentionally does not contain Tribal huts.
use crate::{
    asset_catalog::{self, PartDefinition},
    dbpf::ResourceKey,
    OriginalModel, Originals,
};
use bevy::{mesh::VertexAttributeValues, prelude::*};

pub const HUT_GROUP: u32 = 0x2be764e2;
pub const FOOD_GROUP: u32 = 0xb4b1178d;

/// Named resource IDs use Spore's lowercase FNV-1, not FNV-1a.
pub fn name_id(name: &str) -> u32 {
    name.to_lowercase()
        .chars()
        .fold(0x811c9dc5u32, |h, c| h.wrapping_mul(0x01000193) ^ c as u32)
}

#[derive(Component, Clone)]
pub struct OriginalWorldObject {
    pub definition: ResourceKey,
    pub model: ResourceKey,
    pub name: String,
}

pub fn definition(originals: &Originals, group: u32, name: &str) -> Result<PartDefinition, String> {
    let key = ResourceKey {
        group_id: group,
        instance_id: name_id(name),
        type_id: asset_catalog::PROP,
    };
    definition_at(originals, key, name)
}
pub fn definition_at(
    originals: &Originals,
    key: ResourceKey,
    name: &str,
) -> Result<PartDefinition, String> {
    let p = match originals.resource_index.properties(key) {
        Ok(p) => p,
        Err(e) if e.starts_with("missing resource ") => {
            // Several shipped hut PROP files reference an absent parent. The
            // local authored LOD0/material/transform fields still render; do
            // not invent inherited values or hide the unresolved dependency.
            let local = crate::prop::parse(&originals.resource_index.read(key)?)?;
            if !local.0.contains_key(&0x00f9efbb) {
                return Err(e);
            }
            if !originals.world_models.contains_key(&key) {
                warn!("{name}: rendering local object fields; unresolved inheritance: {e}");
            }
            local
        }
        Err(e) => return Err(e),
    };
    let mut model =
        p.0.get(&0x00f9efbb)
            .and_then(|p| p.key())
            .ok_or("world object has no LOD0 model")?;
    // Native model properties often omit a file extension. Only resolve that
    // case to an actual RW4 at the exact authored group and instance.
    if model.type_id == 0 {
        let supported = [asset_catalog::RW4, crate::decode::TYPE_GMDL];
        let resolved: Vec<_> = supported
            .into_iter()
            .map(|type_id| ResourceKey { type_id, ..model })
            .filter(|k| originals.resource_index.records.contains_key(k))
            .collect();
        if resolved.len() != 1 {
            return Err(format!("{name}: missing or ambiguous LOD0 extension"));
        }
        model = resolved[0];
    }
    if ![asset_catalog::RW4, crate::decode::TYPE_GMDL].contains(&model.type_id)
        || !originals.resource_index.records.contains_key(&model)
    {
        return Err(format!(
            "{name}: unsupported or missing original LOD0 model"
        ));
    }
    let mut d = PartDefinition::raw_model(model);
    d.key = key;
    d.name = name.into();
    d.model_offset =
        p.0.get(&0x00fba610)
            .and_then(|p| p.vector3())
            .unwrap_or([0.; 3]);
    d.model_rotation =
        p.0.get(&0x00fba613)
            .and_then(|p| p.vector3())
            .unwrap_or([0.; 3]);
    d.model_scale = p.0.get(&0x00fba611).and_then(|p| p.number()).unwrap_or(1.);
    // SkinPaint metadata remains separate from ordinary static model textures.
    d.diffuse = p.0.get(&0x02424655).and_then(|p| p.key());
    d.tint = p.0.get(&0x02424657).and_then(|p| p.key());
    d.spec_bump = p.0.get(&0x02424656).and_then(|p| p.key());
    if !d
        .model_offset
        .iter()
        .chain(&d.model_rotation)
        .all(|n| n.is_finite())
        || !d.model_scale.is_finite()
        || d.model_scale <= 0.
    {
        return Err("non-finite original object transform".into());
    }
    Ok(d)
}

pub fn import(
    originals: &mut Originals,
    group: u32,
    name: &str,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<(OriginalModel, OriginalWorldObject), String> {
    let d = definition(originals, group, name)?;
    import_at(originals, d, meshes, images, materials)
}
pub fn import_at(
    originals: &mut Originals,
    d: PartDefinition,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<(OriginalModel, OriginalWorldObject), String> {
    if !originals.world_models.contains_key(&d.key) {
        let mut model = if d.model.type_id == crate::decode::TYPE_GMDL {
            crate::baked_model::import(originals, d.model, meshes, images, materials)?
        } else {
            crate::editor::import_definition(originals, &d, meshes, images, materials)?
        };
        model.source_definition = Some(d.key);
        originals.world_models.insert(d.key, model);
    }
    Ok((
        originals.world_models[&d.key].clone(),
        OriginalWorldObject {
            definition: d.key,
            model: d.model,
            name: d.name,
        },
    ))
}

/// Fit a world object to the prototype's gameplay footprint while preserving
/// proportions and all of its native submeshes. Align its actual lowest vertex
/// with the terrain baseline; mesh origins are not assumed to be at its feet.
pub fn fitted_transform(
    model: &OriginalModel,
    meshes: &Assets<Mesh>,
    position: Vec3,
    width: f32,
) -> Result<Transform, String> {
    let mesh = meshes
        .get(&model.handle)
        .ok_or("missing original object mesh")?;
    let Some(VertexAttributeValues::Float32x3(vertices)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Err("missing original object positions".into());
    };
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for v in vertices {
        let v = Vec3::from_array(*v);
        min = min.min(v);
        max = max.max(v);
    }
    let span = (max.x - min.x).max(max.z - min.z);
    if !span.is_finite() || span <= 0. || !width.is_finite() || width <= 0. {
        return Err("invalid original scenery bounds".into());
    }
    let scale = width / span;
    Ok(
        Transform::from_translation(position - Vec3::Y * (min.y * scale))
            .with_scale(Vec3::splat(scale)),
    )
}

pub fn spawn(
    commands: &mut Commands,
    originals: &mut Originals,
    group: u32,
    name: &str,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    position: Vec3,
    width: f32,
) -> Option<Entity> {
    spawn_at(
        commands,
        originals,
        ResourceKey {
            group_id: group,
            instance_id: name_id(name),
            type_id: asset_catalog::PROP,
        },
        name,
        meshes,
        images,
        materials,
        position,
        width,
    )
}
pub fn spawn_at(
    commands: &mut Commands,
    originals: &mut Originals,
    key: ResourceKey,
    name: &str,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    position: Vec3,
    width: f32,
) -> Option<Entity> {
    // Headless gameplay tests have no installation. Their existing primitives
    // remain available; a desktop import failure is logged with its source.
    if originals.resource_index.records.is_empty() {
        return None;
    }
    let result = (|| {
        let d = definition_at(originals, key, name)?;
        let (model, source) = import_at(originals, d, meshes, images, materials)?;
        let transform = fitted_transform(&model, meshes, position, width)?;
        let material = model
            .material
            .as_ref()
            .and_then(|h| materials.get(h))
            .cloned()
            .ok_or("original object has no material")?;
        Ok::<_, String>(
            commands
                .spawn((
                    crate::stages::StageOwned,
                    Name::new(name.to_string()),
                    source,
                    Mesh3d(model.draw_mesh()),
                    MeshMaterial3d(materials.add(material)),
                    transform,
                ))
                .id(),
        )
    })();
    match result {
        Ok(e) => Some(e),
        Err(e) => {
            warn!("original world object {name}: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_native_names_use_fnv1() {
        assert_eq!(name_id("he_simple_1"), 0xb9845539);
        assert_eq!(name_id("HE_SIMPLE_1"), 0xb9845539);
        assert_eq!(name_id("foodpile_fruit_01"), 0xaeacaf8c);
    }
    #[test]
    #[ignore = "requires installed original Spore assets"]
    fn installed_world_objects_keep_native_geometry_and_textures() {
        let install = crate::spore_install::discover().unwrap();
        let mut originals = Originals::default();
        originals.resource_index = asset_catalog::ResourceIndex::load(&install.packages);
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        for (group, name) in [
            (HUT_GROUP, "he_simple_1"),
            (HUT_GROUP, "he_canvas_1"),
            (HUT_GROUP, "he_yurt_1"),
            (HUT_GROUP, "he_timber_1"),
            (FOOD_GROUP, "foodpile_fruit_01"),
        ] {
            let (model, source) = import(
                &mut originals,
                group,
                name,
                &mut meshes,
                &mut images,
                &mut materials,
            )
            .unwrap();
            assert_eq!(source.definition.instance_id, name_id(name));
            assert!(!model.surfaces.is_empty());
            let Some(VertexAttributeValues::Float32x3(normals)) = meshes
                .get(&model.handle)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_NORMAL)
            else {
                panic!("missing normals")
            };
            assert!(
                normals
                    .iter()
                    .any(|n| Vec3::from_array(*n).distance(Vec3::from_array(normals[0])) > 0.1),
                "{name}: original packed normals must not collapse to one fallback direction"
            );
            assert!(model.surfaces.iter().all(|s| !s.state_sections.is_empty()));
            assert!(
                model.surfaces.iter().all(|s| materials
                    .get(&s.material)
                    .unwrap()
                    .base_color_texture
                    .is_some()),
                "{name}: native diffuse missing"
            );
            let count = meshes.len();
            let cached = import(
                &mut originals,
                group,
                name,
                &mut meshes,
                &mut images,
                &mut materials,
            )
            .unwrap()
            .0;
            assert_eq!(cached.handle, model.handle);
            assert_eq!(meshes.len(), count);
            let t = fitted_transform(&model, &meshes, Vec3::new(2., -0.6, 4.), 3.).unwrap();
            assert!(t.translation.is_finite() && t.scale.is_finite());
        }
    }
}
