//! Editable parts cut from the exact starter's original baked mesh and atlas.
//! This curated donor preserves its authored part shape/paint during body edits;
//! other purchased parts continue to use their own RW4 definitions.
use crate::{
    asset_catalog::{self, PartCatalog, PartDefinition},
    baked_model::{self, Piece},
    dbpf::ResourceKey,
    OriginalModel, OriginalSurface, Originals,
};
use bevy::prelude::*;
pub const DEFINITIONS: [u32; 5] = [0x32AC9620, 0xEC3B4B7E, 0x7E262CF0, 0x7F815C91, 0x4AF28CE7];
const BONE_RANGES: [std::ops::Range<usize>; 5] = [18..22, 22..26, 26..30, 30..32, 32..34];

pub fn recipe(catalog: &PartCatalog) -> Option<Vec<crate::game::PartInstance>> {
    DEFINITIONS
        .into_iter()
        .enumerate()
        .map(|(slot, id)| {
            let d = catalog.parts.iter().find(|p| {
                p.key.group_id == asset_catalog::CREATURE_PARTS
                    && p.key.instance_id == id
                    && !p.is_null
            })?;
            Some(crate::game::PartInstance::new(
                d.model.instance_id,
                slot as u32,
            ))
        })
        .collect()
}

fn triangle_part(piece: &Piece, triangle: &[u32]) -> Result<Option<usize>, String> {
    let mut selected = None;
    for &i in triangle {
        for (&bone, &weight) in piece.bones[i as usize]
            .iter()
            .zip(&piece.weights[i as usize])
        {
            if weight == 0. {
                continue;
            }
            if bone % 3 != 0 {
                return Err("unsupported donor bone encoding".into());
            }
            let bone = bone as usize / 3;
            if bone < 18 {
                continue;
            }
            let group = BONE_RANGES
                .iter()
                .position(|r| r.contains(&bone))
                .ok_or("donor influence outside starter groups")?;
            if selected.is_some_and(|old| old != group) {
                return Err("donor triangle crosses independent part groups".into());
            }
            selected = Some(group);
        }
    }
    Ok(selected)
}

