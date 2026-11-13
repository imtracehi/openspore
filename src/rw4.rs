//! RW4 model import (clean-room parser).
//!
//! File layout (verified against retail records + the documented object
//! model used by the modding community):
//! - 28-byte magic, u32LE type @0x1C (1 = MODEL), section count @0x24,
//!   section table @0x30 (`pSectionInfo`), raw-data base @0x44.
//! - Section infos: 24 bytes each (pData, field_04, size, alignment,
//!   typeCodeIndex, typeCode). Raw (`0x10030`) sections are relocated:
//!   file offset = pData + pBufferData. Others are absolute.
//! - Objects, in section order: mesh `0x20009`, vertex buffer `0x20005`,
//!   index buffer `0x20007`, vertex description `0x20004`, raw `0x10030`.
//! - Refs are section indices with the top bits as tag: `ref >> 22 == 0`
//!   means a direct object; anything else (sub-reference / null) is
//!   treated as unresolvable and the mesh is skipped.
//! - Vertex elements: 12 bytes (stream:u16, offset:u16, type:u8,
//!   method:u8, usage:u8, usageIndex:u8, code:u32). Meaning comes from
//!   `code`: 0 = position, 2 = normal, 6 = texcoord0. D3D type gives the
//!   size: FLOAT3 = 12B, FLOAT2 = 8B.
//!
//! Scope: static triangle-list meshes with u16 indices. Skinned meshes
//! (null vertex-buffer refs, skin buffer `0x200af`) are counted and skipped.

use crate::dbpf;

pub const RW_MAGIC: &[u8] = &[
    0x89, 0x52, 0x57, 0x34, 0x77, 0x33, 0x32, 0x00, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x20, 0x04, 0x00,
    0x34, 0x35, 0x34, 0x00, 0x30, 0x30, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00,
];

pub const RW_TYPE_MODEL: u32 = 1;
/// Rig-bearing records referenced by gmdl model assemblies: same header
/// layout as MODEL, but tagged 0xCAFED00D (skeleton/anim only, no meshes).
pub const RW_TYPE_RIG: u32 = 0xCAFE_D00D;

pub const T_RAW: u32 = 0x10030;
pub const T_MESH: u32 = 0x20009;
pub const T_VBUF: u32 = 0x20005;
pub const T_IBUF: u32 = 0x20007;
pub const T_VDESC: u32 = 0x20004;
pub const T_SKELETON: u32 = 0x70002;
pub const T_ANIMSKIN: u32 = 0x70003;
pub const T_SKIN_MATRIX_BUFFER: u32 = 0x7000f;
// Legacy preview API name. The native section is a SkinMatrixBuffer, not a
// named attachment/mount table (verified by FX and the native SkinsInK path).
pub const T_BONE_MOUNTS: u32 = T_SKIN_MATRIX_BUFFER;
pub const T_SKINS_IN_K: u32 = 0x7000c;
pub const T_BLENDSHAPES: u32 = 0x200af;
pub const T_KEYANIM: u32 = 0x70001;
pub const T_MORPH_HANDLE: u32 = 0xff0000;
pub const T_MORPH_SHAPES: u32 = 0xff0002;
pub const T_COMPILED_STATE: u32 = 0x2000b;
pub const T_MESH_MATERIAL_LINK: u32 = 0x2001a;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialLink {
    pub mesh_section: usize,
    pub state_sections: Vec<usize>,
}

/// Resolve each authored mesh-to-material-state link by direct section index.
/// Keeping per-mesh links is essential when an RW4 model has several materials.
pub fn material_links(blob: &[u8]) -> Result<Vec<MaterialLink>, String> {
    let parts = sections(blob)?;
    let mut out = vec![];
    for (i, (kind, bytes)) in parts.iter().enumerate() {
        if *kind != T_MESH_MATERIAL_LINK {
            continue;
        }
        let reference = u32le(bytes, 0).ok_or("truncated mesh material link")?;
        if reference >> 22 != 0 {
            return Err(format!(
                "link {i} has unsupported mesh reference {reference:08X}"
            ));
        }
        let mesh_section = reference as usize;
        if parts.get(mesh_section).map(|p| p.0) != Some(T_MESH) {
            return Err(format!("link {i} does not reference a mesh"));
        }
        let count = u32le(bytes, 4).ok_or("truncated material link count")? as usize;
        if count > 1024 {
            return Err("implausible per-mesh material count".into());
        }
        let end = 8usize
            .checked_add(count.checked_mul(4).ok_or("material link overflow")?)
            .ok_or("material link overflow")?;
        if end > bytes.len() {
            return Err("truncated material state reference list".into());
        }
        let mut state_sections = Vec::with_capacity(count);
        for j in 0..count {
            let reference = u32le(bytes, 8 + j * 4).unwrap();
            if reference >> 22 != 0 {
                return Err(format!(
                    "link {i} has unsupported material reference {reference:08X}"
                ));
            }
            let section = reference as usize;
            if parts.get(section).map(|p| p.0) != Some(T_COMPILED_STATE) {
                return Err(format!("link {i} references a non-material section"));
            }
            state_sections.push(section);
        }
        out.push(MaterialLink {
            mesh_section,
            state_sections,
        });
    }
    Ok(out)
}

/// Borrowed, validated section payloads. Includes texture/material-only RW4
/// containers; object indices stay stable so direct references can be resolved.
pub fn sections(blob: &[u8]) -> Result<Vec<(u32, &[u8])>, String> {
    if blob.len() < 96 || blob.get(..28) != Some(RW_MAGIC) {
        return Err("bad RW4 header".into());
    }
    let count = u32le(blob, 0x24).ok_or("short section count")? as usize;
    let table = u32le(blob, 0x30).ok_or("short section table")? as usize;
    let buffers = u32le(blob, 0x44).ok_or("short buffer base")? as usize;
    if count > 10_000 {
        return Err("implausible RW4 section count".into());
    }
    let end = table
        .checked_add(count.checked_mul(24).ok_or("section table overflow")?)
        .ok_or("section table overflow")?;
    if end > blob.len() {
        return Err("truncated RW4 section table".into());
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let row = table + i * 24;
        let mut offset = u32le(blob, row).unwrap() as usize;
        let size = u32le(blob, row + 8).unwrap() as usize;
        let kind = u32le(blob, row + 20).unwrap();
        if kind == T_RAW {
            offset = offset
                .checked_add(buffers)
                .ok_or("raw relocation overflow")?;
        }
        let end = offset.checked_add(size).ok_or("section size overflow")?;
        let payload = blob
            .get(offset..end)
            .ok_or_else(|| format!("section {i} ({kind:08X}) exceeds RW4 record"))?;
        out.push((kind, payload));
    }
    Ok(out)
}

pub const COMP_LOCROTSCALE: u32 = 0x601;
pub const COMP_LOCROT: u32 = 0x101;
pub const COMP_BLEND: u32 = 0x100;

const IDX16_FORMAT: u32 = 101; // D3DFMT_INDEX16
const PRIM_TRIANGLE_LIST: u32 = 4;

// D3DDECLTYPE -> byte size
fn decl_size(t: u8) -> Option<usize> {
    match t {
        0 => Some(4),     // FLOAT1
        1 => Some(8),     // FLOAT2
        2 => Some(12),    // FLOAT3
        3 => Some(16),    // FLOAT4
        4 | 5 => Some(4), // D3DCOLOR / UBYTE4
        6 => Some(4),     // SHORT2
        7 => Some(8),     // SHORT4
        8 | 9 => Some(4),
        10 => Some(8),
        11 | 12 | 13 | 14 => Some(4),
        15 => Some(4), // FLOAT16_2
        16 => Some(8), // FLOAT16_4
        _ => None,
    }
}

/// Spore's RWDECL_NORMAL UBYTE4/UBYTE4N channels pack signed components
/// into 0..255. The fourth byte is padding, not another normal component.
fn unpack_byte_normal(bytes: [u8; 3]) -> [f32; 3] {
    bytes.map(|v| v as f32 / 127.5 - 1.)
}

#[cfg(test)]
#[test]
fn native_packed_normals_preserve_signed_directions() {
    let n = unpack_byte_normal([0, 128, 255]);
    assert_eq!(n[0], -1.);
    assert_eq!(n[2], 1.);
    assert!((n[1] - 1. / 255.).abs() < 1e-6);
}

#[derive(Debug, Clone)]
struct Section {
    p_data: usize,
    size: usize,
    type_code: u32,
}

#[derive(Debug, Clone)]
enum Object {
    Raw {
        bytes: Vec<u8>,
    },
    Mesh {
        f: [u32; 9],
        refs: Vec<u32>,
    },
    VBuf {
        f: [u32; 7],
    },
    IBuf {
        f: [u32; 7],
    },
    VDesc {
        vertex_size: usize,
        elements: Vec<(u16, u8, u32)>,
    }, // (offset, decl_type, code)
    Skeleton {
        bones: Vec<Bone>,
    },
    AnimSkin {
        poses: Vec<BonePose>,
    },
    KeyAnim(KeyAnim),
    MorphHandle(MorphHandle),
    MorphShapes(MorphShapes),
    /// Morph-target vertex pool (base arrays only). Streams: 0 = positions
    /// (base float4 array + `shape_count` delta arrays), 1 = normals,
    /// 2 = tangents, 3 = texcoords (float4), 9 = blend-shape index pairs
    /// (u16 pairs — NOT bone indices), 10 = blend weights (float pairs,
    /// sum to 1). RW4 parts are rigid morph models; no vertex skinning.
    BlendShapes {
        vertex_count: usize,
        shape_count: usize,
        positions: Vec<[f32; 3]>,
        position_deltas: Vec<Vec<[f32; 3]>>,
        normals: Vec<[f32; 3]>,
        uvs: Vec<[f32; 2]>,
        skin_indices: Vec<[u16; 2]>,
        skin_weights: Vec<[f32; 2]>,
    },
    Opaque {
        type_code: u32,
    },
    BoneMounts(BoneMounts),
}

/// Per-bone 4x3 mount matrices (RW4 section `0x7000f`): 16-byte header
/// ([name u32][count u32][unk u32][unk u32]) then `count` row-major 3x4
/// matrices — where a part's mesh is anchored relative to its bone.
#[derive(Debug, Clone)]
pub struct BoneMounts {
    /// Legacy field name: this is the absolute matrix-array file offset.
    pub name: u32,
    pub field_8: u32,
    pub field_c: u32,
    pub count: u32,
    pub mats: Vec<Mat34>,
}

#[derive(Debug, Clone)]
pub struct Bone {
    pub name: u32,
    pub flags: u32,
    /// Index into the skeleton's bone list, or None for roots.
    pub parent: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct BonePose {
    /// Absolute bind-pose rotation, row-major 3x3.
    pub rotation: [[f32; 3]; 3],
    /// Inverse bind translation (negated for the debug bind position).
    pub inv_translation: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct Keyframe {
    pub time: f32,
    pub rotation: [f32; 4], // xyzw quat (identity for blend channels)
    pub location: [f32; 3],
    pub scale: [f32; 3], // 1s for LocRot
    pub factor: f32,     // blend channels only
    pub blend: bool,
}

#[derive(Debug, Clone)]
pub struct AnimChannel {
    pub id: u32,
    pub keys: Vec<Keyframe>,
}

#[derive(Debug, Clone)]
pub struct KeyAnim {
    pub skeleton_id: u32,
    pub length: f32,
    pub channels: Vec<AnimChannel>,
}

impl KeyAnim {
    /// Preserve scale as well as rotation/location for skeletal morph clips.
    pub fn sample_frame_clamped(&self, channel: usize, t: f32) -> Option<Keyframe> {
        if !t.is_finite() || !self.length.is_finite() || self.length <= 0. {
            return None;
        }
        let keys = &self.channels.get(channel)?.keys;
        let t = t.clamp(0., self.length);
        let first = keys.first()?;
        if t <= first.time {
            return Some(first.clone());
        }
        for pair in keys.windows(2) {
            if t <= pair[1].time {
                let a =
                    ((t - pair[0].time) / (pair[1].time - pair[0].time).max(1e-9)).clamp(0., 1.);
                return Some(Keyframe {
                    time: t,
                    rotation: slerp(pair[0].rotation, pair[1].rotation, a),
                    location: lerp3(pair[0].location, pair[1].location, a),
                    scale: lerp3(pair[0].scale, pair[1].scale, a),
                    factor: pair[0].factor + (pair[1].factor - pair[0].factor) * a,
                    blend: pair[0].blend,
                });
            }
        }
        keys.last().cloned()
    }
    /// Handle endpoints clamp; skeletal animation playback still loops.
    pub fn sample_clamped(&self, channel: usize, t: f32) -> Option<([f32; 4], [f32; 3])> {
        if !t.is_finite() || !self.length.is_finite() || self.length <= 0. {
            return None;
        }
        self.sample_at(channel, t.clamp(0., self.length))
    }
    /// Sample a channel at time t (loops by length): (quat, loc).
    /// Blend channels report factor in location[0].
    pub fn sample(&self, channel: usize, mut t: f32) -> Option<([f32; 4], [f32; 3])> {
        let ch = self.channels.get(channel)?;
        if ch.keys.is_empty() || self.length <= 0.0 {
            return None;
        }
        t = t.rem_euclid(self.length);
        self.sample_at(channel, t)
    }
    fn sample_at(&self, channel: usize, t: f32) -> Option<([f32; 4], [f32; 3])> {
        let ch = self.channels.get(channel)?;
        if ch.keys.is_empty() {
            return None;
        }
        let mut prev = &ch.keys[0];
        if t <= prev.time {
            return Some((prev.rotation, loc_of(prev)));
        }
        for k in &ch.keys[1..] {
            if t <= k.time {
                let span = (k.time - prev.time).max(1e-9);
                let a = ((t - prev.time) / span).clamp(0.0, 1.0);
                return Some((
                    slerp(prev.rotation, k.rotation, a),
                    lerp3(loc_of(prev), loc_of(k), a),
                ));
            }
            prev = k;
        }
        let last = &ch.keys[ch.keys.len() - 1];
        Some((last.rotation, loc_of(last)))
    }
}

fn loc_of(k: &Keyframe) -> [f32; 3] {
    if k.blend {
        [k.factor, 0.0, 0.0]
    } else {
        k.location
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Normalized lerp for near-identical quats, slerp otherwise. Pure + safe.
pub fn slerp(a: [f32; 4], mut b: [f32; 4], t: f32) -> [f32; 4] {
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if dot < 0.0 {
        dot = -dot;
        b = [-b[0], -b[1], -b[2], -b[3]];
    }
    if dot > 0.9995 {
        let mut o = [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            a[3] + (b[3] - a[3]) * t,
        ];
        let n = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2] + o[3] * o[3])
            .sqrt()
            .max(1e-9);
        o[0] /= n;
        o[1] /= n;
        o[2] /= n;
        o[3] /= n;
        return o;
    }
    let th = dot.clamp(-1.0, 1.0).acos();
    let s = th.sin().max(1e-9);
    let (fa, fb) = (((1.0 - t) * th).sin() / s, (t * th).sin() / s);
    [
        a[0] * fa + b[0] * fb,
        a[1] * fa + b[1] * fb,
        a[2] * fa + b[2] * fb,
        a[3] * fa + b[3] * fb,
    ]
}

impl BonePose {
    /// Approximate bind-space offset of this bone relative to its parent.
    /// Convention: local matrix [R | -invT], composed hierarchically.
    /// Used for the debug skeleton view (not the animation system).
    pub fn local_offset(&self) -> [f32; 3] {
        [
            -self.inv_translation[0],
            -self.inv_translation[1],
            -self.inv_translation[2],
        ]
    }
}

/// Rigid 3x4 row-major transform helpers (rotation + translation only).
pub type Mat34 = [[f32; 4]; 3];

pub fn mat34_identity() -> Mat34 {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]
}

pub fn mat34_mul(a: &Mat34, b: &Mat34) -> Mat34 {
    let mut o = [[0.0f32; 4]; 3];
    for r in 0..3 {
        for c in 0..4 {
            o[r][c] = a[r][0] * b[0][c]
                + a[r][1] * b[1][c]
                + a[r][2] * b[2][c]
                + if c == 3 { a[r][3] } else { 0.0 };
        }
    }
    o
}

/// Rigid inverse: transpose rotation, translation = -(R^T * t).
pub fn mat34_inverse_rigid(m: &Mat34) -> Mat34 {
    let mut o = [[0.0f32; 4]; 3];
    for r in 0..3 {
        for c in 0..3 {
            o[r][c] = m[c][r];
        }
    }
    for r in 0..3 {
        o[r][3] = -(o[r][0] * m[0][3] + o[r][1] * m[1][3] + o[r][2] * m[2][3]);
    }
    o
}

pub fn mat34_apply(m: &Mat34, p: &[f32; 3]) -> [f32; 3] {
    [
        m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + m[0][3],
        m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + m[1][3],
        m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3],
    ]
}

