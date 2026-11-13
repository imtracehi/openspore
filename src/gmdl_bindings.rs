//! Evidence-only inspection of GMDL animation bindings and baked deform data.
//!
//! Serialized Transform layout comes from SporeModder-FX; its row-vector
//! rotation convention and native inverse come from captured math routines.
//! Constructor transform blocks are resolved, but 0xA0 baked bone records and
//! procedural controllers still require native proof before assembled motion.

use std::collections::BTreeMap;

use crate::{baked_model::Model, dbpf::ResourceKey};

const TRANSFORM_BYTES: usize = 56;
const DEFORM_BYTES: usize = 160;

#[derive(Clone, Debug, serde::Serialize)]
pub struct NativeTransform {
    /// Exact bytes stored as a vector so serde need not support large arrays.
    pub raw: Vec<u8>,
    pub flags: u16,
    pub transform_count: i16,
    pub offset: [f32; 3],
    pub scale: f32,
    /// Row-major native 3x3 sequence. Native vectors multiply it on the left,
    /// as verified by captured VectorMatrixProduct_431a0.
    pub rotation_values: [f32; 9],
    pub all_values_finite: bool,
}

impl NativeTransform {
    pub fn decode(raw: [u8; TRANSFORM_BYTES]) -> Self {
        let flags = u16::from_le_bytes([raw[0], raw[1]]);
        let transform_count = i16::from_le_bytes([raw[2], raw[3]]);
        let f = |at: usize| f32::from_le_bytes(raw[at..at + 4].try_into().unwrap());
        let offset = [f(4), f(8), f(12)];
        let scale = f(16);
        let rotation_values = std::array::from_fn(|i| f(20 + i * 4));
        let all_values_finite = offset
            .iter()
            .chain(std::iter::once(&scale))
            .chain(rotation_values.iter())
            .all(|v| v.is_finite());
        Self {
            raw: raw.to_vec(),
            flags,
            transform_count,
            offset,
            scale,
            rotation_values,
            all_values_finite,
        }
    }

    /// Port of captured TransformOperation_2efa0: reciprocal uniform scale,
    /// transposed rotation, and (-offset / scale) times the transposed matrix.
    /// This preserves flags/count and raw layout. Native code assumes a
    /// rotation matrix; this operation does not solve assembled bone poses.
    pub fn inverted_native(&self) -> Option<Self> {
        if !self.all_values_finite || self.scale == 0. {
            return None;
        }
        let inverse_scale = 1. / self.scale;
        let rotation: [f32; 9] = std::array::from_fn(|i| self.rotation_values[(i % 3) * 3 + i / 3]);
        let scaled = self.offset.map(|v| v * -inverse_scale);
        let offset: [f32; 3] = std::array::from_fn(|col| {
            scaled[0] * rotation[col]
                + scaled[1] * rotation[3 + col]
                + scaled[2] * rotation[6 + col]
        });
        let mut raw: [u8; TRANSFORM_BYTES] = self.raw.as_slice().try_into().ok()?;
        for (ordinal, value) in offset
            .into_iter()
            .chain([inverse_scale])
            .chain(rotation)
            .enumerate()
        {
            if !value.is_finite() {
                return None;
            }
            raw[4 + ordinal * 4..8 + ordinal * 4].copy_from_slice(&value.to_le_bytes());
        }
        Some(Self::decode(raw))
    }
}

/// Data values of the three Transform blocks in the native 0xC4 skin descriptor
/// (offsets 0x10, 0x48, 0x80), recovered from SkinGroupBuilder_35f080.
/// Runtime change counters/reference pointers are deliberately not fabricated.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SkinTransforms {
    pub primary: NativeTransform,
    pub secondary: NativeTransform,
    pub inverse_primary: NativeTransform,
}

