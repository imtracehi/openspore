//! Resolve saved handle progress through the RW4 animation/shape graph.
//! The CPU result is committed only after every block buffer validates.
use crate::rw4::{ExtractedMesh, MorphData};
use bevy::math::{Mat3, Mat4, Quat, Vec3};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default)]
pub struct MorphApplication {
    pub handle_count: usize,
    pub shape_weights: Vec<(u32, f32)>,
    pub moved_vertices: usize,
    pub attachment_channels: usize,
}

pub fn apply_creation_handles(
    meshes: &mut [ExtractedMesh],
    data: &MorphData,
    overrides: &[(u32, f32)],
) -> Result<MorphApplication, String> {
    if !data.errors.is_empty() {
        return Err(data.errors.join("; "));
    }
    let mut overrides_by_id = HashMap::new();
    for &(id, w) in overrides {
        if !w.is_finite() {
            return Err("non-finite handle progress".into());
        }
        if overrides_by_id.insert(id, w.clamp(0., 1.)).is_some() {
            return Err("duplicate XML handle".into());
        }
        if !data.handles.iter().any(|h| h.id == id) {
            return Err(format!("unknown XML handle {id:08X}"));
        }
    }
    if data.handles.is_empty() {
        return Ok(MorphApplication::default());
    }
    if data.shapes.is_empty() && data.rig.is_some() {
        return apply_rig_handles(meshes, data, &overrides_by_id);
    }
    // The current mesh reader binds each mesh slice to one pool. Reject
    // multiple pools rather than guessing a pool or a shape-set association.
    if data.shapes.len() != 1 || data.pools.len() != 1 {
        return Err(format!(
            "ambiguous morph shape set or vertex pool ({} shape sets, {} pools)",
            data.shapes.len(),
            data.pools.len()
        ));
    }
    let ids = &data.shapes[0].shape_ids;
    if ids.is_empty() || data.pools[0].shapes != ids.len() {
        return Err("morph shape count mismatch".into());
    }
    let mut shape_index = HashMap::new();
    for (index, &id) in ids.iter().enumerate() {
        if shape_index.insert(id, index).is_some() {
            return Err("duplicate shape ID".into());
        }
    }
    let mut weights = vec![0f32; ids.len()];
    let mut attachment_channels = 0;
    for handle in &data.handles {
        if !handle.default_time.is_finite() {
            return Err("non-finite default progress".into());
        }
        let anim = handle
            .animation
            .as_ref()
            .ok_or("missing handle animation")?;
        if !anim.length.is_finite() || anim.length <= 0. || anim.channels.is_empty() {
            return Err("invalid handle animation".into());
        }
        let progress = overrides_by_id
            .get(&handle.id)
            .copied()
            .unwrap_or(handle.default_time.clamp(0., 1.));
        let mut channels = HashSet::new();
        for (ci, channel) in anim.channels.iter().enumerate() {
            if !channels.insert(channel.id) {
                return Err("duplicate blend channel".into());
            }
            // Some parts have a second clip on the same handle for their
            // attachment skeleton. Geometry uses the separate blend clip.
            if channel.keys.is_empty() {
                return Err("empty handle channel".into());
            }
            if channel.keys.iter().all(|k| !k.blend) {
                if channel.keys.iter().any(|k| {
                    !k.time.is_finite()
                        || k.rotation
                            .iter()
                            .chain(&k.location)
                            .chain(&k.scale)
                            .any(|v| !v.is_finite())
                }) || channel.keys.windows(2).any(|k| k[1].time < k[0].time)
                {
                    return Err("invalid attachment channel".into());
                }
                attachment_channels += 1;
                continue;
            }
            let index = *shape_index
                .get(&channel.id)
                .ok_or_else(|| format!("unmapped blend channel {:08X}", channel.id))?;
            if channel.keys.is_empty()
                || channel
                    .keys
                    .iter()
                    .any(|k| !k.blend || !k.time.is_finite() || !k.factor.is_finite())
                || channel.keys.windows(2).any(|k| k[1].time < k[0].time)
            {
                return Err("invalid blend channel keys".into());
            }
            // XML/default values are normalized handle progress. Retail clips
            // are audited by --dump-creation-morphs; keep clip endpoints closed.
            let factor = anim
                .sample_clamped(ci, progress * anim.length)
                .ok_or("cannot sample morph channel")?
                .1[0];
            weights[index] += factor;
            if !weights[index].is_finite() {
                return Err("non-finite blend contribution".into());
            }
        }
    }
    let mut prepared = Vec::with_capacity(meshes.len());
    let mut moved = 0;
    for mesh in meshes.iter() {
        let count = mesh.positions.len();
        if count == 0
            || mesh.normals.len() != count
            || mesh.uvs.len() != count
            || mesh.indices.len() % 3 != 0
            || mesh.indices.iter().any(|&i| i as usize >= count)
            || mesh
                .positions
                .iter()
                .chain(&mesh.normals)
                .any(|p| p.iter().any(|v| !v.is_finite()))
            || mesh.uvs.iter().any(|p| p.iter().any(|v| !v.is_finite()))
        {
            return Err("invalid morph mesh buffers".into());
        }
        if mesh.morphs.is_empty() {
            prepared.push((mesh.positions.clone(), mesh.normals.clone()));
            continue;
        }
        if mesh.morphs.len() != ids.len()
            || mesh
                .morphs
                .iter()
                .any(|d| d.len() != count || d.iter().any(|p| p.iter().any(|v| !v.is_finite())))
        {
            return Err("morph delta count mismatch or non-finite deltas".into());
        }
        let mut positions = mesh.positions.clone();
        for (i, p) in positions.iter_mut().enumerate() {
            let mut v = Vec3::from_array(*p);
            for (delta, &w) in mesh.morphs.iter().zip(&weights) {
                v += Vec3::from_array(delta[i]) * w;
            }
            if !v.is_finite() {
                return Err("non-finite deformed vertex".into());
            }
            if v.to_array() != *p {
                moved += 1;
            }
            *p = v.to_array();
        }
        let mut normals = vec![Vec3::ZERO; count];
        for triangle in mesh.indices.chunks_exact(3) {
            let [a, b, c] = [
                triangle[0] as usize,
                triangle[1] as usize,
                triangle[2] as usize,
            ];
            let n = (Vec3::from_array(positions[b]) - Vec3::from_array(positions[a]))
                .cross(Vec3::from_array(positions[c]) - Vec3::from_array(positions[a]));
            if !n.is_finite() {
                return Err("non-finite deformed face normal".into());
            }
            normals[a] += n;
            normals[b] += n;
            normals[c] += n;
        }
        let normals = normals
            .into_iter()
            .enumerate()
            .map(|(i, n)| {
                if n.length_squared() > 1e-12 {
                    n.normalize().to_array()
                } else {
                    mesh.normals[i]
                }
            })
            .collect();
        prepared.push((positions, normals));
    }
    for (mesh, (positions, normals)) in meshes.iter_mut().zip(prepared) {
        mesh.positions = positions;
        mesh.normals = normals;
    }
    Ok(MorphApplication {
        handle_count: data.handles.len(),
        shape_weights: ids.iter().copied().zip(weights).collect(),
        moved_vertices: moved,
        attachment_channels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rw4::{AnimChannel, KeyAnim, Keyframe, MorphHandle, MorphShapes, PoolInfo};
    fn mesh() -> ExtractedMesh {
        ExtractedMesh {
            source_section: 0,
            bone_indices: vec![],
            bone_weights: vec![],
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            normals: vec![[0., 0., 1.]; 3],
            uvs: vec![[0., 0.], [1., 0.], [0., 1.]],
            indices: vec![0, 1, 2],
            morphs: vec![vec![[0., 0., 0.]; 3], vec![[0., 0., 1.]; 3]],
            skin_idx: Vec::new(),
            skin_w: Vec::new(),
        }
    }
    fn data() -> MorphData {
        let keys = [(0., 0.), (2., 1.)]
            .into_iter()
            .map(|(time, factor)| Keyframe {
                time,
                factor,
                blend: true,
                rotation: [0., 0., 0., 1.],
                location: [0.; 3],
                scale: [1.; 3],
            })
            .collect();
        MorphData {
            rig: None,
            handles: vec![MorphHandle {
                id: 7,
                start: [0.; 3],
                end: [0.; 3],
                default_time: 0.5,
                animation_reference: 123,
                animation: Some(KeyAnim {
                    skeleton_id: 0,
                    length: 2.,
                    channels: vec![AnimChannel { id: 100, keys }],
                }),
            }],
            shapes: vec![MorphShapes {
                id: 88,
                shape_ids: vec![200, 100],
                object_index: 45,
            }],
            pools: vec![PoolInfo {
                vertex_count: 3,
                shapes: 2,
                skinned: false,
                max_bone_idx: 0,
            }],
            errors: Vec::new(),
        }
    }
    #[test]
    fn ids_and_closed_endpoints_drive_shape_deltas() {
        let mut m = [mesh()];
        let r = apply_creation_handles(&mut m, &data(), &[(7, 1.)]).unwrap();
        assert_eq!(r.shape_weights, vec![(200, 0.), (100, 1.)]);
        assert_eq!(r.moved_vertices, 3);
        assert_eq!(m[0].positions[0], [0., 0., 1.]);
        assert_eq!(m[0].uvs, mesh().uvs);
        assert_eq!(m[0].indices, mesh().indices);
        let mut m = [mesh()];
        apply_creation_handles(&mut m, &data(), &[]).unwrap();
        assert_eq!(m[0].positions[0], [0., 0., 0.5]);
    }
    #[test]
    fn unknown_duplicate_and_ambiguous_metadata_are_atomic_errors() {
        for overrides in [vec![(99, 1.)], vec![(7, 0.), (7, 1.)], vec![(7, f32::NAN)]] {
            let mut m = [mesh()];
            assert!(apply_creation_handles(&mut m, &data(), &overrides).is_err());
            assert_eq!(m[0].positions, mesh().positions);
        }
        let mut d = data();
        d.pools.push(d.pools[0].clone());
        assert!(apply_creation_handles(&mut [mesh()], &d, &[]).is_err());
        let mut d = data();
        d.handles[0].animation.as_mut().unwrap().channels[0].id = 999;
        assert!(apply_creation_handles(&mut [mesh()], &d, &[]).is_err());
        let mut d = data();
        d.handles[0].animation = None;
        assert!(apply_creation_handles(&mut [mesh()], &d, &[]).is_err());
    }
    #[test]
    fn contributions_sum_and_normals_follow_geometry() {
        let mut d = data();
        let mut other = d.handles[0].clone();
        other.id = 8;
        d.handles.push(other);
        let mut m = mesh();
        m.morphs[1][0] = [0., 0., 0.];
        m.morphs[1][1] = [0., 0., 0.];
        m.morphs[1][2] = [0., 0., 1.];
        let result = apply_creation_handles(std::slice::from_mut(&mut m), &d, &[]).unwrap();
        assert_eq!(result.shape_weights[1], (100, 1.));
        let expected = Vec3::new(0., -1., 1.).normalize();
        assert!(Vec3::from_array(m.normals[0]).abs_diff_eq(expected, 1e-6));
    }
    #[test]
    fn malformed_later_mesh_does_not_change_earlier_mesh() {
        let mut meshes = [mesh(), mesh()];
        meshes[1].indices[2] = 9;
        assert!(apply_creation_handles(&mut meshes, &data(), &[]).is_err());
        assert_eq!(meshes[0].positions, mesh().positions);
    }
    #[test]
    fn blend_validation_and_zero_input() {
        let mut d = data();
        d.handles[0].animation.as_mut().unwrap().channels[0].keys[0].blend = false;
        assert!(apply_creation_handles(&mut [mesh()], &d, &[]).is_err());
        let d = MorphData {
            rig: None,
            handles: vec![],
            shapes: vec![],
            pools: vec![],
            errors: vec![],
        };
        assert_eq!(
            apply_creation_handles(&mut [mesh()], &d, &[])
                .unwrap()
                .moved_vertices,
            0
        );
    }

    #[test]
    fn shared_handle_id_drives_blend_and_attachment_clips() {
        let mut d = data();
        let mut attachment = d.handles[0].clone();
        attachment.animation_reference += 1;
        let channel = &mut attachment.animation.as_mut().unwrap().channels[0];
        channel.id = 999;
        for key in &mut channel.keys {
            key.blend = false;
        }
        d.handles.push(attachment);
        let mut actual = [mesh()];
        let result = apply_creation_handles(&mut actual, &d, &[(d.handles[0].id, 1.)]).unwrap();
        assert_eq!(result.attachment_channels, 1);
        assert_eq!(actual[0].positions[0], [0., 0., 1.]);
        d.handles[1].animation.as_mut().unwrap().channels[0]
            .keys
            .clear();
        assert!(apply_creation_handles(&mut [mesh()], &d, &[]).is_err());
    }
}

/// Skeletal handle clips use four real bone influences from the classic
/// vertex declaration. Compute model-space poses with parent scale
/// compensation, then deform relative to the authored default handle pose.
fn apply_rig_handles(
    meshes: &mut [ExtractedMesh],
    data: &MorphData,
    overrides: &HashMap<u32, f32>,
) -> Result<MorphApplication, String> {
    let rig = data.rig.as_ref().ok_or("missing morph rig")?;
    let n = rig.bones.len();
    fn worlds(
        bones: &[crate::rw4::Bone],
        frames: &[crate::rw4::Keyframe],
    ) -> Result<Vec<Mat4>, String> {
        let mut rotations: Vec<Mat3> = Vec::new();
        let mut locations: Vec<Vec3> = Vec::new();
        let mut matrices = Vec::new();
        for (i, (b, p)) in bones.iter().zip(frames).enumerate() {
            let local = Mat3::from_quat(Quat::from_array(p.rotation).normalize())
                * Mat3::from_diagonal(Vec3::from_array(p.scale));
            let (m, t) = if let Some(parent) = b.parent {
                if parent >= i || frames[parent].scale.iter().any(|s| s.abs() < 1e-6) {
                    return Err("invalid skeletal hierarchy or parent scale".into());
                }
                (
                    rotations[parent]
                        * Mat3::from_diagonal(Vec3::ONE / Vec3::from_array(frames[parent].scale))
                        * local,
                    rotations[parent] * Vec3::from_array(p.location) + locations[parent],
                )
            } else {
                (local, Vec3::from_array(p.location))
            };
            rotations.push(m);
            locations.push(t);
            let matrix = Mat4::from_cols(
                m.x_axis.extend(0.),
                m.y_axis.extend(0.),
                m.z_axis.extend(0.),
                t.extend(1.),
            );
            if !matrix.is_finite() || matrix.determinant().abs() < 1e-9 {
                return Err("singular skeletal world pose".into());
            }
            matrices.push(matrix);
        }
        Ok(matrices)
    }
    let mut deltas = vec![Mat4::IDENTITY; n];
    for handle in &data.handles {
        let anim = handle
            .animation
            .as_ref()
            .ok_or("missing skeletal handle clip")?;
        let progress = overrides
            .get(&handle.id)
            .copied()
            .unwrap_or(handle.default_time.clamp(0., 1.));
        let mut neutral = vec![None; n];
        let mut posed = vec![None; n];
        for (ci, c) in anim.channels.iter().enumerate() {
            if c.keys.is_empty()
                || c.keys.iter().any(|k| {
                    k.blend
                        || !k.time.is_finite()
                        || k.rotation
                            .iter()
                            .chain(&k.location)
                            .chain(&k.scale)
                            .any(|x| !x.is_finite())
                })
                || c.keys.windows(2).any(|k| k[0].time > k[1].time)
            {
                return Err("invalid skeletal handle channel".into());
            }
            let bi = rig
                .bones
                .iter()
                .position(|b| b.name == c.id)
                .ok_or("unmapped skeletal handle channel")?;
            if neutral[bi].is_some() {
                return Err("duplicate skeletal channel".into());
            }
            neutral[bi] =
                anim.sample_frame_clamped(ci, handle.default_time.clamp(0., 1.) * anim.length);
            posed[bi] = anim.sample_frame_clamped(ci, progress * anim.length);
        }
        let neutral = neutral
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or("incomplete default skeletal pose")?;
        let posed = posed
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or("incomplete skeletal pose")?;
        let base = worlds(&rig.bones, &neutral)?;
        let current = worlds(&rig.bones, &posed)?;
        for i in 0..n {
            deltas[i] = current[i] * base[i].inverse() * deltas[i];
        }
    }
    let mut prepared = Vec::new();
    let mut moved = 0;
    for mesh in meshes.iter() {
        if mesh.positions.is_empty()
            || mesh
                .positions
                .iter()
                .chain(&mesh.normals)
                .any(|p| p.iter().any(|v| !v.is_finite()))
            || mesh.uvs.len() != mesh.positions.len()
            || mesh.uvs.iter().any(|p| p.iter().any(|v| !v.is_finite()))
            || mesh.positions.len() != mesh.bone_indices.len()
            || mesh.positions.len() != mesh.bone_weights.len()
            || mesh.positions.len() != mesh.normals.len()
            || mesh.indices.len() % 3 != 0
            || mesh
                .indices
                .iter()
                .any(|&i| i as usize >= mesh.positions.len())
        {
            return Err("missing or invalid four-bone influence buffers".into());
        }
        let mut positions = Vec::with_capacity(mesh.positions.len());
        for (i, p) in mesh.positions.iter().enumerate() {
            let mut v = Vec3::ZERO;
            let mut sum = 0.;
            for k in 0..4 {
                let w = mesh.bone_weights[i][k];
                if !w.is_finite() || w < 0. {
                    return Err("invalid bone influence weight".into());
                }
                if w == 0. {
                    continue;
                }
                // D3D stores a matrix's first constant-register index: each
                // bone occupies three rows. It is not a direct bone index.
                let register = mesh.bone_indices[i][k] as usize;
                if register % 3 != 0 {
                    return Err("bone matrix register is not a multiple of three".into());
                }
                let m = deltas
                    .get(register / 3)
                    .ok_or("bone influence out of range")?;
                v += m.transform_point3(Vec3::from_array(*p)) * w;
                sum += w;
            }
            if (sum - 1.).abs() > 0.02 {
                return Err("bone weights do not sum to one".into());
            }
            v /= sum;
            if !v.is_finite() {
                return Err("non-finite skeletal vertex".into());
            }
            if (v - Vec3::from_array(*p)).length_squared() > 1e-12 {
                moved += 1;
            }
            positions.push(v.to_array());
        }
        let normals = rebuild_normals(&positions, &mesh.indices, &mesh.normals)?;
        prepared.push((positions, normals));
    }
    for (mesh, (positions, normals)) in meshes.iter_mut().zip(prepared) {
        mesh.positions = positions;
        mesh.normals = normals;
    }
    Ok(MorphApplication {
        handle_count: data.handles.len(),
        shape_weights: Vec::new(),
        moved_vertices: moved,
        attachment_channels: 0,
    })
}
fn rebuild_normals(
    positions: &[[f32; 3]],
    indices: &[u16],
    fallback: &[[f32; 3]],
) -> Result<Vec<[f32; 3]>, String> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [
            triangle[0] as usize,
            triangle[1] as usize,
            triangle[2] as usize,
        ];
        let n = (Vec3::from_array(positions[b]) - Vec3::from_array(positions[a]))
            .cross(Vec3::from_array(positions[c]) - Vec3::from_array(positions[a]));
        if !n.is_finite() {
            return Err("invalid skeletal normal".into());
        }
        normals[a] += n;
        normals[b] += n;
        normals[c] += n;
    }
    Ok(normals
        .into_iter()
        .enumerate()
        .map(|(i, n)| {
            if n.length_squared() > 1e-12 {
                n.normalize().to_array()
            } else {
                fallback[i]
            }
        })
        .collect())
}

