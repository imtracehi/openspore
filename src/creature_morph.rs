//! Morph (deform) channels of the creature clips on the part rigs, composed
//! with the parts' deform handles the retail way (`notes/creature-morph-notes.md`;
//! the Cell parts use the older weighted average of `cell_anim::PartRig`).
//!
//! ORIGINAL (code, march2017 addresses):
//! * a clip channel's deform components accumulate per block as (id, flags,
//!   value x weight, -, weight) records (`AddDeform` 0x99C3B0, cyclic
//!   components (flag 0x40) as an angle vector through 0x99C450; component
//!   flag 0x80 marks a record the model update skips) in the anim block's list
//!   (+0xA0 count, at most 8, +0xA4 records);
//! * every frame `FUN_009C5BD0` hands a block's list to its part model: phase
//!   = sum(v w) / (sum(w) + 1e-8) (cyclic: atan2 / 2 pi, +1 below 0), weight =
//!   the record's weight over the block's total, then `MoveToTime(id, start +
//!   (end - start) x phase)` and `SetWeight(id, weight)` (IModelWorld +0x74 /
//!   +0x78);
//! * once per model `FUN_009C6EF0` starts every deform of the creature data's
//!   rigblock (`AnimAction` 3, then `MoveToTime(value)`, `SetWeight(weight)`);
//!   a slot keeps its time and weight until set again;
//! * the part skeleton's multiblender (`kFlagMultiblenderEnabled`, evaluated
//!   by 0x75AAB0) is a running weighted average of the slots' local bone poses
//!   (quaternion, translation and scale components lerped by w / sum so far),
//!   blended with a zero pose when the weights sum below 1; the keyframe
//!   player's time wraps by the clip length past either end (0x11FE1B0);
//! * a baked creature (GMDL) carries per keyed binding one 160-byte record per
//!   binding-rig bone: the creature's handle pose minus the rig's bind pose,
//!   translation at +0 and rotation quaternion (x, y, z, w) at +0x10 (checked
//!   against the handle KeyAnims of two installed creatures, e.g. 07BBABB4's
//!   Toucan't bones 2A0C9847 / 043A0A88 and 07EDB5D6's D0BE09E3), the parent
//!   bone's handle scale at +0x2C; the binding rigs (group 0x41606100) are per
//!   part and carry the deform clips but no handle anims, so the handles reach
//!   the bones as these deltas on top of the blended clip pose.
//! INFERRED: the part skeleton evaluates with the parent's scale compensated
//! (the records store the parent's scale; Maya joint rule; `morph.rs`).
//! ADAPTED (marked inline): composition order of the deltas, the own-scale
//! field of the records, a zero-weight blend shows the bind pose, the player's
//! own (unbaked) creature computes its deltas from the part model's handle
//! KeyAnims at its handle weights (what the bake writes) and starts with no
//! creature-data rest entries.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::math::{Affine3A, Mat3, Quat, Vec3, Vec4};

use crate::cell_anim::{Bone3, PartRig};

/// One deform record of a block for a frame: (id, phase, summed weight).
pub type Track = (u32, f32, f32);

/// Part morphs (blinks, mouths, hands, feet, ears) run by default;
/// `OPENSPORE_CREATURE_MORPH=0` turns them off.
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OPENSPORE_CREATURE_MORPH").map_or(true, |v| v != "0"))
}

/// The player's own creature's parts morph too (same switch: on by default,
/// `OPENSPORE_CREATURE_MORPH=0` off). The starter's parts are donors cut from
/// its baked model and pose on its binding rigs exactly as the installed
/// starter (close-ups: stage, Test Drive, cinematic; CHANGES.md).
pub fn player_parts_enabled() -> bool {
    enabled()
}

/// `OPENSPORE_MORPH_DEBUG`: log the avatar parts' records and slots.
pub fn debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OPENSPORE_MORPH_DEBUG").is_some())
}

fn debug_phase() -> Option<f32> {
    static ON: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OPENSPORE_MORPH_PHASE").ok().and_then(|v| v.parse().ok()))
}

/// `DAT_013EC494`: the epsilon of `FUN_009C5BD0`'s phase division.
pub const PHASE_EPSILON: f32 = 1e-8;
/// `DAT_014472A0` = 1 / (2 pi).
const INV_TAU: f32 = 0.159_154_94;

/// The phase of an accumulated record: sum(v w) / (sum(w) + 1e-8).
pub fn phase_of(sum_vw: f32, sum_w: f32) -> f32 {
    sum_vw / (sum_w + PHASE_EPSILON)
}

/// The phase of an accumulated cyclic record (x = sum(cos 2 pi v) w, y =
/// sum(sin 2 pi v) w): atan2(y, x) / 2 pi, plus 1 below 0.
pub fn cyclic_phase(x: f32, y: f32) -> f32 {
    let p = y.atan2(x) * INV_TAU;
    if p < 0.0 {
        p + 1.0
    } else {
        p
    }
}

/// The keyframe player's time: wrapped by the length past either end
/// (0x11FE1B0 advance / 0x11FE2F0 rewind); the end itself stays.
pub fn wrap_time(t: f32, length: f32) -> f32 {
    if !t.is_finite() || !(length > 0.0) {
        return 0.0;
    }
    if t > length {
        let r = t - (t / length).floor() * length;
        if r <= 0.0 {
            length
        } else {
            r
        }
    } else if t < 0.0 {
        let r = t - (t / length).floor() * length;
        if r >= length {
            0.0
        } else {
            r
        }
    } else {
        t
    }
}

fn quat_xyzw(q: [f32; 4]) -> Quat {
    let q = Quat::from_xyzw(q[0], q[1], q[2], q[3]);
    if q.is_finite() && q.length_squared() > 1e-12 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

// ---------------------------------------------------------------------------
// Slots (FUN_009C6EF0 setup, FUN_009C5BD0 per frame)
// ---------------------------------------------------------------------------

/// One multiblender slot of a part skeleton.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub id: u32,
    pub phase: f32,
    pub weight: f32,
}

/// The slots of one part, in the order they were started.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slots(pub Vec<Slot>);

impl Slots {
    fn set(&mut self, id: u32, phase: f32, weight: f32) {
        match self.0.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                s.phase = phase;
                s.weight = weight;
            }
            None => self.0.push(Slot { id, phase, weight }),
        }
    }

    /// `FUN_009C6EF0`: start every creature-data entry (id, value, weight)
    /// the rig has a clip for, in order (a later duplicate id wins).
    pub fn setup(rig: &PartRig, entries: &[Track]) -> Self {
        let mut s = Self::default();
        for &(id, value, weight) in entries {
            if rig.has_deform(id) && value.is_finite() && weight.is_finite() {
                s.set(id, value, weight);
            }
        }
        s
    }

    /// `FUN_009C5BD0`: this frame's records of the block; weights are
    /// normalized by the block's total (ids the rig lacks still count).
    pub fn update(&mut self, rig: &PartRig, tracks: &[Track]) {
        let total: f32 = tracks.iter().filter(|t| t.2.is_finite()).map(|t| t.2).sum();
        for &(id, phase, weight) in tracks {
            if !rig.has_deform(id) || !phase.is_finite() || !weight.is_finite() {
                continue;
            }
            self.set(id, phase, if total > 0.0 { weight / total } else { 0.0 });
        }
    }
}

/// A slot clip's local pose of `bone` at `phase` (the bind pose where the
/// clip has no channel for it).
fn slot_pose(rig: &PartRig, id: u32, phase: f32, bone: usize) -> Option<Bone3> {
    let (_, clip) = rig.clips.iter().find(|(d, _)| *d == id)?;
    let t = wrap_time(phase * clip.anim.length, clip.anim.length);
    Some(match clip.channel_for_bone.get(bone).copied().flatten().and_then(|c| clip.anim.sample_frame_clamped(c, t)) {
        Some(k) => Bone3 { rotation: quat_xyzw(k.rotation), translation: Vec3::from_array(k.location), scale: Vec3::from_array(k.scale) },
        None => rig.bind_local[bone],
    })
}

/// The multiblender (0x75AAB0): local poses of the rig's bones from its
/// slots; false (and the bind pose) when no slot has weight.
pub fn blend_locals(rig: &PartRig, slots: &Slots, out: &mut Vec<Bone3>) -> bool {
    out.clear();
    out.extend_from_slice(&rig.bind_local);
    let active: Vec<&Slot> = slots.0.iter().filter(|s| s.weight > 0.0 && rig.has_deform(s.id)).collect();
    if active.is_empty() {
        // ADAPTED: retail clears the pose with nothing playing; the part
        // shows its bind pose instead.
        return false;
    }
    let total: f32 = active.iter().map(|s| s.weight).sum();
    for (bone, slot_out) in out.iter_mut().enumerate() {
        let (mut q, mut t, mut s, mut acc, rest);
        if total >= 1.0 {
            let first = slot_pose(rig, active[0].id, active[0].phase, bone).unwrap_or(rig.bind_local[bone]);
            q = Vec4::from(first.rotation);
            t = first.translation;
            s = first.scale;
            acc = active[0].weight;
            rest = &active[1..];
        } else {
            // Below a total of 1 the rest of the weight is a zero pose.
            q = Vec4::ZERO;
            t = Vec3::ZERO;
            s = Vec3::ZERO;
            acc = 1.0 - total;
            rest = &active[..];
        }
        for slot in rest {
            let p = slot_pose(rig, slot.id, slot.phase, bone).unwrap_or(rig.bind_local[bone]);
            acc += slot.weight;
            let f = slot.weight / acc;
            q += (Vec4::from(p.rotation) - q) * f;
            t += (p.translation - t) * f;
            s += (p.scale - s) * f;
        }
        // ADAPTED: the blended quaternion is normalized (the matrix
        // conversion is not traced).
        let rotation = if q.length_squared() > 1e-12 { Quat::from_vec4(q).normalize() } else { Quat::IDENTITY };
        *slot_out = Bone3 { rotation, translation: t, scale: s };
    }
    true
}

// ---------------------------------------------------------------------------
// Handle deltas (the GMDL baked records)
// ---------------------------------------------------------------------------

/// A bone's handle delta: the handle pose minus the bind pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Delta {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Delta {
    pub const IDENTITY: Self = Self { translation: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE };

    pub fn is_identity(&self) -> bool {
        self.translation.length_squared() < 1e-14 && self.rotation.angle_between(Quat::IDENTITY) < 1e-6 && (self.scale - Vec3::ONE).abs().max_element() < 1e-6
    }

    /// ADAPTED order: translation added and rotation applied in the parent's
    /// frame, scale multiplied (the bake's evaluation is not traced; at the
    /// bind pose this gives the handle pose exactly).
    pub fn apply(&self, b: Bone3) -> Bone3 {
        Bone3 { translation: b.translation + self.translation, rotation: (self.rotation * b.rotation).normalize(), scale: b.scale * self.scale }
    }

    /// The delta that turns `bind` into `pose`.
    pub fn between(bind: Bone3, pose: Bone3) -> Self {
        let safe = |v: f32| if v.abs() > 1e-6 { v } else { 1.0 };
        Self {
            translation: pose.translation - bind.translation,
            rotation: (pose.rotation * bind.rotation.inverse()).normalize(),
            scale: pose.scale / Vec3::new(safe(bind.scale.x), safe(bind.scale.y), safe(bind.scale.z)),
        }
    }

    /// Accumulate another handle's delta on the same bone (ADAPTED: sums;
    /// a multi-handle bake is not checked).
    pub fn then(&self, other: &Self) -> Self {
        Self { translation: self.translation + other.translation, rotation: (other.rotation * self.rotation).normalize(), scale: self.scale * other.scale }
    }
}