impl SkinTransforms {
    pub fn from_binding(binding: &crate::baked_model::GameModelAnimData) -> Result<Self, String> {
        let primary = NativeTransform::decode(binding.transform);
        let secondary = NativeTransform::decode(binding.field_38);
        if !secondary.all_values_finite {
            return Err("non-finite secondary skin transform".into());
        }
        let inverse_primary = primary
            .inverted_native()
            .ok_or("non-invertible primary skin transform")?;
        Ok(Self {
            primary,
            secondary,
            inverse_primary,
        })
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MatrixCandidate {
    /// Byte offset in one 160-byte baked-deform record.
    pub record_offset: usize,
    pub width: usize,
    pub height: usize,
    pub values: Vec<f32>,
    /// Homogeneous boundary pattern only; this does not establish whether the
    /// sequence is a transform, its convention, or its role in the rig.
    pub homogeneous_boundary_score: u8,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DeformRecordAnalysis {
    pub ordinal: usize,
    pub raw_hex: String,
    pub finite_float_count: usize,
    pub aligned_float_count: usize,
    pub matrix_candidates: Vec<MatrixCandidate>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum KeyResolution {
    MatchedReference { reference_indices: Vec<usize> },
    Missing,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MeshPaletteAudit {
    pub piece_index: usize,
    /// Native first-bone/count palette for this piece.
    pub declared_pair: [u32; 2],
    pub weighted_blend_index_bytes: Vec<u8>,
    /// `byte / 3`, matching the reference importer's vertex-group derivation.
    pub weighted_triplet_slots: Vec<u32>,
    /// First pair value plus the triplet slot, matching runtime base addition.
    /// This audit reports indices; animation_layout validates their group owners.
    pub candidate_global_indices: Vec<u32>,
    pub weighted_slots_without_index_data: usize,
    pub range_pair_semantics: &'static str,
    pub index_error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct AnimationBindingAudit {
    pub binding_index: usize,
    pub key: serde_json::Value,
    /// Offsets relative to the start of a GameModelAnimData record. The
    /// parser does not retain the enclosing GMDL file offset.
    pub field_offsets_in_record: BTreeMap<&'static str, usize>,
    pub key_resolution: KeyResolution,
    pub field_70: u32,
    pub primary_transform: NativeTransform,
    pub secondary_transform: NativeTransform,
    pub deform_groups: Vec<Vec<DeformRecordAnalysis>>,
    pub raw_baked_deforms_preserved: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct BindingAudit {
    pub source_key: serde_json::Value,
    pub gmdl_version: u32,
    pub model_reference_count: usize,
    pub model_piece_count: usize,
    pub bone_range_count: usize,
    pub root_bones_field: u32,
    pub mesh_palettes: Vec<MeshPaletteAudit>,
    pub animation_bindings: Vec<AnimationBindingAudit>,
    pub summary: BindingAuditSummary,
    pub limits: Vec<&'static str>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct BindingAuditSummary {
    pub binding_count: usize,
    pub exact_reference_matches: usize,
    pub missing_binding_keys: usize,
    pub ambiguous_binding_keys: usize,
    pub deform_group_count: usize,
    pub deform_record_count: usize,
    pub finite_primary_transforms: usize,
    pub weighted_triplet_slots: usize,
    pub candidate_global_indices: usize,
}

/// Decode the native transform record without changing or discarding bytes.
/// Layout verified against SporeModder-FX `Transform.readComplete`.
pub fn decode_transform(raw: [u8; TRANSFORM_BYTES]) -> NativeTransform {
    NativeTransform::decode(raw)
}

/// Inspect aligned little-endian floats and likely homogeneous 4x4 windows in
/// a baked record. Candidates are descriptive signals only, not decoded rigs.
pub fn analyze_deform_record(raw: &[u8; DEFORM_BYTES], ordinal: usize) -> DeformRecordAnalysis {
    let values: Vec<f32> = (0..DEFORM_BYTES)
        .step_by(4)
        .map(|i| f32::from_le_bytes(raw[i..i + 4].try_into().unwrap()))
        .collect();
    let mut matrix_candidates = Vec::new();
    // The common 4x4 homogeneous patterns are detected in either boundary
    // orientation. Every value must be finite; no matrix is assumed otherwise.
    for start in (0..=DEFORM_BYTES - 64).step_by(4) {
        let block: Vec<f32> = (0..16)
            .map(|i| f32::from_le_bytes(raw[start + i * 4..start + i * 4 + 4].try_into().unwrap()))
            .collect();
        if block.iter().any(|v| !v.is_finite()) {
            continue;
        }
        let near = |a: f32, b: f32| (a - b).abs() <= 1.0e-4;
        let row_boundary = block[12..15].iter().all(|v| near(*v, 0.0)) && near(block[15], 1.0);
        let column_boundary = [block[3], block[7], block[11]]
            .iter()
            .all(|v| near(*v, 0.0))
            && near(block[15], 1.0);
        let score = u8::from(row_boundary) + u8::from(column_boundary);
        if score > 0 {
            matrix_candidates.push(MatrixCandidate {
                record_offset: start,
                width: 4,
                height: 4,
                values: block,
                homogeneous_boundary_score: score,
            });
        }
    }
    DeformRecordAnalysis {
        ordinal,
        raw_hex: raw.iter().map(|b| format!("{b:02x}")).collect(),
        finite_float_count: values.iter().filter(|v| v.is_finite()).count(),
        aligned_float_count: values.len(),
        matrix_candidates,
    }
}

fn resolution(key: ResourceKey, references: &[ResourceKey]) -> KeyResolution {
    let indices: Vec<usize> = references
        .iter()
        .enumerate()
        .filter_map(|(i, candidate)| (*candidate == key).then_some(i))
        .collect();
    if indices.is_empty() {
        KeyResolution::Missing
    } else {
        KeyResolution::MatchedReference {
            reference_indices: indices,
        }
    }
}

fn mesh_palette_audit(model: &Model) -> Vec<MeshPaletteAudit> {
    model
        .pieces
        .iter()
        .enumerate()
        .map(|(i, piece)| {
            let declared_pair = model.bone_ranges.get(i).copied().unwrap_or([0, 0]);
            let drawn = crate::animation_layout::drawn_vertices(piece);
            let index_error = drawn.as_ref().err().cloned();
            let mut bytes = std::collections::BTreeSet::new();
            let mut weighted_slots_without_index_data = 0;
            for (vertex, weights) in piece.weights.iter().enumerate() {
                if !drawn
                    .as_ref()
                    .ok()
                    .and_then(|used| used.get(vertex))
                    .copied()
                    .unwrap_or(false)
                {
                    continue;
                }
                let Some(indices) = piece.bones.get(vertex) else {
                    weighted_slots_without_index_data +=
                        weights.iter().filter(|w| **w != 0.0).count();
                    continue;
                };
                for slot in 0..4 {
                    if weights[slot] != 0.0 {
                        // The importer derives groups using byte / 3. Runtime
                        // adds the first pair value. Exact group ownership is
                        // validated independently by animation_layout.
                        bytes.insert(indices[slot]);
                    }
                }
            }
            let weighted_blend_index_bytes: Vec<u8> = bytes.into_iter().collect();
            let weighted_triplet_slots: Vec<u32> = weighted_blend_index_bytes
                .iter()
                .map(|byte| u32::from(*byte) / 3)
                .collect();
            let candidate_global_indices = weighted_triplet_slots
                .iter()
                .filter_map(|slot| declared_pair[0].checked_add(*slot))
                .collect();
            MeshPaletteAudit {
                piece_index: i,
                declared_pair,
                weighted_blend_index_bytes,
                weighted_triplet_slots,
                candidate_global_indices,
                weighted_slots_without_index_data,
                range_pair_semantics:
                    "native first-bone/count; group ownership checked by animation_layout",
                index_error,
            }
        })
        .collect()
}

/// Produce structured, data-driven diagnostics for an assembled GMDL model.
/// `source_key` is supplied by the caller so the report retains exact origin.
pub fn audit_model(source_key: ResourceKey, model: &Model) -> BindingAudit {
    let mesh_palettes = mesh_palette_audit(model);
    let animation_bindings: Vec<AnimationBindingAudit> = model
        .anim_data
        .iter()
        .enumerate()
        .map(|(binding_index, data)| {
            let mut field_offsets_in_record = BTreeMap::new();
            field_offsets_in_record.insert("transform", 0);
            field_offsets_in_record.insert("field_38", 0x38);
            field_offsets_in_record.insert("field_70", 0x70);
            field_offsets_in_record.insert("key", 0x74);
            field_offsets_in_record.insert("baked_deform_group_count", 0x80);
            let deform_groups = data
                .baked_deforms
                .iter()
                .map(|group| {
                    group
                        .iter()
                        .enumerate()
                        .map(|(ordinal, raw)| analyze_deform_record(raw, ordinal))
                        .collect()
                })
                .collect();
            AnimationBindingAudit {
                binding_index,
                key: crate::resource_audit::key_json(data.key),
                field_offsets_in_record,
                key_resolution: resolution(data.key, &model.references),
                field_70: data.field_70,
                primary_transform: decode_transform(data.transform),
                secondary_transform: decode_transform(data.field_38),
                deform_groups,
                raw_baked_deforms_preserved: true,
            }
        })
        .collect();
    let exact_reference_matches = animation_bindings
        .iter()
        .filter(|b| matches!(b.key_resolution, KeyResolution::MatchedReference { .. }))
        .count();
    let missing_binding_keys = animation_bindings.len() - exact_reference_matches;
    let ambiguous_binding_keys = animation_bindings.iter().filter(|b| matches!(&b.key_resolution, KeyResolution::MatchedReference { reference_indices } if reference_indices.len() > 1)).count();
    let deform_group_count = animation_bindings
        .iter()
        .map(|b| b.deform_groups.len())
        .sum();
    let deform_record_count = animation_bindings
        .iter()
        .flat_map(|b| &b.deform_groups)
        .map(Vec::len)
        .sum();
    let summary = BindingAuditSummary {
        binding_count: animation_bindings.len(),
        exact_reference_matches,
        missing_binding_keys,
        ambiguous_binding_keys,
        deform_group_count,
        deform_record_count,
        finite_primary_transforms: animation_bindings
            .iter()
            .filter(|b| b.primary_transform.all_values_finite)
            .count(),
        weighted_triplet_slots: mesh_palettes
            .iter()
            .map(|p| p.weighted_triplet_slots.len())
            .sum(),
        candidate_global_indices: mesh_palettes
            .iter()
            .map(|p| p.candidate_global_indices.len())
            .sum(),
    };
    BindingAudit {
        source_key: crate::resource_audit::key_json(source_key),
        gmdl_version: model.version,
        model_reference_count: model.references.len(),
        model_piece_count: model.pieces.len(),
        bone_range_count: model.bone_ranges.len(),
        root_bones_field: model.root_bones,
        mesh_palettes,
        animation_bindings,
        summary,
        limits: vec![
            "Resource keys are matched exactly against this GMDL's dependency records; a key match does not establish a skeleton edge.",
            "The parser retains no absolute file offsets, so reported field offsets are relative to GameModelAnimData.",
            "Finite values and homogeneous 4x4 patterns are candidates only; no 160-byte field or parent hierarchy is assigned semantics.",
            "Only triangle-indexed vertices use this piece's palette. Runtime adds first-bone to byte/3; animation_layout separately verifies slot ownership. This does not determine baked bone poses or procedural controllers.",
        ],
    }
}

/// Write an installed-model audit under ignored build output. Callers provide
/// decoded models from the read-only resource index; no package data is changed.
pub fn write_installed_audit(rows: &[BindingAudit]) -> Result<std::path::PathBuf, String> {
    let path = std::path::PathBuf::from("target/gmdl-bindings-063/audit.json");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "version": 1,
        "models": rows,
        "summary": {
            "model_count": rows.len(),
            "binding_count": rows.iter().map(|r| r.summary.binding_count).sum::<usize>(),
            "exact_reference_matches": rows.iter().map(|r| r.summary.exact_reference_matches).sum::<usize>(),
            "missing_binding_keys": rows.iter().map(|r| r.summary.missing_binding_keys).sum::<usize>(),
            "ambiguous_binding_keys": rows.iter().map(|r| r.summary.ambiguous_binding_keys).sum::<usize>(),
            "deform_group_count": rows.iter().map(|r| r.summary.deform_group_count).sum::<usize>(),
            "deform_record_count": rows.iter().map(|r| r.summary.deform_record_count).sum::<usize>(),
            "weighted_triplet_slots": rows.iter().map(|r| r.summary.weighted_triplet_slots).sum::<usize>(),
            "candidate_global_indices": rows.iter().map(|r| r.summary.candidate_global_indices).sum::<usize>(),
        }
    })).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baked_model::{GameModelAnimData, Material, Piece};

    #[test]
    fn transform_decode_uses_verified_complete_field_order_and_keeps_bytes() {
        let mut raw = [0u8; TRANSFORM_BYTES];
        raw[0..2].copy_from_slice(&5u16.to_le_bytes());
        raw[2..4].copy_from_slice(&2u16.to_le_bytes());
        for (i, value) in [
            1.25f32, -2.0, 3.5, 0.75, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
        ]
        .into_iter()
        .enumerate()
        {
            raw[4 + i * 4..8 + i * 4].copy_from_slice(&value.to_le_bytes());
        }
        let decoded = decode_transform(raw);
        assert_eq!(decoded.flags, 5);
        assert_eq!(decoded.transform_count, 2);
        assert_eq!(decoded.offset, [1.25, -2.0, 3.5]);
        assert_eq!(decoded.scale, 0.75);
        assert_eq!(
            decoded.rotation_values,
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        );
        assert_eq!(decoded.raw, raw);
        assert!(decoded.all_values_finite);
        // Transform.readComplete uses readLEShort, unlike unsigned flags.
        raw[2..4].copy_from_slice(&(-1i16).to_le_bytes());
        assert_eq!(decode_transform(raw).transform_count, -1);
        let serialized = serde_json::to_value(&decoded).unwrap();
        assert_eq!(serialized["raw"].as_array().unwrap().len(), TRANSFORM_BYTES);
    }

    #[test]
    fn deform_matrix_detection_is_aligned_and_does_not_drop_raw_record() {
        let mut raw = [0xCC; DEFORM_BYTES];
        for (i, value) in [
            1.0f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 4.0, 5.0, 6.0, 1.0,
        ]
        .into_iter()
        .enumerate()
        {
            raw[32 + i * 4..36 + i * 4].copy_from_slice(&value.to_le_bytes());
        }
        let analysis = analyze_deform_record(&raw, 7);
        assert_eq!(analysis.ordinal, 7);
        assert_eq!(analysis.raw_hex.len(), DEFORM_BYTES * 2);
        assert_eq!(
            analysis
                .matrix_candidates
                .iter()
                .find(|c| c.record_offset == 32)
                .unwrap()
                .homogeneous_boundary_score,
            1
        );
    }

    #[test]
    fn native_inverse_roundtrips_row_vector_rotation_scale_and_offset() {
        // Quarter-turn with unequal translations catches row/column and sign
        // mistakes that an identity transform would hide.
        let mut raw = [0u8; TRANSFORM_BYTES];
        raw[0..2].copy_from_slice(&7u16.to_le_bytes());
        raw[2..4].copy_from_slice(&13i16.to_le_bytes());
        for (ordinal, value) in [2.0f32, -3., 5., -2., 0., 1., 0., -1., 0., 0., 0., 0., 1.]
            .into_iter()
            .enumerate()
        {
            raw[4 + ordinal * 4..8 + ordinal * 4].copy_from_slice(&value.to_le_bytes());
        }
        let source = NativeTransform::decode(raw);
        let inverse = source.inverted_native().unwrap();
        let apply = |transform: &NativeTransform, point: [f32; 3]| -> [f32; 3] {
            std::array::from_fn(|col| {
                transform.offset[col]
                    + transform.scale
                        * (point[0] * transform.rotation_values[col]
                            + point[1] * transform.rotation_values[3 + col]
                            + point[2] * transform.rotation_values[6 + col])
            })
        };
        let point = [1., 4., -6.];
        assert_eq!(apply(&inverse, apply(&source, point)), point);
        assert_eq!(inverse.flags, 7);
        assert_eq!(inverse.transform_count, 13);
        assert_eq!(inverse.inverted_native().unwrap().raw, source.raw);
        raw[16..20].copy_from_slice(&0f32.to_le_bytes());
        assert!(NativeTransform::decode(raw).inverted_native().is_none());
        raw[16..20].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(NativeTransform::decode(raw).inverted_native().is_none());
    }

    #[test]
    fn skin_descriptor_keeps_secondary_separate_and_inverts_primary() {
        let mut raw = [0u8; TRANSFORM_BYTES];
        for (ordinal, value) in [1.0f32, 2., 3., 2., 1., 0., 0., 0., 1., 0., 0., 0., 1.]
            .into_iter()
            .enumerate()
        {
            raw[4 + ordinal * 4..8 + ordinal * 4].copy_from_slice(&value.to_le_bytes());
        }
        let mut secondary = raw;
        secondary[4..8].copy_from_slice(&20f32.to_le_bytes());
        let binding = crate::baked_model::GameModelAnimData {
            transform: raw,
            field_38: secondary,
            field_70: u32::MAX,
            key: ResourceKey {
                instance_id: 1,
                type_id: 2,
                group_id: 3,
            },
            baked_deforms: vec![],
        };
        let result = SkinTransforms::from_binding(&binding).unwrap();
        assert_eq!(result.primary.offset, [1., 2., 3.]);
        assert_eq!(result.secondary.offset, [20., 2., 3.]);
        assert_eq!(result.inverse_primary.offset, [-0.5, -1., -1.5]);
        assert_eq!(result.inverse_primary.scale, 0.5);
    }

    #[test]
    fn exact_key_resolution_reports_missing_and_duplicate_edges() {
        let key = ResourceKey {
            type_id: 1,
            group_id: 2,
            instance_id: 3,
        };
        assert!(matches!(resolution(key, &[]), KeyResolution::Missing));
        assert!(
            matches!(resolution(key, &[key, key]), KeyResolution::MatchedReference { reference_indices } if reference_indices == vec![0, 1])
        );
    }

    #[test]
    fn mesh_palette_audit_reports_local_and_candidate_global_indices_without_validating_range() {
        let model = Model {
            version: 9,
            references: vec![],
            pieces: vec![Piece {
                positions: vec![[0.0; 3]],
                normals: vec![],
                uvs: vec![],
                indices: vec![0, 0, 0],
                bones: vec![[0, 3, 6, 9]],
                weights: vec![[1.0, 1.0, 0.0, 1.0]],
                material_id: 0,
            }],
            materials: vec![Material::default()],
            bone_ranges: vec![[2, 1]],
            root_bones: 1,
            anim_data: vec![],
            unknown_key: ResourceKey {
                type_id: 0,
                group_id: 0,
                instance_id: 0,
            },
            native_geometry: vec![],
        };
        let palette = mesh_palette_audit(&model);
        assert_eq!(palette[0].weighted_blend_index_bytes, vec![0, 3, 9]);
        assert_eq!(palette[0].weighted_triplet_slots, vec![0, 1, 3]);
        assert_eq!(palette[0].candidate_global_indices, vec![2, 3, 5]);
        assert_eq!(palette[0].declared_pair, [2, 1]);
        assert!(palette[0].index_error.is_none());
    }

    #[test]
    #[ignore = "reads installed Creature and Cell GMDL records and writes ignored target audit output"]
    fn installed_creature_cell_binding_census_has_320_models_and_writes_audit() {
        let install = crate::spore_install::discover().expect("installed Spore assets");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut rows = Vec::new();
        let mut errors = Vec::new();
        for group in [
            crate::baked_model::CREATURE_GROUP,
            crate::cell_visuals::CELL_MODEL_GROUP,
        ] {
            for key in crate::baked_model::candidates(&index, group) {
                match index
                    .read(key)
                    .and_then(|bytes| crate::baked_model::parse(&bytes))
                {
                    Ok(model) => rows.push(audit_model(key, &model)),
                    Err(error) => errors.push(format!("{key:?}: {error}")),
                }
            }
        }
        assert!(
            errors.is_empty(),
            "failed to parse installed GMDLs: {errors:?}"
        );
        assert_eq!(rows.len(), 320, "Creature+Cell candidate count changed");
        let path = write_installed_audit(&rows).expect("write ignored installed audit");
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read generated audit"))
                .expect("valid audit JSON");
        assert_eq!(report["summary"]["model_count"], 320);
        assert_eq!(report["models"].as_array().unwrap().len(), 320);
        assert!(report["models"].as_array().unwrap().iter().all(|row| {
            row["source_key"]["type"]
                .as_str()
                .is_some_and(|s| s.len() == 8)
                && row["source_key"]["group"]
                    .as_str()
                    .is_some_and(|s| s.len() == 8)
                && row["source_key"]["instance"]
                    .as_str()
                    .is_some_and(|s| s.len() == 8)
        }));
    }
}