/// Inverse-bind matrix straight from a stored pose: [R_abs | invT].
pub fn inv_bind_from_pose(pose: &BonePose) -> Mat34 {
    [
        [
            pose.rotation[0][0],
            pose.rotation[0][1],
            pose.rotation[0][2],
            pose.inv_translation[0],
        ],
        [
            pose.rotation[1][0],
            pose.rotation[1][1],
            pose.rotation[1][2],
            pose.inv_translation[1],
        ],
        [
            pose.rotation[2][0],
            pose.rotation[2][1],
            pose.rotation[2][2],
            pose.inv_translation[2],
        ],
    ]
}

/// Linear-blend skinning: v' = sum_k w_k * world_k * invBind_k * v.
/// Influences targeting unknown bones hold bind pose (identity fallback).
/// Returns None only for structural problems (length mismatch, non-finite).
pub fn skin_positions(
    base: &[[f32; 3]],
    skin_idx: &[[u16; 2]],
    skin_w: &[[f32; 2]],
    world: &[Mat34],
    inv_bind: &[Mat34],
) -> Option<Vec<[f32; 3]>> {
    if base.len() != skin_idx.len() || base.len() != skin_w.len() {
        return None;
    }
    const IDENT: Mat34 = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let mut out = Vec::with_capacity(base.len());
    for (i, v) in base.iter().enumerate() {
        let mut p = [0.0f32; 3];
        for k in 0..2 {
            let b = skin_idx[i][k] as usize;
            let w = skin_w[i][k];
            if !w.is_finite() {
                return None;
            }
            if w == 0.0 {
                continue;
            }
            let (wm, im) = match (world.get(b), inv_bind.get(b)) {
                (Some(wm), Some(im)) => (wm, im),
                _ => (&IDENT, &IDENT),
            };
            let q = mat34_apply(wm, &mat34_apply(im, v));
            p[0] += q[0] * w;
            p[1] += q[1] * w;
            p[2] += q[2] * w;
        }
        if !p.iter().all(|x| x.is_finite()) {
            return None;
        }
        out.push(p);
    }
    Some(out)
}

/// Forward kinematics over parent-relative local matrices.
/// Requires parents to precede children (same rule as bind positions).
pub fn fk_worlds(parents: &[Option<usize>], locals: &[Mat34]) -> Option<Vec<Mat34>> {
    if parents.len() != locals.len() {
        return None;
    }
    let mut worlds = Vec::with_capacity(parents.len());
    for (i, parent) in parents.iter().enumerate() {
        match parent {
            None => worlds.push(locals[i]),
            Some(par) if *par < i => worlds.push(mat34_mul(&worlds[*par], &locals[i])),
            _ => return None,
        }
    }
    Some(worlds)
}

/// Derive bind-local matrices + inverse-bind matrices from bones + poses.
/// local_bind[i] = inverse(world_bind[parent]) * world_bind[i],
/// world_bind[i] = inverse(inv_bind[i]).
pub fn bind_locals(bones: &[Bone], poses: &[BonePose]) -> Option<(Vec<Mat34>, Vec<Mat34>)> {
    if bones.len() != poses.len() || bones.is_empty() {
        return None;
    }
    let invs: Vec<Mat34> = poses.iter().map(inv_bind_from_pose).collect();
    let worlds: Vec<Mat34> = invs.iter().map(mat34_inverse_rigid).collect();
    let mut locals = Vec::with_capacity(bones.len());
    for (i, b) in bones.iter().enumerate() {
        match b.parent {
            None => locals.push(worlds[i]),
            Some(par) if par < i => {
                locals.push(mat34_mul(&mat34_inverse_rigid(&worlds[par]), &worlds[i]))
            }
            _ => return None,
        }
    }
    Some((locals, invs))
}

/// Rotation about Y (for procedural posing).
pub fn rot_y(angle: f32) -> Mat34 {
    let (s, c) = angle.sin_cos();
    [[c, 0.0, s, 0.0], [0.0, 1.0, 0.0, 0.0], [-s, 0.0, c, 0.0]]
}