/// Decode one 160-byte baked record: translation (+0), rotation (+0x10,
/// x y z w). ADAPTED: the vec3 at +0x20 is read as the bone's own scale (all
/// installed records seen hold 1 there); +0x2C (the parent's scale) and the
/// second half are not used.
pub fn decode_record(rec: &[u8; 160]) -> Delta {
    let f = |i: usize| f32::from_le_bytes(rec[i * 4..i * 4 + 4].try_into().unwrap());
    let finite = |v: Vec3, d: Vec3| if v.is_finite() { v } else { d };
    let translation = finite(Vec3::new(f(0), f(1), f(2)), Vec3::ZERO);
    let rotation = quat_xyzw([f(4), f(5), f(6), f(7)]);
    let scale = Vec3::new(f(8), f(9), f(10));
    let scale = if scale.is_finite() && scale.min_element() > 1e-4 { scale } else { Vec3::ONE };
    Delta { translation, rotation, scale }
}

/// A part model's skeletal deform handle (`RWMorphHandle` with a bone clip).
#[derive(Clone, Debug)]
pub struct HandleAnim {
    pub id: u32,
    pub default: f32,
    pub anim: crate::rw4::KeyAnim,
    pub channel_for_bone: Vec<Option<usize>>,
}

/// The skeletal handles of a part model whose clips key `rig`'s bones
/// (blend-shape handles stay vertex shapes, `morph.rs`).
pub fn skeletal_handles(rig: &PartRig, blob: &[u8]) -> Vec<HandleAnim> {
    let Ok(data) = crate::rw4::extract_morph_data(blob) else { return Vec::new() };
    data.handles
        .iter()
        .filter_map(|h| {
            let anim = h.animation.as_ref()?;
            if anim.channels.is_empty() || anim.channels.iter().any(|c| c.keys.iter().any(|k| k.blend)) || !(anim.length > 0.0) {
                return None;
            }
            let channel_for_bone: Vec<Option<usize>> = rig.names.iter().map(|n| anim.channels.iter().position(|c| c.id == *n)).collect();
            channel_for_bone.iter().any(|c| c.is_some()).then(|| HandleAnim { id: h.id, default: h.default_time.clamp(0.0, 1.0), anim: anim.clone(), channel_for_bone })
        })
        .collect()
}

/// Per-bone deltas of the skeletal handles at the creature's weights (a
/// handle not listed keeps its default): what the bake writes into the
/// records, handle pose minus bind pose.
pub fn handle_deltas(rig: &PartRig, handles: &[HandleAnim], weights: &[(u32, f32)]) -> Vec<Delta> {
    let mut out = vec![Delta::IDENTITY; rig.bone_count()];
    for h in handles {
        let value = weights.iter().find(|w| w.0 == h.id).map_or(h.default, |w| w.1.clamp(0.0, 1.0));
        let t = value * h.anim.length;
        for (bone, channel) in h.channel_for_bone.iter().enumerate() {
            let Some(k) = channel.and_then(|c| h.anim.sample_frame_clamped(c, t)) else { continue };
            let pose = Bone3 { rotation: quat_xyzw(k.rotation), translation: Vec3::from_array(k.location), scale: Vec3::from_array(k.scale) };
            if !pose.translation.is_finite() || !pose.scale.is_finite() {
                continue;
            }
            out[bone] = out[bone].then(&Delta::between(rig.bind_local[bone], pose));
        }
    }
    out
}

/// World matrices of local poses with the parent's scale compensated: a
/// child's translation is scaled by its parent, its own frame is not
/// (INFERRED, see the module notes).
pub fn worlds(rig: &PartRig, locals: &[Bone3], out: &mut Vec<Affine3A>) {
    out.clear();
    for (bone, parent) in rig.parents.iter().enumerate() {
        let l = locals.get(bone).copied().unwrap_or(rig.bind_local[bone]);
        let local = Mat3::from_quat(l.rotation) * Mat3::from_diagonal(l.scale);
        let world = match parent.and_then(|p| out.get(p).copied().map(|w| (p, w))) {
            Some((p, pw)) => {
                let ps = locals.get(p).map_or(Vec3::ONE, |b| b.scale);
                let inv = Vec3::new(1.0 / nonzero(ps.x), 1.0 / nonzero(ps.y), 1.0 / nonzero(ps.z));
                let basis = Mat3::from(pw.matrix3) * Mat3::from_diagonal(inv) * local;
                Affine3A::from_mat3_translation(basis, pw.transform_point3(l.translation))
            }
            None => Affine3A::from_mat3_translation(local, l.translation),
        };
        out.push(world);
    }
}

fn nonzero(v: f32) -> f32 {
    if v.abs() > 1e-6 {
        v
    } else {
        1.0
    }
}

/// Rig-space skin matrices (`world * inverse_rest`) of a part for its slots
/// and handle deltas; false (identities) when nothing moves the part.
pub fn part_skin(rig: &PartRig, slots: &Slots, deltas: &[Delta], inverse_rest: &[Affine3A], out: &mut Vec<Affine3A>) -> bool {
    let n = rig.bone_count();
    let mut locals = Vec::with_capacity(n);
    let blended = blend_locals(rig, slots, &mut locals);
    let handled = deltas.iter().any(|d| !d.is_identity());
    out.clear();
    if !blended && !handled {
        out.resize(n, Affine3A::IDENTITY);
        return false;
    }
    for (bone, l) in locals.iter_mut().enumerate() {
        if let Some(d) = deltas.get(bone) {
            *l = d.apply(*l);
        }
    }
    let mut w = Vec::with_capacity(n);
    worlds(rig, &locals, &mut w);
    out.extend((0..n).map(|b| w[b] * inverse_rest.get(b).copied().unwrap_or(rig.inverse_bind[b])));
    true
}

/// Inverse rest worlds of a baked binding (bind pose plus its handle deltas):
/// the baked mesh already carries the handles.
pub fn inverse_rest(rig: &PartRig, deltas: &[Delta]) -> Vec<Affine3A> {
    if deltas.iter().all(|d| d.is_identity()) {
        return rig.inverse_bind.clone();
    }
    let locals: Vec<Bone3> = (0..rig.bone_count()).map(|b| deltas.get(b).map_or(rig.bind_local[b], |d| d.apply(rig.bind_local[b]))).collect();
    let mut w = Vec::new();
    worlds(rig, &locals, &mut w);
    w.iter().map(|m| m.inverse()).collect()
}

// ---------------------------------------------------------------------------
// Installed (baked) creatures: part rigs whose bones are skin joints
// ---------------------------------------------------------------------------

/// A part rig whose bones are joints of a skinned model.
#[derive(Clone, Debug)]
pub struct MorphGroup {
    /// The GMDL binding (anim data index) the part rig comes from.
    pub binding: usize,
    /// The clip-rig block the part rides (its deform tracks).
    pub block: usize,
    /// First joint of the part's bones.
    pub first_joint: usize,
    pub rig: Arc<PartRig>,
    /// Rig space -> model space and back.
    pub to_model: Affine3A,
    pub from_model: Affine3A,
    /// The rigblock's creature-data morph entries (slot setup).
    pub rest: Vec<Track>,
    /// The binding's baked handle deltas per rig bone.
    pub deltas: Vec<Delta>,
    /// Inverse rest worlds (bind pose plus deltas).
    pub inverse_rest: Vec<Affine3A>,
}

impl MorphGroup {
    pub fn new(binding: usize, block: usize, first_joint: usize, rig: Arc<PartRig>, to_model: Affine3A, from_model: Affine3A) -> Self {
        let n = rig.bone_count();
        let inverse_rest = rig.inverse_bind.clone();
        Self { binding, block, first_joint, rig, to_model, from_model, rest: Vec::new(), deltas: vec![Delta::IDENTITY; n], inverse_rest }
    }
}

/// The morph joints of one skinned model.
#[derive(Clone, Debug, Default)]
pub struct MorphSet {
    pub groups: Vec<MorphGroup>,
}

/// Per-creature slot state of a `MorphSet` (the slots persist across frames).
#[derive(bevy::prelude::Component, Clone, Debug, Default)]
pub struct MorphState {
    pub slots: Vec<Slots>,
}

impl MorphSet {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn joints(&self) -> usize {
        self.groups.iter().map(|g| g.rig.bone_count()).sum()
    }

    /// Model-space matrices (joint index, matrix) of every morph joint for the
    /// blocks' posed matrices and their deform records.
    pub fn pose(&self, blocks: &[Affine3A], deforms: &[Vec<Track>], state: &mut MorphState, out: &mut Vec<(usize, Affine3A)>) {
        out.clear();
        if state.slots.len() != self.groups.len() {
            state.slots = self.groups.iter().map(|g| Slots::setup(&g.rig, &g.rest)).collect();
        }
        let mut skin = Vec::new();
        for (g, slots) in self.groups.iter().zip(state.slots.iter_mut()) {
            let m = blocks.get(g.block).copied().unwrap_or(Affine3A::IDENTITY);
            if let Some(tracks) = deforms.get(g.block) {
                slots.update(&g.rig, tracks);
            }
            if let Some(forced) = debug_phase() {
                for s in &mut slots.0 {
                    s.phase = forced;
                }
            }
            let moved = part_skin(&g.rig, slots, &g.deltas, &g.inverse_rest, &mut skin);
            for b in 0..g.rig.bone_count() {
                out.push((g.first_joint + b, if moved { m * g.to_model * skin[b] * g.from_model } else { m }));
            }
        }
    }

    /// Give the groups their creature's rest entries and baked handle deltas.
    pub fn attach_baked(&mut self, model: &crate::baked_model::Model, anatomy: &crate::creature_data::CreatureData) {
        for g in &mut self.groups {
            g.rest = rest_entries(anatomy, g.block);
            if let Some(records) = model.anim_data.get(g.binding).and_then(|d| (d.baked_deforms.len() == 1).then(|| &d.baked_deforms[0])) {
                if records.len() == g.rig.bone_count() {
                    g.deltas = records.iter().map(decode_record).collect();
                    g.inverse_rest = inverse_rest(&g.rig, &g.deltas);
                }
            }
        }
    }
}