pub fn ensure(
    originals: &mut Originals,
    definition: &PartDefinition,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<Option<OriginalModel>, String> {
    if definition.key.group_id != asset_catalog::CREATURE_PARTS
        || !DEFINITIONS.contains(&definition.key.instance_id)
    {
        return Ok(None);
    }
    if let Some(model) = originals.starter_parts.get(&definition.key) {
        return Ok(Some(model.clone()));
    }
    let Some(source) = baked_model::player_source_key(originals) else {
        return Ok(None);
    };
    let raw = baked_model::parse(&originals.resource_index.read(source)?)?;
    let anatomy = crate::creature_data::parse(&originals.resource_index.read(ResourceKey {
        group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
        instance_id: source.instance_id,
        type_id: crate::creature_data::TYPE,
    })?)?;
    if raw.root_bones != 18
        || raw.pieces.len() != 1
        || raw.bone_ranges != [[0, 34]]
        || raw.anim_data.len() != 6
        || anatomy.rigblocks.len() != 18
    {
        return Err("unsupported starter donor layout".into());
    }
    for (i, &id) in DEFINITIONS.iter().enumerate() {
        let block = &anatomy.rigblocks[13 + i];
        if block.block_type() != 2
            || block.instance_id() != id
            || block.group_id() != asset_catalog::CREATURE_PARTS
        {
            return Err("starter anatomy does not match donor groups".into());
        }
    }
    // Resolve all definitions before creating assets, so this is atomic when a
    // native part is absent from a restricted/test installation.
    let definitions = DEFINITIONS
        .iter()
        .map(|id| {
            originals
                .part_catalog
                .parts
                .iter()
                .find(|p| {
                    p.key.group_id == asset_catalog::CREATURE_PARTS && p.key.instance_id == *id
                })
                .cloned()
                .ok_or("starter donor definition absent".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if definitions.iter().enumerate().any(|(i, d)| {
        definitions[..i]
            .iter()
            .any(|old| old.model.instance_id == d.model.instance_id)
    }) {
        return Err("starter donor needs distinct model identities".into());
    }
    let full = baked_model::ensure(originals, source, meshes, images, materials)?;
    if full.surfaces.len() != 1 {
        return Err("starter donor material partition differs".into());
    }
    let fit = baked_model::player_fitted_transform(&full, meshes)?;
    let piece = &raw.pieces[0];
    let mut partitions: [Vec<u32>; 5] = std::array::from_fn(|_| vec![]);
    for triangle in piece.indices.chunks_exact(3) {
        if let Some(group) = triangle_part(piece, triangle)? {
            partitions[group].extend_from_slice(triangle);
        }
    }
    if partitions.iter().any(Vec::is_empty) {
        return Err("empty starter donor part".into());
    }
    let mut models = vec![];
    for (slot, indices) in partitions.into_iter().enumerate() {
        let p = anatomy.rigblocks[13 + slot]
            .position_bits()
            .map(f32::from_bits);
        let anchor = fit.transform_point(Vec3::new(-p[0], p[2], p[1]));
        let mut output = Piece {
            positions: vec![],
            normals: vec![],
            uvs: vec![],
            indices: vec![],
            bones: vec![],
            weights: vec![],
            material_id: piece.material_id,
        };
        let mut remap = std::collections::HashMap::new();
        for old in indices {
            let new = *remap.entry(old).or_insert_with(|| {
                let i = old as usize;
                let p = piece.positions[i];
                let p = fit.transform_point(Vec3::new(-p[0], p[2], p[1])) - anchor;
                let new = output.positions.len() as u32;
                output.positions.push([-p.x, p.z, p.y]);
                output.normals.push(piece.normals[i]);
                output.uvs.push(piece.uvs[i]);
                output.bones.push(piece.bones[i]);
                output.weights.push(piece.weights[i]);
                new
            });
            output.indices.push(new);
        }
        let handle = meshes.add(baked_model::mesh(&output));
        let material = full.surfaces[0].material.clone();
        models.push((
            definitions[slot].key,
            OriginalModel {
                source_definition: Some(source),
                instance: definitions[slot].model.instance_id,
                handle: handle.clone(),
                material: Some(material.clone()),
                surfaces: vec![OriginalSurface {
                    handle,
                    material,
                    source_section: 0,
                    state_sections: vec![],
                }],
                // Mounts use scale * .5. Donor positions are already in actor units.
                scale: 2.,
                ..default()
            },
        ));
    }
    originals.starter_parts.extend(models);
    Ok(originals.starter_parts.get(&definition.key).cloned())
}

/// The skin of a donor part (`creature_morph` poses it on the starter's
/// binding rig): per output vertex of `ensure` (same partition, same order)
/// the joints relative to the part's bone group and their weights, the
/// donor mesh's transform from GMDL model space (mesh = fit(basis(v)) -
/// anchor), the slot and the source GMDL.
pub struct DonorSkin {
    pub slot: usize,
    pub group: std::ops::Range<usize>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub mesh_from_gmdl: bevy::math::Affine3A,
    pub source: ResourceKey,
}

/// `DonorSkin` of a starter definition whose donor model `ensure` built.
/// ADAPTED: influences of body bones (< 18) are dropped and the part's
/// group weights renormalized (the donor mesh no longer rides the body).
pub fn donor_skin(originals: &Originals, meshes: &Assets<Mesh>, definition: ResourceKey) -> Result<Option<DonorSkin>, String> {
    if definition.group_id != asset_catalog::CREATURE_PARTS || !originals.starter_parts.contains_key(&definition) {
        return Ok(None);
    }
    let Some(slot) = DEFINITIONS.iter().position(|id| *id == definition.instance_id) else { return Ok(None) };
    let Some(source) = baked_model::player_source_key(originals) else { return Ok(None) };
    let Some(full) = originals.baked_models.get(&source) else { return Ok(None) };
    let raw = baked_model::parse(&originals.resource_index.read(source)?)?;
    let anatomy = crate::creature_data::parse(&originals.resource_index.read(ResourceKey {
        group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
        instance_id: source.instance_id,
        type_id: crate::creature_data::TYPE,
    })?)?;
    if raw.pieces.len() != 1 || anatomy.rigblocks.len() != 18 {
        return Err("unsupported starter donor layout".into());
    }
    let fit = baked_model::player_fitted_transform(full, meshes)?;
    let piece = &raw.pieces[0];
    let group = BONE_RANGES[slot].clone();
    let p = anatomy.rigblocks[13 + slot].position_bits().map(f32::from_bits);
    let anchor = fit.transform_point(Vec3::new(-p[0], p[2], p[1]));
    let (mut joints, mut weights) = (Vec::new(), Vec::new());
    let mut seen = std::collections::HashSet::new();
    for triangle in piece.indices.chunks_exact(3) {
        if triangle_part(piece, triangle)? != Some(slot) {
            continue;
        }
        for &old in triangle {
            if !seen.insert(old) {
                continue;
            }
            let i = old as usize;
            let (mut j, mut w) = ([0u16; 4], [0f32; 4]);
            for k in 0..4 {
                let bone = piece.bones[i][k] as usize / 3;
                if piece.weights[i][k] > 0.0 && group.contains(&bone) {
                    j[k] = (bone - group.start) as u16;
                    w[k] = piece.weights[i][k];
                }
            }
            let total: f32 = w.iter().sum();
            weights.push(if total > 0.0 { w.map(|x| x / total) } else { [1.0, 0.0, 0.0, 0.0] });
            joints.push(j);
        }
    }
    // basis(v) = (-x, z, y); then fit; then minus the anchor.
    let basis = bevy::math::Affine3A::from_mat3(bevy::math::Mat3::from_cols(Vec3::NEG_X, Vec3::Z, Vec3::Y));
    let mesh_from_gmdl = bevy::math::Affine3A::from_translation(-anchor) * bevy::math::Affine3A::from_scale_rotation_translation(fit.scale, fit.rotation, fit.translation) * basis;
    Ok(Some(DonorSkin { slot, group, joints, weights, mesh_from_gmdl, source }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original Spore installation"]
    fn installed_starter_parts_reconstruct_native_triangles_positions_and_atlas() {
        let install = crate::spore_install::discover().unwrap();
        let mut originals = Originals::default();
        originals.resource_index = asset_catalog::ResourceIndex::load(&install.packages);
        originals.part_catalog = PartCatalog::from_index(&originals.resource_index);
        let definition = originals
            .part_catalog
            .parts
            .iter()
            .find(|p| {
                p.key.group_id == asset_catalog::CREATURE_PARTS
                    && p.key.instance_id == DEFINITIONS[0]
            })
            .unwrap()
            .clone();
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        ensure(
            &mut originals,
            &definition,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .unwrap()
        .unwrap();
        assert_eq!(originals.starter_parts.len(), 5);
        let source = baked_model::player_source_key(&originals).unwrap();
        let raw = baked_model::parse(&originals.resource_index.read(source).unwrap()).unwrap();
        let full = originals.baked_models[&source].clone();
        let fit = baked_model::player_fitted_transform(&full, &meshes).unwrap();
        let anatomy = crate::creature_data::parse(
            &originals
                .resource_index
                .read(ResourceKey {
                    group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
                    instance_id: source.instance_id,
                    type_id: crate::creature_data::TYPE,
                })
                .unwrap(),
        )
        .unwrap();
        // The GMDL torso partition (the body path the metaball skin replaces).
        let body = crate::creature_skin_view::without_skin(|| {
            baked_model::prepare_body(&mut originals, &default(), &mut meshes, &mut images, &mut materials)
        })
        .unwrap();
        let mut total = meshes.get(&body.handle).unwrap().indices().unwrap().len();
        for (slot, id) in DEFINITIONS.iter().enumerate() {
            let key = ResourceKey {
                group_id: asset_catalog::CREATURE_PARTS,
                instance_id: *id,
                type_id: asset_catalog::PROP,
            };
            let model = &originals.starter_parts[&key];
            let mesh = meshes.get(&model.handle).unwrap();
            total += mesh.indices().unwrap().len();
            assert_eq!(model.material, full.material);
            assert_eq!(model.source_definition, Some(source));
            let p = anatomy.rigblocks[13 + slot]
                .position_bits()
                .map(f32::from_bits);
            let anchor = fit.transform_point(Vec3::new(-p[0], p[2], p[1]));
            let expected = raw.pieces[0]
                .indices
                .chunks_exact(3)
                .filter(|tri| triangle_part(&raw.pieces[0], tri).unwrap() == Some(slot))
                .flat_map(|tri| tri.iter().copied())
                .collect::<std::collections::BTreeSet<_>>();
            let bevy::mesh::VertexAttributeValues::Float32x3(actual) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
            else {
                panic!("positions")
            };
            assert_eq!(actual.len(), expected.len());
            for p in actual {
                let restored = Vec3::from_array(*p) + anchor;
                assert!(expected.iter().any(|&i| {
                    let v = raw.pieces[0].positions[i as usize];
                    restored.abs_diff_eq(fit.transform_point(Vec3::new(-v[0], v[2], v[1])), 1e-5)
                }));
            }
        }
        assert_eq!(
            total,
            raw.pieces[0].indices.len(),
            "body plus five parts retains every original triangle exactly once"
        );
    }
}
