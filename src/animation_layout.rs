//! Native animation-group bone budgets, recovered from the captured builders.
//! This resolves slot ownership, not baked bind matrices or procedural IK.
use crate::{
    asset_catalog::{ResourceIndex, RW4},
    baked_model::Model,
    dbpf::ResourceKey,
    rw4,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct BoneGroup {
    pub binding: usize,
    pub first_bone: usize,
    pub bone_count: usize,
    pub rig_source: Option<serde_json::Value>,
    pub skin_link: Option<rw4::SkinRigLink>,
    pub reserved: bool,
    pub transforms: Option<crate::gmdl_bindings::SkinTransforms>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AnimationLayout {
    pub groups: Vec<BoneGroup>,
    pub bone_count: usize,
    pub declared_range_end: usize,
    pub referenced_weighted_slots: usize,
    pub unused_vertex_slots: usize,
    pub all_drawn_slots_covered: bool,
    pub errors: Vec<String>,
}

fn no_rig(key: ResourceKey) -> bool {
    key.group_id == u32::MAX && key.instance_id == u32::MAX && key.type_id == u32::MAX
}

/// A piece may share a larger vertex buffer with other pieces. Only vertices
/// referenced by this piece's triangle list use its local bone palette.
pub fn drawn_vertices(piece: &crate::baked_model::Piece) -> Result<Vec<bool>, String> {
    if piece.indices.len() % 3 != 0 {
        return Err("piece index list is not triangles".into());
    }
    let mut used = vec![false; piece.positions.len()];
    for &vertex in &piece.indices {
        *used
            .get_mut(vertex as usize)
            .ok_or("piece index outside vertex buffer")? = true;
    }
    Ok(used)
}

pub fn resolve(index: &ResourceIndex, model: &Model) -> Result<AnimationLayout, String> {
    let mut groups = Vec::new();
    let mut first_bone = 0usize;
    for (binding, data) in model.anim_data.iter().enumerate() {
        let (bone_count, skin_link, rig_source) = if no_rig(data.key) {
            // Native 362E30 packs the low byte into its placeholder group's
            // signed bone-count byte. Reject unsupported negative counts.
            let count = (data.field_70 as u8) as i8;
            if count < 0 {
                return Err(format!("binding {binding}: negative reserved bone count"));
            }
            (count as usize, None, None)
        } else {
            if data.key.type_id != RW4 {
                return Err(format!("binding {binding}: unsupported rig type"));
            }
            let rigs = rw4::extract_linked_skin_rigs(&index.read(data.key)?)?;
            if rigs.len() != 1 {
                return Err(format!("binding {binding}: {} exact skin links; multi-skin group construction remains unresolved",rigs.len()));
            }
            let rig = &rigs[0];
            if data.baked_deforms.len() != 1 || data.baked_deforms[0].len() != rig.source.bone_count
            {
                return Err(format!(
                    "binding {binding}: baked records do not match its linked skin bones"
                ));
            }
            (
                rig.source.bone_count,
                Some(rig.source.clone()),
                Some(crate::resource_audit::key_json(data.key)),
            )
        };
        groups.push(BoneGroup {
            binding,
            first_bone,
            bone_count,
            rig_source,
            skin_link,
            reserved: no_rig(data.key),
            transforms: if no_rig(data.key) {
                None
            } else {
                Some(crate::gmdl_bindings::SkinTransforms::from_binding(data)?)
            },
        });
        first_bone = first_bone
            .checked_add(bone_count)
            .filter(|n| *n <= i16::MAX as usize)
            .ok_or("animation bone budget exceeds native positive short range")?;
    }
    let mut layout = AnimationLayout {
        groups,
        bone_count: first_bone,
        declared_range_end: 0,
        referenced_weighted_slots: 0,
        unused_vertex_slots: 0,
        all_drawn_slots_covered: true,
        errors: Vec::new(),
    };
    if model.pieces.len() != model.bone_ranges.len() {
        return Err("missing per-piece bone range".into());
    }
    for (ordinal, (piece, range)) in model.pieces.iter().zip(&model.bone_ranges).enumerate() {
        let start = range[0] as usize;
        let end = start
            .checked_add(range[1] as usize)
            .ok_or("piece bone range overflow")?;
        layout.declared_range_end = layout.declared_range_end.max(end);
        let used = drawn_vertices(piece)?;
        for (vertex, &drawn) in used.iter().enumerate() {
            if !drawn {
                layout.unused_vertex_slots += 1;
                continue;
            }
            let bones = piece.bones.get(vertex).ok_or("missing vertex bone bytes")?;
            let weights = piece.weights.get(vertex).ok_or("missing vertex weights")?;
            for (&bone, &weight) in bones.iter().zip(weights) {
                if !weight.is_finite() || weight < 0. {
                    return Err("invalid vertex weight".into());
                }
                if weight <= 0. {
                    continue;
                }
                layout.referenced_weighted_slots += 1;
                let global = start + bone as usize / 3;
                if bone % 3 != 0 || global >= layout.bone_count || global >= end {
                    layout.all_drawn_slots_covered = false;
                    if layout.errors.len() < 16 {
                        layout.errors.push(format!("piece {ordinal}, vertex {vertex}: byte {bone}, global slot {global}, piece range {start}..{end}, bone budget {}",layout.bone_count));
                    }
                }
            }
        }
    }
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reserved_model() -> Model {
        let no_rig = ResourceKey {
            group_id: u32::MAX,
            instance_id: u32::MAX,
            type_id: u32::MAX,
        };
        Model {
            version: 9,
            references: vec![],
            materials: vec![],
            root_bones: 2,
            unknown_key: no_rig,
            native_geometry: vec![],
            anim_data: [2, 3]
                .into_iter()
                .map(|count| crate::baked_model::GameModelAnimData {
                    key: no_rig,
                    field_70: count,
                    transform: [0; 56],
                    field_38: [0; 56],
                    baked_deforms: vec![],
                })
                .collect(),
            bone_ranges: vec![[2, 3]],
            pieces: vec![crate::baked_model::Piece {
                positions: vec![[0.; 3]; 2],
                normals: vec![[0., 1., 0.]; 2],
                uvs: vec![[0.; 2]; 2],
                indices: vec![0, 0, 0],
                bones: vec![[6, 0, 0, 0], [255; 4]],
                weights: vec![[1., 0., 0., 0.]; 2],
                material_id: 0,
            }],
        }
    }

    #[test]
    fn reserved_groups_preserve_slot_order_and_shared_buffer_palettes() {
        let layout = resolve(&ResourceIndex::default(), &reserved_model()).unwrap();
        assert_eq!(layout.bone_count, 5);
        assert_eq!(layout.declared_range_end, 5);
        assert_eq!(
            layout
                .groups
                .iter()
                .map(|g| (g.first_bone, g.bone_count))
                .collect::<Vec<_>>(),
            vec![(0, 2), (2, 3)]
        );
        assert!(layout.all_drawn_slots_covered);
        assert_eq!(layout.unused_vertex_slots, 1);
        assert_eq!(layout.referenced_weighted_slots, 1);
    }

    #[test]
    fn invalid_drawn_palette_and_native_negative_count_are_rejected() {
        let mut model = reserved_model();
        model.pieces[0].bones[0][0] = 9;
        assert!(
            !resolve(&ResourceIndex::default(), &model)
                .unwrap()
                .all_drawn_slots_covered
        );
        model.pieces[0].indices[0] = 2;
        assert!(resolve(&ResourceIndex::default(), &model).is_err());
        model = reserved_model();
        model.anim_data[0].field_70 = 255;
        assert!(resolve(&ResourceIndex::default(), &model).is_err());
    }
    #[test]
    #[ignore = "requires original Spore assets"]
    fn installed_assembled_group_budgets_match_drawn_vertices() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut rows = Vec::new();
        let mut mismatches = 0;
        for group in [
            crate::baked_model::CREATURE_GROUP,
            crate::cell_visuals::CELL_MODEL_GROUP,
        ] {
            for key in crate::baked_model::candidates(&index, group) {
                let model = crate::baked_model::parse(&index.read(key).unwrap()).unwrap();
                let layout = resolve(&index, &model).unwrap_or_else(|e| panic!("{key:?}: {e}"));
                mismatches += usize::from(
                    !layout.all_drawn_slots_covered
                        || layout.bone_count != layout.declared_range_end,
                );
                rows.push(serde_json::json!({"source":crate::resource_audit::key_json(key),"layout":layout}));
            }
        }
        std::fs::create_dir_all("target/native-animation-065").unwrap();
        std::fs::write("target/native-animation-065/assembled-layouts.json",serde_json::to_vec_pretty(&serde_json::json!({"models":rows.len(),"mismatches":mismatches,"model_layouts":rows})).unwrap()).unwrap();
        assert_eq!(
            mismatches, 0,
            "drawn vertex palette slots must fit the recovered group budget"
        );
        println!("Verified {} original assembled group budgets", rows.len());
    }
}