/// A rigblock's creature-data morph entries (start i16 at +0x0E, count byte
/// at +0x11 of the 0x8C-byte record): (id, value, weight).
pub fn rest_entries(anatomy: &crate::creature_data::CreatureData, block: usize) -> Vec<Track> {
    let Some(b) = anatomy.rigblocks.get(block) else { return Vec::new() };
    let Ok(start) = usize::try_from(b.morph_index()) else { return Vec::new() };
    let count = b.raw_bytes.get(0x11).copied().unwrap_or(0) as usize;
    (start..start + count)
        .filter_map(|i| Some((*anatomy.animation_ids.get(i)?, f32::from_bits(*anatomy.animation_value_bits.get(i)?), f32::from_bits(*anatomy.animation_weight_bits.get(i)?))))
        .collect()
}

/// Part rigs by RW4 key (decoded once).
pub type RigCache = HashMap<crate::dbpf::ResourceKey, Option<Arc<PartRig>>>;

pub fn part_rig(cache: &mut RigCache, index: &crate::asset_catalog::ResourceIndex, key: crate::dbpf::ResourceKey) -> Option<Arc<PartRig>> {
    cache
        .entry(key)
        .or_insert_with(|| index.read(key).and_then(|b| PartRig::from_rw4(&b)).map(Arc::new).ok())
        .clone()
}

/// The keyed GMDL bindings whose part rig decodes with the binding's bone
/// count and has deform clips: (binding index, rig, rig -> model, model -> rig).
pub fn rigged_bindings(index: &crate::asset_catalog::ResourceIndex, model: &crate::baked_model::Model, cache: &mut RigCache) -> Vec<(usize, Arc<PartRig>, Affine3A, Affine3A)> {
    let mut out = Vec::new();
    for (i, data) in model.anim_data.iter().enumerate().skip(1) {
        let no_rig = data.key.group_id == u32::MAX && data.key.instance_id == u32::MAX && data.key.type_id == u32::MAX;
        if no_rig || data.baked_deforms.len() != 1 {
            continue;
        }
        let Some(rig) = part_rig(cache, index, data.key) else { continue };
        if rig.bone_count() != data.baked_deforms[0].len() || rig.clips.is_empty() {
            continue;
        }
        let Some(primary) = crate::cell_anim::native_transform(data.transform) else { continue };
        out.push((i, rig, primary.inverse(), primary));
    }
    out
}

// ---------------------------------------------------------------------------
// The player's custom creature: its parts are separate meshes (`Accessory`).
// ---------------------------------------------------------------------------

use bevy::{
    camera::visibility::DynamicSkinnedMeshBounds,
    mesh::{
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
        VertexAttributeValues,
    },
    prelude::*,
};

/// A player part skinned on its own rig (joints are its children).
#[derive(Component)]
pub struct PartMorph {
    /// The mesh it was built from (to put back) and the skinned copy.
    pub source: Handle<Mesh>,
    pub skinned: Handle<Mesh>,
    pub joints: Vec<Entity>,
    pub rig: Arc<PartRig>,
    /// Rig space -> the part mesh's space.
    pub mesh_from_rig: Affine3A,
    pub handles: Arc<Vec<HandleAnim>>,
    pub slots: Slots,
    /// The handle weights the deltas were made for, and the deltas.
    pub weights: Vec<(u32, f32)>,
    pub deltas: Vec<Delta>,
    /// Inverse rest worlds (the bind pose, or a baked donor's bind + deltas).
    pub inverse_rest: Vec<Affine3A>,
    /// A donor cut from the baked starter: its deltas are the GMDL records.
    pub baked: bool,
}

/// A part that cannot be skinned (no rig, rigid pieces): not tried again.
#[derive(Component)]
pub struct PartRigid(pub AssetId<Mesh>);

/// A built skinned player part.
#[derive(Clone)]
pub struct BuiltPart {
    pub mesh: Handle<Mesh>,
    pub rig: Arc<PartRig>,
    pub mesh_from_rig: Affine3A,
    pub handles: Arc<Vec<HandleAnim>>,
    /// A baked donor: (record deltas, inverse rest worlds, creature-data rest entries).
    pub baked: Option<(Vec<Delta>, Vec<Affine3A>, Vec<Track>)>,
}

#[derive(Resource, Default)]
pub struct PartMorphCache {
    rigs: RigCache,
    /// (definition instance, quantized blend-shape handle weights) -> part.
    meshes: HashMap<(u32, Vec<(u32, u16)>), Option<BuiltPart>>,
    bindposes: HashMap<usize, Handle<SkinnedMeshInverseBindposes>>,
    /// Definition instance -> its model has blend shapes.
    shapes: HashMap<u32, bool>,
}

/// Joint palette of a part model: every imported piece in the order of
/// `editor::assembled_mesh`, skin bytes / 3 (static buffers) or the blend
/// pool's u16 pairs / 3 (as `cell_anim::build_part_mesh`).
fn part_skin_attributes(blob: &[u8], bones: usize) -> Option<(Vec<[u16; 4]>, Vec<[f32; 4]>)> {
    let mut stats = crate::rw4::MeshStats::default();
    let pieces = crate::rw4::extract_meshes(blob, &mut stats);
    let (mut joints, mut weights) = (Vec::new(), Vec::new());
    let mut skinned_any = false;
    for p in &pieces {
        let n = p.positions.len();
        if p.bone_indices.len() == n && p.bone_weights.len() == n {
            skinned_any = true;
            for (b, w) in p.bone_indices.iter().zip(&p.bone_weights) {
                let total: f32 = w.iter().sum();
                joints.push(b.map(|x| x as u16 / 3));
                weights.push(if total > 0.0 { w.map(|x| x / total) } else { [1.0, 0.0, 0.0, 0.0] });
            }
        } else if p.skin_idx.len() == n && p.skin_w.len() == n {
            skinned_any = true;
            for (b, w) in p.skin_idx.iter().zip(&p.skin_w) {
                let total = w[0] + w[1];
                joints.push([b[0] / 3, b[1] / 3, 0, 0]);
                weights.push(if total > 0.0 { [w[0] / total, w[1] / total, 0.0, 0.0] } else { [1.0, 0.0, 0.0, 0.0] });
            }
        } else {
            // ADAPTED: a rigid piece of a rigged part rides its root bone.
            joints.extend(std::iter::repeat_n([0u16; 4], n));
            weights.extend(std::iter::repeat_n([1.0, 0.0, 0.0, 0.0], n));
        }
    }
    (skinned_any && joints.iter().flatten().all(|&j| (j as usize) < bones)).then_some((joints, weights))
}

/// The quantized blend-shape handle weights of a part (its skeletal handles
/// ride the bones instead), as `creature_handles` quantizes them.
fn shape_weights(part: &crate::game::PartInstance, shapes: bool) -> Vec<(u32, u16)> {
    if !shapes {
        return Vec::new();
    }
    let mut q: Vec<(u32, u16)> = part.edits.handles.iter().map(|(id, w)| (*id, (w.clamp(0.0, 1.0) * 256.0).round() as u16)).collect();
    q.sort_unstable();
    q
}

/// The part definition of a mounted player part.
fn part_definition<'a>(originals: &'a crate::Originals, part: &crate::game::PartInstance) -> Option<&'a crate::asset_catalog::PartDefinition> {
    let model = crate::editor::model_for_part(originals, part)?;
    model
        .source_definition
        .and_then(|k| originals.part_catalog.parts.iter().find(|p| p.key == k))
        .or_else(|| originals.part_catalog.by_model_in(crate::asset_catalog::CREATURE_PARTS, model.instance))
}

/// The model a player part was spawned with (`creature::spawn_creature_parts`
/// takes the first original model of its instance) and the definition its
/// mesh was assembled with (`PartDefinition::raw_model` for the startup
/// imports: no definition rotation or offset).
fn spawned_model<'a>(originals: &'a crate::Originals, part: &crate::game::PartInstance, catalog: &crate::asset_catalog::PartDefinition) -> Option<(&'a crate::OriginalModel, crate::asset_catalog::PartDefinition)> {
    let model = originals.models.iter().find(|m| m.instance == part.instance)?;
    let definition = model
        .source_definition
        .and_then(|k| originals.part_catalog.parts.iter().find(|p| p.key == k))
        .cloned()
        .unwrap_or_else(|| crate::asset_catalog::PartDefinition::raw_model(catalog.model));
    Some((model, definition))
}

/// Skinned copy of a player part: the part model's own vertices (its
/// blend-shape handles applied, its skeletal handles left to the bones) with
/// the rig's joint weights, the rig, rig -> mesh and the skeletal handles.
fn build_part(
    originals: &crate::Originals,
    catalog: &crate::asset_catalog::PartDefinition,
    mesh_definition: &crate::asset_catalog::PartDefinition,
    weights: &[(u32, u16)],
    plain: Option<&Mesh>,
    rigs: &mut RigCache,
) -> Option<(Mesh, Arc<PartRig>, Affine3A, Arc<Vec<HandleAnim>>)> {
    let key = catalog.model;
    let rig = part_rig(rigs, &originals.resource_index, key).filter(|r| !r.clips.is_empty())?;
    let rig = with_binding_clips(rigs, &originals.resource_index, key, rig);
    let blob = originals.resource_index.read(key).ok()?;
    let (joints, skin_weights) = part_skin_attributes(&blob, rig.bone_count())?;
    let handles: Vec<(u32, f32)> = weights.iter().map(|(id, s)| (*id, *s as f32 / 256.0)).collect();
    // The spawned mesh, or with blend-shape handles a copy assembled the same way.
    let mut mesh = match plain.filter(|_| handles.is_empty()) {
        Some(m) => m.clone(),
        None => crate::editor::creation_mesh(originals, mesh_definition, &handles).ok()?.0,
    };
    let count = match mesh.attribute(Mesh::ATTRIBUTE_POSITION)? {
        VertexAttributeValues::Float32x3(v) => v.len(),
        _ => return None,
    };
    if count != joints.len() {
        return None;
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joints));
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, skin_weights);
    mesh.generate_skinned_mesh_bounds().ok()?;
    let rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
        * Quat::from_euler(
            EulerRot::XYZ,
            mesh_definition.model_rotation[0].to_radians(),
            mesh_definition.model_rotation[1].to_radians(),
            mesh_definition.model_rotation[2].to_radians(),
        );
    let mesh_from_rig = Affine3A::from_quat(rotation) * Affine3A::from_translation(Vec3::from_array(mesh_definition.model_offset));
    let handles = Arc::new(skeletal_handles(&rig, &blob));
    Some((mesh, rig, mesh_from_rig, handles))
}