/// Quaternion (xyzw) to row-major 3x3 rotation matrix.
pub fn quat_to_mat33(q: [f32; 4]) -> [[f32; 3]; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let n = (x * x + y * y + z * z + w * w).sqrt().max(1e-9);
    let (x, y, z, w) = (x / n, y / n, z / n, w / n);
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

fn mat_mul(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut o = [[0.0f32; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            o[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
    }
    o
}

fn mat_vec(m: &[[f32; 3]; 3], v: &[f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// Hierarchical bind-space joint positions for a bone/pose pair.
/// Requires `bones.len() == poses.len()`; returns empty vec otherwise.
pub fn bind_world_positions(bones: &[Bone], poses: &[BonePose]) -> Vec<[f32; 3]> {
    if bones.len() != poses.len() || bones.is_empty() {
        return Vec::new();
    }
    let mut rots: Vec<[[f32; 3]; 3]> = Vec::with_capacity(bones.len());
    let mut poss: Vec<[f32; 3]> = Vec::with_capacity(bones.len());
    for (i, (b, p)) in bones.iter().zip(poses.iter()).enumerate() {
        match b.parent {
            None => {
                rots.push(p.rotation);
                poss.push(p.local_offset());
            }
            Some(par) if par < i => {
                rots.push(mat_mul(&rots[par], &p.rotation));
                let o = mat_vec(&rots[par], &p.local_offset());
                let pp = poss[par];
                poss.push([pp[0] + o[0], pp[1] + o[1], pp[2] + o[2]]);
            }
            // Forward/sibling refs break the single-pass assumption; skip pair.
            _ => return Vec::new(),
        }
    }
    poss
}

#[derive(Debug, Clone, Default)]
pub struct MeshStats {
    pub meshes: usize,
    pub static_ok: usize,
    pub blend_ok: usize,
    pub morph_shapes: usize,
    pub verts: usize,
    pub tris: usize,
    pub skipped_unresolved: usize,
    pub skipped_unsupported: usize,
    pub skeletons: usize,
    pub bones: usize,
    pub bones_valid_parents: usize,
    pub skins: usize,
    pub poses: usize,
    pub keyanims: usize,
    pub channels: usize,
    pub keys: usize,
}

#[derive(Debug, Clone)]
pub struct ExtractedMesh {
    pub source_section: usize,
    /// Classic D3D UBYTE4 bone indices and UBYTE4N weights. These are separate
    /// from the blend-pool's two shape-index streams.
    pub bone_indices: Vec<[u8; 4]>,
    pub bone_weights: Vec<[f32; 4]>,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u16>,
    /// Per-shape position deltas aligned to `positions` (may be empty).
    pub morphs: Vec<Vec<[f32; 3]>>,
    /// Skin influences aligned to `positions`: (bone indices, weights).
    /// Empty when the mesh is rigid.
    pub skin_idx: Vec<[u16; 2]>,
    pub skin_w: Vec<[f32; 2]>,
}

impl ExtractedMesh {
    /// Apply morph weights on top of the base pose (pure function).
    pub fn morphed_positions(&self, weights: &[f32]) -> Vec<[f32; 3]> {
        let mut out = self.positions.clone();
        for (shape, w) in self.morphs.iter().zip(weights.iter()) {
            if *w == 0.0 {
                continue;
            }
            for (p, d) in out.iter_mut().zip(shape.iter()) {
                p[0] += d[0] * w;
                p[1] += d[1] * w;
                p[2] += d[2] * w;
            }
        }
        out
    }

    /// Axis-aligned bounding box (min, max) of the base positions.
    pub fn bbox(&self) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &self.positions {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        if self.positions.is_empty() {
            return ([0.0; 3], [0.0; 3]);
        }
        (min, max)
    }

    pub fn bbox_diagonal(&self) -> f32 {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &self.positions {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        let mut d = 0.0;
        for i in 0..3 {
            d += (max[i] - min[i]).powi(2);
        }
        d.sqrt()
    }

    pub fn valid(&self) -> bool {
        !self.positions.is_empty()
            && self
                .positions
                .iter()
                .all(|p| p.iter().all(|v| v.is_finite()))
            && self
                .indices
                .iter()
                .all(|&i| (i as usize) < self.positions.len())
            && self.bbox_diagonal() > 0.0
            && self.bbox_diagonal() < 1e9
    }
}

fn u32le(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn f32le(b: &[u8], off: usize) -> Option<f32> {
    u32le(b, off).map(f32::from_bits)
}

fn f64le(b: &[u8], off: usize) -> Option<f64> {
    let arr = b.get(off..off + 8)?.try_into().ok()?;
    Some(f64::from_le_bytes(arr))
}

/// Parse section table + objects. Returns (objects, stats-notes).
fn parse_objects(blob: &[u8]) -> Result<Vec<Object>, String> {
    if blob.len() < 96 || &blob[0..28] != RW_MAGIC {
        return Err("bad RW4 magic".to_string());
    }
    let rw_type = u32le(blob, 0x1C).ok_or("short header")?;
    // MODEL records carry meshes; the 0xCAFED00D variant referenced by gmdl
    // assemblies is skeleton/anim-only (same header layout).
    if rw_type != RW_TYPE_MODEL && rw_type != RW_TYPE_RIG {
        return Err(format!("not a MODEL rw4 (type {rw_type:#X})"));
    }
    let section_count = u32le(blob, 0x24).ok_or("short header")? as usize;
    let p_info = u32le(blob, 0x30).ok_or("short header")? as usize;
    let p_buf = u32le(blob, 0x44).ok_or("short header")? as usize;
    if section_count > 10000 {
        return Err("implausible section count".to_string());
    }

    let mut sections = Vec::with_capacity(section_count);
    for i in 0..section_count {
        let o = p_info + i * 24;
        let (p_data, _f04, size, _align, _tci, tc) = (
            u32le(blob, o).ok_or("short section table")? as usize,
            u32le(blob, o + 4).ok_or("short section table")?,
            u32le(blob, o + 8).ok_or("short section table")? as usize,
            u32le(blob, o + 12).ok_or("short section table")?,
            u32le(blob, o + 16).ok_or("short section table")?,
            u32le(blob, o + 20).ok_or("short section table")?,
        );
        let abs = if tc == T_RAW {
            p_data.wrapping_add(p_buf)
        } else {
            p_data
        };
        sections.push(Section {
            p_data: abs,
            size,
            type_code: tc,
        });
    }

    let mut objects = Vec::with_capacity(section_count);
    for s in &sections {
        let data = blob
            .get(s.p_data..s.p_data.saturating_add(s.size))
            .filter(|d| d.len() == s.size)
            .ok_or_else(|| format!("section {:#X} out of bounds", s.type_code))?;
        let obj = match s.type_code {
            T_MORPH_HANDLE => {
                if data.len() < 64 {
                    return Err("short morph handle".into());
                }
                let start = [
                    f64le(data, 8).unwrap(),
                    f64le(data, 16).unwrap(),
                    f64le(data, 24).unwrap(),
                ];
                let end = [
                    f64le(data, 32).unwrap(),
                    f64le(data, 40).unwrap(),
                    f64le(data, 48).unwrap(),
                ];
                let default_time = f32le(data, 56).unwrap();
                if start.iter().chain(&end).any(|x| !x.is_finite()) || !default_time.is_finite() {
                    return Err("non-finite morph handle".into());
                }
                Object::MorphHandle(MorphHandle {
                    id: u32le(data, 0).unwrap(),
                    start,
                    end,
                    default_time,
                    animation_reference: u32le(data, 60).unwrap(),
                    animation: None,
                })
            }
            T_MORPH_SHAPES => {
                if data.len() < 28 {
                    return Err("short morph shape IDs".into());
                }
                let count = u32le(data, 12).unwrap() as usize;
                if count > 1024
                    || u32le(data, 20).unwrap() as usize != count
                    || data.len() < 28 + count * 8
                {
                    return Err("invalid morph shape count or data length".into());
                }
                let shape_ids = (0..count)
                    .map(|i| u32le(data, 28 + count * 4 + i * 4).unwrap())
                    .collect();
                Object::MorphShapes(MorphShapes {
                    id: u32le(data, 24).unwrap(),
                    shape_ids,
                    object_index: objects.len(),
                })
            }
            T_RAW => Object::Raw {
                bytes: data.to_vec(),
            },
            T_MESH => {
                if data.len() < 36 {
                    return Err("short mesh".to_string());
                }
                let mut f = [0u32; 9];
                for (i, v) in f.iter_mut().enumerate() {
                    *v = u32le(data, i * 4).ok_or("short mesh")?;
                }
                let buf_count = f[4] as usize;
                if buf_count > 8 || data.len() < 36 + buf_count * 4 {
                    return Err("bad mesh buffer count".to_string());
                }
                let mut refs = Vec::new();
                for i in 0..buf_count {
                    refs.push(u32le(data, 36 + i * 4).ok_or("short mesh")?);
                }
                Object::Mesh { f, refs }
            }
            T_VBUF | T_IBUF => {
                if data.len() < 28 {
                    return Err("short buffer".to_string());
                }
                let mut f = [0u32; 7];
                for (i, v) in f.iter_mut().enumerate() {
                    *v = u32le(data, i * 4).ok_or("short buffer")?;
                }
                if s.type_code == T_VBUF {
                    Object::VBuf { f }
                } else {
                    Object::IBuf { f }
                }
            }
            T_VDESC => {
                if data.len() < 24 {
                    return Err("short vdesc".to_string());
                }
                let count = u16::from_le_bytes([data[12], data[13]]) as usize;
                let vsize = data[15] as usize;
                if count > 32 || data.len() < 24 + count * 12 {
                    return Err("bad vdesc element count".to_string());
                }
                let mut elements = Vec::new();
                for i in 0..count {
                    let o = 24 + i * 12;
                    let off = u16::from_le_bytes([data[o + 2], data[o + 3]]);
                    let dtype = data[o + 4];
                    let code = u32le(data, o + 8).ok_or("short element")?;
                    elements.push((off, dtype, code));
                }
                Object::VDesc {
                    vertex_size: vsize,
                    elements,
                }
            }
            T_SKELETON => {
                if data.len() < 24 {
                    return Err("short skeleton".to_string());
                }
                let p_flags = u32le(data, 0).ok_or("short skeleton")? as usize;
                let p_parents = u32le(data, 4).ok_or("short skeleton")? as usize;
                let p_names = u32le(data, 8).ok_or("short skeleton")? as usize;
                let count = u32le(data, 12).ok_or("short skeleton")? as usize;
                if count > 4096 {
                    return Err("implausible bone count".to_string());
                }
                let mut bones = Vec::with_capacity(count);
                for i in 0..count {
                    let nm = blob
                        .get(p_names + i * 4..p_names + i * 4 + 4)
                        .and_then(|s| u32le(s, 0))
                        .ok_or("skeleton names OOB")?;
                    let fl = blob
                        .get(p_flags + i * 4..p_flags + i * 4 + 4)
                        .and_then(|s| u32le(s, 0))
                        .ok_or("skeleton flags OOB")?;
                    let pi = blob
                        .get(p_parents + i * 4..p_parents + i * 4 + 4)
                        .and_then(|s| u32le(s, 0))
                        .ok_or("skeleton parents OOB")?;
                    let parent = if pi == 0xFFFF_FFFF {
                        None
                    } else if (pi as usize) < count {
                        Some(pi as usize)
                    } else {
                        return Err("bad bone parent".to_string());
                    };
                    bones.push(Bone {
                        name: nm,
                        flags: fl,
                        parent,
                    });
                }
                Object::Skeleton { bones }
            }
            T_ANIMSKIN => {
                if data.len() < 16 {
                    return Err("short skin".to_string());
                }
                let data_off = u32le(data, 0).ok_or("short skin")? as usize;
                let count = u32le(data, 4).ok_or("short skin")? as usize;
                if count > 4096 {
                    return Err("implausible pose count".to_string());
                }
                let mut poses = Vec::with_capacity(count);
                for i in 0..count {
                    let o = data_off + i * 64;
                    let mut rot = [[0.0f32; 3]; 3];
                    for r in 0..3 {
                        for c in 0..3 {
                            // 3x4 rows: 16 bytes each; rotation is the first 3.
                            rot[r][c] = f32le(blob, o + r * 16 + c * 4).ok_or("skin OOB")?;
                        }
                    }
                    let inv = [
                        f32le(blob, o + 48).ok_or("skin OOB")?,
                        f32le(blob, o + 52).ok_or("skin OOB")?,
                        f32le(blob, o + 56).ok_or("skin OOB")?,
                    ];
                    if !rot.iter().flatten().all(|v| v.is_finite())
                        || !inv.iter().all(|v| v.is_finite())
                    {
                        return Err("non-finite pose".to_string());
                    }
                    poses.push(BonePose {
                        rotation: rot,
                        inv_translation: inv,
                    });
                }
                Object::AnimSkin { poses }
            }
            T_BONE_MOUNTS => {
                // SkinMatrixBuffer: absolute array offset, count, two fields.
                // Arrays may be relocated; they need not follow this header.
                if data.len() < 16 {
                    return Err("short bone mounts".to_string());
                }
                let name = u32le(data, 0).ok_or("short bone mounts")?;
                let count = u32le(data, 4).ok_or("short bone mounts")?;
                let array_start = name as usize;
                let array_end = array_start
                    .checked_add(
                        (count as usize)
                            .checked_mul(48)
                            .ok_or("skin matrix count overflow")?,
                    )
                    .ok_or("skin matrix array overflow")?;
                if count > 4096 || array_end > blob.len() || (count > 0 && array_start < 76) {
                    return Err("skin matrix array outside file".to_string());
                }
                let mut mats = Vec::with_capacity(count as usize);
                for i in 0..count as usize {
                    let base = array_start + i * 48;
                    let mut m = [[0.0f32; 4]; 3];
                    let mut finite = true;
                    for r in 0..3 {
                        for c in 0..4 {
                            let v = f32le(blob, base + r * 16 + c * 4).unwrap_or(f32::NAN);
                            if !v.is_finite() {
                                finite = false;
                            }
                            m[r][c] = v;
                        }
                    }
                    if !finite {
                        return Err("non-finite bone mount".to_string());
                    }
                    mats.push(m);
                }
                Object::BoneMounts(BoneMounts {
                    name,
                    field_8: u32le(data, 8).unwrap(),
                    field_c: u32le(data, 12).unwrap(),
                    count,
                    mats,
                })
            }
            T_KEYANIM => {
                // RWKeyframeAnim: 48B header, absolute name/info tables,
                // section-relative keyframe positions. Validated hard:
                // counts, component types, finite quats/times.
                if data.len() < 48 {
                    return Err("short keyanim".to_string());
                }
                let base = s.p_data;
                let p_names = u32le(data, 0).ok_or("short keyanim")? as usize;
                let count = u32le(data, 4).ok_or("short keyanim")? as usize;
                let skeleton_id = u32le(data, 8).ok_or("short keyanim")?;
                let p_info = u32le(data, 44).ok_or("short keyanim")? as usize;
                let length = f32le(data, 32).ok_or("short keyanim")?;
                let section_end = base + data.len();
                if count == 0
                    || count > 4096
                    || !length.is_finite()
                    || length <= 0.0
                    || length > 3600.0
                {
                    return Err("bad keyanim header".to_string());
                }
                if p_names < base + 48
                    || p_names + count * 4 > section_end
                    || p_info < base + 48
                    || p_info + count * 12 > section_end
                {
                    return Err("keyanim table outside section".to_string());
                }
                let mut ids = Vec::with_capacity(count);
                for i in 0..count {
                    ids.push(
                        blob.get(p_names + i * 4..p_names + i * 4 + 4)
                            .and_then(|s| u32le(s, 0))
                            .ok_or("keyanim names OOB")?,
                    );
                }
                let mut infos = Vec::with_capacity(count);
                for i in 0..count {
                    let o = p_info + i * 12;
                    let pos = u32le(blob, o).ok_or("keyanim info OOB")? as usize;
                    let size = u32le(blob, o + 4).ok_or("short keyanim")? as usize;
                    let comp = u32le(blob, o + 8).ok_or("short keyanim")?;
                    if comp != COMP_LOCROTSCALE && comp != COMP_LOCROT && comp != COMP_BLEND {
                        return Err("bad keyanim components".to_string());
                    }
                    let expect = match comp {
                        COMP_LOCROTSCALE => 48,
                        COMP_LOCROT => 32,
                        _ => 8,
                    };
                    if size != expect || pos < 48 || base + pos + size > section_end {
                        return Err("bad keyanim channel".to_string());
                    }
                    infos.push((pos, size, comp));
                }
                let mut channels = Vec::with_capacity(count);
                for i in 0..count {
                    let (pos, size, comp) = infos[i];
                    let key_count = if i + 1 < count {
                        let next = infos[i + 1].0;
                        if next < pos || (next - pos) % size != 0 {
                            return Err("bad keyanim layout".to_string());
                        }
                        (next - pos) / size
                    } else {
                        // Padding has no key count. Stop at decreasing time,
                        // but never consume the next RW4 section as more keys.
                        let mut nk = 0;
                        let mut last = f32::NEG_INFINITY;
                        while nk < 100_000 {
                            let o = base + pos + nk * size;
                            if o + size > section_end {
                                break;
                            }
                            let t = f32le(blob, o + size - 4).ok_or("keyanim keys OOB")?;
                            if !t.is_finite() || t < last {
                                break;
                            }
                            last = t;
                            nk += 1;
                        }
                        nk
                    };
                    if key_count == 0 || key_count > 100_000 {
                        return Err("bad keyframe count".to_string());
                    }
                    let mut keys = Vec::with_capacity(key_count.min(1024));
                    for k in 0..key_count {
                        let o = base + pos + k * size;
                        let key = match comp {
                            COMP_LOCROTSCALE => {
                                let q = [
                                    f32le(blob, o).ok_or("key OOB")?,
                                    f32le(blob, o + 4).ok_or("key OOB")?,
                                    f32le(blob, o + 8).ok_or("key OOB")?,
                                    f32le(blob, o + 12).ok_or("key OOB")?,
                                ];
                                let loc = [
                                    f32le(blob, o + 16).ok_or("key OOB")?,
                                    f32le(blob, o + 20).ok_or("key OOB")?,
                                    f32le(blob, o + 24).ok_or("key OOB")?,
                                ];
                                let sc = [
                                    f32le(blob, o + 28).ok_or("key OOB")?,
                                    f32le(blob, o + 32).ok_or("key OOB")?,
                                    f32le(blob, o + 36).ok_or("key OOB")?,
                                ];
                                let t = f32le(blob, o + 44).ok_or("key OOB")?;
                                Keyframe {
                                    time: t,
                                    rotation: q,
                                    location: loc,
                                    scale: sc,
                                    factor: 0.0,
                                    blend: false,
                                }
                            }
                            COMP_LOCROT => {
                                let q = [
                                    f32le(blob, o).ok_or("key OOB")?,
                                    f32le(blob, o + 4).ok_or("key OOB")?,
                                    f32le(blob, o + 8).ok_or("key OOB")?,
                                    f32le(blob, o + 12).ok_or("key OOB")?,
                                ];
                                let loc = [
                                    f32le(blob, o + 16).ok_or("key OOB")?,
                                    f32le(blob, o + 20).ok_or("key OOB")?,
                                    f32le(blob, o + 24).ok_or("key OOB")?,
                                ];
                                let t = f32le(blob, o + 28).ok_or("key OOB")?;
                                Keyframe {
                                    time: t,
                                    rotation: q,
                                    location: loc,
                                    scale: [1.0, 1.0, 1.0],
                                    factor: 0.0,
                                    blend: false,
                                }
                            }
                            _ => {
                                let f_ = f32le(blob, o).ok_or("key OOB")?;
                                let t = f32le(blob, o + 4).ok_or("key OOB")?;
                                Keyframe {
                                    time: t,
                                    rotation: [0.0, 0.0, 0.0, 1.0],
                                    location: [0.0, 0.0, 0.0],
                                    scale: [1.0, 1.0, 1.0],
                                    factor: f_,
                                    blend: true,
                                }
                            }
                        };
                        if !key.time.is_finite()
                            || !key.rotation.iter().all(|v| v.is_finite())
                            || !key.location.iter().all(|v| v.is_finite())
                            || !key.scale.iter().all(|v| v.is_finite())
                            || !key.factor.is_finite()
                        {
                            return Err("non-finite keyframe".to_string());
                        }
                        keys.push(key);
                        if keys.len() == 1024 && key_count > 1024 {
                            // Cap stored keys for giant channels; layout already validated.
                            break;
                        }
                    }
                    channels.push(AnimChannel { id: ids[i], keys });
                }
                Object::KeyAnim(KeyAnim {
                    skeleton_id,
                    length,
                    channels,
                })
            }
            T_BLENDSHAPES => {
                // RWBlendShapeBuffer: [type==1][11 offsets][shapeCount,
                // vertexCount, unk, boneIndicesCount]; stream sizes come from
                // consecutive non-zero offsets (section size terminates).
                if data.len() < 64 {
                    return Err("short blendshapes".to_string());
                }
                if u32le(data, 0).ok_or("short blendshapes")? != 1 {
                    return Err("unsupported blendshape type".to_string());
                }
                let mut offs = [0usize; 11];
                for i in 0..11 {
                    offs[i] = u32le(data, 4 + i * 4).ok_or("short blendshapes")? as usize;
                }
                let vertex_count = u32le(data, 52).ok_or("short blendshapes")? as usize;
                if vertex_count == 0 || vertex_count > 1_000_000 {
                    return Err("implausible blend vertex count".to_string());
                }
                let shape_count = u32le(data, 48).ok_or("short blendshapes")? as usize;
                if shape_count > 64 {
                    return Err("implausible shape count".to_string());
                }
                let mut sizes = [0usize; 11];
                for i in 0..11 {
                    if offs[i] == 0 {
                        continue;
                    }
                    let mut end = data.len();
                    for j in 0..11 {
                        if offs[j] > offs[i] && offs[j] < end {
                            end = offs[j];
                        }
                    }
                    if end < offs[i] {
                        return Err("bad blend offsets".to_string());
                    }
                    sizes[i] = end - offs[i];
                }
                // Stream 0: base positions (float4) + morph deltas.
                let np = offs[0];
                if np == 0 || sizes[0] < vertex_count * 16 {
                    return Err("bad position stream".to_string());
                }
                let mut positions = Vec::with_capacity(vertex_count);
                for i in 0..vertex_count {
                    let o = np + i * 16;
                    let p = [
                        f32le(data, o).ok_or("blend pos OOB")?,
                        f32le(data, o + 4).ok_or("blend pos OOB")?,
                        f32le(data, o + 8).ok_or("blend pos OOB")?,
                    ];
                    if !p.iter().all(|v| v.is_finite()) {
                        return Err("non-finite blend pos".to_string());
                    }
                    positions.push(p);
                }
                let mut normals = Vec::new();
                if offs[1] != 0 && sizes[1] >= vertex_count * 16 {
                    for i in 0..vertex_count {
                        let o = offs[1] + i * 16;
                        normals.push([
                            f32le(data, o).unwrap_or(0.0),
                            f32le(data, o + 4).unwrap_or(0.0),
                            f32le(data, o + 8).unwrap_or(1.0),
                        ]);
                    }
                }
                // Morph deltas: shape_count arrays after the base, guarded.
                let mut position_deltas = Vec::new();
                if shape_count > 0
                    && shape_count <= 64
                    && vertex_count * shape_count <= 400_000
                    && sizes[0] >= (1 + shape_count) * vertex_count * 16
                {
                    for s in 0..shape_count {
                        let base = np + (1 + s) * vertex_count * 16;
                        let mut deltas = Vec::with_capacity(vertex_count);
                        let mut bad = false;
                        for i in 0..vertex_count {
                            let o = base + i * 16;
                            let p = [
                                f32le(data, o).unwrap_or(f32::NAN),
                                f32le(data, o + 4).unwrap_or(f32::NAN),
                                f32le(data, o + 8).unwrap_or(f32::NAN),
                            ];
                            if !p.iter().all(|v| v.is_finite()) {
                                bad = true;
                                break;
                            }
                            deltas.push(p);
                        }
                        if bad {
                            position_deltas.clear();
                            break;
                        }
                        position_deltas.push(deltas);
                    }
                }
                let mut uvs = Vec::new();
                if offs[3] != 0 && sizes[3] >= vertex_count * 8 {
                    // float4 texcoords; stride is 16 when the stream is big enough.
                    let stride = if sizes[3] >= vertex_count * 16 { 16 } else { 8 };
                    for i in 0..vertex_count {
                        let o = offs[3] + i * stride;
                        uvs.push([
                            f32le(data, o).unwrap_or(0.0),
                            f32le(data, o + 4).unwrap_or(0.0),
                        ]);
                    }
                }
                // Streams 9/10: bone indices (u16 pairs) + weights (float pairs).
                let mut skin_indices = Vec::new();
                let mut skin_weights = Vec::new();
                if offs[9] != 0
                    && offs[10] != 0
                    && sizes[9] >= vertex_count * 4
                    && sizes[10] >= vertex_count * 8
                {
                    let mut sane = true;
                    for i in 0..vertex_count {
                        let o = offs[9] + i * 4;
                        let a = u16::from_le_bytes([data[o], data[o + 1]]);
                        let b = u16::from_le_bytes([data[o + 2], data[o + 3]]);
                        let w = offs[10] + i * 8;
                        let w0 = f32le(data, w).unwrap_or(f32::NAN);
                        let w1 = f32le(data, w + 4).unwrap_or(f32::NAN);
                        if !w0.is_finite() || !w1.is_finite() || (w0 + w1 - 1.0).abs() > 0.05 {
                            sane = false;
                            break;
                        }
                        skin_indices.push([a, b]);
                        skin_weights.push([w0, w1]);
                    }
                    if !sane {
                        skin_indices.clear();
                        skin_weights.clear();
                    }
                }
                Object::BlendShapes {
                    vertex_count,
                    shape_count,
                    positions,
                    position_deltas,
                    normals,
                    uvs,
                    skin_indices,
                    skin_weights,
                }
            }
            other => Object::Opaque { type_code: other },
        };
        objects.push(obj);
    }
    Ok(objects)
}

fn resolve<'a>(objects: &'a [Object], r: u32) -> Option<&'a Object> {
    if r >> 22 != 0 {
        return None;
    }
    objects.get(r as usize)
}

/// Authored SkinsInK association. Equal bone counts alone do not establish
/// which skeleton, inverse poses and matrix buffer belong together.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SkinRigLink {
    pub source_section: usize,
    pub matrix_section: usize,
    pub skeleton_section: usize,
    pub pose_section: usize,
    pub bone_count: usize,
    pub matrix_count: usize,
    pub pose_count: usize,
}

#[derive(Clone, Debug)]
pub struct LinkedSkinRig {
    pub source: SkinRigLink,
    pub bones: Vec<Bone>,
    pub poses: Vec<BonePose>,
    pub matrices: Vec<Mat34>,
}

/// Resolve the three exact direct references in native RW4 0x7000C records.
/// Sub-reference/null tags are explicitly unsupported here; they cannot be
/// treated as section indices or replaced with same-sized unrelated tables.
pub fn extract_linked_skin_rigs(blob: &[u8]) -> Result<Vec<LinkedSkinRig>, String> {
    let parts = sections(blob)?;
    let objects = parse_objects(blob)?;
    let mut links = Vec::new();
    for (source_section, (kind, data)) in parts.iter().enumerate() {
        if *kind != T_SKINS_IN_K {
            continue;
        }
        if data.len() < 20 {
            return Err(format!("truncated SkinsInK section {source_section}"));
        }
        let section = |at: usize, expected: u32| -> Result<usize, String> {
            let raw = u32le(data, at).unwrap();
            if raw >> 22 != 0 {
                return Err(format!(
                    "SkinsInK {source_section} unsupported reference {raw:08X}"
                ));
            }
            let index = raw as usize;
            if parts.get(index).map(|p| p.0) != Some(expected) {
                return Err(format!(
                    "SkinsInK {source_section} reference {index} must be type {expected:08X}"
                ));
            }
            Ok(index)
        };
        let matrix_section = section(8, T_SKIN_MATRIX_BUFFER)?;
        let skeleton_section = section(12, T_SKELETON)?;
        let pose_section = section(16, T_ANIMSKIN)?;
        let Object::Skeleton { bones } = &objects[skeleton_section] else {
            return Err("linked skeleton not decoded".into());
        };
        let Object::AnimSkin { poses } = &objects[pose_section] else {
            return Err("linked inverse poses not decoded".into());
        };
        let Object::BoneMounts(buffer) = &objects[matrix_section] else {
            return Err("linked skin buffer not decoded".into());
        };
        if bones.len() != poses.len() || bones.len() != buffer.mats.len() {
            return Err(format!(
                "SkinsInK {source_section} mismatched counts: bones {}, poses {}, matrices {}",
                bones.len(),
                poses.len(),
                buffer.mats.len()
            ));
        }
        links.push(LinkedSkinRig {
            source: SkinRigLink {
                source_section,
                matrix_section,
                skeleton_section,
                pose_section,
                bone_count: bones.len(),
                matrix_count: buffer.mats.len(),
                pose_count: poses.len(),
            },
            bones: bones.clone(),
            poses: poses.clone(),
            matrices: buffer.mats.clone(),
        });
    }
    Ok(links)
}

/// Extract all statically-bound meshes from a decoded MODEL record.
pub fn extract_meshes(blob: &[u8], stats: &mut MeshStats) -> Vec<ExtractedMesh> {
    let mut out = Vec::new();
    // Rig-only containers share the object parser, but never provide a body.
    if u32le(blob, 0x1C) != Some(RW_TYPE_MODEL) {
        stats.skipped_unsupported += 1;
        return out;
    }
    let objects = match parse_objects(blob) {
        Ok(o) => o,
        Err(_) => {
            stats.skipped_unsupported += 1;
            return out;
        }
    };
    for (source_section, obj) in objects.iter().enumerate() {
        let (f, vrefs) = match obj {
            Object::Mesh { f, refs } => (f, refs),
            _ => continue,
        };
        stats.meshes += 1;
        let prim = f[1];
        let ibuf_ref = f[2];
        let tri_count = f[3] as usize;
        let first_index = f[5] as usize;
        let first_vertex = f[7] as usize;
        let vert_count = f[8] as usize;
        if prim != PRIM_TRIANGLE_LIST || tri_count == 0 || tri_count > 1_000_000 {
            stats.skipped_unsupported += 1;
            continue;
        }
        // Shared: resolve the index buffer + raw indices.
        let (ibuf_start, idata) = match (|| {
            let ibuf = match resolve(&objects, ibuf_ref)? {
                Object::IBuf { f } => f,
                _ => return None,
            };
            if ibuf[4] != IDX16_FORMAT {
                return None;
            }
            let idata = match resolve(&objects, ibuf[6])? {
                Object::Raw { bytes } => bytes,
                _ => return None,
            };
            Some((ibuf[1] as usize, idata, ibuf[2] as usize))
        })() {
            Some((start, data, provisional_count)) => {
                let _ = provisional_count;
                (start, data)
            }
            None => {
                stats.skipped_unresolved += 1;
                continue;
            }
        };
        let need_index = tri_count * 3;
        if idata.len() < (ibuf_start + first_index + need_index) * 2 {
            stats.skipped_unsupported += 1;
            continue;
        }
        // Vertex source: classic vertex buffer, else blend-shape base arrays.
        enum Verts<'a> {
            Static {
                stride: usize,
                pos_off: usize,
                nrm_off: Option<(usize, u8)>,
                uv_off: Option<usize>,
                bone_off: Option<usize>,
                weight_off: Option<usize>,
                vdata: &'a [u8],
                base_vert: usize,
            },
            Blend {
                base_vert: usize,
                positions: &'a [[f32; 3]],
                normals: &'a [[f32; 3]],
                uvs: &'a [[f32; 2]],
                deltas: &'a [Vec<[f32; 3]>],
                skin_idx: &'a [[u16; 2]],
                skin_w: &'a [[f32; 2]],
            },
        }
        let verts = match (|| {
            if vrefs.len() != 1 {
                return None;
            }
            let vbuf = match resolve(&objects, vrefs[0])? {
                Object::VBuf { f } => f,
                _ => return None,
            };
            let (stride, elements) = match resolve(&objects, vbuf[0])? {
                Object::VDesc {
                    vertex_size,
                    elements,
                } => (*vertex_size, elements),
                _ => return None,
            };
            let vdata = match resolve(&objects, vbuf[6])? {
                Object::Raw { bytes } => bytes,
                _ => return None,
            };
            let find = |code: u32, dtype: u8| {
                elements
                    .iter()
                    .find(|(_, t, c)| *c == code && *t == dtype)
                    .map(|(o, _, _)| *o as usize)
            };
            let pos_off = find(0, 2)?;
            let base_vert = (first_vertex + vbuf[2] as usize) as usize;
            let vcount = vert_count.min(vbuf[3] as usize);
            if vcount == 0 || vcount > 500_000 {
                return None;
            }
            if vdata.len() < (base_vert + vcount) * stride {
                return None;
            }
            Some((
                Verts::Static {
                    stride,
                    pos_off,
                    nrm_off: elements
                        .iter()
                        .find(|(_, t, code)| *code == 2 && matches!(*t, 2 | 5 | 8))
                        .map(|(o, t, _)| (*o as usize, *t)),
                    uv_off: find(6, 1),
                    bone_off: find(14, 5),
                    weight_off: find(15, 8),
                    vdata,
                    base_vert,
                },
                vcount,
            ))
        })() {
            Some(v) => v,
            None => {
                // Static bind failed (null vbuf or bad refs): skinned/morphed
                // mesh — bind via the blend-shape base arrays instead.
                let mut found = None;
                for obj in &objects {
                    if let Object::BlendShapes {
                        vertex_count,
                        shape_count: _,
                        positions,
                        position_deltas,
                        normals,
                        uvs,
                        skin_indices,
                        skin_weights,
                    } = obj
                    {
                        if *vertex_count >= first_vertex + vert_count
                            && vert_count > 0
                            && vert_count <= 500_000
                        {
                            found = Some(Verts::Blend {
                                base_vert: first_vertex,
                                positions,
                                normals,
                                uvs,
                                deltas: position_deltas,
                                skin_idx: skin_indices,
                                skin_w: skin_weights,
                            });
                            break;
                        }
                    }
                }
                match found {
                    Some(v) => (v, vert_count),
                    None => {
                        stats.skipped_unresolved += 1;
                        continue;
                    }
                }
            }
        };
        let (verts, vcount) = verts;
        let base_vert = match &verts {
            Verts::Static { base_vert, .. } | Verts::Blend { base_vert, .. } => *base_vert,
        };
        let mut positions = Vec::with_capacity(vcount);
        let mut normals = Vec::with_capacity(vcount);
        let mut uvs = Vec::with_capacity(vcount);
        let mut ok = true;
        let mut bone_indices = Vec::new();
        let mut bone_weights = Vec::new();
        match &verts {
            Verts::Static {
                stride,
                pos_off,
                nrm_off,
                uv_off,
                bone_off,
                weight_off,
                vdata,
                base_vert,
            } => {
                for i in 0..vcount {
                    let vo = (base_vert + i) * stride;
                    let p = [
                        f32le(vdata, vo + pos_off),
                        f32le(vdata, vo + pos_off + 4),
                        f32le(vdata, vo + pos_off + 8),
                    ];
                    if p.iter().any(|v| v.is_none()) {
                        ok = false;
                        break;
                    }
                    positions.push([p[0].unwrap(), p[1].unwrap(), p[2].unwrap()]);
                    if let (Some(b), Some(w)) = (bone_off, weight_off) {
                        bone_indices.push(vdata[vo + b..vo + b + 4].try_into().unwrap());
                        bone_weights.push(std::array::from_fn(|i| vdata[vo + w + i] as f32 / 255.));
                    }
                    if let Some((o, kind)) = nrm_off {
                        let normal = if *kind == 2 {
                            [
                                f32le(vdata, vo + o).unwrap_or(0.0),
                                f32le(vdata, vo + o + 4).unwrap_or(0.0),
                                f32le(vdata, vo + o + 8).unwrap_or(1.0),
                            ]
                        } else {
                            unpack_byte_normal(vdata[vo + o..vo + o + 3].try_into().unwrap())
                        };
                        normals.push(normal);
                    }
                    if let Some(o) = uv_off {
                        uvs.push([
                            f32le(vdata, vo + o).unwrap_or(0.0),
                            f32le(vdata, vo + o + 4).unwrap_or(0.0),
                        ]);
                    }
                }
            }
            Verts::Blend {
                base_vert,
                positions: bp,
                normals: bn,
                uvs: bu,
                ..
            } => {
                if bp.len() < base_vert + vcount {
                    ok = false;
                } else {
                    positions.extend_from_slice(&bp[*base_vert..*base_vert + vcount]);
                    if bn.len() >= base_vert + vcount {
                        normals.extend_from_slice(&bn[*base_vert..*base_vert + vcount]);
                    }
                    if bu.len() >= base_vert + vcount {
                        uvs.extend_from_slice(&bu[*base_vert..*base_vert + vcount]);
                    }
                    if positions.iter().any(|p| p.iter().any(|v| !v.is_finite())) {
                        ok = false;
                    }
                }
            }
        }
        // Window the morph deltas + skin influences to the vertex range.
        let mut morphs: Vec<Vec<[f32; 3]>> = Vec::new();
        let mut skin_idx: Vec<[u16; 2]> = Vec::new();
        let mut skin_w: Vec<[f32; 2]> = Vec::new();
        if ok {
            if let Verts::Blend {
                base_vert,
                deltas,
                skin_idx: si,
                skin_w: sw,
                ..
            } = &verts
            {
                for shape in deltas.iter() {
                    if shape.len() >= base_vert + vcount {
                        morphs.push(shape[*base_vert..*base_vert + vcount].to_vec());
                    }
                }
                if si.len() >= base_vert + vcount && sw.len() >= base_vert + vcount {
                    skin_idx.extend_from_slice(&si[*base_vert..*base_vert + vcount]);
                    skin_w.extend_from_slice(&sw[*base_vert..*base_vert + vcount]);
                }
            }
        }
        if !ok {
            stats.skipped_unsupported += 1;
            continue;
        }
        let mut indices = Vec::with_capacity(need_index);
        for i in 0..need_index {
            let o = (ibuf_start + first_index + i) * 2;
            if o + 2 > idata.len() {
                ok = false;
                break;
            }
            indices.push(u16::from_le_bytes([idata[o], idata[o + 1]]));
        }
        if !ok {
            stats.skipped_unsupported += 1;
            continue;
        }
        // Rebase indices to the extracted vertex window.
        for idx in indices.iter_mut() {
            let v = (*idx as usize).saturating_sub(base_vert) as u16;
            if (v as usize) >= vcount {
                ok = false;
                break;
            }
            *idx = v;
        }
        if !ok {
            stats.skipped_unsupported += 1;
            continue;
        }
        // Fill missing attributes so Bevy always gets a complete mesh.
        if normals.len() != vcount {
            normals = vec![[0.0, 1.0, 0.0]; vcount];
        }
        if uvs.len() != vcount {
            uvs = vec![[0.0, 0.0]; vcount];
        }
        let mesh = ExtractedMesh {
            source_section,
            bone_indices,
            bone_weights,
            positions,
            normals,
            uvs,
            indices,
            morphs,
            skin_idx,
            skin_w,
        };
        if !mesh.valid() {
            stats.skipped_unsupported += 1;
            continue;
        }
        match verts {
            Verts::Static { .. } => stats.static_ok += 1,
            Verts::Blend { .. } => stats.blend_ok += 1,
        }
        stats.morph_shapes += mesh.morphs.len();
        stats.verts += mesh.positions.len();
        stats.tris += mesh.indices.len() / 3;
        out.push(mesh);
    }
    out
}

/// Scan result for one RW4 record.
#[derive(Debug, Default)]
pub struct Rw4Report {
    pub rw_type: u32,
    pub sections: usize,
    pub stats: MeshStats,
}

/// One blend-shape pool summary for headless validation.
#[derive(Debug, Clone, Default)]
pub struct PoolInfo {
    pub vertex_count: usize,
    pub shapes: usize,
    pub skinned: bool,
    pub max_bone_idx: u16,
}

/// List blend-shape pools in a decoded record.
pub fn pool_report(blob: &[u8]) -> Vec<PoolInfo> {
    let mut out = Vec::new();
    let Ok(objects) = parse_objects(blob) else {
        return out;
    };
    for obj in &objects {
        if let Object::BlendShapes {
            vertex_count,
            shape_count,
            skin_indices,
            ..
        } = obj
        {
            out.push(PoolInfo {
                vertex_count: *vertex_count,
                shapes: *shape_count,
                skinned: !skin_indices.is_empty(),
                max_bone_idx: skin_indices.iter().flatten().copied().max().unwrap_or(0),
            });
        }
    }
    out
}

/// Classify + extract a decoded RW4 record (any type; non-MODEL reported).
pub fn process_record(blob: &[u8], stats: &mut MeshStats) -> (Rw4Report, Vec<ExtractedMesh>) {
    let mut rep = Rw4Report::default();
    if blob.len() < 96 || &blob[0..28] != RW_MAGIC {
        return (rep, Vec::new());
    }
    rep.rw_type = u32le(blob, 0x1C).unwrap_or(0);
    rep.sections = u32le(blob, 0x24).unwrap_or(0) as usize;
    if rep.rw_type != RW_TYPE_MODEL {
        return (rep, Vec::new());
    }
    let meshes = extract_meshes(blob, stats);
    rep.stats = stats.clone();
    (rep, meshes)
}

/// Extract keyframe animations from a decoded MODEL record.
pub fn extract_anims(blob: &[u8], stats: &mut MeshStats) -> Vec<KeyAnim> {
    let mut out = Vec::new();
    let objects = match parse_objects(blob) {
        Ok(o) => o,
        Err(_) => return out,
    };
    for obj in &objects {
        if let Object::KeyAnim(anim) = obj {
            stats.keyanims += 1;
            stats.channels += anim.channels.len();
            stats.keys += anim.channels.iter().map(|c| c.keys.len()).sum::<usize>();
            out.push(anim.clone());
        }
    }
    out
}

/// Convenience: decode a DBPF RW4 record and extract its animations.
pub fn anims_from_package_record(
    pkg: &std::path::Path,
    entry: &dbpf::DbpfEntry,
    stats: &mut MeshStats,
) -> Vec<KeyAnim> {
    match dbpf::read_entry_bytes(pkg, entry) {
        Ok(bytes) => extract_anims(&bytes, stats),
        Err(_) => Vec::new(),
    }
}

/// Extract skeletons + animation skins from a decoded MODEL record.
pub fn extract_rig(blob: &[u8], stats: &mut MeshStats) -> (Vec<Vec<Bone>>, Vec<Vec<BonePose>>) {
    let mut skels = Vec::new();
    let mut skins = Vec::new();
    let objects = match parse_objects(blob) {
        Ok(o) => o,
        Err(_) => return (skels, skins),
    };
    for obj in &objects {
        match obj {
            Object::Skeleton { bones } => {
                stats.skeletons += 1;
                stats.bones += bones.len();
                stats.bones_valid_parents += bones
                    .iter()
                    .filter(|b| match b.parent {
                        None => true,
                        Some(p) => p < bones.len(),
                    })
                    .count();
                skels.push(bones.clone());
            }
            Object::AnimSkin { poses } => {
                stats.skins += 1;
                stats.poses += poses.len();
                skins.push(poses.clone());
            }
            _ => {}
        }
    }
    (skels, skins)
}

/// A real animation clip bound to a skeleton: per-bone channel mapping
/// plus the bind/key reference translations for delta application.
#[derive(Debug, Clone)]
pub struct ClipPair {
    pub clip: KeyAnim,
    pub chan_for_bone: Vec<Option<usize>>,
    pub bind_t: Vec<[f32; 3]>,
    pub key0_loc: Vec<[f32; 3]>,
}

/// Bind the best clip to a skeleton by bone-name == channel-id.
/// Requires at least one mapped non-blend channel.
pub fn pair_clip(bones: &[Bone], locals: &[Mat34], anims: &[KeyAnim]) -> Option<ClipPair> {
    let mut best: Option<(usize, Vec<Option<usize>>)> = None;
    let mut best_hits = 0;
    for (ai, anim) in anims.iter().enumerate() {
        let mut map = vec![None; bones.len()];
        let mut hits = 0;
        for (bi, b) in bones.iter().enumerate() {
            for (ci, c) in anim.channels.iter().enumerate() {
                let non_blend = c.keys.first().map(|k| !k.blend).unwrap_or(false);
                if non_blend && c.id == b.name {
                    map[bi] = Some(ci);
                    hits += 1;
                    break;
                }
            }
        }
        if hits > best_hits {
            best_hits = hits;
            best = Some((ai, map));
        }
    }
    let (ai, map) = best?;
    let clip = anims.get(ai)?.clone();
    let bind_t: Vec<[f32; 3]> = locals.iter().map(|m| [m[0][3], m[1][3], m[2][3]]).collect();
    let key0_loc: Vec<[f32; 3]> = map
        .iter()
        .map(|mc| {
            mc.and_then(|ci| clip.channels.get(ci))
                .and_then(|c| c.keys.first())
                .map(|k| k.location)
                .unwrap_or([0.0, 0.0, 0.0])
        })
        .collect();
    Some(ClipPair {
        clip,
        chan_for_bone: map,
        bind_t,
        key0_loc,
    })
}
/// Score a blob as a skinned-demo source: a skin-carrying mesh paired with
/// a file-local skeleton/skin of matching size, plus the best bone-mapped
/// animation clip when one exists.
pub fn score_skinned_candidate(
    blob: &[u8],
) -> Option<(
    ExtractedMesh,
    Vec<Mat34>,
    Vec<Mat34>,
    Vec<Option<usize>>,
    usize,
    Option<ClipPair>,
)> {
    let mut stats = MeshStats::default();
    let meshes = extract_meshes(blob, &mut stats);
    if meshes.is_empty() {
        return None;
    }
    let (skels, skins) = extract_rig(blob, &mut stats);
    let mut best: Option<(
        ExtractedMesh,
        Vec<Mat34>,
        Vec<Mat34>,
        Vec<Option<usize>>,
        usize,
        usize,
        Option<ClipPair>,
    )> = None;
    for em in &meshes {
        if em.skin_idx.is_empty() || em.positions.len() > 20000 || em.positions.len() < 8 {
            continue;
        }
        for bones in &skels {
            for poses in &skins {
                if bones.len() != poses.len() || bones.is_empty() {
                    continue;
                }
                let n = bones.len();
                let score = em
                    .skin_idx
                    .iter()
                    .filter(|p| (p[0] as usize) < n || (p[1] as usize) < n)
                    .count();
                if score < 50 {
                    continue;
                }
                if let Some((locals, invs)) = bind_locals(bones, poses) {
                    let better = match &best {
                        None => true,
                        Some((_, _, _, _, _, s, _)) => score > *s,
                    };
                    if better {
                        let mut per_bone = vec![0usize; n];
                        for pair in &em.skin_idx {
                            for b in [pair[0] as usize, pair[1] as usize] {
                                if b < n {
                                    per_bone[b] += 1;
                                }
                            }
                        }
                        let wave = per_bone
                            .iter()
                            .enumerate()
                            .max_by_key(|(_, c)| **c)
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        // Pair a real clip against this skeleton when possible.
                        let mut anim_stats = MeshStats::default();
                        let anims = extract_anims(blob, &mut anim_stats);
                        let pair = pair_clip(bones, &locals, &anims);
                        best = Some((
                            em.clone(),
                            locals,
                            invs,
                            bones.iter().map(|b| b.parent).collect(),
                            wave,
                            score,
                            pair,
                        ));
                    }
                }
            }
        }
    }
    best.map(|(em, locals, invs, parents, wave, _, pair)| (em, locals, invs, parents, wave, pair))
}

/// Convenience: decode a DBPF RW4 record and extract its rig.
pub fn rig_from_package_record(
    pkg: &std::path::Path,
    entry: &dbpf::DbpfEntry,
    stats: &mut MeshStats,
) -> (Vec<Vec<Bone>>, Vec<Vec<BonePose>>) {
    match dbpf::read_entry_bytes(pkg, entry) {
        Ok(bytes) => extract_rig(&bytes, stats),
        Err(_) => (Vec::new(), Vec::new()),
    }
}

/// Convenience: decode a DBPF RW4 record and extract its meshes.
pub fn meshes_from_package_record(
    pkg: &std::path::Path,
    entry: &dbpf::DbpfEntry,
    stats: &mut MeshStats,
) -> Vec<ExtractedMesh> {
    match dbpf::read_entry_bytes(pkg, entry) {
        Ok(bytes) => extract_meshes(&bytes, stats),
        Err(_) => {
            stats.skipped_unsupported += 1;
            Vec::new()
        }
    }
}

#[derive(Debug, Clone)]
pub struct MorphHandle {
    pub id: u32,
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub default_time: f32,
    pub animation_reference: u32,
    pub animation: Option<KeyAnim>,
}
#[derive(Debug, Clone)]
pub struct MorphShapes {
    pub id: u32,
    pub shape_ids: Vec<u32>,
    pub object_index: usize,
}
#[derive(Debug, Clone)]
pub struct MorphData {
    pub handles: Vec<MorphHandle>,
    pub shapes: Vec<MorphShapes>,
    pub pools: Vec<PoolInfo>,
    pub errors: Vec<String>,
    pub rig: Option<MorphRig>,
}
#[derive(Debug, Clone)]
pub struct MorphRig {
    pub bones: Vec<Bone>,
    pub poses: Vec<BonePose>,
}

/// ff0000 and ff0002 are independently indexed RW4 sections. Resolve each
/// handle's animation reference through that section graph, never by channel
/// array index or by XML order. Unsupported reference namespaces are explicit.
pub fn extract_morph_data(blob: &[u8]) -> Result<MorphData, String> {
    let objects = parse_objects(blob)?;
    let mut data = MorphData {
        handles: Vec::new(),
        shapes: Vec::new(),
        pools: pool_report(blob),
        errors: Vec::new(),
        rig: None,
    };
    for obj in &objects {
        match obj {
            Object::MorphHandle(h) => {
                let mut h = h.clone();
                h.animation = match resolve(&objects, h.animation_reference) {
                    Some(Object::KeyAnim(a)) => Some(a.clone()),
                    _ => {
                        data.errors.push(format!(
                            "handle {:08X} unresolved animation reference {:08X}",
                            h.id, h.animation_reference
                        ));
                        None
                    }
                };
                if !data.handles.iter().any(|existing| {
                    existing.id == h.id
                        && existing.animation_reference == h.animation_reference
                        && existing.default_time == h.default_time
                }) {
                    data.handles.push(h);
                }
            }
            Object::MorphShapes(s) => {
                let mut ids = std::collections::HashSet::new();
                if s.shape_ids.iter().any(|id| !ids.insert(*id)) {
                    data.errors.push("duplicate shape IDs".into());
                }
                data.shapes.push(s.clone());
            }
            _ => {}
        }
    }
    if data
        .handles
        .iter()
        .filter_map(|h| h.animation.as_ref())
        .flat_map(|a| &a.channels)
        .flat_map(|c| &c.keys)
        .any(|k| !k.blend)
    {
        let skeletons: Vec<_> = objects
            .iter()
            .filter_map(|o| match o {
                Object::Skeleton { bones } => Some(bones),
                _ => None,
            })
            .filter(|bones| {
                data.handles
                    .iter()
                    .filter_map(|h| h.animation.as_ref())
                    .flat_map(|a| &a.channels)
                    .filter(|c| c.keys.iter().any(|k| !k.blend))
                    .all(|c| bones.iter().any(|b| b.name == c.id))
            })
            .collect();
        if skeletons.len() == 1 {
            let poses: Vec<_> = objects
                .iter()
                .filter_map(|o| match o {
                    Object::AnimSkin { poses } if poses.len() == skeletons[0].len() => Some(poses),
                    _ => None,
                })
                .collect();
            if poses.len() == 1 {
                data.rig = Some(MorphRig {
                    bones: skeletons[0].clone(),
                    poses: poses[0].clone(),
                });
            }
        }
        if data.rig.is_none() && data.shapes.is_empty() {
            data.errors
                .push("missing or ambiguous handle skeleton/skin".into());
        }
    }
    Ok(data)
}

pub fn dump_record_layout(blob: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let objects = match parse_objects(blob) {
        Ok(o) => o,
        Err(e) => return vec![format!("parse_objects failed: {e}")],
    };
    for (i, obj) in objects.iter().enumerate() {
        match obj {
            Object::MorphHandle(h) => out.push(format!(
                "obj#{i} MorphHandle id={:08X} progress={} anim={:08X}",
                h.id, h.default_time, h.animation_reference
            )),
            Object::MorphShapes(s) => out.push(format!(
                "obj#{i} MorphShapes id={:08X} shapes={:?}",
                s.id, s.shape_ids
            )),
            Object::VDesc {
                vertex_size,
                elements,
            } => {
                let els: Vec<String> = elements
                    .iter()
                    .map(|(off, dt, code)| format!("code={code:#x} type={dt} off={off}"))
                    .collect();
                out.push(format!(
                    "obj#{i} VDecl vsize={vertex_size} elems=[{}]",
                    els.join(", ")
                ));
            }
            Object::VBuf { f } => {
                out.push(format!(
                    "obj#{i} VBuf f={:?}",
                    f.iter().map(|v| format!("{v:#x}")).collect::<Vec<_>>()
                ));
            }
            Object::IBuf { f } => {
                out.push(format!(
                    "obj#{i} IBuf f={:?}",
                    f.iter().map(|v| format!("{v:#x}")).collect::<Vec<_>>()
                ));
            }
            Object::Mesh { f, refs } => {
                out.push(format!(
                    "obj#{i} Mesh f={:?} refs={:?}",
                    f.iter().map(|v| format!("{v:#x}")).collect::<Vec<_>>(),
                    refs.iter().map(|v| format!("{v:#x}")).collect::<Vec<_>>()
                ));
            }
            Object::Raw { bytes } => {
                out.push(format!(
                    "obj#{i} Raw {} bytes: {}",
                    bytes.len(),
                    hex16(bytes)
                ));
            }
            Object::Skeleton { bones } => {
                out.push(format!("obj#{i} Skeleton {} bones", bones.len()));
            }
            Object::AnimSkin { poses } => {
                out.push(format!("obj#{i} AnimSkin {} poses", poses.len()));
            }
            Object::KeyAnim(a) => {
                out.push(format!("obj#{i} KeyAnim {} channels", a.channels.len()));
            }
            Object::BlendShapes {
                vertex_count,
                shape_count,
                skin_indices,
                skin_weights,
                ..
            } => {
                out.push(format!(
                    "obj#{i} BlendShapes verts={vertex_count} shapes={shape_count} idx_pairs={} w_pairs={}",
                    skin_indices.len(),
                    skin_weights.len()
                ));
            }
            Object::Opaque { type_code } => {
                out.push(format!("obj#{i} Opaque tc={type_code:#x}"));
            }
            Object::BoneMounts(m) => {
                out.push(format!("obj#{i} BoneMounts {} mats", m.mats.len()));
            }
        }
    }
    out
}

/// Probe the trailing 4-byte vertex elements of the first static mesh as
/// potential skin data (weights + bone indices). Returns
/// (vert_count, stride, weight_offs, idx_offs, weights_u8x4, indices_u8,
/// max_index) when the trailing pair looks like normalized weights
/// (each vertex's 4 bytes summing to ~255) with all indices below a bound.
pub fn vertex_skin_probe(
    blob: &[u8],
    bound: usize,
) -> Option<(usize, usize, usize, usize, Vec<[u8; 4]>, Vec<[u8; 4]>)> {
    let objects = parse_objects(blob).ok()?;
    for obj in &objects {
        let Object::Mesh { f, refs } = obj else {
            continue;
        };
        if f[1] != PRIM_TRIANGLE_LIST || refs.is_empty() {
            continue;
        }
        let vbuf = resolve(&objects, refs[0])?;
        let Object::VBuf { f: vf } = vbuf else {
            continue;
        };
        let (stride, elements) = match resolve(&objects, vf[0])? {
            Object::VDesc {
                vertex_size,
                elements,
            } => (*vertex_size, elements),
            _ => continue,
        };
        let vdata = match resolve(&objects, vf[6]) {
            Some(Object::Raw { bytes }) => bytes,
            _ => continue,
        };
        let base = (vf[2] as usize).min(vdata.len() / stride.max(1));
        let count = (vf[3] as usize).min((vdata.len() - base * stride) / stride.max(1));
        // Trailing pair of 4-byte elements (RW decl types 5/8 are 4 bytes).
        let mut fours: Vec<usize> = elements
            .iter()
            .filter(|(_, t, _)| *t == 5 || *t == 8)
            .map(|(o, _, _)| *o as usize)
            .collect();
        fours.sort_unstable();
        if fours.len() < 2 {
            return None;
        }
        let w_off = fours[fours.len() - 2];
        let i_off = fours[fours.len() - 1];
        if i_off + 4 > stride || w_off + 4 > stride || count == 0 {
            return None;
        }
        let mut weights = Vec::with_capacity(count);
        let mut indices = Vec::with_capacity(count);
        let mut max_idx = 0usize;
        for i in 0..count {
            let o = (base + i) * stride;
            let w = [
                vdata[o + w_off],
                vdata[o + w_off + 1],
                vdata[o + w_off + 2],
                vdata[o + w_off + 3],
            ];
            let b = [
                vdata[o + i_off],
                vdata[o + i_off + 1],
                vdata[o + i_off + 2],
                vdata[o + i_off + 3],
            ];
            for v in b {
                max_idx = max_idx.max(v as usize);
            }
            weights.push(w);
            indices.push(b);
        }
        return Some((count, stride, w_off, i_off, weights, indices)).filter(|_| max_idx < bound);
    }
    None
}

/// Mirror map derived from bind-world positions: `map[i]` is the bone whose
/// bind position is the sagittal reflection of bone `i`'s ((-x, y, z)) within
/// `tol`. Bones on the midline (|x| < tol) map to themselves; unmatched bones
/// map to `None`. Pure.
pub fn mirror_map(worlds: &[Mat34], tol: f32) -> Vec<Option<usize>> {
    let pos: Vec<[f32; 3]> = worlds.iter().map(|w| [w[0][3], w[1][3], w[2][3]]).collect();
    let mut out = vec![None; worlds.len()];
    for (i, p) in pos.iter().enumerate() {
        if p[0].abs() < tol {
            // Midline bone (spine, tail, head): already symmetrical.
            out[i] = Some(i);
            continue;
        }
        let mut best: Option<(f32, usize)> = None;
        for (j, q) in pos.iter().enumerate() {
            if j == i {
                continue;
            }
            let d = ((q[0] + p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt();
            if d < tol {
                if best.map(|(bd, _)| d < bd).unwrap_or(true) {
                    best = Some((d, j));
                }
            }
        }
        out[i] = best.map(|(_, j)| j);
    }
    out
}

/// A renderable character with one explicitly linked SkinsInK rig. This does
/// not resolve multiple native skin groups or procedural assembled animation.
#[derive(Debug, Clone)]
pub struct RiggedModel {
    pub meshes: Vec<ExtractedMesh>,
    pub bones: Vec<Bone>,
    pub poses: Vec<BonePose>,
    pub skin_matrices: Vec<Mat34>,
    pub matrix_buffer_offset: u32,
    pub anims: Vec<KeyAnim>,
}

/// Follow the native skin record rather than picking equally-sized tables.
/// Multiple skin records require per-mesh associations and remain unsupported.
pub fn extract_rigged_model(blob: &[u8]) -> Option<RiggedModel> {
    let objects = parse_objects(blob).ok()?;
    let mut rigs = extract_linked_skin_rigs(blob).ok()?;
    if rigs.len() != 1 {
        return None;
    }
    let rig = rigs.pop()?;
    let mut anims: Vec<KeyAnim> = Vec::new();
    for obj in &objects {
        match obj {
            Object::KeyAnim(a) => anims.push(a.clone()),
            _ => {}
        }
    }
    let Object::BoneMounts(buffer) = objects.get(rig.source.matrix_section)? else {
        return None;
    };
    let mut stats = MeshStats::default();
    let meshes = extract_meshes(blob, &mut stats);
    if meshes.is_empty() {
        return None;
    }
    Some(RiggedModel {
        meshes,
        bones: rig.bones,
        poses: rig.poses,
        skin_matrices: rig.matrices,
        matrix_buffer_offset: buffer.name,
        anims,
    })
}

/// Layout lines for one RW4 blob (vet: section table + parsed objects).
pub fn extract_rigged_model_layout(blob: &[u8]) -> Vec<String> {
    dump_record_layout(blob)
}

fn hex16(b: &[u8]) -> String {
    b.iter().take(16).map(|x| format!("{x:02X}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vdesc_bytes(stride: u8, elems: &[(u16, u8, u32)]) -> Vec<u8> {
        let mut b = vec![0u8; 24];
        b[12] = elems.len() as u8;
        b[13] = 0;
        b[14] = 0;
        b[15] = stride;
        for (off, dtype, code) in elems {
            b.extend_from_slice(&0u16.to_le_bytes());
            b.extend_from_slice(&off.to_le_bytes());
            b.push(*dtype);
            b.push(0);
            b.push(0);
            b.push(0);
            b.extend_from_slice(&code.to_le_bytes());
        }
        b
    }

    /// Build a minimal in-memory object graph: mesh + vdesc + vbuf + ibuf + raws.
    fn tiny_graph() -> (Vec<Object>, usize) {
        // 2 triangles (quad), 4 verts, pos-only stride 12.
        let positions: Vec<f32> = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0];
        let mut vraw = Vec::new();
        for f in &positions {
            vraw.extend_from_slice(&f.to_le_bytes());
        }
        let iraw: Vec<u8> = vec![0, 0, 1, 0, 2, 0, 0, 0, 2, 0, 3, 0];
        let objects = vec![
            Object::Mesh {
                f: [0, 4, 3, 2, 1, 0, 6, 0, 4],
                refs: vec![2],
            },
            Object::VDesc {
                vertex_size: 12,
                elements: vec![(0, 2, 0)],
            },
            Object::VBuf {
                f: [1, 0, 0, 4, 0, 12, 4],
            },
            Object::IBuf {
                f: [0, 0, 6, 8, 101, 4, 5],
            },
            Object::Raw { bytes: vraw },
            Object::Raw { bytes: iraw },
        ];
        (objects, 0)
    }

    /// Assemble a complete synthetic MODEL blob exercising the full pipeline.
    fn tiny_blob() -> Vec<u8> {
        let mut mesh = vec![0u8; 0];
        for v in [0u32, 4, 3, 2, 1, 0, 6, 0, 4, 2] {
            mesh.extend_from_slice(&v.to_le_bytes());
        }
        let vdesc = vdesc_bytes(12, &[(0, 2, 0)]);
        let mut vbuf = vec![];
        for v in [1u32, 0, 0, 4, 0, 12, 4] {
            vbuf.extend_from_slice(&v.to_le_bytes());
        }
        let mut ibuf = vec![];
        for v in [0u32, 0, 6, 8, 101, 4, 5] {
            ibuf.extend_from_slice(&v.to_le_bytes());
        }
        let mut vraw = vec![];
        for f in [
            0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0,
        ] {
            vraw.extend_from_slice(&f.to_le_bytes());
        }
        let iraw: Vec<u8> = vec![0, 0, 1, 0, 2, 0, 0, 0, 2, 0, 3, 0];
        // Order: mesh, vdesc, vbuf, ibuf, rawV, rawI. Raw sections relocate by
        // pBufferData, so place raw data in a trailing buffer region.
        assemble(vec![
            (T_MESH, mesh),
            (T_VDESC, vdesc),
            (T_VBUF, vbuf),
            (T_IBUF, ibuf),
            (T_RAW, vraw),
            (T_RAW, iraw),
        ])
    }

    /// Assemble parts into a MODEL blob: 96B header, inline objects,
    /// section table, relocated raw buffer.
    fn assemble(parts: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
        let mut blob = vec![0u8; 96];
        blob[0..28].copy_from_slice(RW_MAGIC);
        blob[0x1C..0x20].copy_from_slice(&1u32.to_le_bytes());
        blob[0x24..0x28].copy_from_slice(&(parts.len() as u32).to_le_bytes());
        // Header body (96B) then inline objects, then table, then raw buffer.
        let mut off = 96usize;
        let mut inline: Vec<(usize, Vec<u8>)> = Vec::new();
        let mut raws: Vec<Vec<u8>> = Vec::new();
        for (tc, data) in &parts {
            if *tc == T_RAW {
                raws.push(data.clone());
            } else {
                inline.push((off, data.clone()));
                off += data.len();
            }
        }
        let table_at = off;
        let buf_base = table_at + parts.len() * 24;
        // Relocated raw offsets are relative to buf_base.
        let mut raw_off = 0usize;
        let mut table = vec![];
        let mut raw_idx = 0;
        let mut inline_idx = 0;
        for (tc, data) in &parts {
            let (p, sz) = if *tc == T_RAW {
                let p = raw_off;
                raw_off += data.len();
                raw_idx += 1;
                (p, data.len())
            } else {
                let (o, _) = &inline[inline_idx];
                inline_idx += 1;
                (*o, data.len())
            };
            let _ = (sz, raw_idx);
            table.extend_from_slice(&(p as u32).to_le_bytes());
            table.extend_from_slice(&0u32.to_le_bytes());
            table.extend_from_slice(&(data.len() as u32).to_le_bytes());
            table.extend_from_slice(&4u32.to_le_bytes());
            table.extend_from_slice(&0u32.to_le_bytes());
            table.extend_from_slice(&tc.to_le_bytes());
        }
        blob[0x30..0x34].copy_from_slice(&(table_at as u32).to_le_bytes());
        blob[0x44..0x48].copy_from_slice(&(buf_base as u32).to_le_bytes());
        for (o, data) in inline {
            if blob.len() < o + data.len() {
                blob.resize(o + data.len(), 0);
            }
            blob[o..o + data.len()].copy_from_slice(&data);
        }
        blob.extend_from_slice(&table);
        for r in raws {
            blob.extend(r);
        }
        blob
    }

    /// Assemble a skeleton (2 bones) + skin (2 poses) blob.
    fn rig_blob() -> Vec<u8> {
        // Skeleton: header [pFlags, pParents, pNames, count=2, id, count]
        // then inline arrays: names[2], flags[2], parents[2].
        // Section will live at offset 96; arrays follow the 24B header.
        let sec_off = 96usize;
        let (p_names, p_flags, p_parents) = (sec_off + 24, sec_off + 32, sec_off + 40);
        let mut skel = vec![];
        for v in [
            p_flags as u32,
            p_parents as u32,
            p_names as u32,
            2u32,
            0x1234u32,
            2u32,
        ] {
            skel.extend_from_slice(&v.to_le_bytes());
        }
        for v in [0xAAAABBBBu32, 0xCCCCDDDDu32] {
            skel.extend_from_slice(&v.to_le_bytes());
        }
        for v in [3u32, 7u32] {
            skel.extend_from_slice(&v.to_le_bytes());
        }
        skel.extend_from_slice(&(-1i32).to_le_bytes());
        skel.extend_from_slice(&0u32.to_le_bytes());
        // Skin: [dataOff, count=2, 0, 0] + 2x64B poses inline.
        // Pose 0: identity + invT (1,0,0). Pose 1: identity + invT (0,2,0).
        let skin_off = sec_off + skel.len();
        let mut skin = vec![];
        let data_off = (skin_off + 16) as u32;
        for v in [data_off, 2u32, 0u32, 0u32] {
            skin.extend_from_slice(&v.to_le_bytes());
        }
        for t in [[1.0f32, 0.0, 0.0], [0.0, 2.0, 0.0]] {
            for r in 0..3 {
                for c in 0..3 {
                    let v = if r == c { 1.0f32 } else { 0.0f32 };
                    skin.extend_from_slice(&v.to_le_bytes());
                }
                skin.extend_from_slice(&0f32.to_le_bytes());
            }
            for c in &t {
                skin.extend_from_slice(&c.to_le_bytes());
            }
            skin.extend_from_slice(&0u32.to_le_bytes());
        }
        assemble(vec![(T_SKELETON, skel), (T_ANIMSKIN, skin)])
    }

    #[test]
    fn decl_sizes() {
        assert_eq!(decl_size(2), Some(12));
        assert_eq!(decl_size(1), Some(8));
        assert_eq!(decl_size(17), None);
    }

    #[test]
    fn ref_tagging() {
        let (objs, _) = tiny_graph();
        assert!(resolve(&objs, 2).is_some());
        assert!(resolve(&objs, 0x400000).is_none());
        assert!(resolve(&objs, 99).is_none());
    }

    #[test]
    fn tiny_mesh_extracts() {
        // Exercise the extractor core via parse path is complex; instead
        // validate element logic + index math directly here.
        let b = vdesc_bytes(12, &[(0, 2, 0)]);
        assert_eq!(b.len(), 36);
        assert_eq!(b[12], 1);
        assert_eq!(b[15], 12);
    }

    #[test]
    fn synthetic_blob_end_to_end() {
        let blob = tiny_blob();
        let mut stats = MeshStats::default();
        let meshes = extract_meshes(&blob, &mut stats);
        assert_eq!(meshes.len(), 1);
        let m = &meshes[0];
        assert_eq!(m.positions.len(), 4);
        assert_eq!(m.indices, vec![0, 1, 2, 0, 2, 3]);
        assert!(m.valid());
        assert!((m.bbox_diagonal() - 2f32.sqrt()).abs() < 1e-5);
        assert_eq!(stats.static_ok, 1);
        assert_eq!(stats.tris, 2);
    }

    #[test]
    fn authored_material_links_preserve_mesh_and_state_indices() {
        let link = [0u32, 2, 1, 2]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let blob = assemble(vec![
            (T_MESH, vec![0; 36]),
            (T_COMPILED_STATE, vec![0; 36]),
            (T_COMPILED_STATE, vec![0; 36]),
            (T_MESH_MATERIAL_LINK, link),
        ]);
        assert_eq!(
            material_links(&blob).unwrap(),
            vec![MaterialLink {
                mesh_section: 0,
                state_sections: vec![1, 2]
            }]
        );
        let parts = sections(&blob).unwrap();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[1].1.len(), 36);
        let mut bad = blob.clone();
        let table = u32le(&bad, 0x30).unwrap() as usize;
        let at = u32le(&bad, table + 3 * 24).unwrap() as usize;
        bad[at..at + 4].copy_from_slice(&0x00400000u32.to_le_bytes());
        assert!(material_links(&bad).is_err());
        bad = blob.clone();
        bad[at + 8..at + 12].copy_from_slice(&0u32.to_le_bytes());
        assert!(material_links(&bad).is_err());
        bad = blob.clone();
        bad[table + 3 * 24 + 8..table + 3 * 24 + 12].copy_from_slice(&8u32.to_le_bytes());
        assert!(material_links(&bad).is_err());
        bad = blob.clone();
        bad[table..table + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(sections(&bad).is_err());
    }

    #[test]
    fn rejects_non_model() {
        let mut blob = tiny_blob();
        blob[0x1C] = 0x0D; // SPECIAL-ish
        blob[0x1D] = 0xD0;
        blob[0x1E] = 0xFE;
        blob[0x1F] = 0xCA;
        let mut stats = MeshStats::default();
        assert!(extract_meshes(&blob, &mut stats).is_empty());
    }

    #[test]
    fn bone_mounts_parse() {
        // 2 mounts: identity, then a pure translation (+1,+2,+3).
        let mut data = vec![];
        data.extend_from_slice(&0u32.to_le_bytes()); // absolute array offset, patched below
        data.extend_from_slice(&2u32.to_le_bytes()); // count
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        for tr in [[0.0f32; 3], [1.0, 2.0, 3.0]] {
            for r in 0..3 {
                for c in 0..4 {
                    let v = if c < 3 && r == c {
                        1.0
                    } else if c == 3 {
                        tr[r]
                    } else {
                        0.0
                    };
                    data.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        let mut blob = assemble(vec![(T_BONE_MOUNTS, data)]);
        let data_at = 96usize;
        blob[data_at..data_at + 4].copy_from_slice(&((data_at + 16) as u32).to_le_bytes());
        let objs = parse_objects(&blob).expect("parse");
        let mounts = objs
            .iter()
            .find_map(|o| match o {
                Object::BoneMounts(m) => Some(m.clone()),
                _ => None,
            })
            .expect("mounts");
        assert_eq!(mounts.count, 2);
        assert_eq!(mounts.mats.len(), 2);
        assert_eq!(mounts.mats[1][0][3], 1.0);
        assert_eq!(mounts.mats[1][1][3], 2.0);
        assert_eq!(mounts.mats[1][2][3], 3.0);
        assert_eq!(mounts.mats[0][0][0], 1.0);
        assert_eq!(
            mounts.name, 112,
            "legacy name field preserves the actual array offset"
        );

        // Move the matrix array away from its header. Inline decoding would
        // read the cleared bytes and lose the authored translation.
        let matrices = blob[112..208].to_vec();
        let relocated = blob.len();
        blob[112..208].fill(0);
        blob.extend_from_slice(&matrices);
        blob[data_at..data_at + 4].copy_from_slice(&(relocated as u32).to_le_bytes());
        let objects = parse_objects(&blob).unwrap();
        let Object::BoneMounts(buffer) = &objects[0] else {
            panic!("matrix buffer");
        };
        assert_eq!(buffer.mats[1][2][3], 3.);
        blob[data_at..data_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_objects(&blob).unwrap_err().contains("outside file"));
    }

    #[test]
    fn skin_links_choose_authored_sections_and_reject_wrong_or_tagged_refs() {
        let original = rig_blob();
        let mut parts = sections(&original)
            .unwrap()
            .into_iter()
            .map(|(t, b)| (t, b.to_vec()))
            .collect::<Vec<_>>();
        let skeleton = parts.iter().position(|p| p.0 == T_SKELETON).unwrap();
        let pose = parts.iter().position(|p| p.0 == T_ANIMSKIN).unwrap();
        let count = u32le(&parts[skeleton].1, 12).unwrap();
        // A same-sized decoy pose set must not produce a second candidate.
        let decoy = parts.len();
        parts.push((T_ANIMSKIN, parts[pose].1.clone()));
        let matrix = parts.len();
        let mut buffer = vec![0u8; 16];
        buffer[4..8].copy_from_slice(&count.to_le_bytes());
        for _ in 0..count {
            for row in mat34_identity() {
                for value in row {
                    buffer.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        parts.push((T_SKIN_MATRIX_BUFFER, buffer));
        let link_index = parts.len();
        let mut link = vec![0u8; 20];
        link[..4].copy_from_slice(&0x400000u32.to_le_bytes());
        for (at, index) in [(8, matrix), (12, skeleton), (16, pose)] {
            link[at..at + 4].copy_from_slice(&(index as u32).to_le_bytes());
        }
        parts.push((T_SKINS_IN_K, link));
        let mut blob = assemble(parts);
        let table = u32le(&blob, 48).unwrap() as usize;
        for index in [matrix, decoy] {
            let at = u32le(&blob, table + index * 24).unwrap() as usize;
            blob[at..at + 4].copy_from_slice(&((at + 16) as u32).to_le_bytes());
        }
        let rigs = extract_linked_skin_rigs(&blob).unwrap();
        assert_eq!(rigs.len(), 1);
        assert_eq!(rigs[0].source.pose_section, pose);
        assert_ne!(rigs[0].source.pose_section, decoy);
        assert_eq!(rigs[0].source.bone_count, count as usize);
        assert_eq!(rigs[0].bones.len(), rigs[0].matrices.len());
        let at = u32le(&blob, table + link_index * 24).unwrap() as usize;
        blob[at + 16..at + 20].copy_from_slice(&(skeleton as u32).to_le_bytes());
        assert!(extract_linked_skin_rigs(&blob)
            .unwrap_err()
            .contains("must be type"));
        blob[at + 16..at + 20].copy_from_slice(&(pose as u32).to_le_bytes());
        blob[at + 8..at + 12].copy_from_slice(&((matrix as u32) | 0x400000).to_le_bytes());
        assert!(extract_linked_skin_rigs(&blob)
            .unwrap_err()
            .contains("unsupported reference"));
    }

    #[test]
    #[ignore = "requires original Spore assets"]
    fn installed_gmdl_rig_links_follow_native_skin_records() {
        let install = crate::spore_install::discover().expect("original install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut keys = std::collections::HashSet::new();
        for group in [
            crate::baked_model::CREATURE_GROUP,
            crate::cell_visuals::CELL_MODEL_GROUP,
        ] {
            for key in crate::baked_model::candidates(&index, group) {
                let model = crate::baked_model::parse(&index.read(key).unwrap()).unwrap();
                keys.extend(
                    model
                        .references
                        .into_iter()
                        .filter(|k| k.type_id == crate::asset_catalog::RW4),
                );
            }
        }
        let mut keys = keys.into_iter().collect::<Vec<_>>();
        keys.sort_by_key(|k| (k.group_id, k.instance_id, k.type_id));
        let mut rows = Vec::new();
        let mut link_count = 0;
        for key in keys {
            let result = index
                .read(key)
                .and_then(|blob| extract_linked_skin_rigs(&blob));
            match result {
                Ok(links)=>{link_count+=links.len();rows.push(serde_json::json!({"source":crate::resource_audit::key_json(key),"links":links.iter().map(|l|&l.source).collect::<Vec<_>>() }));}
                Err(error)=>rows.push(serde_json::json!({"source":crate::resource_audit::key_json(key),"error":error})),
            }
        }
        std::fs::create_dir_all("target/native-animation-065").unwrap();
        std::fs::write("target/native-animation-065/rig-links.json",serde_json::to_vec_pretty(&serde_json::json!({"rig_resources":rows.len(),"linked_skin_records":link_count,"resources":rows})).unwrap()).unwrap();
        assert!(
            link_count > 0,
            "no native SkinsInK links found; inspect exact source diagnostics"
        );
        println!("Resolved {link_count} authored SkinsInK references");
    }

    #[test]
    fn rig_blob_end_to_end() {
        let blob = rig_blob();
        let mut stats = MeshStats::default();
        let (skels, skins) = extract_rig(&blob, &mut stats);
        assert_eq!(skels.len(), 1);
        assert_eq!(skins.len(), 1);
        assert_eq!(skels[0].len(), 2);
        assert_eq!(skels[0][0].parent, None);
        assert_eq!(skels[0][1].parent, Some(0));
        assert_eq!(skels[0][0].name, 0xAAAABBBB);
        assert_eq!(skins[0].len(), 2);
        assert_eq!(skins[0][0].local_offset(), [-1.0, -0.0, -0.0]);
        assert_eq!(skins[0][1].local_offset(), [-0.0, -2.0, -0.0]);
        assert_eq!(stats.skeletons, 1);
        assert_eq!(stats.bones, 2);
        assert_eq!(stats.bones_valid_parents, 2);
        assert_eq!(stats.poses, 2);
    }

    #[test]
    fn bind_chain_composes() {
        let bones = vec![
            Bone {
                name: 1,
                flags: 0,
                parent: None,
            },
            Bone {
                name: 2,
                flags: 0,
                parent: Some(0),
            },
        ];
        let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let poses = vec![
            BonePose {
                rotation: ident,
                inv_translation: [0.0, 0.0, 0.0],
            },
            BonePose {
                rotation: ident,
                inv_translation: [1.0, 0.0, 0.0],
            },
        ];
        let w = bind_world_positions(&bones, &poses);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0], [0.0, 0.0, 0.0]);
        assert_eq!(w[1], [-1.0, 0.0, 0.0]);
        // Mismatched lengths / forward refs are rejected, not panicked.
        assert!(bind_world_positions(&bones[..1], &poses).is_empty());
        let mut bad = bones.clone();
        bad[0].parent = Some(1);
        assert!(bind_world_positions(&bad, &poses).is_empty());
    }

    /// Blend-shape blob: mesh with no vertex buffer + morph pool (base pose).
    fn blend_blob() -> Vec<u8> {
        let mut mesh = vec![];
        // field_0, tri-list, ibuf=1, 1 tri, 0 vbufs, firstIdx=0, 3 idx, firstVert=0, 2 verts.
        for v in [0u32, 4, 1, 1, 0, 0, 3, 0, 2] {
            mesh.extend_from_slice(&v.to_le_bytes());
        }
        let mut ibuf = vec![];
        for v in [0u32, 0, 3, 8, 101, 4, 2] {
            ibuf.extend_from_slice(&v.to_le_bytes());
        }
        let iraw: Vec<u8> = vec![0, 0, 1, 0, 0, 0];
        // Blend pool: type=1, stream0 @64 (pos 32B + 1 delta 32B),
        // skin indices @128 (u16 pairs), skin weights @136 (float pairs).
        let mut blend = vec![];
        blend.extend_from_slice(&1u32.to_le_bytes());
        blend.extend_from_slice(&64u32.to_le_bytes());
        for _ in 0..8 {
            blend.extend_from_slice(&0u32.to_le_bytes());
        }
        blend.extend_from_slice(&128u32.to_le_bytes());
        blend.extend_from_slice(&136u32.to_le_bytes());
        for v in [1u32, 2u32, 0u32, 0u32] {
            blend.extend_from_slice(&v.to_le_bytes());
        }
        for f in [0.0f32, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0] {
            blend.extend_from_slice(&f.to_le_bytes());
        }
        // One morph delta set: shift vert0 by (+1,0,0), vert1 stays.
        for f in [1.0f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] {
            blend.extend_from_slice(&f.to_le_bytes());
        }
        // Skin: vert0 -> bones (0,1) w (0.5,0.5); vert1 -> bones (1,1) w (1,0).
        for v in [0u16, 1u16, 1u16, 1u16] {
            blend.extend_from_slice(&v.to_le_bytes());
        }
        for f in [0.5f32, 0.5, 1.0, 0.0] {
            blend.extend_from_slice(&f.to_le_bytes());
        }
        assemble(vec![
            (T_MESH, mesh),
            (T_IBUF, ibuf),
            (T_RAW, iraw),
            (T_BLENDSHAPES, blend),
        ])
    }

    #[test]
    fn quat_to_mat_yaw90() {
        // Yaw-90 quat (0, sin45, 0, cos45) maps +X to -Z.
        let s = std::f32::consts::FRAC_PI_4.sin();
        let m = quat_to_mat33([0.0, s, 0.0, s]);
        let x = [m[0][0], m[1][0], m[2][0]];
        assert!((x[0] - 0.0).abs() < 1e-5, "{x:?}");
        assert!((x[1] - 0.0).abs() < 1e-5, "{x:?}");
        assert!((x[2] + 1.0).abs() < 1e-5, "{x:?}");
    }

    #[test]
    fn mat34_roundtrips() {
        let m: Mat34 = [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 2.0],
            [0.0, 0.0, 1.0, 3.0],
        ];
        let inv = mat34_inverse_rigid(&m);
        let p = [4.0, 5.0, 6.0];
        let q = mat34_apply(&inv, &mat34_apply(&m, &p));
        for i in 0..3 {
            assert!((q[i] - p[i]).abs() < 1e-5);
        }
        // Composition: translate then translate.
        let t = mat34_mul(&m, &m);
        assert_eq!(t[0][3], 2.0);
        assert_eq!(t[1][3], 4.0);
    }

    #[test]
    fn skin_identity_reproduces_base() {
        let base = vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        let idx = vec![[0u16, 1u16], [1u16, 0u16]];
        let w = vec![[0.7f32, 0.3], [0.25, 0.75]];
        let inv = vec![
            inv_bind_from_pose(&BonePose {
                rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                inv_translation: [10.0, 0.0, 0.0],
            }),
            inv_bind_from_pose(&BonePose {
                rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                inv_translation: [0.0, 20.0, 0.0],
            }),
        ];
        let world: Vec<Mat34> = inv.iter().map(mat34_inverse_rigid).collect();
        let out = skin_positions(&base, &idx, &w, &world, &inv).unwrap();
        for (a, b) in out.iter().zip(base.iter()) {
            for i in 0..3 {
                assert!((a[i] - b[i]).abs() < 1e-4, "{a:?} vs {b:?}");
            }
        }
        // Unknown bones hold bind pose (identity fallback).
        let held = skin_positions(&base[..1], &[[0u16, 9u16]], &w[..1], &world, &inv).unwrap();
        assert!((held[0][0] - base[0][0]).abs() < 1e-4);
    }

    #[test]
    fn fk_bind_roundtrip_and_wave() {
        let bones = vec![
            Bone {
                name: 1,
                flags: 0,
                parent: None,
            },
            Bone {
                name: 2,
                flags: 0,
                parent: Some(0),
            },
        ];
        let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let poses = vec![
            BonePose {
                rotation: ident,
                inv_translation: [0.0, 0.0, 0.0],
            },
            BonePose {
                rotation: ident,
                inv_translation: [1.0, 0.0, 0.0],
            },
        ];
        let (locals, invs) = bind_locals(&bones, &poses).expect("bind");
        // Zero-extra FK reproduces the bind worlds.
        let worlds = fk_worlds(&[None, Some(0)], &locals).expect("fk");
        let base = vec![[2.0, 0.0, 0.0]];
        let idx = vec![[1u16, 1u16]];
        let w = vec![[1.0f32, 0.0]];
        let out = skin_positions(&base, &idx, &w, &worlds, &invs).unwrap();
        assert!((out[0][0] - 2.0).abs() < 1e-4, "{:?}", out[0]);
        // Waving bone 1 by 90° about Y moves its verts; bone 0 is untouched.
        let mut waved = locals.clone();
        waved[1] = mat34_mul(&waved[1], &rot_y(std::f32::consts::FRAC_PI_2));
        let worlds2 = fk_worlds(&[None, Some(0)], &waved).expect("fk2");
        let out2 = skin_positions(&base, &idx, &w, &worlds2, &invs).unwrap();
        assert!(
            (out2[0][0] - 2.0).abs() > 0.1 || (out2[0][2] - 0.0).abs() > 0.1,
            "{:?}",
            out2[0]
        );
    }

    /// Synthetic 0x70001 section: 1 LocRot channel, 2 keys (t=0 identity,
    /// t=1 yaw-180 + loc (1,0,0)), length 2.
    fn keyanim_blob() -> Vec<u8> {
        // Section will sit at offset 96; header 48B, names @144, info @148, keys @160.
        let (names_at, info_at, keys_rel) = (144u32, 148u32, 64u32);
        let mut sec = vec![];
        for v in [names_at, 1u32, 0xBEEFu32, 0, 0, 0, 1, 0] {
            sec.extend_from_slice(&v.to_le_bytes());
        }
        sec.extend_from_slice(&2.0f32.to_le_bytes());
        for v in [12u32, 0u32, info_at] {
            sec.extend_from_slice(&v.to_le_bytes());
        }
        sec.extend_from_slice(&0xDEADBEEFu32.to_le_bytes()); // channel id
        sec.extend_from_slice(&keys_rel.to_le_bytes());
        sec.extend_from_slice(&32u32.to_le_bytes());
        sec.extend_from_slice(&0x101u32.to_le_bytes());
        // k0: identity quat, origin, t=0.
        for f in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0] {
            sec.extend_from_slice(&f.to_le_bytes());
        }
        // k1: yaw-180 quat (0,1,0,0), loc (1,0,0), t=1.
        for f in [0.0f32, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0] {
            sec.extend_from_slice(&f.to_le_bytes());
        }
        assemble(vec![(T_KEYANIM, sec)])
    }

    #[test]
    fn keyanim_never_consumes_adjacent_sections_or_external_tables() {
        let original = keyanim_blob();
        let mut adjacent = vec![0u8; 64];
        adjacent[28..32].copy_from_slice(&2f32.to_le_bytes());
        adjacent[60..64].copy_from_slice(&3f32.to_le_bytes());
        let blob = assemble(vec![
            (T_KEYANIM, original[96..224].to_vec()),
            (0x80009, adjacent),
        ]);
        let clips = extract_anims(&blob, &mut MeshStats::default());
        assert_eq!(clips[0].channels[0].keys.len(), 2);
        let mut invalid = original;
        invalid[96..100].copy_from_slice(&224u32.to_le_bytes());
        assert!(parse_objects(&invalid).is_err());
    }

    #[test]
    fn keyanim_parses_and_samples() {
        let blob = keyanim_blob();
        let mut stats = MeshStats::default();
        let anims = extract_anims(&blob, &mut stats);
        assert_eq!(anims.len(), 1);
        assert_eq!(stats.keyanims, 1);
        assert_eq!(stats.channels, 1);
        assert_eq!(stats.keys, 2);
        let clamped = anims[0].sample_clamped(0, 2.).unwrap();
        assert_eq!(clamped.1, [1., 0., 0.]);
        assert_eq!(anims[0].sample(0, 2.).unwrap().1, [0.; 3]);
        assert!(anims[0].sample_clamped(0, f32::NAN).is_none());
        let a = &anims[0];
        assert_eq!(a.channels[0].id, 0xDEADBEEF);
        assert!((a.length - 2.0).abs() < 1e-6);
        let (q0, l0) = a.sample(0, 0.0).unwrap();
        assert_eq!(q0, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(l0, [0.0, 0.0, 0.0]);
        let (q1, l1) = a.sample(0, 1.0).unwrap();
        assert_eq!(q1, [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(l1, [1.0, 0.0, 0.0]);
        // Midpoint: slerp identity->yaw180 gives yaw-90; loc halves.
        let (qm, lm) = a.sample(0, 0.5).unwrap();
        assert!((qm[1] - 0.7071).abs() < 1e-3, "{qm:?}");
        assert!((qm[3] - 0.7071).abs() < 1e-3, "{qm:?}");
        assert_eq!(lm, [0.5, 0.0, 0.0]);
        // Loop wrap: t=2.5 -> 0.5.
        let (qw, _) = a.sample(0, 2.5).unwrap();
        assert!((qw[1] - qm[1]).abs() < 1e-5);
        assert!(a.sample(9, 0.0).is_none());
    }

    #[test]
    fn blend_mesh_extracts_base_pose() {
        let blob = blend_blob();
        let mut stats = MeshStats::default();
        let meshes = extract_meshes(&blob, &mut stats);
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].positions, vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
        assert_eq!(meshes[0].indices, vec![0, 1, 0]);
        assert!(meshes[0].valid());
        assert_eq!(stats.blend_ok, 1);
        assert_eq!(stats.static_ok, 0);
        // Morph application: weight 1 shifts vert0 by the delta.
        assert_eq!(meshes[0].morphs.len(), 1);
        let morphed = meshes[0].morphed_positions(&[1.0]);
        assert_eq!(morphed, vec![[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
        let neutral = meshes[0].morphed_positions(&[0.0]);
        assert_eq!(neutral, meshes[0].positions);
    }

    #[test]
    fn blend_skin_streams_parse() {
        let blob = blend_blob();
        let mut stats = MeshStats::default();
        // Parse objects directly to inspect the pool.
        let objects = parse_objects(&blob).expect("parse");
        let mut found = false;
        for obj in &objects {
            if let Object::BlendShapes {
                skin_indices,
                skin_weights,
                ..
            } = obj
            {
                assert_eq!(skin_indices, &vec![[0u16, 1u16], [1u16, 1u16]]);
                assert_eq!(skin_weights, &vec![[0.5f32, 0.5], [1.0, 0.0]]);
                found = true;
            }
        }
        assert!(found);
        let _ = stats;
    }
}

#[cfg(test)]
mod morph_metadata_tests {
    use super::*;
    fn record(parts: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
        let mut bytes = vec![0u8; 96];
        bytes[..28].copy_from_slice(RW_MAGIC);
        bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&(parts.len() as u32).to_le_bytes());
        let mut table = Vec::new();
        for (kind, data) in parts {
            let at = bytes.len() as u32;
            table.extend(
                [at, 0, data.len() as u32, 4, 0, kind]
                    .into_iter()
                    .flat_map(u32::to_le_bytes),
            );
            bytes.extend(data);
        }
        let table_at = bytes.len() as u32;
        bytes[48..52].copy_from_slice(&table_at.to_le_bytes());
        bytes.extend(table);
        bytes
    }
    fn handle() -> Vec<u8> {
        let mut h = vec![0; 64];
        h[..4].copy_from_slice(&123u32.to_le_bytes());
        h[8..16].copy_from_slice(&2.5f64.to_le_bytes());
        h[56..60].copy_from_slice(&0.33f32.to_le_bytes());
        h[60..64].copy_from_slice(&99u32.to_le_bytes());
        h
    }
    fn shapes(ids: &[u32]) -> Vec<u8> {
        let mut s = vec![0u8; 28 + ids.len() * 8];
        s[12..16].copy_from_slice(&(ids.len() as u32).to_le_bytes());
        s[20..24].copy_from_slice(&(ids.len() as u32).to_le_bytes());
        for (i, id) in ids.iter().enumerate() {
            let at = 28 + ids.len() * 4 + i * 4;
            s[at..at + 4].copy_from_slice(&id.to_le_bytes());
        }
        s
    }
    #[test]
    fn metadata_uses_section_offsets_and_preserves_full_ids() {
        let d = extract_morph_data(&record(vec![
            (T_MORPH_SHAPES, shapes(&[77, 88])),
            (T_MORPH_HANDLE, handle()),
        ]))
        .unwrap();
        assert_eq!(d.handles[0].id, 123);
        assert_eq!(d.handles[0].start[0], 2.5);
        assert_eq!(d.shapes[0].shape_ids, vec![77, 88]);
        assert_eq!(d.shapes[0].object_index, 0);
        assert_eq!(d.errors.len(), 1);
        assert!(d.handles[0].animation.is_none());
    }
    #[test]
    fn bounded_malformed_metadata_is_rejected() {
        assert!(extract_morph_data(&record(vec![(T_MORPH_HANDLE, vec![0; 63])])).is_err());
        let mut s = shapes(&[7]);
        s[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(extract_morph_data(&record(vec![(T_MORPH_SHAPES, s)])).is_err());
        let mut h = handle();
        h[8..16].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(extract_morph_data(&record(vec![(T_MORPH_HANDLE, h)])).is_err());
        assert!(
            extract_morph_data(&record(vec![(T_MORPH_SHAPES, shapes(&[7, 7]))]))
                .unwrap()
                .errors
                .len()
                > 0
        );
    }
}
