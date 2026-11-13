//! Clip-driven skinning for installed creature GMDLs.
//!
//! GMDL vertices store four byte weights and bone slots local to each piece.
//! Exact RW4 dependencies supply linked skeletons, inverse bind poses, and
//! native KeyAnim clips. Pieces are animated independently when their drawn
//! weighted slots belong to one validated rig group; unresolved pieces stay in
//! their authored bind pose.

use crate::{
    asset_catalog::ResourceIndex,
    dbpf::ResourceKey,
    rw4::{self, Mat34},
};
use bevy::prelude::*;

#[derive(Clone)]
pub struct CreatureMotion {
    pub model_key: ResourceKey,
    pub rig_key: ResourceKey,
    pieces: Vec<crate::baked_model::Piece>,
    piece_rigs: Vec<Option<(usize, usize)>>,
    rigs: Vec<RigAnimation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MotionRejectionReason {
    EmptyModel,
    InvalidPaletteLayout,
    IncompletePaletteCoverage,
    NoExactLinkedAnimatedRig,
    NoPieceMapsToOneExactRigGroup,
    BindPoseCouldNotBeSampled,
    BindPoseCouldNotBeSkinned,
    BindPoseDoesNotMatchAuthoredGeometry { piece_index: usize },
}

#[derive(Clone)]
pub enum MotionPreparation {
    Ready(CreatureMotion),
    Unsupported(MotionRejectionReason),
}

#[derive(Clone)]
struct RigAnimation {
    rig_key: ResourceKey,
    parents: Vec<Option<usize>>,
    locals: Vec<Mat34>,
    inv_bind: Vec<Mat34>,
    clip: rw4::KeyAnim,
    clip_index: usize,
    clip_match_count: usize,
    clip_tied_best_count: usize,
    chan_for_bone: Vec<Option<usize>>,
    bind_t: Vec<[f32; 3]>,
    key0_loc: Vec<[f32; 3]>,
}

impl CreatureMotion {
    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    pub fn bone_count(&self) -> usize {
        self.rigs.iter().map(|rig| rig.parents.len()).sum()
    }

    pub fn clip_length(&self) -> f32 {
        self.rigs
            .iter()
            .map(|rig| rig.clip.length)
            .fold(0.0, f32::max)
    }

    /// Build actor-owned mesh copies. Each copy retains the imported piece's
    /// atlas UVs, topology, and vertex layout; callers pair each handle with
    /// the matching `OriginalModel.surfaces[i].material`.
    pub fn clone_meshes(
        &self,
        source: &crate::OriginalModel,
        meshes: &mut Assets<Mesh>,
    ) -> Option<Vec<Handle<Mesh>>> {
        if source.surfaces.len() != self.pieces.len() {
            return None;
        }
        source
            .surfaces
            .iter()
            .map(|surface| {
                meshes
                    .get(&surface.handle)
                    .cloned()
                    .map(|mesh| meshes.add(mesh))
            })
            .collect()
    }

    /// Update actor-owned piece meshes for one clip time. Each actor must use
    /// its own handles so two NPCs cannot overwrite each other's deformation.
    /// `time` is seconds and loops over the native clip's own length.
    pub fn update_meshes(
        &self,
        time: f32,
        handles: &[Handle<Mesh>],
        meshes: &mut Assets<Mesh>,
    ) -> bool {
        if handles.len() != self.pieces.len() || !time.is_finite() || self.rigs.is_empty() {
            return false;
        }
        let worlds: Vec<_> = self
            .rigs
            .iter()
            .map(|rig| rig.sample_worlds(time))
            .collect();
        let mut updated = false;
        for ((piece, handle), rig_index) in self.pieces.iter().zip(handles).zip(&self.piece_rigs) {
            let Some((rig_index, bone_base)) = rig_index else {
                continue;
            };
            let Some(rig) = self.rigs.get(*rig_index) else {
                continue;
            };
            let Some(worlds) = worlds.get(*rig_index).and_then(Option::as_ref) else {
                continue;
            };
            let Some(deformed) = skin_piece(piece, *bone_base, worlds, &rig.inv_bind) else {
                continue;
            };
            let Some(mut mesh) = meshes.get_mut(handle) else {
                continue;
            };
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, deformed.positions);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, deformed.normals);
            updated = true;
        }
        updated
    }
}