/// A starter donor part (`starter_parts`: cut from the installed starter's
/// baked GMDL in its baked orientation, handles baked in) skinned on that
/// GMDL's keyed binding rig exactly as the installed starter (its records,
/// rest entries and baked clips): the donor carries the retail roll.
#[allow(clippy::type_complexity)]
fn build_donor(originals: &crate::Originals, catalog: &crate::asset_catalog::PartDefinition, plain: &Mesh, meshes: &Assets<Mesh>, rigs: &mut RigCache) -> Option<(Mesh, Arc<PartRig>, Affine3A, (Vec<Delta>, Vec<Affine3A>, Vec<Track>))> {
    let skin = crate::starter_parts::donor_skin(originals, meshes, catalog.key).ok()??;
    let model = crate::baked_model::parse(&originals.resource_index.read(skin.source).ok()?).ok()?;
    let anatomy_key = crate::dbpf::ResourceKey { type_id: crate::creature_data::TYPE, group_id: crate::original_species::CREATURE_ANATOMY_GROUP, instance_id: skin.source.instance_id };
    let anatomy = crate::creature_data::parse(&originals.resource_index.read(anatomy_key).ok()?).ok()?;
    let (binding, rig, to_model, _) = rigged_bindings(&originals.resource_index, &model, rigs).into_iter().find(|b| model.anim_data[b.0].key.instance_id == catalog.model.instance_id)?;
    if rig.bone_count() != skin.group.len() {
        return None;
    }
    let mut mesh = plain.clone();
    let count = match mesh.attribute(Mesh::ATTRIBUTE_POSITION)? {
        VertexAttributeValues::Float32x3(v) => v.len(),
        _ => return None,
    };
    if count != skin.joints.len() {
        return None;
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(skin.joints));
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, skin.weights);
    mesh.generate_skinned_mesh_bounds().ok()?;
    let deltas: Vec<Delta> = model.anim_data[binding].baked_deforms.first().map_or_else(|| vec![Delta::IDENTITY; rig.bone_count()], |r| r.iter().map(decode_record).collect());
    let rest = inverse_rest(&rig, &deltas);
    let entries = anatomy.rigblocks.iter().position(|b| b.instance_id() == catalog.key.instance_id).map_or_else(Vec::new, |i| rest_entries(&anatomy, i));
    Some((mesh, rig, skin.mesh_from_gmdl * to_model, (deltas, rest, entries)))
}

/// The baked binding rig of a part model (group 0x41606100, same instance):
/// the rig a baked creature's keyed binding names.
pub const BINDING_RIG_GROUP: u32 = 0x4160_6100;

/// A part rig with the deform clips of its baked binding rig when that rig
/// has the same skeleton (names, parents, bind pose): the clips a baked
/// creature plays (e.g. `Beady Eye`'s 89E06A31 also shrinks the eyeball so the
/// closing lids cover it; the editor model's own clip does not). ADAPTED: a
/// binding with a collapsed skeleton (proboscis, feet) keeps the part
/// model's own clips; the unbaked editor creature uses the baked clips too.
fn with_binding_clips(rigs: &mut RigCache, index: &crate::asset_catalog::ResourceIndex, key: crate::dbpf::ResourceKey, rig: Arc<PartRig>) -> Arc<PartRig> {
    let binding_key = crate::dbpf::ResourceKey { group_id: BINDING_RIG_GROUP, ..key };
    let Some(binding) = part_rig(rigs, index, binding_key) else { return rig };
    let same = binding.names == rig.names
        && binding.parents == rig.parents
        && !binding.clips.is_empty()
        && binding.inverse_bind.iter().zip(&rig.inverse_bind).all(|(a, b)| a.abs_diff_eq(*b, 1e-3));
    if !same {
        return rig;
    }
    Arc::new(PartRig { clips: binding.clips.clone(), ..(*rig).clone() })
}

/// Joint transforms (in the part mesh's space) of a part for its slots and
/// handle deltas.
fn joint_locals(rig: &PartRig, mesh_from_rig: Affine3A, slots: &Slots, deltas: &[Delta], inverse_rest: &[Affine3A]) -> Vec<Affine3A> {
    let mut skin = Vec::new();
    let moved = part_skin(rig, slots, deltas, inverse_rest, &mut skin);
    let back = mesh_from_rig.inverse();
    skin.iter().map(|m| if moved { mesh_from_rig * *m * back } else { Affine3A::IDENTITY }).collect()
}

/// Skin the avatar's parts on their rigs and pose them from the clip deform
/// records `creature_clips::drive_player` leaves in `DriveState`, with the
/// parts' skeletal handles as bone deltas (the mesh is the part's own).
#[allow(clippy::too_many_arguments)]
pub fn pose_player_parts(
    mut commands: Commands,
    drive: Option<Res<crate::creature_clips::DriveState>>,
    run: Res<crate::game::RunProgress>,
    originals: Res<crate::Originals>,
    mut cache: ResMut<PartMorphCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut parts: Query<(Entity, &crate::creature::Accessory, &mut Mesh3d, Option<&mut PartMorph>, Option<&PartRigid>)>,
    mut joints: Query<&mut Transform, Without<crate::creature::Accessory>>,
) {
    let tracks = drive.as_ref().and_then(|d| d.part_tracks()).filter(|_| player_parts_enabled());
    for (entity, acc, mut mesh3d, morph, rigid) in &mut parts {
        let Some(tracks) = tracks else {
            // No clip-driven avatar: back to the plain mesh.
            if let Some(m) = morph {
                if mesh3d.0 == m.skinned {
                    mesh3d.0 = m.source.clone();
                }
                commands.entity(entity).remove::<(PartMorph, SkinnedMesh, DynamicSkinnedMeshBounds)>();
                for &j in &m.joints {
                    commands.entity(j).despawn();
                }
            }
            continue;
        };
        if let Some(mut m) = morph {
            if mesh3d.0 != m.skinned {
                // The part's mesh was replaced: build again next frame.
                commands.entity(entity).remove::<(PartMorph, SkinnedMesh, DynamicSkinnedMeshBounds)>();
                for &j in &m.joints {
                    commands.entity(j).despawn();
                }
                continue;
            }
            let weights = run.parts.get(acc.part).map(|p| p.edits.handles.clone()).unwrap_or_default();
            if weights != m.weights && !m.baked {
                m.deltas = handle_deltas(&m.rig, &m.handles, &weights);
                m.weights = weights;
            }
            let part_tracks = tracks.get(&acc.part).map_or(&[][..], |t| &t[..]);
            let rig = m.rig.clone();
            m.slots.update(&rig, part_tracks);
            if let Some(forced) = debug_phase() {
                // `OPENSPORE_MORPH_PHASE=<phase>` (debug): every slot at that phase.
                for s in &mut m.slots.0 {
                    s.phase = forced;
                }
            }
            if debug() {
                info!("morph debug part {}: tracks {:08X?} slots {:?}", acc.part, part_tracks, m.slots.0.iter().map(|s| (format!("{:08X}", s.id), s.phase, s.weight)).collect::<Vec<_>>());
            }
            let locals = joint_locals(&rig, m.mesh_from_rig, &m.slots, &m.deltas, &m.inverse_rest);
            for (&j, local) in m.joints.iter().zip(&locals) {
                if let Ok(mut tf) = joints.get_mut(j) {
                    *tf = Transform::from_matrix(Mat4::from(*local));
                }
            }
            continue;
        }
        if rigid.is_some_and(|r| r.0 == mesh3d.0.id()) {
            continue;
        }
        let Some(part) = run.parts.get(acc.part) else { continue };
        let Some(definition) = part_definition(&originals, part) else {
            commands.entity(entity).insert(PartRigid(mesh3d.0.id()));
            continue;
        };
        let instance = definition.key.instance_id;
        let shapes = *cache.shapes.entry(instance).or_insert_with(|| {
            originals.resource_index.read(definition.model).ok().and_then(|b| crate::rw4::extract_morph_data(&b).ok()).is_some_and(|d| !d.shapes.is_empty())
        });
        let weights = shape_weights(part, shapes);
        let key = (instance, weights.clone());
        let built = match cache.meshes.get(&key) {
            Some(b) => b.clone(),
            None => {
                let made = spawned_model(&originals, part, definition).and_then(|(model, mesh_definition)| {
                    let plain = meshes.get(&model.handle).cloned()?;
                    if originals.starter_parts.values().any(|d| d.handle == model.handle) {
                        let (m, rig, map, baked) = build_donor(&originals, definition, &plain, &meshes, &mut cache.rigs)?;
                        return Some((m, rig, map, Arc::new(Vec::new()), Some(baked)));
                    }
                    let (m, rig, map, handles) = build_part(&originals, definition, &mesh_definition, &weights, Some(&plain), &mut cache.rigs)?;
                    Some((m, rig, map, handles, None))
                });
                let made = made.map(|(m, rig, mesh_from_rig, handles, baked)| BuiltPart { mesh: meshes.add(m), rig, mesh_from_rig, handles, baked });
                cache.meshes.insert(key, made.clone());
                made
            }
        };
        let Some(BuiltPart { mesh: skinned, rig, mesh_from_rig, handles, baked }) = built else {
            debug!("part morph: {} stays rigid", definition.name);
            commands.entity(entity).insert(PartRigid(mesh3d.0.id()));
            continue;
        };
        info!(
            "part morph: {} on its rig ({} bones, deforms {:08X?}, {} skeletal handles, weights {:?}, baked donor {})",
            definition.name,
            rig.bone_count(),
            rig.clips.iter().map(|c| c.0).collect::<Vec<_>>(),
            handles.len(),
            part.edits.handles,
            baked.is_some()
        );
        let n = rig.bone_count();
        let inverse = cache.bindposes.entry(n).or_insert_with(|| bindposes.add(vec![Mat4::IDENTITY; n])).clone();
        let weights = part.edits.handles.clone();
        let (deltas, inverse_rest, slots) = match &baked {
            Some((deltas, rest, entries)) => (deltas.clone(), rest.clone(), Slots::setup(&rig, entries)),
            None => (handle_deltas(&rig, &handles, &weights), rig.inverse_bind.clone(), Slots::default()),
        };
        // The joints start at the handle pose (no blank frame).
        let locals = joint_locals(&rig, mesh_from_rig, &slots, &deltas, &inverse_rest);
        let joint_entities: Vec<Entity> = locals
            .iter()
            .map(|l| commands.spawn((Transform::from_matrix(Mat4::from(*l)), crate::stages::StageOwned, ChildOf(entity))).id())
            .collect();
        let source = std::mem::replace(&mut mesh3d.0, skinned.clone());
        commands.entity(entity).insert((
            SkinnedMesh { inverse_bindposes: inverse, joints: joint_entities.clone() },
            DynamicSkinnedMeshBounds,
            PartMorph { source, skinned, joints: joint_entities, rig, mesh_from_rig, handles, slots, weights, deltas, inverse_rest, baked: baked.is_some() },
        ));
    }
}

/// The mesh a copy of an avatar part should draw: the plain source while the
/// part is skinned (`cinematics::spawn_avatar_copy`; a copy has no joints).
pub fn plain_mesh(world: &World, entity: Entity) -> Option<Handle<Mesh>> {
    world.get::<PartMorph>(entity).map(|m| m.source.clone())
}

// ---------------------------------------------------------------------------
// Capture: close-ups of the player's own creature's part morphs
// ---------------------------------------------------------------------------

/// The camera of `--morph-demo`: in front of a part of the avatar.
#[derive(Resource)]
pub struct MorphDemo {
    pub focus: Entity,
    pub center: Entity,
    pub distance: f32,
    /// Turn of the view about the up axis (radians, 0 = straight at the part).
    pub angle: f32,
    pub height: f32,
    /// Focus on the creature itself (installed model): the view looks at
    /// this height above its root from its facing (-Z in the Bevy frame).
    pub root_height: Option<f32>,
}