#[cfg(test)]
mod skeletal_tests {
    use super::*;
    use crate::rw4::*;
    fn fixture() -> (ExtractedMesh, MorphData) {
        let m = ExtractedMesh {
            source_section: 0,
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            normals: vec![[0., 0., 1.]; 3],
            uvs: vec![[0.; 2]; 3],
            indices: vec![0, 1, 2],
            morphs: vec![],
            skin_idx: vec![],
            skin_w: vec![],
            bone_indices: vec![[0; 4]; 3],
            bone_weights: vec![[1., 0., 0., 0.]; 3],
        };
        let keys = [(0., 0., 1.), (1., 3., 2.)]
            .into_iter()
            .map(|(time, x, s)| Keyframe {
                time,
                rotation: [0., 0., 0., 1.],
                location: [x, 0., 0.],
                scale: [s, 1., 1.],
                factor: 0.,
                blend: false,
            })
            .collect();
        let d = MorphData {
            handles: vec![MorphHandle {
                id: 7,
                start: [0.; 3],
                end: [0.; 3],
                default_time: 0.,
                animation_reference: 0,
                animation: Some(KeyAnim {
                    skeleton_id: 0,
                    length: 1.,
                    channels: vec![AnimChannel { id: 99, keys }],
                }),
            }],
            shapes: vec![],
            pools: vec![],
            errors: vec![],
            rig: Some(MorphRig {
                bones: vec![Bone {
                    name: 99,
                    flags: 1,
                    parent: None,
                }],
                poses: vec![BonePose {
                    rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                    inv_translation: [0.; 3],
                }],
            }),
        };
        (m, d)
    }
    #[test]
    fn skeletal_handles_preserve_default_and_apply_translation_and_scale() {
        let (m, d) = fixture();
        let mut neutral = [m.clone()];
        assert_eq!(
            apply_creation_handles(&mut neutral, &d, &[])
                .unwrap()
                .moved_vertices,
            0
        );
        assert_eq!(neutral[0].positions, m.positions);
        let mut posed = [m];
        let result = apply_creation_handles(&mut posed, &d, &[(7, 1.)]).unwrap();
        assert_eq!(result.moved_vertices, 3);
        assert_eq!(posed[0].positions[1], [5., 0., 0.]);
        let frame = d.handles[0]
            .animation
            .as_ref()
            .unwrap()
            .sample_frame_clamped(0, 0.5)
            .unwrap();
        assert_eq!(frame.scale, [1.5, 1., 1.]);
        assert_eq!(frame.location, [1.5, 0., 0.]);
    }
    #[test]
    fn invalid_bone_weights_do_not_commit_positions() {
        let (mut m, d) = fixture();
        let before = m.positions.clone();
        m.bone_weights[1] = [0.5, 0., 0., 0.];
        assert!(apply_creation_handles(std::slice::from_mut(&mut m), &d, &[(7, 1.)]).is_err());
        assert_eq!(m.positions, before);
    }

    #[test]
    fn register_indices_and_parent_scale_compensation_follow_child_bone() {
        let (mut m, mut d) = fixture();
        let rig = d.rig.as_mut().unwrap();
        rig.bones.push(Bone {
            name: 100,
            flags: 0,
            parent: Some(0),
        });
        rig.poses.push(rig.poses[0].clone());
        let clip = d.handles[0].animation.as_mut().unwrap();
        let mut child = clip.channels[0].clone();
        child.id = 100;
        for key in &mut child.keys {
            key.location = [1., 0., 0.];
            key.scale = [1.; 3];
        }
        clip.channels.push(child);
        m.bone_indices = vec![[3, 0, 0, 0]; 3];
        let mut posed = [m.clone()];
        apply_creation_handles(&mut posed, &d, &[(7, 1.)]).unwrap();
        assert_eq!(posed[0].positions[0], [4., 0., 0.]);
        assert_eq!(
            posed[0].positions[1],
            [5., 0., 0.],
            "child does not inherit root stretch"
        );
        m.bone_indices[0][0] = 1;
        let before = m.positions.clone();
        assert!(apply_creation_handles(std::slice::from_mut(&mut m), &d, &[(7, 1.)]).is_err());
        assert_eq!(m.positions, before);
    }
}