impl RigAnimation {
    fn sample_worlds(&self, time: f32) -> Option<Vec<Mat34>> {
        if !time.is_finite() || self.clip.length <= 0. {
            return None;
        }
        let mut locals = self.locals.clone();
        let t = time.rem_euclid(self.clip.length);
        for (bone, channel) in self.chan_for_bone.iter().enumerate() {
            let Some(channel) = channel else { continue };
            let Some((rotation, location)) = self.clip.sample(*channel, t) else {
                continue;
            };
            let matrix = rw4::quat_to_mat33(rotation);
            for row in 0..3 {
                for col in 0..3 {
                    locals[bone][row][col] = matrix[row][col];
                }
            }
            let bind = self.bind_t[bone];
            let first = self.key0_loc[bone];
            for axis in 0..3 {
                locals[bone][axis][3] = bind[axis] + location[axis] - first[axis];
            }
        }
        rw4::fk_worlds(&self.parents, &locals)
    }
}

/// Resolve exact RW4 dependencies and animate pieces whose weighted slots map
/// wholly to one linked skin group. Reserved and cross-group pieces retain the
/// authored bind pose.
pub fn prepare(
    index: &ResourceIndex,
    model_key: ResourceKey,
) -> Result<Option<CreatureMotion>, String> {
    Ok(match prepare_diagnostic(index, model_key)? {
        MotionPreparation::Ready(motion) => Some(motion),
        MotionPreparation::Unsupported(_) => None,
    })
}