/// Keep the camera on the demo part (runs last, after every camera system).
pub fn demo_camera(
    demo: Option<Res<MorphDemo>>,
    subjects: Query<&GlobalTransform, Without<crate::GameCamera>>,
    mut cameras: Query<(&mut Transform, &mut GlobalTransform), With<crate::GameCamera>>,
) {
    let Some(demo) = demo else { return };
    let (Ok(focus), Ok(center)) = (subjects.get(demo.focus), subjects.get(demo.center)) else { return };
    let (mut f, c) = (focus.translation(), center.translation());
    let mut out = Vec3::new(f.x - c.x, 0.0, f.z - c.z).normalize_or(Vec3::NEG_Z);
    if let Some(h) = demo.root_height {
        let t = focus.compute_transform();
        f += Vec3::Y * h * t.scale.y;
        let front = t.rotation * Vec3::NEG_Z;
        out = Vec3::new(front.x, 0.0, front.z).normalize_or(Vec3::NEG_Z);
    }
    out = Quat::from_rotation_y(demo.angle) * out;
    let eye = f + out * demo.distance + Vec3::Y * demo.height;
    for (mut t, mut g) in &mut cameras {
        *t = Transform::from_translation(eye).looking_at(f, Vec3::Y);
        *g = GlobalTransform::from(*t);
    }
}

/// `--morph-demo CLIP [--morph-part Mouths|Eyes|Feet|Hands] [--morph-frames N]
/// [--morph-every K] [--morph-dist D] [--morph-angle A] [--morph-height H]
/// [--morph-up U] [--morph-raw] [--morph-cinematic NAME] [--morph-testdrive]`:
/// after the Creature warm-up the avatar becomes the player's own skinned
/// creature (as `--look-demo custom`), plays CLIP and frames
/// `morph-<part>-NN.png` are taken in front of that part.
pub fn quick_capture_demo(app: &mut App, args: &[String], output: &std::path::Path, target: &Handle<Image>, stage: &str) {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
    let Some(clip) = flag("--morph-demo").and_then(|c| u32::from_str_radix(c.trim_start_matches("0x"), 16).ok()) else { return };
    if stage != "creature" {
        return;
    }
    let num = |name: &str, d: f32| flag(name).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
    let category = flag("--morph-part").unwrap_or_else(|| "Mouths".into());
    // `--morph-part Head`: the installed starter's model, framed at its head.
    let installed = category == "Head";
    if !installed {
        // As after a Creature Creator visit: the starter's parts become the
        // donors cut from its baked model (`editor::ensure_model`), unless
        // `--morph-raw` keeps the startup part imports.
        if !args.iter().any(|a| a == "--morph-raw") {
            let world = app.world_mut();
            let defs: Vec<crate::asset_catalog::PartDefinition> = {
                let run = world.resource::<crate::game::RunProgress>();
                let originals = world.resource::<crate::Originals>();
                run.parts.iter().filter_map(|p| originals.part_catalog.by_part_in(crate::asset_catalog::CREATURE_PARTS, p).cloned()).collect()
            };
            world.resource_scope(|world, mut originals: Mut<crate::Originals>| {
                world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
                    world.resource_scope(|world, mut images: Mut<Assets<Image>>| {
                        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
                        for d in &defs {
                            if let Err(e) = crate::editor::ensure_model(&mut originals, d, &mut meshes, &mut images, &mut materials) {
                                warn!("--morph-demo: part model {}: {e}", d.name);
                            }
                        }
                    })
                })
            });
        }
        if let Some(p) = app.world_mut().resource_mut::<crate::game::RunProgress>().body.spine.first_mut() {
            p.radius += 0.03;
        }
    }
    for _ in 0..30 {
        app.update();
        crate::runtime::gpu_flush(app);
    }
    let world = app.world_mut();
    let Some(player) = world.query_filtered::<Entity, With<crate::game::Player>>().iter(world).next() else { return };
    if args.iter().any(|a| a == "--morph-testdrive") {
        // `--morph-testdrive`: the Creature Creator's Test Drive on this
        // avatar (pass `--creator-mode play --creator-anim <control>`, read
        // when the creator opens).
        world.resource_mut::<crate::editor::EditorState>().show();
        for _ in 0..120 {
            app.update();
            crate::runtime::gpu_flush(app);
        }
    } else {
        world.entity_mut(player).insert(crate::locomotion_preview::PoseClip { action: Some(clip), idle: crate::creature_look::LOOK_IDLE, serial: 1 });
    }
    let mut focus = installed.then_some(player);
    for _ in 0..30 {
        if focus.is_some() {
            for _ in 0..30 {
                app.update();
                crate::runtime::gpu_flush(app);
            }
            break;
        }
        app.update();
        crate::runtime::gpu_flush(app);
        let world = app.world_mut();
        let parts: Vec<(Entity, usize)> = world.query::<(Entity, &crate::creature::Accessory, Has<PartMorph>)>().iter(world).filter(|(_, _, m)| *m || !player_parts_enabled()).map(|(e, a, _)| (e, a)).map(|(e, a)| (e, a.part)).collect();
        let run = world.resource::<crate::game::RunProgress>();
        let originals = world.resource::<crate::Originals>();
        focus = parts.into_iter().find(|(_, part)| run.parts.get(*part).and_then(|p| part_definition(originals, p)).is_some_and(|d| d.category() == category)).map(|(e, _)| e);
        if focus.is_some() {
            break;
        }
    }
    let Some(focus) = focus else {
        warn!("--morph-demo: no skinned {category} part on the avatar");
        return;
    };
    info!("morph demo: clip {clip:08X}, {category} part {focus:?}");
    // `--morph-cinematic NAME`: play that cinematic on the prepared avatar
    // instead (its frames as `--cinematic` writes them).
    if let Some(name) = flag("--morph-cinematic") {
        let every = flag("--morph-every").unwrap_or_else(|| "15".into());
        let frames = flag("--morph-frames").unwrap_or_else(|| "600".into());
        let cinematic: Vec<String> = ["--cinematic", name.as_str(), "--cinematic-every", every.as_str(), "--cinematic-frames", frames.as_str()].iter().map(|s| s.to_string()).collect();
        crate::cinematics::quick_capture(app, &cinematic, output, target);
        return;
    }
    app.world_mut().insert_resource(MorphDemo {
        focus,
        center: player,
        distance: num("--morph-dist", 1.6),
        angle: num("--morph-angle", 0.5),
        height: num("--morph-height", 0.3),
        root_height: installed.then(|| num("--morph-up", 1.2)),
    });
    let frames = num("--morph-frames", 120.0) as usize;
    let every = (num("--morph-every", 12.0) as usize).max(1);
    let label = category.to_lowercase();
    for i in 0..frames {
        app.update();
        crate::runtime::gpu_flush(app);
        if i % every == every - 1 {
            screenshot(app, target, &output.join(format!("morph-{label}-{:02}.png", i / every)));
        }
    }
    app.world_mut().remove_resource::<MorphDemo>();
}