/// Resolve animation support while preserving the reason a model stayed static.
/// The legacy `prepare` API intentionally maps every unsupported case to None.
pub fn prepare_diagnostic(
    index: &ResourceIndex,
    model_key: ResourceKey,
) -> Result<MotionPreparation, String> {
    if model_key.type_id != crate::decode::TYPE_GMDL {
        return Err("creature motion requires a GMDL key".into());
    }
    let model = crate::baked_model::parse(&index.read(model_key)?)?;
    if model.pieces.is_empty() || model.bone_ranges.len() != model.pieces.len() {
        return Ok(MotionPreparation::Unsupported(
            MotionRejectionReason::EmptyModel,
        ));
    }
    let Ok(layout) = crate::animation_layout::resolve(index, &model) else {
        return Ok(MotionPreparation::Unsupported(
            MotionRejectionReason::InvalidPaletteLayout,
        ));
    };
    if !layout.all_drawn_slots_covered || layout.declared_range_end != layout.bone_count {
        return Ok(MotionPreparation::Unsupported(
            MotionRejectionReason::IncompletePaletteCoverage,
        ));
    }

    // Keep one optional animation per native group. The group order and sizes
    // come from the validated GMDL binding layout, including reserved slots.
    let mut rigs = Vec::new();
    let mut rig_for_binding = vec![None; layout.groups.len()];
    for group in &layout.groups {
        if group.reserved {
            continue;
        }
        let binding = &model.anim_data[group.binding];
        let Ok(bytes) = index.read(binding.key) else {
            continue;
        };
        let mut stats = rw4::MeshStats::default();
        let animations = rw4::extract_anims(&bytes, &mut stats);
        let Ok(linked) = rw4::extract_linked_skin_rigs(&bytes) else {
            continue;
        };
        if linked.len() != 1
            || binding.baked_deforms.len() != 1
            || binding.baked_deforms[0].len() != group.bone_count
        {
            continue;
        }
        let linked = &linked[0];
        let Some((locals, inv_bind)) = rw4::bind_locals(&linked.bones, &linked.poses) else {
            continue;
        };
        let Some(selection) = select_clip(&linked.bones, &animations) else {
            continue;
        };
        let Some(pair) = rw4::pair_clip(
            &linked.bones,
            &locals,
            std::slice::from_ref(&animations[selection.index]),
        ) else {
            continue;
        };
        if pair.clip.length <= 0. || !pair.chan_for_bone.iter().any(Option::is_some) {
            continue;
        }
        let rig_index = rigs.len();
        rig_for_binding[group.binding] = Some(rig_index);
        rigs.push(RigAnimation {
            rig_key: binding.key,
            parents: linked.bones.iter().map(|bone| bone.parent).collect(),
            locals,
            inv_bind,
            clip: pair.clip,
            clip_index: selection.index,
            clip_match_count: selection.matches,
            clip_tied_best_count: selection.tied_best_count,
            chan_for_bone: pair.chan_for_bone,
            bind_t: pair.bind_t,
            key0_loc: pair.key0_loc,
        });
    }
    if rigs.is_empty() {
        return Ok(MotionPreparation::Unsupported(
            MotionRejectionReason::NoExactLinkedAnimatedRig,
        ));
    }
    let piece_rigs: Vec<_> = model
        .pieces
        .iter()
        .zip(&model.bone_ranges)
        .map(|(piece, range)| piece_group(piece, *range, &layout.groups, &rig_for_binding))
        .collect();
    if !piece_rigs.iter().any(Option::is_some) {
        return Ok(MotionPreparation::Unsupported(
            MotionRejectionReason::NoPieceMapsToOneExactRigGroup,
        ));
    }
    // A linked RW4 skin is not enough: refuse to animate if its first sampled
    // pose fails to reproduce the assembled GMDL's authored piece geometry.
    // This also catches missing primary/secondary transform composition until
    // native group-space math is independently established.
    for (piece_index, (piece, binding)) in model.pieces.iter().zip(&piece_rigs).enumerate() {
        let Some((rig_index, bone_base)) = binding else {
            continue;
        };
        let Some(rig) = rigs.get(*rig_index) else {
            return Ok(MotionPreparation::Unsupported(
                MotionRejectionReason::BindPoseCouldNotBeSampled,
            ));
        };
        let Some(worlds) = rig.sample_worlds(0.0) else {
            return Ok(MotionPreparation::Unsupported(
                MotionRejectionReason::BindPoseCouldNotBeSampled,
            ));
        };
        let Some(deformed) = skin_piece(piece, *bone_base, &worlds, &rig.inv_bind) else {
            return Ok(MotionPreparation::Unsupported(
                MotionRejectionReason::BindPoseCouldNotBeSkinned,
            ));
        };
        if !matches_authored_pose(piece, &deformed.positions) {
            return Ok(MotionPreparation::Unsupported(
                MotionRejectionReason::BindPoseDoesNotMatchAuthoredGeometry { piece_index },
            ));
        }
    }
    Ok(MotionPreparation::Ready(CreatureMotion {
        model_key,
        rig_key: rigs[0].rig_key,
        pieces: model.pieces,
        piece_rigs,
        rigs,
    }))
}

fn matches_authored_pose(piece: &crate::baked_model::Piece, positions: &[[f32; 3]]) -> bool {
    if piece.positions.len() != positions.len() {
        return false;
    }
    piece
        .positions
        .iter()
        .zip(positions)
        .all(|(source, actual)| {
            let expected = [-source[0], source[2], source[1]];
            (0..3).all(|axis| {
                let tolerance = 1.0e-3 + expected[axis].abs() * 1.0e-5;
                (expected[axis] - actual[axis]).abs() <= tolerance
            })
        })
}

struct ClipSelection {
    index: usize,
    matches: usize,
    tied_best_count: usize,
}

/// Mirror `rw4::pair_clip` selection and expose its tie behavior for reports.
/// Channels are matched only by exact bone-name/channel-ID equality and the
/// first key must be non-blend, just as in that sampler.
fn select_clip(bones: &[rw4::Bone], animations: &[rw4::KeyAnim]) -> Option<ClipSelection> {
    let mut best = None;
    let mut best_matches = 0usize;
    let mut tied_best_count = 0usize;
    for (index, animation) in animations.iter().enumerate() {
        let matches = bones
            .iter()
            .filter(|bone| {
                animation.channels.iter().any(|channel| {
                    channel.id == bone.name && channel.keys.first().is_some_and(|key| !key.blend)
                })
            })
            .count();
        if matches > best_matches {
            best = Some(index);
            best_matches = matches;
            tied_best_count = 1;
        } else if matches > 0 && matches == best_matches {
            tied_best_count += 1;
        }
    }
    Some(ClipSelection {
        index: best?,
        matches: best_matches,
        tied_best_count,
    })
}

/// A piece is safe to animate only when every drawn, positively weighted slot
/// belongs to one exact linked rig group. The returned base converts its GMDL
/// palette indices to indices local to that linked skeleton.
fn piece_group(
    piece: &crate::baked_model::Piece,
    range: [u32; 2],
    groups: &[crate::animation_layout::BoneGroup],
    rig_for_binding: &[Option<usize>],
) -> Option<(usize, usize)> {
    let used = crate::animation_layout::drawn_vertices(piece).ok()?;
    if piece.bones.len() != piece.positions.len() || piece.weights.len() != piece.positions.len() {
        return None;
    }
    let range_start = range[0] as usize;
    let range_end = range_start.checked_add(range[1] as usize)?;
    let mut owner: Option<(usize, usize)> = None;
    for (vertex, is_drawn) in used.iter().copied().enumerate() {
        if !is_drawn {
            continue;
        }
        for (&encoded, &weight) in piece.bones[vertex].iter().zip(&piece.weights[vertex]) {
            if !weight.is_finite() || weight < 0. {
                return None;
            }
            if weight == 0. {
                continue;
            }
            if encoded % 3 != 0 {
                return None;
            }
            let global = range_start.checked_add(encoded as usize / 3)?;
            if global >= range_end {
                return None;
            }
            let group = groups.iter().find(|group| {
                global >= group.first_bone && global < group.first_bone + group.bone_count
            })?;
            let rig_index = rig_for_binding.get(group.binding).copied().flatten()?;
            let local_base = range_start.checked_sub(group.first_bone)?;
            let candidate = (rig_index, local_base);
            if owner.is_some_and(|existing| existing != candidate) {
                return None;
            }
            owner = Some(candidate);
        }
    }
    owner
}

fn key_json(key: ResourceKey) -> serde_json::Value {
    serde_json::json!({
        "group": format!("{:08X}", key.group_id),
        "instance": format!("{:08X}", key.instance_id),
        "type": format!("{:08X}", key.type_id)
    })
}

#[cfg(test)]
fn valid_gmdl_binding(model: &crate::baked_model::Model, bone_count: usize) -> bool {
    model.pieces.len() == model.bone_ranges.len()
        && model
            .pieces
            .iter()
            .zip(&model.bone_ranges)
            .all(|(piece, range)| {
                let base = range[0] as usize;
                let Some(end) = base.checked_add(range[1] as usize) else {
                    return false;
                };
                let Ok(used) = crate::animation_layout::drawn_vertices(piece) else {
                    return false;
                };
                piece.positions.len() == piece.bones.len()
                    && piece.positions.len() == piece.weights.len()
                    && end <= bone_count
                    && piece.bones.iter().zip(&piece.weights).zip(&used).all(
                        |((indices, weights), drawn)| {
                            if !drawn {
                                return true;
                            }
                            let sum: f32 = weights.iter().sum();
                            sum.is_finite()
                                && indices.iter().zip(weights).all(|(encoded, weight)| {
                                    weight.is_finite()
                                        && *weight >= 0.
                                        && (*weight == 0.
                                            || (*encoded % 3 == 0
                                                && base
                                                    .checked_add((*encoded as usize) / 3)
                                                    .is_some_and(|bone| {
                                                        bone < end && bone < bone_count
                                                    })))
                                })
                        },
                    )
            })
}

struct DeformedPiece {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
}