/// A plain screenshot of the capture target (as `creature_look`'s demo).
fn screenshot(app: &mut App, target: &Handle<Image>, path: &std::path::Path) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    let _ = std::fs::remove_file(path);
    app.world_mut().spawn(Screenshot::image(target.clone())).observe(save_to_disk(path.to_path_buf()));
    for _ in 0..600 {
        app.update();
        crate::runtime::gpu_flush(app);
        if std::fs::metadata(path).is_ok_and(|m| m.len() > 1000) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_anim::{Bone3, RigClip};

    fn key(time: f32, rotation: Quat, location: Vec3) -> crate::rw4::Keyframe {
        crate::rw4::Keyframe { time, rotation: rotation.to_array(), location: location.to_array(), scale: [1.0; 3], factor: 0.0, blend: false }
    }

    /// A two-bone rig with one deform clip that turns bone 1 by 90 degrees
    /// about Z over its length.
    fn rig() -> PartRig {
        let anim = crate::rw4::KeyAnim {
            skeleton_id: 0,
            length: 1.25,
            channels: vec![crate::rw4::AnimChannel { id: 2, keys: vec![key(0.0, Quat::IDENTITY, Vec3::Y), key(1.25, Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), Vec3::Y)] }],
        };
        let bind = vec![Bone3::IDENTITY, Bone3 { translation: Vec3::Y, ..Bone3::IDENTITY }];
        PartRig {
            names: vec![1, 2],
            parents: vec![None, Some(0)],
            inverse_bind: vec![Affine3A::IDENTITY, Affine3A::from_translation(-Vec3::Y)],
            bind_local: bind,
            clips: vec![(0x89E0_6A31, RigClip { anim, channel_for_bone: vec![None, Some(0)] })],
        }
    }

    fn skin(rig: &PartRig, slots: &Slots, deltas: &[Delta]) -> (bool, Vec<Affine3A>) {
        let mut out = Vec::new();
        let moved = part_skin(rig, slots, deltas, &rig.inverse_bind, &mut out);
        (moved, out)
    }

    #[test]
    fn phases_and_times_follow_the_retail_rules() {
        assert!((phase_of(0.5, 1.0) - 0.5).abs() < 1e-6);
        assert_eq!(phase_of(0.0, 0.0), 0.0);
        assert!((cyclic_phase(0.0, 1.0) - 0.25).abs() < 1e-6);
        assert!((cyclic_phase(0.0, -1.0) - 0.75).abs() < 1e-6);
        // The end stays; past it the time wraps.
        assert_eq!(wrap_time(1.25, 1.25), 1.25);
        assert!((wrap_time(1.5, 1.25) - 0.25).abs() < 1e-6);
        assert_eq!(wrap_time(2.5, 1.25), 1.25);
        assert!((wrap_time(-0.25, 1.25) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn slots_start_from_the_creature_data_and_take_normalized_frame_weights() {
        let rig = rig();
        // Unknown ids are not started; a later duplicate wins.
        let mut s = Slots::setup(&rig, &[(0x1234, 0.5, 1.0), (0x89E0_6A31, 0.5, 1.0), (0x89E0_6A31, 0.15, 1.0)]);
        assert_eq!(s.0, vec![Slot { id: 0x89E0_6A31, phase: 0.15, weight: 1.0 }]);
        // A frame's weights are over the block's total (unknown ids count).
        s.update(&rig, &[(0x89E0_6A31, 0.8, 0.5), (0x1234, 0.0, 1.5)]);
        assert_eq!(s.0, vec![Slot { id: 0x89E0_6A31, phase: 0.8, weight: 0.25 }]);
        // A frame without the id keeps the slot.
        s.update(&rig, &[]);
        assert_eq!(s.0[0].phase, 0.8);
    }

    #[test]
    fn the_multiblender_averages_slot_poses_and_fills_with_zero_below_one() {
        let rig = rig();
        let mut out = Vec::new();
        assert!(!blend_locals(&rig, &Slots::default(), &mut out));
        assert_eq!(out, rig.bind_local);
        // Phase 0 is the bind pose, phase 1 the quarter turn.
        let mut s = Slots::default();
        s.set(0x89E0_6A31, 1.0, 1.0);
        assert!(blend_locals(&rig, &s, &mut out));
        assert!(out[1].rotation.angle_between(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)) < 1e-4);
        // Weight below 1: the translation is scaled toward the zero pose.
        s.set(0x89E0_6A31, 0.0, 0.5);
        blend_locals(&rig, &s, &mut out);
        assert!((out[1].translation - Vec3::Y * 0.5).length() < 1e-5, "{:?}", out[1]);
        assert!(out[1].rotation.angle_between(Quat::IDENTITY) < 1e-4);
    }

    #[test]
    fn deform_slots_pose_the_part_bones() {
        let rig = rig();
        let (moved, out) = skin(&rig, &Slots::default(), &[Delta::IDENTITY; 2]);
        assert!(!moved && out.iter().all(|m| m.abs_diff_eq(Affine3A::IDENTITY, 1e-6)));
        let mut s = Slots::default();
        s.set(0x89E0_6A31, 1.0, 1.0);
        let (moved, out) = skin(&rig, &s, &[Delta::IDENTITY; 2]);
        assert!(moved);
        // A vertex one unit along +Y from bone 1 turns to -X.
        let p = out[1].transform_point3(Vec3::new(0.0, 2.0, 0.0));
        assert!(p.distance(Vec3::new(-1.0, 1.0, 0.0)) < 1e-4, "{p}");
    }

    #[test]
    fn handle_deltas_ride_on_top_of_the_clip_pose() {
        let rig = rig();
        // A handle that moves bone 1 up by 1 at weight 1 (bind at weight 0).
        let anim = crate::rw4::KeyAnim { skeleton_id: 0, length: 1.25, channels: vec![crate::rw4::AnimChannel { id: 2, keys: vec![key(0.0, Quat::IDENTITY, Vec3::Y), key(1.25, Quat::IDENTITY, Vec3::Y * 2.0)] }] };
        let handles = vec![HandleAnim { id: 7, default: 0.0, anim, channel_for_bone: vec![None, Some(0)] }];
        assert!(handle_deltas(&rig, &handles, &[]).iter().all(|d| d.is_identity()));
        let deltas = handle_deltas(&rig, &handles, &[(7, 1.0)]);
        assert!((deltas[1].translation - Vec3::Y).length() < 1e-6);
        // At rest the vertex at bone 1 moves with the handle.
        let (moved, out) = skin(&rig, &Slots::default(), &deltas);
        assert!(moved);
        assert!(out[1].transform_point3(Vec3::Y).distance(Vec3::Y * 2.0) < 1e-5);
        // The clip turns the bone about its handled position: no double offset.
        let mut s = Slots::default();
        s.set(0x89E0_6A31, 1.0, 1.0);
        let (_, out) = skin(&rig, &s, &deltas);
        let p = out[1].transform_point3(Vec3::new(0.0, 2.0, 0.0));
        assert!(p.distance(Vec3::new(-1.0, 2.0, 0.0)) < 1e-4, "{p}");
        // A baked rig's rest worlds include the deltas: at rest nothing moves.
        let inverse = inverse_rest(&rig, &deltas);
        let mut out = Vec::new();
        part_skin(&rig, &Slots::default(), &deltas, &inverse, &mut out);
        assert!(out.iter().all(|m| m.abs_diff_eq(Affine3A::IDENTITY, 1e-5)));
    }

    #[test]
    fn baked_records_decode_translation_and_rotation() {
        let mut rec = [0u8; 160];
        let put = |rec: &mut [u8; 160], i: usize, v: f32| rec[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        // The Toucan't's bone 043A0A88 on creature 07BBABB4 (-0.066, -0.011, 0).
        put(&mut rec, 0, -0.066);
        put(&mut rec, 1, -0.011);
        put(&mut rec, 6, 0.02);
        put(&mut rec, 7, 1.0);
        for i in 8..14 {
            put(&mut rec, i, 1.0);
        }
        let d = decode_record(&rec);
        assert!((d.translation - Vec3::new(-0.066, -0.011, 0.0)).length() < 1e-6);
        assert!((d.rotation.z - 0.02).abs() < 1e-3 && d.rotation.w > 0.99);
        assert_eq!(d.scale, Vec3::ONE);
        // An all-zero record (the root) is the identity.
        let mut root = [0u8; 160];
        put(&mut root, 7, 1.0);
        assert!(decode_record(&root).rotation.angle_between(Quat::IDENTITY) < 1e-6);
    }

    #[test]
    fn parent_scale_does_not_carry_into_the_child_frame() {
        let mut rig = rig();
        rig.clips.clear();
        let locals = vec![Bone3 { scale: Vec3::splat(2.0), ..Bone3::IDENTITY }, Bone3 { translation: Vec3::Y, ..Bone3::IDENTITY }];
        let mut w = Vec::new();
        worlds(&rig, &locals, &mut w);
        // The child's position is scaled by its parent, its frame is not.
        assert!((w[1].translation - bevy::math::Vec3A::new(0.0, 2.0, 0.0)).length() < 1e-6);
        assert!((Mat3::from(w[1].matrix3).determinant() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn morph_sets_ride_their_block_and_keep_it_at_rest() {
        let set = MorphSet { groups: vec![MorphGroup::new(1, 1, 10, Arc::new(rig()), Affine3A::from_translation(Vec3::new(0.0, 0.0, 2.0)), Affine3A::from_translation(Vec3::new(0.0, 0.0, -2.0)))] };
        assert_eq!(set.joints(), 2);
        let blocks = vec![Affine3A::IDENTITY, Affine3A::from_translation(Vec3::X)];
        let mut out = Vec::new();
        let mut state = MorphState::default();
        set.pose(&blocks, &[vec![], vec![]], &mut state, &mut out);
        assert_eq!(out.iter().map(|o| o.0).collect::<Vec<_>>(), vec![10, 11]);
        assert!(out.iter().all(|o| o.1.abs_diff_eq(blocks[1], 1e-6)));
        set.pose(&blocks, &[vec![], vec![(0x89E0_6A31, 1.0, 1.0)]], &mut state, &mut out);
        let p = out[1].1.transform_point3(Vec3::new(0.0, 2.0, 2.0));
        assert!(p.distance(Vec3::new(0.0, 1.0, 2.0)) < 1e-4, "{p}");
        // The slot persists into a frame without records.
        set.pose(&blocks, &[vec![], vec![]], &mut state, &mut out);
        assert!(out[1].1.transform_point3(Vec3::new(0.0, 2.0, 2.0)).distance(p) < 1e-5);
    }

    /// Data probe (not an assertion test): per installed creature part rig,
    /// the deform table, which bones each KeyAnim keys, the KeyAnims' skeleton
    /// ids, the morph handles and how far each clip's frames sit from the
    /// bind pose. `OPENSPORE_MORPH_PROBE=<name substring>` narrows the list.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn probe_installed_part_rigs() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        let filter = std::env::var("OPENSPORE_MORPH_PROBE").unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        let (mut rigs, mut full, mut partial) = (0, 0, 0);
        for def in catalog.parts.iter().filter(|d| d.key.group_id == crate::asset_catalog::CREATURE_PARTS) {
            if !seen.insert(def.model) || (!filter.is_empty() && !def.name.contains(&filter)) {
                continue;
            }
            let Ok(blob) = index.read(def.model) else { continue };
            let Ok(rig) = PartRig::from_rw4(&blob) else { continue };
            if rig.clips.is_empty() {
                continue;
            }
            rigs += 1;
            let morph = crate::rw4::extract_morph_data(&blob).ok();
            let n = rig.bone_count();
            println!("{} {:08X}: {} bones, skel names {:08X?}", def.name, def.model.instance_id, n, rig.names);
            if let Some(m) = &morph {
                for h in &m.handles {
                    let a = h.animation.as_ref();
                    let blend = a.is_some_and(|a| a.channels.iter().any(|c| c.keys.iter().any(|k| k.blend)));
                    println!(
                        "  handle {:08X} default {:.3} anim skel {:08X?} chans {} blend {} len {:?} in-table {}",
                        h.id,
                        h.default_time,
                        a.map(|a| a.skeleton_id),
                        a.map_or(0, |a| a.channels.len()),
                        blend,
                        a.map(|a| a.length),
                        rig.has_deform(h.id)
                    );
                }
                println!("  shapes {} pools {} rig {}", m.shapes.len(), m.pools.len(), m.rig.is_some());
                // Blend-shape handle anims: factor range per channel.
                for h in &m.handles {
                    let Some(a) = h.animation.as_ref().filter(|a| a.channels.iter().any(|c| c.keys.iter().any(|k| k.blend))) else { continue };
                    let ranges: Vec<String> = a
                        .channels
                        .iter()
                        .map(|c| {
                            let lo = c.keys.iter().map(|k| k.factor).fold(f32::MAX, f32::min);
                            let hi = c.keys.iter().map(|k| k.factor).fold(f32::MIN, f32::max);
                            format!("{:08X}:{lo:.2}..{hi:.2}@{:.2}", c.id, a.sample_clamped(a.channels.iter().position(|x| x.id == c.id).unwrap(), h.default_time.clamp(0.0, 1.0) * a.length).map_or(f32::NAN, |s| s.1[0]))
                        })
                        .collect();
                    println!("  blend handle {:08X} default {:.3}: {}", h.id, h.default_time, ranges.join(" "));
                }
                for (id, clip) in &rig.clips {
                    if clip.anim.channels.iter().any(|c| c.keys.iter().any(|k| k.blend)) {
                        let ranges: Vec<String> = clip
                            .anim
                            .channels
                            .iter()
                            .map(|c| {
                                let lo = c.keys.iter().map(|k| k.factor).fold(f32::MAX, f32::min);
                                let hi = c.keys.iter().map(|k| k.factor).fold(f32::MIN, f32::max);
                                format!("{:08X}:{lo:.2}..{hi:.2}", c.id)
                            })
                            .collect();
                        println!("  blend deform {id:08X}: {}", ranges.join(" "));
                    }
                }
                // Skeletal handle anims on the rig's own bones: each at its
                // default vs the bind pose, and their component average.
                let skeletal: Vec<&crate::rw4::KeyAnim> = m
                    .handles
                    .iter()
                    .filter_map(|h| h.animation.as_ref())
                    .filter(|a| a.channels.iter().all(|c| c.keys.iter().all(|k| !k.blend)) && a.channels.iter().any(|c| rig.names.contains(&c.id)))
                    .collect();
                if !skeletal.is_empty() {
                    let frame = |a: &crate::rw4::KeyAnim, phase: f32, b: usize| -> Bone3 {
                        a.channels
                            .iter()
                            .position(|c| c.id == rig.names[b])
                            .and_then(|c| a.sample_frame_clamped(c, phase * a.length))
                            .map_or(rig.bind_local[b], |k| Bone3 { rotation: quat_xyzw(k.rotation), translation: Vec3::from_array(k.location), scale: Vec3::from_array(k.scale) })
                    };
                    let dev = |poses: &[Bone3]| {
                        let (mut dt, mut dr) = (0.0f32, 0.0f32);
                        for (b, p) in poses.iter().enumerate() {
                            dt = dt.max(p.translation.distance(rig.bind_local[b].translation));
                            dr = dr.max(p.rotation.angle_between(rig.bind_local[b].rotation));
                        }
                        format!("t{dt:.3} r{dr:.3}")
                    };
                    let mut each = Vec::new();
                    let defaults: Vec<f32> = m.handles.iter().filter(|h| h.animation.as_ref().is_some_and(|a| skeletal.iter().any(|s| std::ptr::eq(*s, a)))).map(|h| h.default_time.clamp(0.0, 1.0)).collect();
                    for (a, d) in skeletal.iter().zip(&defaults) {
                        let poses: Vec<Bone3> = (0..n).map(|b| frame(a, *d, b)).collect();
                        each.push(dev(&poses));
                    }
                    let average = |extra: &[(&crate::rw4::KeyAnim, f32)]| {
                        let all: Vec<(&crate::rw4::KeyAnim, f32)> = skeletal.iter().copied().zip(defaults.iter().copied()).chain(extra.iter().copied()).collect();
                        let poses: Vec<Bone3> = (0..n)
                            .map(|b| {
                                let (mut q, mut t, mut s) = (Vec4::ZERO, Vec3::ZERO, Vec3::ZERO);
                                for (a, p) in &all {
                                    let f = frame(a, *p, b);
                                    let mut v = Vec4::from(f.rotation);
                                    if q.dot(v) < 0.0 {
                                        v = -v;
                                    }
                                    q += v;
                                    t += f.translation;
                                    s += f.scale;
                                }
                                let k = all.len() as f32;
                                Bone3 { rotation: Quat::from_vec4(q / k).normalize(), translation: t / k, scale: s / k }
                            })
                            .collect();
                        dev(&poses)
                    };
                    let table0: Vec<(&crate::rw4::KeyAnim, f32)> = rig.clips.iter().filter(|(id, _)| !m.handles.iter().any(|h| h.id == *id)).map(|(_, c)| (&c.anim, 0.0)).collect();
                    let table5: Vec<(&crate::rw4::KeyAnim, f32)> = table0.iter().map(|(a, _)| (*a, 0.5)).collect();
                    println!("  handle defaults vs bind: [{}]; average {}; +table@0 {}; +table@.5 {}", each.join(", "), average(&[]), average(&table0), average(&table5));
                    // Bone travel (rig space) of each skeletal handle from
                    // phase 0 to 1 against its drag line length.
                    let worlds = |locals: &[Bone3]| -> Vec<Vec3> {
                        let mut w: Vec<Affine3A> = Vec::with_capacity(n);
                        for b in 0..n {
                            let l = Affine3A::from_scale_rotation_translation(locals[b].scale, locals[b].rotation, locals[b].translation);
                            let m = rig.parents[b].map_or(l, |p| w[p] * l);
                            w.push(m);
                        }
                        w.iter().map(|m| m.translation.into()).collect()
                    };
                    for h in m.handles.iter().filter(|h| h.animation.as_ref().is_some_and(|a| skeletal.iter().any(|s| std::ptr::eq(*s, a)))) {
                        let a = h.animation.as_ref().unwrap();
                        let p0 = worlds(&(0..n).map(|b| frame(a, 0.0, b)).collect::<Vec<_>>());
                        let p1 = worlds(&(0..n).map(|b| frame(a, 1.0, b)).collect::<Vec<_>>());
                        let (bone, travel) = p0.iter().zip(&p1).map(|(x, y)| y - x).enumerate().max_by(|x, y| x.1.length().total_cmp(&y.1.length())).unwrap();
                        let line = Vec3::new((h.end[0] - h.start[0]) as f32, (h.end[1] - h.start[1]) as f32, (h.end[2] - h.start[2]) as f32);
                        println!(
                            "  handle {:08X} line {:.3} {:?} | max bone travel {:.3} {:?} (bone {bone}) ratio {:.2} cos {:.2}",
                            h.id,
                            line.length(),
                            line.normalize_or_zero(),
                            travel.length(),
                            travel.normalize_or_zero(),
                            travel.length() / line.length().max(1e-6),
                            travel.normalize_or_zero().dot(line.normalize_or_zero())
                        );
                    }
                }
            }
            for (id, clip) in &rig.clips {
                let keyed = clip.channel_for_bone.iter().filter(|c| c.is_some()).count();
                if keyed == n {
                    full += 1;
                } else {
                    partial += 1;
                }
                // Distance of the frames at phase 0 / 0.5 / 1 from the bind pose.
                let dev = |phase: f32| {
                    let t = phase * clip.anim.length;
                    let (mut dt, mut dr, mut ds) = (0.0f32, 0.0f32, 0.0f32);
                    for (b, c) in clip.channel_for_bone.iter().enumerate() {
                        let Some(k) = c.and_then(|c| clip.anim.sample_frame_clamped(c, t)) else { continue };
                        let bind = rig.bind_local[b];
                        dt = dt.max(Vec3::from_array(k.location).distance(bind.translation));
                        dr = dr.max(quat_xyzw(k.rotation).angle_between(bind.rotation));
                        ds = ds.max((Vec3::from_array(k.scale) - bind.scale).abs().max_element());
                    }
                    format!("t{dt:.3} r{dr:.3} s{ds:.3}")
                };
                println!(
                    "  deform {id:08X} skel {:08X} len {:.2} keys {keyed}/{n} chans {} | p0 {} | p.5 {} | p1 {}",
                    clip.anim.skeleton_id,
                    clip.anim.length,
                    clip.anim.channels.len(),
                    dev(0.0),
                    dev(0.5),
                    dev(1.0)
                );
            }
        }
        println!("{rigs} rigs; deform clips keying every bone {full}, partial {partial}");
        // The installed starter's per-rigblock morph (handle) entries.
        let key = crate::dbpf::ResourceKey {
            type_id: crate::creature_data::TYPE,
            group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
            instance_id: crate::baked_model::PLAYER_BASELINE_INSTANCE,
        };
        if let Ok(data) = index.read(key).and_then(|b| crate::creature_data::parse(&b)) {
            for b in &data.rigblocks {
                let name = catalog.parts.iter().find(|d| d.key.instance_id == b.instance_id()).map_or("?", |d| d.name.as_str());
                let start = b.morph_index();
                let count = b.raw_bytes[0x0A] as i8;
                let entries: Vec<String> = (0..count.max(0) as usize)
                    .filter_map(|k| {
                        let i = usize::try_from(start).ok()? + k;
                        Some(format!(
                            "{:08X}={:.3}/{:.3}",
                            data.animation_ids.get(i)?,
                            f32::from_bits(*data.animation_value_bits.get(i)?),
                            f32::from_bits(*data.animation_weight_bits.get(i)?)
                        ))
                    })
                    .collect();
                println!(
                    "starter block {} {name} {:08X} morphs @{start} x{count}: {} raw {:02X?}",
                    b.index(),
                    b.instance_id(),
                    entries.join(" "),
                    &b.raw_bytes[..0x14]
                );
            }
            for i in 0..data.animation_ids.len() {
                println!(
                    "  morph[{i}] {:08X} {:.3} {:.3}",
                    data.animation_ids[i],
                    f32::from_bits(data.animation_value_bits[i]),
                    f32::from_bits(data.animation_weight_bits[i])
                );
            }
        }
    }

    /// Data probe: per bone of a part rig, the centroid of the vertices it
    /// dominates against the bone's bind world position (is rig space the
    /// RW4 vertex space?). `OPENSPORE_MORPH_PROBE=<RW4 instance hex>`.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn probe_part_vertex_space() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let instance = std::env::var("OPENSPORE_MORPH_PROBE").ok().and_then(|v| u32::from_str_radix(&v, 16).ok()).unwrap_or(0x7E26_2CF0);
        let key = crate::dbpf::ResourceKey { type_id: crate::asset_catalog::RW4, group_id: 0x4060_6000, instance_id: instance };
        let blob = index.read(key).unwrap();
        let rig = PartRig::from_rw4(&blob).unwrap();
        let mut stats = crate::rw4::MeshStats::default();
        let pieces = crate::rw4::extract_meshes(&blob, &mut stats);
        let (joints, weights) = part_skin_attributes(&blob, rig.bone_count()).unwrap();
        let positions: Vec<Vec3> = pieces.iter().flat_map(|p| p.positions.iter().map(|v| Vec3::from_array(*v))).collect();
        let n = rig.bone_count();
        let mut sum = vec![(Vec3::ZERO, 0usize); n];
        for (i, p) in positions.iter().enumerate() {
            let (k, _) = weights[i].iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap();
            let b = joints[i][k] as usize;
            sum[b].0 += *p;
            sum[b].1 += 1;
        }
        for b in 0..n {
            let world = rig.inverse_bind[b].inverse();
            let c = if sum[b].1 > 0 { sum[b].0 / sum[b].1 as f32 } else { Vec3::NAN };
            println!("bone {b} {:08X}: {} verts, centroid {c:.4}, bind world t {:.4}, axes x {:.3} z {:.3}", rig.names[b], sum[b].1, Vec3::from(world.translation), Vec3::from(world.matrix3.x_axis), Vec3::from(world.matrix3.z_axis));
        }
        let (lo, hi) = positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
        println!("vertex bounds {lo:.4} .. {hi:.4}");
        // Skinned vertices per dominant bone at a few phases of 89E06A31.
        for phase in [1.0f32, 0.5, 0.0] {
            let mut slots = Slots::default();
            slots.set(0x89E0_6A31, phase, 1.0);
            let mut skin = Vec::new();
            part_skin(&rig, &slots, &vec![Delta::IDENTITY; n], &rig.inverse_bind, &mut skin);
            let mut locals = Vec::new();
            blend_locals(&rig, &slots, &mut locals);
            println!("phase {phase}: locals {:?}", locals.iter().map(|l| format!("q{:.2?} t{:.3?} s{:.2?}", l.rotation.to_array(), l.translation.to_array(), l.scale.to_array())).collect::<Vec<_>>());
            println!("phase {phase}: skin3 {:?} map {:?}", skin[3], rig.clips.iter().find(|c| c.0 == 0x89E0_6A31).map(|c| c.1.channel_for_bone.clone()));
            let mut acc =vec![(Vec3::ZERO, Vec3::splat(f32::MAX), Vec3::splat(f32::MIN), 0usize); n];
            for (i, p) in positions.iter().enumerate() {
                let mut q = Vec3::ZERO;
                for k in 0..4 {
                    q += skin[joints[i][k] as usize].transform_point3(*p) * weights[i][k];
                }
                let (k, _) = weights[i].iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap();
                let b = joints[i][k] as usize;
                acc[b].0 += q;
                acc[b].1 = acc[b].1.min(q);
                acc[b].2 = acc[b].2.max(q);
                acc[b].3 += 1;
            }
            for (b, a) in acc.iter().enumerate().filter(|(_, a)| a.3 > 0) {
                println!("phase {phase}: bone {b} centroid {:.4} bounds {:.4} .. {:.4}", a.0 / a.3 as f32, a.1, a.2);
            }
        }
        for (k, p) in pieces.iter().enumerate() {
            let mut used: Vec<u8> = p.bone_indices.iter().flatten().copied().collect::<std::collections::BTreeSet<u8>>().into_iter().collect();
            used.truncate(16);
            let skin: Vec<u16> = p.skin_idx.iter().flatten().copied().collect::<std::collections::BTreeSet<u16>>().into_iter().take(16).collect();
            let c = p.positions.iter().map(|v| Vec3::from_array(*v)).sum::<Vec3>() / p.positions.len().max(1) as f32;
            println!("piece {k} section {}: {} verts, bone idx {used:?}, skin idx {skin:?}, centroid {c:.4}", p.source_section, p.positions.len());
        }
    }

    /// Data probe: the starter GMDL's keyed bindings, their 160-byte baked
    /// deform records as floats, and the part rig's bind / handle keys for
    /// comparison.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn probe_gmdl_baked_deforms() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let instance = std::env::var("OPENSPORE_GMDL_PROBE").ok().and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()).unwrap_or(crate::baked_model::PLAYER_BASELINE_INSTANCE);
        let key = index
            .records
            .keys()
            .filter(|k| k.instance_id == instance && k.type_id == crate::decode::TYPE_GMDL)
            .min_by_key(|k| k.group_id != crate::baked_model::CREATURE_GROUP)
            .copied()
            .unwrap_or(crate::dbpf::ResourceKey { type_id: crate::decode::TYPE_GMDL, group_id: crate::baked_model::CREATURE_GROUP, instance_id: instance });
        println!("model {:08X}!{:08X}; same instance: {:?}", key.group_id, key.instance_id, index.records.keys().filter(|k| k.instance_id == instance).map(|k| format!("{:08X}.{:08X}", k.group_id, k.type_id)).collect::<Vec<_>>());
        let model = crate::baked_model::parse(&index.read(key).unwrap()).unwrap();
        let mut cache = RigCache::default();
        let only = std::env::var("OPENSPORE_GMDL_PART").ok().and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok());
        for (i, data) in model.anim_data.iter().enumerate() {
            if only.is_some_and(|p| p != data.key.instance_id) {
                continue;
            }
            let rig = part_rig(&mut cache, &index, data.key);
            let anatomy = index
                .read(crate::dbpf::ResourceKey { type_id: crate::creature_data::TYPE, group_id: crate::original_species::CREATURE_ANATOMY_GROUP, instance_id: instance })
                .and_then(|b| crate::creature_data::parse(&b))
                .ok();
            if let Some(a) = &anatomy {
                for b in a.rigblocks.iter().filter(|b| b.instance_id() == data.key.instance_id) {
                    let start = b.morph_index().max(0) as usize;
                    let count = b.raw_bytes[0x11] as usize;
                    let e: Vec<String> = (start..start + count)
                        .filter_map(|k| Some(format!("{:08X}={:.3}/{:.3}", a.animation_ids.get(k)?, f32::from_bits(*a.animation_value_bits.get(k)?), f32::from_bits(*a.animation_weight_bits.get(k)?))))
                        .collect();
                    println!("  rigblock {} morphs {}", b.index(), e.join(" "));
                }
            }
            println!(
                "binding {i} key {:08X}!{:08X} groups {:?} rig bones {:?}",
                data.key.group_id,
                data.key.instance_id,
                data.baked_deforms.iter().map(|g| g.len()).collect::<Vec<_>>(),
                rig.as_ref().map(|r| r.bone_count())
            );
            for (g, group) in data.baked_deforms.iter().enumerate() {
                for (b, rec) in group.iter().enumerate().take(24) {
                    let f: Vec<String> = rec.chunks_exact(4).map(|c| format!("{:.3}", f32::from_le_bytes(c.try_into().unwrap()))).collect();
                    println!("  g{g} b{b}: {}", f.join(" "));
                }
            }
            if let Some(rig) = &rig {
                for (id, clip) in &rig.clips {
                    let keyed = clip.channel_for_bone.iter().filter(|c| c.is_some()).count();
                    let frames: Vec<String> = [0.0f32, 0.5, 1.0]
                        .iter()
                        .map(|p| {
                            let t = p * clip.anim.length;
                            let v: Vec<String> = (0..rig.bone_count())
                                .map(|b| {
                                    clip.channel_for_bone[b].and_then(|c| clip.anim.sample_frame_clamped(c, t)).map_or("-".into(), |k| format!("q{:.2?}t{:.3?}s{:.2?}", k.rotation, k.location, k.scale))
                                })
                                .collect();
                            format!("p{p}: {}", v.join(" "))
                        })
                        .collect();
                    println!("  clip {id:08X} skel {:08X} len {:.2} keyed {keyed}\n    {}", clip.anim.skeleton_id, clip.anim.length, frames.join("\n    "));
                }
            }
            // The part model of the same instance in the editor group.
            let part_key = crate::dbpf::ResourceKey { group_id: 0x4060_6000, ..data.key };
            if let Some(prig) = part_rig(&mut cache, &index, part_key) {
                println!("  part model rig {:08X}: {} bones {:08X?}, clips {:08X?}", part_key.instance_id, prig.bone_count(), prig.names, prig.clips.iter().map(|c| c.0).collect::<Vec<_>>());
                for b in 0..prig.bone_count() {
                    let l = prig.bind_local[b];
                    println!("    pbind b{b} {:08X} q{:.3?} t{:.3?} s{:.3?}", prig.names[b], l.rotation.to_array(), l.translation.to_array(), l.scale.to_array());
                }
            }
            if let Some(rig) = rig {
                for b in 0..rig.bone_count() {
                    let l = rig.bind_local[b];
                    let w = rig.inverse_bind[b].inverse();
                    println!(
                        "  bind b{b} {:08X} parent {:?} local q{:.3?} t{:.3?} s{:.3?} | world t{:.3?}",
                        rig.names[b],
                        rig.parents[b],
                        l.rotation.to_array(),
                        l.translation.to_array(),
                        l.scale.to_array(),
                        w.translation.to_array()
                    );
                }
                if let Ok(m) = index.read(crate::dbpf::ResourceKey { group_id: 0x4060_6000, ..data.key }).and_then(|b| crate::rw4::extract_morph_data(&b)) {
                    for h in &m.handles {
                        println!("  handle {:08X} default {:.3}", h.id, h.default_time);
                        if let Some(a) = &h.animation {
                            for c in a.channels.iter().take(24) {
                                let k: Vec<String> = c.keys.iter().map(|k| format!("@{:.3} q{:.3?} t{:.3?} s{:.3?}", k.time, k.rotation, k.location, k.scale)).collect();
                                println!("    ch {:08X}: {}", c.id, k.join(" | "));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Data probe: over every installed creature GMDL, do the binding rigs'
    /// deform clips reproduce the (handle-baked) binding bind pose at some
    /// phase? And how does the binding bind differ from the part model's?
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn probe_gmdl_binding_clips_vs_bind() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut cache = RigCache::default();
        let keys: Vec<crate::dbpf::ResourceKey> = index.records.keys().filter(|k| k.group_id == crate::baked_model::CREATURE_GROUP && k.type_id == crate::decode::TYPE_GMDL).copied().collect();
        let (mut bindings, mut matched, mut unmatched) = (0, 0, 0);
        let mut worst: Vec<(f32, String)> = Vec::new();
        for key in keys {
            let Ok(model) = index.read(key).and_then(|b| crate::baked_model::parse(&b)) else { continue };
            for data in model.anim_data.iter().skip(1) {
                let Some(rig) = part_rig(&mut cache, &index, data.key) else { continue };
                for (id, clip) in &rig.clips {
                    if clip.channel_for_bone.iter().all(|c| c.is_none()) {
                        continue;
                    }
                    bindings += 1;
                    let dev = |p: f32| {
                        let t = p * clip.anim.length;
                        let mut d = 0.0f32;
                        for (b, c) in clip.channel_for_bone.iter().enumerate() {
                            let Some(k) = c.and_then(|c| clip.anim.sample_frame_clamped(c, t)) else { continue };
                            let bind = rig.bind_local[b];
                            d = d.max(Vec3::from_array(k.location).distance(bind.translation)).max((Vec3::from_array(k.scale) - bind.scale).abs().max_element() * 0.1);
                        }
                        d
                    };
                    let best = (0..=20).map(|i| dev(i as f32 / 20.0)).fold(f32::MAX, f32::min);
                    if best < 2e-3 {
                        matched += 1;
                    } else {
                        unmatched += 1;
                        worst.push((best, format!("{:08X} binding {:08X} clip {id:08X}", key.instance_id, data.key.instance_id)));
                    }
                }
            }
        }
        worst.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!("binding clips {bindings}: some phase == bind {matched}, never {unmatched}");
        for (d, s) in worst.iter().take(25) {
            println!("  {d:.4} {s}");
        }
    }

    /// Data probe: installed creatures whose rigblocks set a skeletal handle
    /// far from its default (candidates for decoding the baked records).
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn probe_stretched_installed_handles() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let keys: Vec<crate::dbpf::ResourceKey> = index.records.keys().filter(|k| k.group_id == crate::original_species::CREATURE_ANATOMY_GROUP && k.type_id == crate::creature_data::TYPE).copied().collect();
        let mut handles: HashMap<u32, Option<Vec<(u32, f32, bool)>>> = HashMap::new();
        let mut found = 0;
        for key in keys {
            let gmdl = crate::dbpf::ResourceKey { type_id: crate::decode::TYPE_GMDL, group_id: crate::baked_model::CREATURE_GROUP, instance_id: key.instance_id };
            if !index.records.contains_key(&gmdl) {
                continue;
            }
            let Ok(data) = index.read(key).and_then(|b| crate::creature_data::parse(&b)) else { continue };
            for b in &data.rigblocks {
                let part = b.instance_id();
                let list = handles.entry(part).or_insert_with(|| {
                    let k = crate::dbpf::ResourceKey { type_id: crate::asset_catalog::RW4, group_id: 0x4060_6000, instance_id: part };
                    let m = index.read(k).and_then(|x| crate::rw4::extract_morph_data(&x)).ok()?;
                    Some(m.handles.iter().map(|h| (h.id, h.default_time, h.animation.as_ref().is_some_and(|a| a.channels.iter().all(|c| c.keys.iter().all(|k| !k.blend))))).collect())
                });
                let Some(list) = list else { continue };
                let start = b.morph_index();
                let count = b.raw_bytes[0x11] as usize;
                for k in 0..count {
                    let Some(i) = usize::try_from(start).ok().map(|s| s + k) else { continue };
                    let (Some(id), Some(v)) = (data.animation_ids.get(i), data.animation_value_bits.get(i)) else { continue };
                    let v = f32::from_bits(*v);
                    if let Some(&(_, d, skeletal)) = list.iter().find(|h| h.0 == *id) {
                        if skeletal && (v - d).abs() > 0.3 && found < 40 {
                            found += 1;
                            println!("creature {:08X} block {} part {part:08X} handle {id:08X} value {v:.3} default {d:.3}", key.instance_id, b.index());
                        }
                    }
                }
            }
        }
    }
}