fn skin_piece(
    piece: &crate::baked_model::Piece,
    bone_base: usize,
    worlds: &[Mat34],
    inv_bind: &[Mat34],
) -> Option<DeformedPiece> {
    if piece.positions.len() != piece.bones.len()
        || piece.positions.len() != piece.weights.len()
        || piece.positions.len() != piece.normals.len()
        || worlds.len() != inv_bind.len()
    {
        return None;
    }
    let basis = |p: [f32; 3]| [-p[0], p[2], p[1]];
    let used = crate::animation_layout::drawn_vertices(piece).ok()?;
    let mut positions = Vec::with_capacity(piece.positions.len());
    let mut normals = Vec::with_capacity(piece.normals.len());
    for (ordinal, (vertex, (indices, weights))) in piece
        .positions
        .iter()
        .zip(piece.bones.iter().zip(&piece.weights))
        .enumerate()
    {
        if !used[ordinal] {
            positions.push(basis(*vertex));
            normals.push(
                Vec3::from_array(basis(piece.normals[ordinal]))
                    .normalize_or_zero()
                    .to_array(),
            );
            continue;
        }
        let total: f32 = weights.iter().copied().sum();
        if !total.is_finite() || weights.iter().any(|weight| *weight < 0.) {
            return None;
        }
        if total <= f32::EPSILON {
            positions.push(basis(*vertex));
            let normal = piece.normals.get(positions.len() - 1).copied()?;
            normals.push(
                Vec3::from_array(basis(normal))
                    .normalize_or_zero()
                    .to_array(),
            );
            continue;
        }
        let mut out = [0.; 3];
        let mut normal = [0.; 3];
        for influence in 0..4 {
            let weight = weights[influence] / total;
            if weight <= 0. {
                continue;
            }
            if indices[influence] % 3 != 0 {
                return None;
            }
            let bone = bone_base.checked_add(indices[influence] as usize / 3)?;
            let (Some(world), Some(inverse)) = (worlds.get(bone), inv_bind.get(bone)) else {
                return None;
            };
            let skin = rw4::mat34_mul(world, inverse);
            let p = rw4::mat34_apply(&skin, vertex);
            let n = [
                skin[0][0] * piece.normals.get(positions.len())?[0]
                    + skin[0][1] * piece.normals[positions.len()][1]
                    + skin[0][2] * piece.normals[positions.len()][2],
                skin[1][0] * piece.normals[positions.len()][0]
                    + skin[1][1] * piece.normals[positions.len()][1]
                    + skin[1][2] * piece.normals[positions.len()][2],
                skin[2][0] * piece.normals[positions.len()][0]
                    + skin[2][1] * piece.normals[positions.len()][1]
                    + skin[2][2] * piece.normals[positions.len()][2],
            ];
            for axis in 0..3 {
                out[axis] += p[axis] * weight;
                normal[axis] += n[axis] * weight;
            }
        }
        positions.push(basis(out));
        normals.push(
            Vec3::from_array(basis(normal))
                .normalize_or_zero()
                .to_array(),
        );
    }
    if positions
        .iter()
        .flatten()
        .chain(normals.iter().flatten())
        .any(|v| !v.is_finite())
    {
        return None;
    }
    Some(DeformedPiece { positions, normals })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_bind_compensation_keeps_vertex_still_at_bind_pose() {
        let bind = rw4::Mat34::from([[1., 0., 0., 2.], [0., 1., 0., 0.], [0., 0., 1., 0.]]);
        let inverse = rw4::mat34_inverse_rigid(&bind);
        let vertex = [3., 1., 0.];
        let actual = rw4::mat34_apply(&rw4::mat34_mul(&bind, &inverse), &vertex);
        assert_eq!(actual, vertex);
    }

    #[test]
    fn gmdl_skinning_preserves_vertex_and_normal_at_bind_pose() {
        let bind = [[1., 0., 0., 2.], [0., 1., 0., 0.], [0., 0., 1., 0.]];
        let inverse = rw4::mat34_inverse_rigid(&bind);
        let piece = crate::baked_model::Piece {
            positions: vec![[3., 1., 0.]],
            normals: vec![[0., 1., 0.]],
            uvs: vec![[0., 0.]],
            indices: vec![0, 0, 0],
            bones: vec![[0; 4]],
            weights: vec![[1., 0., 0., 0.]],
            material_id: 0,
        };
        let result = skin_piece(&piece, 0, &[bind], &[inverse]).unwrap();
        assert_eq!(result.positions, vec![[-3., 0., 1.]]);
        assert_eq!(result.normals, vec![[0., 0., 1.]]);
    }

    #[test]
    fn gmdl_indices_are_piece_relative_triplet_slots() {
        let mut model = crate::baked_model::Model {
            version: 9,
            references: vec![],
            pieces: vec![crate::baked_model::Piece {
                positions: vec![[0.; 3]],
                normals: vec![[0., 1., 0.]],
                uvs: vec![[0.; 2]],
                indices: vec![0, 0, 0],
                bones: vec![[3, 0, 0, 0]],
                weights: vec![[1., 0., 0., 0.]],
                material_id: 0,
            }],
            materials: vec![],
            bone_ranges: vec![[2, 2]],
            root_bones: 0,
            anim_data: vec![],
            unknown_key: ResourceKey {
                type_id: 0,
                group_id: 0,
                instance_id: 0,
            },
            native_geometry: vec![],
        };
        assert!(valid_gmdl_binding(&model, 4));
        model.pieces[0].bones[0][0] = 6;
        assert!(!valid_gmdl_binding(&model, 4));
        model.pieces[0].bones[0][0] = 4;
        assert!(!valid_gmdl_binding(&model, 4), "unaligned matrix row slot");
        model.pieces[0].bones[0][0] = 3;
        model.pieces[0].weights[0][0] = -1.;
        assert!(!valid_gmdl_binding(&model, 4));
    }

    #[test]
    fn assembled_piece_maps_to_its_exact_rig_group_and_reserved_piece_stays_static() {
        let groups = vec![
            crate::animation_layout::BoneGroup {
                binding: 0,
                first_bone: 0,
                bone_count: 18,
                rig_source: None,
                skin_link: None,
                reserved: true,
                transforms: None,
            },
            crate::animation_layout::BoneGroup {
                binding: 1,
                first_bone: 18,
                bone_count: 4,
                rig_source: None,
                skin_link: None,
                reserved: false,
                transforms: None,
            },
            crate::animation_layout::BoneGroup {
                binding: 2,
                first_bone: 22,
                bone_count: 4,
                rig_source: None,
                skin_link: None,
                reserved: false,
                transforms: None,
            },
        ];
        let rig_for_binding = [None, Some(0), Some(1)];
        let mut piece = crate::baked_model::Piece {
            positions: vec![[0.; 3]],
            normals: vec![[0., 1., 0.]],
            uvs: vec![[0.; 2]],
            indices: vec![0, 0, 0],
            bones: vec![[3, 0, 0, 0]],
            weights: vec![[1., 0., 0., 0.]],
            material_id: 0,
        };

        // Global slot 19 belongs to binding 1, while local triplet slot 1
        // becomes skeleton-local slot 1 after subtracting group first_bone.
        assert_eq!(
            piece_group(&piece, [18, 4], &groups, &rig_for_binding),
            Some((0, 0))
        );

        // A drawn piece bound to the reserved controller group is left alone.
        assert_eq!(
            piece_group(&piece, [0, 18], &groups, &rig_for_binding),
            None
        );

        // Cross-group influences cannot be represented by one linked skin.
        piece.bones[0] = [0, 12, 0, 0];
        piece.weights[0] = [0.5, 0.5, 0., 0.];
        assert_eq!(
            piece_group(&piece, [18, 8], &groups, &rig_for_binding),
            None
        );
    }

    #[test]
    fn clip_selection_matches_exact_non_blend_bone_ids_and_reports_ties() {
        let bones = [rw4::Bone {
            name: 0x1234,
            flags: 0,
            parent: None,
        }];
        let key = rw4::Keyframe {
            time: 0.0,
            rotation: [0.0, 0.0, 0.0, 1.0],
            location: [0.0; 3],
            scale: [1.0; 3],
            factor: 0.0,
            blend: false,
        };
        let matching = |id, blend| rw4::AnimChannel {
            id,
            keys: vec![rw4::Keyframe {
                blend,
                ..key.clone()
            }],
        };
        let animations = vec![
            rw4::KeyAnim {
                skeleton_id: 1,
                length: 1.0,
                channels: vec![matching(0x1234, false)],
            },
            rw4::KeyAnim {
                skeleton_id: 2,
                length: 2.0,
                channels: vec![matching(0x1234, false)],
            },
            rw4::KeyAnim {
                skeleton_id: 3,
                length: 3.0,
                channels: vec![matching(0x1235, false), matching(0x1234, true)],
            },
        ];

        let selected = select_clip(&bones, &animations).unwrap();
        assert_eq!(selected.index, 0);
        assert_eq!(selected.matches, 1);
        assert_eq!(selected.tied_best_count, 2);
    }

    #[test]
    #[ignore = "reads installed starter GMDL and exact linked RW4 clips"]
    fn installed_full_starter_motion_reports_unsupported_reason() {
        let install = crate::spore_install::discover().expect("installed Spore assets");
        let index = ResourceIndex::load(&install.packages);
        let model_key = ResourceKey {
            group_id: 0x4062_7100,
            instance_id: 0x067B_0FBC,
            type_id: crate::decode::TYPE_GMDL,
        };
        let result = prepare_diagnostic(&index, model_key).expect("starter model parses");
        let MotionPreparation::Unsupported(reason) = result else {
            panic!("full starter unexpectedly passed the exact native-motion safety gates");
        };
        println!("full starter remains static: {reason:?}");
        assert!(prepare(&index, model_key).unwrap().is_none());
    }

    #[test]
    fn shared_vertex_buffer_only_deforms_drawn_vertices() {
        let piece = crate::baked_model::Piece {
            positions: vec![[1., 0., 0.], [2., 0., 0.]],
            normals: vec![[0., 1., 0.]; 2],
            uvs: vec![[0.; 2]; 2],
            indices: vec![0, 0, 0],
            bones: vec![[0; 4], [255; 4]],
            weights: vec![[1., 0., 0., 0.]; 2],
            material_id: 0,
        };
        let inverse = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]];
        let world = [[1., 0., 0., 3.], [0., 1., 0., 0.], [0., 0., 1., 0.]];
        let result = skin_piece(&piece, 0, &[world], &[inverse]).unwrap();
        assert_eq!(result.positions, vec![[-4., 0., 0.], [-2., 0., 0.]]);
        let mut invalid = piece.clone();
        invalid.indices[2] = 2;
        assert!(skin_piece(&invalid, 0, &[world], &[inverse]).is_none());
        invalid = piece;
        invalid.bones[0][0] = 1;
        assert!(skin_piece(&invalid, 0, &[world], &[inverse]).is_none());
    }

    #[test]
    #[ignore = "reads the installed Spore GMDL and RW4 dependencies"]
    fn installed_creature_and_cell_motion_binding_census() {
        let install = crate::spore_install::discover().expect("installed Spore assets");
        let index = ResourceIndex::load(&install.packages);
        let groups = [
            ("creature", crate::baked_model::CREATURE_GROUP),
            ("cell", crate::cell_visuals::CELL_MODEL_GROUP),
        ];
        let mut models = Vec::new();
        let mut summary = serde_json::Map::new();
        for (label, group) in groups {
            let mut group_models = Vec::new();
            let keys = crate::baked_model::candidates(&index, group);
            let mut parsed = 0usize;
            let mut parse_errors = 0usize;
            let mut with_rw4_refs = 0usize;
            let mut rig_refs = 0usize;
            let mut models_with_part_motion = 0usize;
            let mut animated_rig_groups = 0usize;
            for model_key in keys {
                let mut row = serde_json::json!({
                    "model": key_json(model_key),
                    "parse": "ok",
                    "references": [],
                    "linked_rig_resources": 0,
                    "resolved": false
                });
                let model = match index
                    .read(model_key)
                    .and_then(|bytes| crate::baked_model::parse(&bytes))
                {
                    Ok(model) => {
                        parsed += 1;
                        model
                    }
                    Err(error) => {
                        parse_errors += 1;
                        row["parse"] = error.into();
                        group_models.push(row);
                        continue;
                    }
                };
                let mut seen = std::collections::HashSet::new();
                let rig_keys: Vec<_> = model
                    .references
                    .iter()
                    .copied()
                    .filter(|key| key.type_id == crate::asset_catalog::RW4 && seen.insert(*key))
                    .collect();
                rig_refs += rig_keys.len();
                with_rw4_refs += usize::from(!rig_keys.is_empty());
                row["linked_rig_resources"] = rig_keys.len().into();
                row["references"] =
                    serde_json::json!(rig_keys.iter().copied().map(key_json).collect::<Vec<_>>());
                let part_motion = prepare(&index, model_key);
                match part_motion {
                    Ok(Some(motion)) => {
                        models_with_part_motion += 1;
                        animated_rig_groups += motion.rigs.len();
                        row["resolved"] = true.into();
                        row["part_motion"] = serde_json::json!({
                            "animated_rig_groups": motion.rigs.len(),
                            "animated_pieces": motion.piece_rigs.iter().filter(|piece| piece.is_some()).count(),
                            "total_pieces": motion.piece_count(),
                            "animated_bones": motion.bone_count(),
                            "clips": motion.rigs.iter().map(|rig| serde_json::json!({
                                "rig": key_json(rig.rig_key),
                                "rw4_object_ordinal": rig.clip_index,
                                "skeleton_id": format!("{:08X}", rig.clip.skeleton_id),
                                "length_seconds": rig.clip.length,
                                "matched_exact_bone_channels": rig.clip_match_count,
                                "tied_best_clips": rig.clip_tied_best_count
                            })).collect::<Vec<_>>(),
                            "clip_selection": "highest exact non-blend bone-name/channel-ID match; ties retain first RW4 KeyAnim in resource order"
                        });
                    }
                    Ok(None) => {
                        row["resolved"] = false.into();
                        row["status"] =
                            "no safely mapped piece with one exact linked rig and sampled clip"
                                .into();
                    }
                    Err(error) => {
                        row["resolved"] = false.into();
                        row["status"] = error.into();
                    }
                }
                group_models.push(row);
            }
            summary.insert(
                label.into(),
                serde_json::json!({
                    "group": format!("{group:08X}"),
                    "models": group_models.len(),
                    "parsed": parsed,
                    "parse_errors": parse_errors,
                    "with_rw4_references": with_rw4_refs,
                    "rw4_references": rig_refs,
                    "models_with_part_motion": models_with_part_motion,
                    "animated_rig_groups": animated_rig_groups
                }),
            );
            models.extend(group_models);
        }
        let total_models = models.len();
        let census = serde_json::json!({
            "install": install.root.display().to_string(),
            "groups": summary,
            "model_details": models
        });
        let output = std::path::Path::new("target/native-animation-067/motion-census.json");
        std::fs::create_dir_all(output.parent().unwrap()).unwrap();
        std::fs::write(output, serde_json::to_vec_pretty(&census).unwrap()).unwrap();
        println!(
            "GMDL motion census: {} models, {} with part motion, {} animated rig groups; wrote {}",
            total_models,
            part_motion_models_total(&census),
            animated_groups_total(&census),
            output.display()
        );
    }

    fn part_motion_models_total(census: &serde_json::Value) -> usize {
        ["creature", "cell"]
            .iter()
            .filter_map(|name| census["groups"][name]["models_with_part_motion"].as_u64())
            .map(|value| value as usize)
            .sum()
    }

    fn animated_groups_total(census: &serde_json::Value) -> usize {
        ["creature", "cell"]
            .iter()
            .filter_map(|name| census["groups"][name]["animated_rig_groups"].as_u64())
            .map(|value| value as usize)
            .sum()
    }
}
