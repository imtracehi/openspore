//! Creature Creator limbs (`notes/skin-re-notes.md` section 3) and the plugin
//! of the creator-limbs modules (`creature_cell_convert`, `creature_handles`,
//! `creature_blocks`).
//!
//! Data (VERIFIED): a limb palette item is an assembly (`ItemBlockAssembly`,
//! `modelBlockAssemblyFile palette_editorModel~!x_leg_09.crt`); only the baked
//! `.creaturedata` is installed: a chain of limb segment rigblocks (type 3,
//! skinned, socket connector distance = segment length, ball connector scales
//! at both joints, thickness `mMuscleScale`, limb type 1 = the smooth 15-ball
//! tube) ending in a foot / hand rigblock placed at the last segment's socket.
//!
//! OpenSpore keeps a limb as one mounted part (the assembly item, no mesh)
//! carrying its chain ([`Limb`]: joints relative to the part's slot + offset,
//! in the player frame) and its end part as an ordinary part riding the last
//! joint (`PartEdits::on_limb`). The segments join the creature skin as limb
//! records (`creature_skin_view::skin_records` adds [`skin_limb_records`]), so
//! the skin's limb branch (section 1.2) draws them.
//!
//! Editing (`notes/limb-re-notes.md`, creator-limbs2): the limb part is
//! placed, mirrored and sold like any part (`editor`, `creature_editor_edit`);
//! a joint ball is dragged with the retail Limb manipulator rules
//! (`creature_limb_solver`: the foot slides on the floor and re-places the
//! joints above it proportionally, hand / knee / elbow take the best point of
//! 400 along the mouse ray under the installed tunings, a knee leaves the foot
//! planted, the root ball re-pins the limb on the body); pressing the limb's
//! skin carries the whole limb (from its root); Ctrl + drag on a section splits
//! the limb there (the section and everything below leave as a new limb),
//! Ctrl + click on a section removes it (its block's price comes back),
//! Ctrl + dropping a limb on another limb's free end joins them and on any
//! other joint of a limb makes it a branch hanging from that joint
//! (`Limb::parent`); the mouse wheel over a joint ball scales it
//! (0.05 per notch), over a section changes its thickness (0.025 per notch);
//! every edit is one undo step. Rigid (exo) assemblies (`.bem` only,
//! `creature_bem`) are chains of segment parts riding the limb
//! (`PartEdits::segment`) instead of skin. ADAPTED: feet stay on their limb
//! until grabbed; joint balls are always shown on limbs (retail: on rollover);
//! the search segment of the line search is the ray from the camera to the
//! floor (retail clips it to the creature's blocks, `FUN_005B1970`); OpenSpore
//! prices a limb per assembly (a limb remembers what removals refunded and
//! joins added, `Limb::extra_value`).

use std::collections::HashMap;
use std::sync::Mutex;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    asset_catalog::{ResourceIndex, CREATURE_PARTS},
    cell_editor_data::{key_of, prop_key},
    creature_editor::{spore_to_bevy, CreatureEditor, CreatureEditorOwned, Mode},
    creature_editor_data::CreatureEditorData,
    creature_handles::{PartEdits, HANDLE_ACTIVE, HANDLE_COLOR, HANDLE_ROLLOVER, TWIN_BIT},
    creature_limb_solver::{clamp_bounds, clamp_length, joint_kind, line_search, place, score, EndKind, JointKind, ScoreCtx, Tunings, SAMPLES},
    creature_skin::{self as skin, BlockKind, SkinRecord},
    dbpf::ResourceKey,
    editor::{Action, EditorState},
    game::{PartInstance, Player, RunProgress},
    stages::{GameStage, StageOwned},
    Originals,
};

/// `modelBlockAssemblyFile`.
pub const ASSEMBLY_FILE: u32 = 0x022E_8410;
/// Segment length range: `LimbRigblockTemplate` socket connector offset
/// (0, -0.03, 0)..(0, -3, 0) (creature units).
pub const MIN_SEGMENT: f32 = 0.03;
pub const MAX_SEGMENT: f32 = 3.0;
/// Ball connector scale range (base template 0.55..2.8).
pub const MIN_BALL: f32 = 0.55;
pub const MAX_BALL: f32 = 2.8;

/// A limb chain (a limb assembly's segments), saved with its part.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limb {
    /// Joins the end part (`PartEdits::on_limb`); unique on the creature,
    /// the twin's differs in `creature_handles::TWIN_BIT`.
    pub id: u32,
    /// Joint positions relative to the limb's root (the part's slot + offset)
    /// in the player frame; `joints[0]` is the root (zero).
    pub joints: Vec<[f32; 3]>,
    /// Ball-connector scale per joint (`mBaseMuscleScale`, 0.55..2.8).
    pub balls: Vec<f32>,
    /// Thickness per segment (`mMuscleScale`, 0..1).
    pub muscle: Vec<f32>,
    /// Segment part PROP instances.
    pub segments: Vec<u32>,
    /// Limb type 1 (the smooth 15-ball tube) rather than muscle balls.
    pub tentacle: bool,
    /// Limb type 2: rigid (exo) segments, drawn as their own part meshes
    /// (segment parts riding the limb, `PartEdits::segment`), no skin.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub rigid: bool,
    /// Per section: rigid (a chain mixing skinned and rigid sections, e.g.
    /// `exo_a_07`); empty = every section as `rigid`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rigid_segments: Vec<bool>,
    /// The assembly's joints (relative to the root, player frame): the rest
    /// pose the rigid segments' meshes were authored in.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rest: Vec<[f32; 3]>,
    /// Rest rotation of each rigid segment's mesh (player frame quaternion).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<[f32; 4]>,
    /// Ball-connector scale range of the segments (0 = the template's).
    #[serde(skip_serializing_if = "is_zero2")]
    pub ball_range: [f32; 2],
    /// Socket length range of the segments, creature units (0 = template).
    #[serde(skip_serializing_if = "is_zero2")]
    pub length_range: [f32; 2],
    /// A branch: the limb (id) this limb is rooted on (0 = the body) and the
    /// joint of that limb its root sits on (`parent_joint` >= 1). Retail
    /// attaches a Ctrl-dropped limb root to the limb block it is dropped on
    /// (`FUN_004A2EB0`), which then has two children.
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub parent: u32,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub parent_joint: u32,
    /// DNA this limb is worth beyond its palette item's price x sell-back:
    /// removed sections refunded (negative), limbs joined onto it (positive).
    #[serde(skip_serializing_if = "is_zero_f32")]
    pub extra_value: f32,
}

fn is_zero2(v: &[f32; 2]) -> bool {
    *v == [0.0, 0.0]
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}

impl Limb {
    /// The twin's limb: joints reflected across x = 0.
    pub fn mirrored(&self) -> Self {
        Self {
            id: if self.id == 0 { 0 } else { self.id ^ TWIN_BIT },
            parent: if self.parent == 0 { 0 } else { self.parent ^ TWIN_BIT },
            joints: self.joints.iter().map(|[x, y, z]| [-x, *y, *z]).collect(),
            rest: self.rest.iter().map(|[x, y, z]| [-x, *y, *z]).collect(),
            frames: self.frames.iter().map(|[x, y, z, w]| [*x, -y, -z, *w]).collect(),
            ..self.clone()
        }
    }
    pub fn joint(&self, k: usize) -> Vec3 {
        self.joints.get(k).map_or(Vec3::ZERO, |j| Vec3::from_array(*j))
    }
    pub fn end(&self) -> Vec3 {
        self.joints.last().map_or(Vec3::ZERO, |j| Vec3::from_array(*j))
    }
    /// Ball-connector scale range (`modelMin/MaxBallConnectorScale`).
    pub fn balls_range(&self) -> (f32, f32) {
        if self.ball_range[1] > 0.0 {
            (self.ball_range[0], self.ball_range[1])
        } else {
            (MIN_BALL, MAX_BALL)
        }
    }
    /// Socket length range in creature units (`modelMin/MaxSocketConnectorOffset`).
    pub fn lengths(&self) -> (f32, f32) {
        if self.length_range[1] > 0.0 {
            (self.length_range[0], self.length_range[1])
        } else {
            (MIN_SEGMENT, MAX_SEGMENT)
        }
    }
    pub fn segment_count(&self) -> usize {
        self.joints.len().saturating_sub(1)
    }
    /// Section `k` is a rigid segment part (not skin).
    pub fn is_rigid(&self, k: usize) -> bool {
        self.rigid_segments.get(k).copied().unwrap_or(self.rigid)
    }
    fn rigid_mask(&self) -> Vec<bool> {
        (0..self.segment_count()).map(|k| self.is_rigid(k)).collect()
    }
    /// Store `mask` compactly (`rigid` alone when every section agrees).
    fn set_rigid_mask(&mut self, mask: Vec<bool>) {
        if mask.iter().all(|r| *r) || mask.iter().all(|r| !*r) {
            self.rigid = mask.first().copied().unwrap_or(self.rigid);
            self.rigid_segments.clear();
        } else {
            self.rigid = true;
            self.rigid_segments = mask;
        }
    }
    /// Some section is skin.
    pub fn has_skin(&self) -> bool {
        (0..self.segment_count()).any(|k| !self.is_rigid(k))
    }

    /// Ctrl + click on section `k` (between joints `k` and `k + 1`): the
    /// section goes, the joints below move up by its length. Not the first
    /// section (the shoulder / hip joint stays), never the last one left.
    pub fn remove_section(&mut self, k: usize) -> bool {
        let n = self.segment_count();
        if k == 0 || k >= n || n < 2 {
            return false;
        }
        let mut mask = self.rigid_mask();
        mask.remove(k);
        let shift = self.joint(k) - self.joint(k + 1);
        let mut joints: Vec<[f32; 3]> = self.joints[..=k].to_vec();
        joints.extend(self.joints[k + 2..].iter().map(|j| (Vec3::from_array(*j) + shift).to_array()));
        self.joints = joints;
        if self.rest.len() == n + 1 {
            let rs = Vec3::from_array(self.rest[k]) - Vec3::from_array(self.rest[k + 1]);
            let mut rest: Vec<[f32; 3]> = self.rest[..=k].to_vec();
            rest.extend(self.rest[k + 2..].iter().map(|j| (Vec3::from_array(*j) + rs).to_array()));
            self.rest = rest;
        }
        fn drop_at<T>(v: &mut Vec<T>, i: usize) {
            if i < v.len() {
                v.remove(i);
            }
        }
        if self.balls.len() == n + 1 {
            self.balls.remove(k + 1);
        }
        drop_at(&mut self.muscle, k);
        drop_at(&mut self.segments, k);
        drop_at(&mut self.frames, k);
        self.set_rigid_mask(mask);
        true
    }

    /// Ctrl + drag on section `k >= 1`: this limb keeps joints `0..=k`; the
    /// returned limb (id `id`) holds joints `k..` re-rooted at joint `k`.
    pub fn split_at(&mut self, k: usize, id: u32) -> Option<Limb> {
        let n = self.segment_count();
        if k == 0 || k >= n {
            return None;
        }
        let base = self.joint(k);
        let mask = self.rigid_mask();
        let rel = |v: &[[f32; 3]], at: Vec3| v.iter().map(|j| (Vec3::from_array(*j) - at).to_array()).collect::<Vec<_>>();
        let rest_base = self.rest.get(k).map_or(Vec3::ZERO, |r| Vec3::from_array(*r));
        let tail = Limb {
            id,
            joints: rel(&self.joints[k..], base),
            balls: self.balls.get(k..).map(<[f32]>::to_vec).unwrap_or_default(),
            muscle: self.muscle.get(k..).map(<[f32]>::to_vec).unwrap_or_default(),
            segments: self.segments.get(k..).map(<[u32]>::to_vec).unwrap_or_default(),
            rest: self.rest.get(k..).map(|r| rel(r, rest_base)).unwrap_or_default(),
            frames: self.frames.get(k..).map(<[[f32; 4]]>::to_vec).unwrap_or_default(),
            ..self.clone()
        };
        let mut tail = tail;
        tail.parent = 0;
        tail.parent_joint = 0;
        tail.extra_value = 0.0;
        tail.set_rigid_mask(mask[k..].to_vec());
        self.set_rigid_mask(mask[..k].to_vec());
        self.joints.truncate(k + 1);
        self.balls.truncate(k + 1);
        self.muscle.truncate(k);
        self.segments.truncate(k);
        self.rest.truncate((k + 1).min(self.rest.len()));
        self.frames.truncate(k.min(self.frames.len()));
        Some(tail)
    }

    /// Ctrl + drop of `other` onto this limb's end: its sections continue
    /// this chain.
    pub fn join(&mut self, other: &Limb) -> bool {
        if other.joints.len() < 2 || self.joints.is_empty() {
            return false;
        }
        let mut mask = self.rigid_mask();
        mask.extend(other.rigid_mask());
        // Rest poses and frames exist for chains with rigid sections.
        let pad = |l: &Limb| -> (Vec<[f32; 3]>, Vec<[f32; 4]>) {
            let rest = if l.rest.len() == l.joints.len() { l.rest.clone() } else { l.joints.clone() };
            let frames = if l.frames.len() == l.segment_count() { l.frames.clone() } else { vec![[0.0, 0.0, 0.0, 1.0]; l.segment_count()] };
            (rest, frames)
        };
        let any_rigid = mask.iter().any(|r| *r);
        let (mine, theirs) = (pad(self), pad(other));
        let end = self.end();
        let root = other.joint(0);
        self.joints.extend(other.joints[1..].iter().map(|j| (end + Vec3::from_array(*j) - root).to_array()));
        if let (Some(last), Some(first)) = (self.balls.last_mut(), other.balls.first()) {
            *last = last.max(*first);
        }
        self.balls.extend(other.balls.iter().skip(1));
        self.muscle.extend(&other.muscle);
        self.segments.extend(&other.segments);
        if any_rigid {
            let (mut rest, mut frames) = mine;
            let rend = Vec3::from_array(*rest.last().unwrap());
            let r0 = Vec3::from_array(theirs.0[0]);
            rest.extend(theirs.0[1..].iter().map(|j| (rend + Vec3::from_array(*j) - r0).to_array()));
            frames.extend(theirs.1);
            self.rest = rest;
            self.frames = frames;
        }
        self.set_rigid_mask(mask);
        true
    }
}

/// Move joint `k` (>= 1) toward `target` (both relative to the root): its
/// parent segment re-aims at the target with its length clamped to
/// `[min_len, max_len]`, the joints below follow rigidly (section 3.3).
pub fn drag_joint(limb: &mut Limb, k: usize, target: Vec3, min_len: f32, max_len: f32) -> bool {
    if k == 0 || k >= limb.joints.len() || !target.is_finite() {
        return false;
    }
    let q = limb.joint(k - 1);
    let v = target - q;
    let len = v.length();
    if len < 1e-6 {
        return false;
    }
    let goal = q + v / len * len.clamp(min_len, max_len);
    let delta = goal - limb.joint(k);
    if delta.length_squared() < 1e-12 {
        return false;
    }
    for j in &mut limb.joints[k..] {
        *j = (Vec3::from_array(*j) + delta).to_array();
    }
    true
}

// ---------------------------------------------------------------------------
// Assemblies
// ---------------------------------------------------------------------------

/// A limb assembly read from its baked `.creaturedata` (creature space), or
/// from its `.bem` (the rigid exo assemblies ship nothing else).
#[derive(Clone, Debug)]
pub struct Assembly {
    pub item: u32,
    /// Segments in chain order (root first).
    pub segments: Vec<SkinRecord>,
    pub segment_parts: Vec<u32>,
    /// The end block (foot / hand): part PROP (group, instance).
    pub end: Option<(u32, u32)>,
    /// Limb type 2: (some of) the segments are rigid part meshes.
    pub rigid: bool,
    /// Per segment: rigid.
    pub rigid_segments: Vec<bool>,
    /// Deform handle weights of each segment (`.bem`).
    pub segment_handles: Vec<Vec<(u32, f32)>>,
    /// The end block's position (creature space) and scale (`.bem`).
    pub end_position: Option<Vec3>,
    pub end_scale: f32,
}

/// The assembly of a `.bem` (`creature_bem`): the segment chain with each
/// segment ending at its child block (the next segment or the end block).
pub fn assembly_from_bem(item: u32, blocks: &[crate::creature_bem::BemBlock]) -> Option<Assembly> {
    let (chain, end) = crate::creature_bem::limb_chain(blocks)?;
    let target = |k: usize| chain.get(k + 1).or(end.as_ref()).map(|&i| blocks[i].position);
    let mut segments = Vec::new();
    for (k, &i) in chain.iter().enumerate() {
        let b = &blocks[i];
        // The socket direction is the block's local -Y; a last segment without
        // an end block keeps the previous length.
        let len = target(k).map(|t| t.distance(b.position)).or_else(|| segments.last().map(|s: &SkinRecord| s.socket_distance)).unwrap_or(0.3);
        let next_ball = chain.get(k + 1).or(end.as_ref()).map_or(1.0, |&j| blocks[j].ball_scale);
        segments.push(SkinRecord {
            index: k as i16,
            parent: k as i16 - 1,
            kind: BlockKind::Limb,
            flags: if b.limb_type == 2 { 0 } else { skin::flag::USES_SKIN } | if b.limb_type == 1 { skin::flag::TENTACLE } else { 0 },
            scale: b.scale,
            orientation: b.orientation,
            position: b.position,
            socket_distance: len,
            muscle_scale: b.muscle_scale,
            base_muscle_scale: b.ball_scale,
            next_muscle_scale: next_ball,
            ..Default::default()
        });
    }
    Some(Assembly {
        item,
        segment_parts: chain.iter().map(|&i| blocks[i].instance).collect(),
        segment_handles: chain.iter().map(|&i| blocks[i].handles.clone()).collect(),
        end: end.map(|e| (blocks[e].group, blocks[e].instance)),
        end_position: end.map(|e| blocks[e].position),
        end_scale: end.map_or(1.0, |e| blocks[e].scale),
        rigid: chain.iter().any(|&i| blocks[i].limb_type == 2),
        rigid_segments: chain.iter().map(|&i| blocks[i].limb_type == 2).collect(),
        segments,
    })
}

/// The `.creaturedata` key of a limb assembly item (`modelBlockAssemblyFile`
/// names the `.crt`; the baked resource has the same group and instance).
pub fn assembly_key(index: &ResourceIndex, item: u32) -> Option<ResourceKey> {
    let p = index.properties(prop_key(CREATURE_PARTS, item)).ok()?;
    let k = key_of(&p, ASSEMBLY_FILE).filter(|k| k.instance_id != 0)?;
    Some(ResourceKey { type_id: crate::creature_data::TYPE, group_id: k.group_id, instance_id: k.instance_id })
}

/// The segment chain and end block of an assembly's creature data.
pub fn parse_assembly(item: u32, data: &crate::creature_data::CreatureData) -> Option<Assembly> {
    let records = skin::records_from_creature_data(data);
    let pos = |index: i16| records.iter().position(|r| r.index == index);
    let is_limb = |r: &SkinRecord| r.kind == BlockKind::Limb;
    let root = records.iter().position(|r| is_limb(r) && pos(r.parent).is_none_or(|p| !is_limb(&records[p])))?;
    let mut chain = vec![root];
    while let Some(next) = records.iter().position(|r| is_limb(r) && r.parent == records[*chain.last().unwrap()].index) {
        if chain.contains(&next) {
            break;
        }
        chain.push(next);
    }
    let last = records[*chain.last().unwrap()].index;
    let end_index = records.iter().position(|r| !is_limb(r) && r.parent == last);
    let end = end_index.map(|i| {
        let b = &data.rigblocks[i];
        (b.group_id(), b.instance_id())
    });
    Some(Assembly {
        item,
        segments: chain.iter().map(|&i| records[i].clone()).collect(),
        segment_parts: chain.iter().map(|&i| data.rigblocks[i].instance_id()).collect(),
        end,
        rigid: false,
        rigid_segments: Vec::new(),
        segment_handles: Vec::new(),
        end_position: None,
        end_scale: end_index.map_or(1.0, |i| records[i].scale.max(1e-3)),
    })
}

static ASSEMBLIES: Mutex<Option<HashMap<u32, Option<Assembly>>>> = Mutex::new(None);

/// The assembly of a palette item (cached; `None` = not a limb assembly):
/// the baked `.creaturedata`, else the `.bem` (rigid exo assemblies).
pub fn assembly(index: &ResourceIndex, item: u32) -> Option<Assembly> {
    let mut guard = ASSEMBLIES.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    map.entry(item)
        .or_insert_with(|| {
            let key = assembly_key(index, item)?;
            if let Some(a) = index.read(key).ok().and_then(|b| crate::creature_data::parse(&b).ok()).and_then(|d| parse_assembly(item, &d)) {
                return Some(a);
            }
            let bem = ResourceKey { type_id: crate::creature_bem::TYPE, ..key };
            let blocks = crate::creature_bem::parse(&index.read(bem).ok()?).ok()?;
            assembly_from_bem(item, &blocks)
        })
        .clone()
}

pub fn is_assembly(index: &ResourceIndex, item: u32) -> bool {
    !index.records.is_empty() && assembly(index, item).is_some()
}

/// The limb of an assembly in the player frame (`units` player units per
/// creature unit, the skin's): joints at the segment starts and the last
/// socket, relative to the first.
pub fn limb_of(asm: &Assembly, units: f32, id: u32) -> Limb {
    limb_of_with(asm, units, id, |_| None)
}

/// `spore_to_bevy` as a rotation.
pub fn spore_to_bevy_mat() -> Mat3 {
    Mat3::from_cols(spore_to_bevy(Vec3::X), spore_to_bevy(Vec3::Y), spore_to_bevy(Vec3::Z))
}

/// Model -> mesh rotation of a part (`creature_handles::load_part_handles`).
pub fn mesh_basis(def: &crate::asset_catalog::PartDefinition) -> Quat {
    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
        * Quat::from_euler(
            EulerRot::XYZ,
            def.model_rotation[0].to_radians(),
            def.model_rotation[1].to_radians(),
            def.model_rotation[2].to_radians(),
        )
}

/// [`limb_of`]; `basis(segment part)` = its model -> mesh rotation (rigid
/// assemblies: each segment mesh's rest rotation is `S R B^-1`, S =
/// `spore_to_bevy`, R = the block orientation (rows = local axes), B = basis).
pub fn limb_of_with(asm: &Assembly, units: f32, id: u32, basis: impl Fn(u32) -> Option<Quat>) -> Limb {
    let s0 = asm.segments.first().map_or(Vec3::ZERO, |s| s.position);
    let to = |v: Vec3| (spore_to_bevy(v - s0) * units).to_array();
    let mut joints: Vec<[f32; 3]> = asm.segments.iter().map(|s| to(s.position)).collect();
    if let Some(last) = asm.segments.last() {
        joints.push(to(asm.end_position.filter(|_| asm.rigid).unwrap_or_else(|| last.segment_end())));
    }
    let mut balls: Vec<f32> = asm.segments.iter().map(|s| s.base_muscle_scale).collect();
    balls.push(asm.segments.last().map_or(1.0, |s| if s.next_muscle_scale < 0.0 { 1.0 } else { s.next_muscle_scale }));
    let s = spore_to_bevy_mat();
    let frames = if asm.rigid {
        asm.segments
            .iter()
            .zip(&asm.segment_parts)
            .enumerate()
            .map(|(k, (seg, part))| {
                if !asm.rigid_segments.get(k).copied().unwrap_or(true) {
                    return [0.0, 0.0, 0.0, 1.0];
                }
                let r = Mat3::from_cols(seg.orientation[0], seg.orientation[1], seg.orientation[2]);
                let b = basis(*part).unwrap_or(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
                let q = Quat::from_mat3(&(s * r * Mat3::from_quat(b.inverse())));
                q.normalize().to_array()
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut limb = Limb {
        id,
        rest: if asm.rigid { joints.clone() } else { Vec::new() },
        joints,
        balls,
        muscle: asm.segments.iter().map(|s| s.muscle_scale).collect(),
        segments: asm.segment_parts.clone(),
        tentacle: asm.segments.first().is_some_and(|s| s.flags & skin::flag::TENTACLE != 0),
        rigid: asm.rigid,
        frames,
        // The exo segment template (`ce_limb_exo_*`): ball 0.7..1.5, socket
        // offset 0.035..3.75 (VERIFIED data, `limb-re-notes` 5).
        ball_range: if asm.rigid { [0.7, 1.5] } else { [0.0; 2] },
        length_range: if asm.rigid { [0.035, 3.75] } else { [0.0; 2] },
        rigid_segments: Vec::new(),
        ..Default::default()
    };
    if asm.rigid {
        limb.set_rigid_mask(asm.rigid_segments.clone());
    }
    limb
}

/// The `PartInstance::scale` that draws a part's mesh at the skin's size: retail
/// draws a block's model at `modelScale x block scale` creature units, the
/// skin uses `units` player units per creature unit, and `editor` draws a part
/// mesh at `model.scale x 0.5 x p.scale` (its import scale).
pub fn skin_scale(originals: &Originals, part: &PartInstance, units: f32, block_scale: f32) -> Option<f32> {
    let def = originals.part_catalog.by_part_in(CREATURE_PARTS, part)?;
    let model = crate::editor::model_for_part(originals, part)?;
    (model.scale > 0.0).then(|| def.model_scale.max(1e-3) * block_scale * units / (model.scale * 0.5))
}

/// Where a rigid segment part sits: joint `k`, its mesh turned from the rest
/// pose onto the segment's direction, and the segment's stretch (length now /
/// at rest). ADAPTED: the length change stretches the whole mesh (the exo
/// segments' four deform handles are width handles; how retail draws a
/// segment longer than its mesh is not traced).
pub fn segment_pose(limb: &Limb, k: usize) -> Option<(Vec3, Quat, f32)> {
    let (a, b) = (limb.joints.get(k)?, limb.joints.get(k + 1)?);
    let (a, b) = (Vec3::from_array(*a), Vec3::from_array(*b));
    let frame = Quat::from_array(*limb.frames.get(k)?).normalize();
    let rest = match (limb.rest.get(k), limb.rest.get(k + 1)) {
        (Some(r0), Some(r1)) => Vec3::from_array(*r1) - Vec3::from_array(*r0),
        _ => b - a,
    };
    let cur = b - a;
    if cur.length_squared() < 1e-12 || rest.length_squared() < 1e-12 {
        return Some((a, frame, 1.0));
    }
    let turn = Quat::from_rotation_arc(rest.normalize(), cur.normalize());
    Some((a, (turn * frame).normalize(), cur.length() / rest.length()))
}

// ---------------------------------------------------------------------------
// Skin
// ---------------------------------------------------------------------------

/// A limb as the skin sees it (player frame).
#[derive(Clone, Debug, PartialEq)]
pub struct LimbGeom {
    pub joints: Vec<Vec3>,
    pub balls: Vec<f32>,
    pub muscle: Vec<f32>,
    pub tentacle: bool,
    /// Per section: drawn by the skin (empty = all).
    pub skinned: Vec<bool>,
    /// A branch: (index of the parent limb in the list, its joint).
    pub parent: Option<(usize, usize)>,
}

static SKIN_LIMBS: Mutex<Vec<LimbGeom>> = Mutex::new(Vec::new());

/// The player's limbs for the visible skin (`creature_skin_view::build`).
pub fn skin_limb_records(blocks: &[SkinRecord], units: f32) -> Vec<SkinRecord> {
    let limbs = SKIN_LIMBS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    limb_records(&limbs, blocks, units)
}

/// Limb segment records (section 1.1 fields) appended after `blocks` (the
/// vertebrae; indices are list positions): each chain hangs from the nearest
/// vertebra, a branch from the record of its parent limb's segment that ends
/// at its joint (parents are emitted first); segment k runs from joint k to
/// joint k + 1 with `S + Rot * (0, -L, 0)` = the next joint. A rigid section
/// (`skinned` false) is a record without `USES_SKIN`: the skin ignores it (as
/// retail's rigid limb rigblocks, which are rigblocks of the chain without
/// skin), the clip player gets its joint, which the section's segment part
/// follows.
pub fn limb_records(limbs: &[LimbGeom], blocks: &[SkinRecord], units: f32) -> Vec<SkinRecord> {
    let to_c = |p: Vec3| spore_to_bevy(p) / units;
    let mut out = Vec::new();
    // Parents before their branches.
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some((p, _)) = limbs[i].parent.filter(|(p, _)| *p < limbs.len() && d < 16) {
            i = p;
            d += 1;
        }
        d
    };
    let mut order: Vec<usize> = (0..limbs.len()).collect();
    order.sort_by_key(|&i| depth(i));
    // Per limb, the record that ends at each joint (index = joint).
    let mut ends: Vec<Vec<Option<i16>>> = vec![Vec::new(); limbs.len()];
    for li in order {
        let limb = &limbs[li];
        if limb.joints.len() < 2 {
            continue;
        }
        let s0 = to_c(limb.joints[0]);
        let branch = limb.parent.and_then(|(p, j)| ends.get(p).and_then(|e| e.get(j).copied().flatten()));
        let mut parent = branch.unwrap_or_else(|| {
            blocks
                .iter()
                .enumerate()
                .filter(|(_, b)| b.kind == BlockKind::Vertebra)
                .min_by(|a, b| a.1.vertebra_ball().0.distance_squared(s0).total_cmp(&b.1.vertebra_ball().0.distance_squared(s0)))
                .map_or(-1, |(i, _)| i as i16)
        });
        ends[li] = vec![None; limb.joints.len()];
        let skinned = |k: usize| limb.skinned.get(k).copied().unwrap_or(true);
        for k in 0..limb.joints.len() - 1 {
            let (s, e) = (to_c(limb.joints[k]), to_c(limb.joints[k + 1]));
            let len = s.distance(e);
            if len < 1e-5 {
                continue;
            }
            let r1 = (s - e) / len;
            let r0 = (Vec3::X - r1 * r1.x).try_normalize().unwrap_or_else(|| r1.any_orthonormal_vector());
            let r2 = r0.cross(r1);
            let index = (blocks.len() + out.len()) as i16;
            ends[li][k + 1] = Some(index);
            out.push(SkinRecord {
                index,
                parent,
                flags: if skinned(k) { skin::flag::USES_SKIN } else { 0 }
                    | skin::flag::BALL_JOINT
                    | if limb.tentacle { skin::flag::TENTACLE } else { 0 },
                kind: BlockKind::Limb,
                orientation: [r0, r1, r2],
                position: s,
                socket_distance: len,
                muscle_scale: limb.muscle.get(k).copied().unwrap_or(0.5),
                base_muscle_scale: limb.balls.get(k).copied().unwrap_or(1.0),
                next_muscle_scale: limb.balls.get(k + 1).copied().unwrap_or(1.0),
                ..Default::default()
            });
            parent = index;
        }
    }
    out
}

/// Player units per creature unit of the current skin.
pub fn skin_units(originals: &Originals, body: &crate::body::BodyProfile) -> f32 {
    crate::creature_skin_view::skin_records(originals.baked_body.as_ref(), body).1
}

/// The limb root (player frame): the part's slot + offset.
pub fn root_of(originals: &Originals, body: &crate::body::BodyProfile, p: &PartInstance) -> Vec3 {
    crate::baked_model::attachment_point(originals, body, p.bone) + Vec3::from_array(p.offset)
}

fn geoms(originals: &Originals, run: &RunProgress) -> Vec<LimbGeom> {
    let limb_ids: Vec<u32> = run.parts.iter().filter_map(|p| p.edits.limb.as_ref().map(|l| l.id)).collect();
    run.parts
        .iter()
        .filter_map(|p| {
            // Rigid sections are drawn by their segment parts, not the skin;
            // they still give (non-skin) limb records, the bones their
            // segment parts follow in clips (`limb_records`).
            let limb = p.edits.limb.as_ref()?;
            let root = root_of(originals, &run.body, p);
            Some(LimbGeom {
                joints: limb.joints.iter().map(|j| root + Vec3::from_array(*j)).collect(),
                balls: limb.balls.clone(),
                muscle: limb.muscle.clone(),
                tentacle: limb.tentacle,
                skinned: if limb.rigid { (0..limb.segment_count()).map(|k| !limb.is_rigid(k)).collect() } else { Vec::new() },
                parent: (limb.parent != 0)
                    .then(|| limb_ids.iter().position(|&id| id == limb.parent).or_else(|| limb_ids.iter().position(|&id| id == limb.parent ^ TWIN_BIT)))
                    .flatten()
                    .map(|i| (i, limb.parent_joint as usize)),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// End parts and twins (pure)
// ---------------------------------------------------------------------------

/// The limb part a branch hangs from: limb `parent`, else its twin's id (a
/// branch twin on a limb that has no twin hangs from the same limb).
fn parent_index(parts: &[PartInstance], parent: u32) -> Option<usize> {
    let find = |id: u32| parts.iter().position(|q| q.edits.limb.as_ref().is_some_and(|l| l.id == id));
    find(parent).or_else(|| find(parent ^ TWIN_BIT))
}

/// Branch roots ride their parent limb's joint (same slot, offset = parent
/// offset + joint), parents first; when `prune`, a branch whose parent is gone
/// goes too (retail deletes a block's children with it).
fn settle_branches(parts: &mut Vec<PartInstance>, prune: bool) {
    for _ in 0..8 {
        let mut changed = false;
        let mut doomed = Vec::new();
        for i in 0..parts.len() {
            let Some((parent, joint)) = parts[i].edits.limb.as_ref().filter(|l| l.parent != 0).map(|l| (l.parent, l.parent_joint as usize)) else { continue };
            let Some(pi) = parent_index(parts, parent).filter(|&pi| pi != i) else {
                doomed.push(i);
                continue;
            };
            let (bone, offset, at) = {
                let q = &parts[pi];
                let l = q.edits.limb.as_ref().unwrap();
                (q.bone, q.offset, l.joint(joint.min(l.joints.len().saturating_sub(1))))
            };
            let offset = crate::game::clamp_offset((Vec3::from_array(offset) + at).to_array(), crate::game::OFFSET_LIMIT);
            let p = &mut parts[i];
            if p.bone != bone || p.offset != offset {
                p.bone = bone;
                p.offset = offset;
                changed = true;
            }
        }
        if prune && !doomed.is_empty() {
            let ids: Vec<u32> = doomed.iter().filter_map(|&i| parts[i].edits.limb.as_ref().map(|l| l.id)).collect();
            parts.retain(|q| !q.edits.limb.as_ref().is_some_and(|l| ids.contains(&l.id)));
            changed = true;
        }
        if !changed {
            break;
        }
    }
}

/// Keep end parts on their limbs: an end part rides its limb's last joint
/// (same slot, offset = limb offset + end); a rigid segment part rides its
/// joints ([`segment_pose`]; `base(part)` = its scale at rest, see
/// [`skin_scale`]); a limb twin gets mirrored copies of its twin's end and segment
/// parts; parts of removed limbs (or past the end of a shortened rigid limb)
/// are removed when `prune`. Returns whether anything changed.
pub fn settle_ends(parts: &mut Vec<PartInstance>, moving: Option<usize>, prune: bool, base: &dyn Fn(&PartInstance) -> f32) -> bool {
    let before = parts.clone();
    // A grabbed end part leaves its limb (a grabbed segment part is taken by
    // `press` before the editor sees it); a grabbed branch leaves its parent.
    if let Some(m) = moving.and_then(|m| parts.get_mut(m)) {
        if m.edits.segment == 0 {
            m.edits.on_limb = 0;
        }
        if let Some(l) = m.edits.limb.as_mut() {
            l.parent = 0;
            l.parent_joint = 0;
        }
    }
    settle_branches(parts, prune);
    let limbs: Vec<(u32, u32, [f32; 3], f32, Limb)> = parts
        .iter()
        .filter_map(|p| p.edits.limb.as_ref().map(|l| (l.id, p.bone, p.offset, p.yaw, l.clone())))
        .collect();
    // Twins of end and segment parts.
    let mut next_pair = parts.iter().map(|p| p.pair_id).max().unwrap_or(0);
    let mut added = Vec::new();
    for i in 0..parts.len() {
        let e = &parts[i];
        if e.edits.on_limb == 0 {
            continue;
        }
        let (twin, seg) = (e.edits.on_limb ^ TWIN_BIT, e.edits.segment);
        let has_limb = limbs.iter().any(|l| l.0 == twin);
        let rides = |p: &PartInstance| p.edits.on_limb == twin && p.edits.segment == seg;
        let has_end = parts.iter().any(rides) || added.iter().any(rides);
        if has_limb && !has_end {
            let pair = if e.pair_id != 0 {
                e.pair_id
            } else {
                next_pair += 1;
                next_pair
            };
            parts[i].pair_id = pair;
            let e = &parts[i];
            let mut m = e.clone();
            m.mirrored = !e.mirrored;
            m.yaw = -e.yaw;
            m.edits = e.edits.mirrored();
            added.push(m);
        }
    }
    parts.extend(added);
    if prune {
        parts.retain(|p| {
            if p.edits.on_limb == 0 {
                return true;
            }
            let Some(l) = limbs.iter().find(|l| l.0 == p.edits.on_limb) else { return false };
            p.edits.segment == 0 || ((p.edits.segment as usize) <= l.4.segment_count() && l.4.is_rigid(p.edits.segment as usize - 1))
        });
    }
    for p in parts.iter_mut() {
        if p.edits.on_limb == 0 {
            continue;
        }
        let Some(l) = limbs.iter().find(|l| l.0 == p.edits.on_limb) else { continue };
        p.bone = l.1;
        if p.edits.segment == 0 {
            p.offset = crate::game::clamp_offset((Vec3::from_array(l.2) + l.4.end()).to_array(), crate::game::OFFSET_LIMIT);
            p.yaw = l.3;
        } else if let Some((at, rot, stretch)) = segment_pose(&l.4, p.edits.segment as usize - 1) {
            p.offset = crate::game::clamp_offset((Vec3::from_array(l.2) + at).to_array(), crate::game::OFFSET_LIMIT);
            p.yaw = 0.0;
            p.edits.set_ring(rot);
            p.scale = base(p) * stretch;
        }
    }
    *parts != before
}

/// The rest scale of rigid segment parts ([`skin_scale`] of their definition
/// with block scale 1).
pub fn segment_base(originals: &Originals, units: f32) -> impl Fn(&PartInstance) -> f32 + '_ {
    move |p: &PartInstance| skin_scale(originals, p, units, 1.0).unwrap_or(1.0)
}

/// The next unused limb id (twins use the same id with [`TWIN_BIT`]).
pub fn next_limb_id(parts: &[PartInstance]) -> u32 {
    parts.iter().filter_map(|p| p.edits.limb.as_ref()).map(|l| l.id & !TWIN_BIT).max().unwrap_or(0) + 1
}

// ---------------------------------------------------------------------------
// Purchase and sale (before `editor::input`)
// ---------------------------------------------------------------------------

/// Buy a limb assembly: the limb part (the item, charged its price) and its
/// end part, mounted on the feet slot until the held press places it.
pub fn buy(run: &mut RunProgress, originals: &Originals, asm: &Assembly, price: f32) -> Option<usize> {
    let units = skin_units(originals, &run.body);
    let id = next_limb_id(&run.parts);
    let catalog = &originals.part_catalog;
    let mut limb_part = PartInstance::new(0, 3).with_definition(asm.item);
    limb_part.edits.limb = Some(limb_of_with(asm, units, id, |seg| catalog.by_definition_in(CREATURE_PARTS, seg).map(mesh_basis)));
    let index = run.parts.len();
    run.parts.push(limb_part);
    // Rigid segments ride the limb as their own part meshes, with the
    // assembly's deform handle weights.
    if asm.rigid {
        for (k, seg) in asm.segment_parts.iter().enumerate().filter(|(k, _)| asm.rigid_segments.get(*k).copied().unwrap_or(true)) {
            let Some(def) = catalog.by_definition_in(CREATURE_PARTS, *seg) else {
                warn!("limb assembly {:08X}: segment {seg:08X} has no installed part", asm.item);
                continue;
            };
            let mut s = PartInstance::new(def.model.instance_id, 3).with_definition(def.key.instance_id);
            s.edits.on_limb = id;
            s.edits.segment = k as u32 + 1;
            s.edits.handles = asm.segment_handles.get(k).cloned().unwrap_or_default();
            s.scale = skin_scale(originals, &s, units, asm.segments.get(k).map_or(1.0, |r| r.scale.max(1e-3))).unwrap_or(1.0);
            run.parts.push(s);
        }
    }
    match asm.end.and_then(|(_, i)| catalog.by_definition_in(CREATURE_PARTS, i)) {
        Some(def) => {
            let mut end = PartInstance::new(def.model.instance_id, 3).with_definition(def.key.instance_id);
            end.edits.on_limb = id;
            // The foot / hand at its block's size on the limb.
            if let Some(scale) = skin_scale(originals, &end, units, asm.end_scale) {
                end.scale = scale.clamp(def.min_scale.min(scale), def.max_scale.max(scale));
            }
            run.parts.push(end);
        }
        None => warn!("limb assembly {:08X}: end block {:08X?} has no installed part", asm.item, asm.end),
    }
    let base = segment_base(originals, units);
    settle_ends(&mut run.parts, None, false, &base);
    run.editor_dna -= price;
    run.focused_part = index as u32;
    run.sync_legacy_accessory();
    run.request_save();
    Some(index)
}

/// Sell limb part `index` with its twin and their end parts (refund =
/// price x sell-back factor).
pub fn sell(run: &mut RunProgress, index: usize, refund: f32) -> bool {
    let Some(p) = run.parts.get(index).filter(|p| p.edits.limb.is_some()).cloned() else { return false };
    let pair = p.pair_id;
    let doomed = |i: usize, q: &PartInstance| q.edits.limb.is_some() && (i == index || (pair != 0 && q.pair_id == pair));
    let mut ids: Vec<u32> = run.parts.iter().enumerate().filter(|(i, q)| doomed(*i, q)).filter_map(|(_, q)| q.edits.limb.as_ref().map(|l| l.id)).collect();
    // Branches hanging from the sold limbs go with them.
    let branches = branches_of(&run.parts, &ids);
    ids.extend(branches);
    let doomed = |i: usize, q: &PartInstance| doomed(i, q) || q.edits.limb.as_ref().is_some_and(|l| ids.contains(&l.id));
    let keep: Vec<bool> = run.parts.iter().enumerate().map(|(i, q)| !doomed(i, q) && !ids.contains(&q.edits.on_limb)).collect();
    let mut flags = keep.into_iter();
    run.parts.retain(|_| flags.next().unwrap_or(true));
    run.editor_dna += refund;
    run.focused_part = run.focused_part.min(run.parts.len().saturating_sub(1) as u32);
    run.sync_legacy_accessory();
    run.request_save();
    true
}

/// Palette purchases of limb assemblies and sales of limbs: taken from the
/// queue `editor::input` would drop (assemblies have no part catalog entry).
#[allow(clippy::too_many_arguments)]
pub fn intercept(
    state_res: Option<Res<State<GameStage>>>,
    mouse: Res<ButtonInput<MouseButton>>,
    editor: Option<Res<CreatureEditor>>,
    data: Option<Res<CreatureEditorData>>,
    mut state: ResMut<EditorState>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
) {
    let (Some(editor), Some(data)) = (editor, data) else { return };
    if !editor.active || state.queued.is_empty() || state_res.is_none_or(|s| !crate::creature_editor::routes(s.get())) {
        return;
    }
    let queued = std::mem::take(&mut state.queued);
    for action in queued {
        match action {
            Action::Buy(item) if originals.part_catalog.by_definition_in(CREATURE_PARTS, item).is_none() => {
                let Some(asm) = assembly(&originals.resource_index, item) else {
                    state.queued.push(Action::Buy(item));
                    continue;
                };
                if let Some(r) = crate::creature_editor::refusal(&data, editor.kind, &run, &originals, item) {
                    state.message = format!("{r:?}");
                    continue;
                }
                let mut models: Vec<u32> = asm.end.map(|(_, i)| i).into_iter().collect();
                if asm.rigid {
                    models.extend(&asm.segment_parts);
                }
                for m in models {
                    if let Some(def) = originals.part_catalog.by_definition_in(CREATURE_PARTS, m).cloned() {
                        if let Err(e) = crate::editor::ensure_model(&mut originals, &def, &mut meshes, &mut images, &mut materials) {
                            warn!("limb part {}: {e}", def.name);
                        }
                    }
                }
                let before = run.clone();
                let price = data.parts.get(&item).map_or(0, |i| i.price) as f32;
                if let Some(index) = buy(&mut run, &originals, &asm, price) {
                    state.record_external(before.clone());
                    state.visuals_dirty = true;
                    if mouse.pressed(MouseButton::Left) {
                        state.start_carry(index, before);
                    }
                }
            }
            Action::Remove if run.parts.get(run.focused_part as usize).is_some_and(|p| p.edits.limb.is_some()) => {
                let index = run.focused_part as usize;
                // The limb's worth plus that of the branches it carries (sold
                // with it); a symmetric pair is sold as one (as before).
                let id = run.parts[index].edits.limb.as_ref().map_or(0, |l| l.id);
                let branches = branches_of(&run.parts, &[id]);
                let refund = limb_value(Some(&*data), &run.parts[index])
                    + run
                        .parts
                        .iter()
                        .filter(|q| q.edits.limb.as_ref().is_some_and(|l| branches.contains(&l.id)))
                        .map(|q| limb_value(Some(&*data), q))
                        .sum::<f32>();
                let before = run.clone();
                if sell(&mut run, index, refund) {
                    state.record_external(before);
                    state.visuals_dirty = true;
                }
            }
            // A rigid segment is part of its limb: it is not sold alone.
            Action::Remove if run.parts.get(run.focused_part as usize).is_some_and(|p| p.edits.segment != 0) => {}
            other => state.queued.push(other),
        }
    }
}

// ---------------------------------------------------------------------------
// Joint drag, wheel, split / join and visuals (creator chain)
// ---------------------------------------------------------------------------

/// ADAPTED: joint ball radius (`ce_simple_sphere` 0.25 x ball scale in
/// retail units) and pick radius.
const JOINT_RADIUS: f32 = 0.05;
const PICK_PX: f32 = 14.0;
/// ADAPTED: cursor travel that turns a Ctrl press on a section into a split
/// (retail's manipulator wrapper threshold, +0x08, is not read).
const DRAG_PX: f32 = 3.0;
/// `editorMouseWheelTimeout` (GlobalTemplate): one undo step per wheel
/// operation.
const WHEEL_TIMEOUT: f32 = 0.6;
/// Joint wheel (hovered ball connector) and thickness wheel (hovered
/// section) steps per notch (`notes/creator-re-notes.md` 2.4, VERIFIED).
pub const BALL_STEP: f32 = 0.05;
pub const MUSCLE_STEP: f32 = 0.025;
/// Creature setup `editorBoundSize` 3, `editorFeetBoundSize` 2.5,
/// `editorMaxHeight` 3 (creature units; the drag keeps 0.99 of them).
const BOUND_SIZE: f32 = 3.0;
const FEET_BOUND_SIZE: f32 = 2.5;
const MAX_HEIGHT: f32 = 3.0;
/// `FUN_00486090`: a limb joint stays 1.2 ball radii above the floor.
const FLOOR_BALLS: f32 = 1.2;

/// What the cursor is over: a joint ball or a section between two joints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimbHover {
    Joint(usize, usize),
    Section(usize, usize),
}

#[derive(Clone, Debug)]
struct JointDrag {
    part: usize,
    joint: usize,
    kind: JointKind,
    before: RunProgress,
    /// The limb root and its joints at press (player frame).
    root: Vec3,
    press: Vec<Vec3>,
    has_feet: bool,
    /// Cursor ray origin offset at press (manipulator +0xD4).
    grab: Vec3,
}

#[derive(Clone, Debug)]
struct SectionPress {
    part: usize,
    section: usize,
    at: Vec2,
    before: RunProgress,
}

#[derive(Resource, Default)]
pub struct LimbState {
    hover: Option<LimbHover>,
    drag: Option<JointDrag>,
    pending: Option<SectionPress>,
    /// (part, joint, world position, ball radius).
    joints: Vec<(usize, usize, Vec3, f32)>,
    /// (part, section, world start, world end, radius).
    sections: Vec<(usize, usize, Vec3, Vec3, f32)>,
    /// One wheel operation: the state before it, idle seconds, its target.
    wheel: Option<(RunProgress, f32, LimbHover)>,
    /// A limb this module started carrying: (limb id, state before).
    carried: Option<(u32, RunProgress)>,
    tunings: Option<Tunings>,
}

impl LimbState {
    pub fn held(&self) -> bool {
        self.drag.is_some() || self.pending.is_some()
    }
    /// The cursor is over a limb joint or section (the creator's part / vertebra
    /// wheel leaves the wheel to the limb).
    pub fn over_limb(&self) -> bool {
        self.hover.is_some()
    }
}

#[derive(Component)]
pub struct JointVisual(pub usize, pub usize);

fn to_world(pt: &Transform, p: Vec3) -> Vec3 {
    pt.translation + pt.rotation * (p * pt.scale)
}

fn to_local(pt: &Transform, p: Vec3) -> Vec3 {
    (pt.rotation.conjugate() * (p - pt.translation)) / pt.scale
}

fn local_ray(pt: &Transform, r: Ray3d) -> (Vec3, Vec3) {
    (to_local(pt, r.origin), ((pt.rotation.conjugate() * *r.direction) / pt.scale).normalize_or(Vec3::NEG_Z))
}

/// What ends limb `id`: its end part's `modelActsLikeFoot` / `...Grasper`.
pub fn end_kind(parts: &[PartInstance], id: u32, data: Option<&CreatureEditorData>) -> EndKind {
    let Some(end) = parts.iter().find(|p| p.edits.on_limb == id && p.edits.segment == 0) else { return EndKind::None };
    match data.and_then(|d| d.info_of(end)) {
        Some(i) if i.acts_like_foot => EndKind::Foot,
        _ => EndKind::Grasper,
    }
}

/// Distance from a ray to a segment and the ray parameter there.
fn ray_segment(o: Vec3, d: Vec3, a: Vec3, b: Vec3) -> (f32, f32) {
    let ab = b - a;
    let (dd, ee, de) = (d.dot(d), ab.dot(ab), d.dot(ab));
    let w = o - a;
    let den = dd * ee - de * de;
    let (mut t, mut s) = if den.abs() < 1e-9 { (0.0, 0.0) } else { ((de * ab.dot(w) - ee * d.dot(w)) / den, (dd * ab.dot(w) - de * d.dot(w)) / den) };
    s = s.clamp(0.0, 1.0);
    if den.abs() >= 1e-9 {
        t = (a + ab * s - o).dot(d) / dd;
    }
    let t = t.max(0.0);
    ((o + d * t).distance(a + ab * s), t)
}

/// Ctrl + drag on section `k` of limb part `part`: the section and everything
/// below leave as a new limb part (`k == 0`: the whole limb). Its end and
/// rigid segment parts go with it; the twin's are dropped (the creator's
/// settle mirrors the new limb, `settle_ends` its parts). Returns the index
/// of the limb part to carry.
pub fn split_limb(run: &mut RunProgress, part: usize, k: usize) -> Option<usize> {
    let p = run.parts.get(part)?.clone();
    let limb = p.edits.limb.clone()?;
    if k == 0 {
        return Some(part);
    }
    let new_id = next_limb_id(&run.parts);
    let mut stub = limb.clone();
    let tail = stub.split_at(k, new_id)?;
    let mut tail_part = p.clone();
    tail_part.pair_id = 0;
    tail_part.offset = crate::game::clamp_offset((Vec3::from_array(p.offset) + limb.joint(k)).to_array(), crate::game::OFFSET_LIMIT);
    tail_part.edits = PartEdits { limb: Some(tail), ..default() };
    run.parts[part].edits.limb = Some(stub);
    for q in run.parts.iter_mut() {
        if q.edits.on_limb == limb.id && (q.edits.segment == 0 || q.edits.segment as usize > k) {
            q.edits.on_limb = new_id;
            if q.edits.segment > 0 {
                q.edits.segment -= k as u32;
            }
        }
        // Branches below the split hang from the tail (the twin's from the
        // tail's future twin).
        if let Some(l) = q.edits.limb.as_mut().filter(|l| l.parent != 0 && (l.parent & !TWIN_BIT) == (limb.id & !TWIN_BIT) && l.parent_joint as usize > k) {
            l.parent = if l.parent == limb.id { new_id } else { new_id ^ TWIN_BIT };
            l.parent_joint -= k as u32;
        }
    }
    let twin = limb.id ^ TWIN_BIT;
    run.parts.retain(|q| !(q.edits.on_limb == twin && (q.edits.segment == 0 || q.edits.segment as usize > k)));
    let index = run.parts.iter().position(|q| q.edits.limb.as_ref().is_some_and(|l| l.id == limb.id))?;
    crate::creature_handles::sync_twins(&mut run.parts, index);
    run.parts.push(tail_part);
    Some(run.parts.len() - 1)
}

/// Ctrl + click on section `k` of limb part `part`: remove it (and its rigid
/// segment part, the twin's too).
pub fn remove_limb_section(run: &mut RunProgress, part: usize, k: usize) -> bool {
    let Some(mut limb) = run.parts.get(part).and_then(|p| p.edits.limb.clone()) else { return false };
    if !limb.remove_section(k) {
        return false;
    }
    let id = limb.id;
    run.parts[part].edits.limb = Some(limb);
    let gone = k as u32 + 1;
    run.parts.retain(|q| !((q.edits.on_limb == id || q.edits.on_limb == id ^ TWIN_BIT) && q.edits.segment == gone));
    for q in run.parts.iter_mut() {
        if (q.edits.on_limb == id || q.edits.on_limb == id ^ TWIN_BIT) && q.edits.segment > gone {
            q.edits.segment -= 1;
        }
        // A branch on the removed joint moves up to the joint above it.
        if let Some(l) = q.edits.limb.as_mut().filter(|l| l.parent != 0 && (l.parent == id || l.parent == id ^ TWIN_BIT) && l.parent_joint >= gone) {
            l.parent_joint -= 1;
        }
    }
    if let Some(index) = run.parts.iter().position(|q| q.edits.limb.as_ref().is_some_and(|l| l.id == id)) {
        crate::creature_handles::sync_twins(&mut run.parts, index);
    }
    true
}

/// What a Ctrl drop of a limb did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attach {
    /// Its sections continue the other limb's chain (one limb now).
    Joined,
    /// It hangs from a joint of the other limb (`Limb::parent`).
    Branched,
}

/// Limb `id` hangs (through branch links) from limb `of`.
fn descends_from(parts: &[PartInstance], id: u32, of: u32) -> bool {
    let mut cur = id;
    for _ in 0..16 {
        let Some(p) = parts.iter().find_map(|q| q.edits.limb.as_ref().filter(|l| l.id == cur)).map(|l| l.parent) else { return false };
        if p == 0 {
            return false;
        }
        if p == of {
            return true;
        }
        cur = p;
    }
    false
}

/// The branches hanging (directly or not) from the limbs `ids`.
pub fn branches_of(parts: &[PartInstance], ids: &[u32]) -> Vec<u32> {
    parts
        .iter()
        .filter_map(|q| q.edits.limb.as_ref())
        .filter(|l| ids.iter().any(|&of| l.id != of && descends_from(parts, l.id, of)))
        .map(|l| l.id)
        .collect()
}

/// Ctrl + drop of the carried limb (id `carried`) whose root is within
/// `reach` of a joint (k >= 1) of another limb: retail attaches the dropped
/// root to the limb block it is dropped on (`FUN_004A2EB0`). On the free end
/// of a limb (no end part, no branch there) the sections continue that chain
/// and the two become one limb (worth `value` more); on any other joint the
/// limb becomes a branch of that limb (`Limb::parent`). The carried limb's own
/// branches stay on it.
pub fn attach_limb(run: &mut RunProgress, carried: u32, roots: &dyn Fn(&PartInstance) -> Vec3, reach: f32, value: f32) -> Option<Attach> {
    let ci = run.parts.iter().position(|p| p.edits.limb.as_ref().is_some_and(|l| l.id == carried))?;
    let c = run.parts[ci].clone();
    let cl = c.edits.limb.clone().unwrap();
    let croot = roots(&c);
    let parts = &run.parts;
    let (ti, k) = parts
        .iter()
        .enumerate()
        .filter(|(i, q)| *i != ci && (c.pair_id == 0 || q.pair_id != c.pair_id))
        .filter_map(|(i, q)| {
            let l = q.edits.limb.as_ref()?;
            if descends_from(parts, l.id, cl.id) {
                return None;
            }
            let root = roots(q);
            (1..l.joints.len())
                .map(|k| (k, (root + l.joint(k)).distance(croot)))
                .filter(|(_, d)| *d < reach)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(k, d)| (i, k, d))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(i, k, _)| (i, k))?;
    let tl = run.parts[ti].edits.limb.clone().unwrap();
    let last = tl.joints.len() - 1;
    let free = !run.parts.iter().any(|q| q.edits.on_limb == tl.id && q.edits.segment == 0)
        && !run.parts.iter().any(|q| q.edits.limb.as_ref().is_some_and(|l| l.parent == tl.id && l.parent_joint as usize == last));
    if k != last || !free {
        if let Some(l) = run.parts[ci].edits.limb.as_mut() {
            l.parent = tl.id;
            l.parent_joint = k as u32;
        }
        crate::creature_handles::sync_twins(&mut run.parts, ci);
        return Some(Attach::Branched);
    }
    let mut tl = tl;
    let before_sections = tl.segment_count() as u32;
    if !tl.join(&cl) {
        return None;
    }
    tl.extra_value += value;
    let tid = tl.id;
    run.parts[ti].edits.limb = Some(tl);
    for q in run.parts.iter_mut() {
        if q.edits.on_limb == cl.id {
            q.edits.on_limb = tid;
            if q.edits.segment > 0 {
                q.edits.segment += before_sections;
            }
        }
        // The carried limb's branches (and its twin's) hang from the joined limb.
        if let Some(l) = q.edits.limb.as_mut().filter(|l| l.parent != 0 && (l.parent == cl.id || l.parent == cl.id ^ TWIN_BIT)) {
            l.parent = if l.parent == cl.id { tid } else { tid ^ TWIN_BIT };
            l.parent_joint += before_sections;
        }
    }
    // The carried limb part, its twin and the twin's parts go.
    let twin = cl.id ^ TWIN_BIT;
    let pair = c.pair_id;
    run.parts.retain(|q| {
        let is_limb = q.edits.limb.as_ref().is_some_and(|l| l.id == cl.id || l.id == twin);
        let twin_limb = pair != 0 && q.pair_id == pair && q.edits.limb.is_some();
        !(is_limb || twin_limb || q.edits.on_limb == twin)
    });
    if let Some(index) = run.parts.iter().position(|q| q.edits.limb.as_ref().is_some_and(|l| l.id == tid)) {
        crate::creature_handles::sync_twins(&mut run.parts, index);
    }
    Some(Attach::Joined)
}

/// [`attach_limb`] joining onto a free limb end only.
#[cfg(test)]
pub fn join_limbs(run: &mut RunProgress, carried: u32, roots: &dyn Fn(&PartInstance) -> Vec3, reach: f32) -> bool {
    attach_limb(run, carried, roots, reach, 0.0) == Some(Attach::Joined)
}

/// `LimbRigblockTemplate` `modelPrice` (VERIFIED data): the price of a limb
/// segment block whose own PROP does not set one.
pub const SEGMENT_PRICE: f32 = 15.0;

/// DNA refunded by a Ctrl + click on section `k` of the limb on `part`:
/// retail charges a creation the sum of its blocks' prices, so removing a
/// segment block gives its price back (the segment PROP's, else the limb
/// template's [`SEGMENT_PRICE`]) at the creator's sell-back factor.
pub fn section_refund(data: Option<&CreatureEditorData>, part: &PartInstance, k: usize) -> f32 {
    let Some(limb) = part.edits.limb.as_ref() else { return 0.0 };
    let sell_back = data.and_then(|d| d.info_of(part)).map_or(1.0, |i| i.sell_back);
    let price = limb
        .segments
        .get(k)
        .and_then(|seg| data.and_then(|d| d.parts.get(seg)))
        .map_or(SEGMENT_PRICE, |i| i.price as f32);
    price.max(0.0) * sell_back
}

/// What selling limb part `part` gives back: its item's price x sell-back plus
/// its `extra_value` (never negative).
pub fn limb_value(data: Option<&CreatureEditorData>, part: &PartInstance) -> f32 {
    let base = data.and_then(|d| d.info_of(part)).map_or(0.0, |i| i.price as f32 * i.sell_back);
    (base + part.edits.limb.as_ref().map_or(0.0, |l| l.extra_value)).max(0.0)
}

/// The joint target of a drag this frame (player frame): mode 0 slides the
/// foot on its height; modes 1..3 take the best of [`SAMPLES`] points of the
/// mouse ray between the camera and the floor.
#[allow(clippy::too_many_arguments)]
pub fn drag_target(drag_kind: JointKind, press: &[Vec3], k: usize, o: Vec3, d: Vec3, tunings: &Tunings, units: f32, lengths: (f32, f32)) -> Option<Vec3> {
    let n = *press.get(k)?;
    match drag_kind {
        JointKind::Foot => {
            if d.y.abs() < 1e-6 {
                return None;
            }
            let t = (n.y - o.y) / d.y;
            (t > 0.0).then(|| o + d * t)
        }
        JointKind::Grasper | JointKind::Knee | JointKind::Elbow => {
            let a = press[k - 1];
            let c = press.get(k + 1).copied().unwrap_or_else(|| n + (n - a).normalize_or_zero() * 0.1 * units);
            let ground = crate::creature_cell_convert::GROUND_Y;
            // ADAPTED search segment: the ray from the camera to 99 % of the way
            // down to the floor, at most 4 bound sizes past the creature.
            let far = o.length() + 4.0 * BOUND_SIZE * units;
            let t = if d.y < -1e-4 { ((o.y - ground) * 0.99 / -d.y).min(far) } else { far };
            let ctx = ScoreCtx { n, a, c, g: (k >= 2).then(|| press[k - 2]), cam: o, min_len: lengths.0, max_len: lengths.1, on_plane: n.x.abs() < 1e-3 * units };
            line_search(o, o + d * t, SAMPLES, ctx.on_plane, d, |p, dir| score(drag_kind, tunings, &ctx, p, dir))
        }
        JointKind::LegRoot | JointKind::ArmRoot => None,
    }
}

/// The joints after a drag step (player frame): the retail placement, the
/// moved joint's segment length and its floor / bounds.
pub fn drag_joints(press: &[Vec3], k: usize, kind: JointKind, target: Vec3, has_feet: bool, units: f32, lengths: (f32, f32), ball: f32) -> Vec<Vec3> {
    let mut j = place(press, k, kind, target, has_feet);
    clamp_length(&mut j, press, k, lengths.0, lengths.1);
    let ground = crate::creature_cell_convert::GROUND_Y;
    let bound = 0.99 * if kind == JointKind::Foot { FEET_BOUND_SIZE } else { BOUND_SIZE } * units;
    let floor = if kind == JointKind::Foot { ground } else { ground + FLOOR_BALLS * ball };
    let fixed = clamp_bounds(j[k], floor, ground + 0.99 * MAX_HEIGHT * units, bound);
    let delta = fixed - j[k];
    if delta.length_squared() > 0.0 {
        let moved = j[k] - press[k];
        for i in k..j.len() {
            if i == k || (j[i] - press[i]).distance(moved) < 1e-5 {
                j[i] += delta;
            }
        }
    }
    j
}

/// Press on a joint ball or a limb section (before `editor::drag`): the
/// press is taken.
#[allow(clippy::too_many_arguments)]
pub fn press(
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    players: Query<&Transform, With<Player>>,
    (editor, data): (Option<Res<CreatureEditor>>, Option<Res<CreatureEditorData>>),
    mut state: ResMut<EditorState>,
    pointer: Res<crate::spui::view::SpuiPointer>,
    handles: Res<crate::creature_handles::HandleState>,
    mut limbs: ResMut<LimbState>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
) {
    if editor.is_none_or(|e| !e.active || e.mode != Mode::Build) {
        limbs.drag = None;
        limbs.pending = None;
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) || limbs.held() || handles.held() || state.dragging() || pointer.over_ui {
        return;
    }
    let Some(hover) = limbs.hover else { return };
    let (Ok(window), Ok((camera, cam)), Ok(pt)) = (windows.single(), cameras.single(), players.single()) else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok(ray) = camera.viewport_to_world(cam, cursor) else { return };
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    match hover {
        // Mode 4 / 5: the root ball re-pins the limb on the body (carried by
        // the creator like a part).
        LimbHover::Joint(part, 0) => {
            if let Some(id) = run.parts.get(part).and_then(|p| p.edits.limb.as_ref()).map(|l| l.id) {
                limbs.carried = Some((id, run.clone()));
                state.start_carry(part, run.clone());
            }
        }
        LimbHover::Joint(part, joint) => {
            let Some(p) = run.parts.get(part) else { return };
            let Some(limb) = p.edits.limb.as_ref() else { return };
            let root = root_of(&originals, &run.body, p);
            let press: Vec<Vec3> = limb.joints.iter().map(|j| root + Vec3::from_array(*j)).collect();
            let end = end_kind(&run.parts, limb.id, data.as_deref());
            let kind = joint_kind(joint, press.len(), end);
            let (o, d) = local_ray(pt, ray);
            let n = press[joint];
            let t = (n - o).dot(d);
            limbs.drag = Some(JointDrag {
                part,
                joint,
                kind,
                before: run.clone(),
                root,
                has_feet: end == EndKind::Foot,
                grab: n - (o + d * t.max(0.0)),
                press,
            });
        }
        LimbHover::Section(part, section) => {
            if ctrl {
                limbs.pending = Some(SectionPress { part, section, at: cursor, before: run.clone() });
            } else if let Some(id) = run.parts.get(part).and_then(|p| p.edits.limb.as_ref()).map(|l| l.id) {
                // Modes 6 / 7: pressing the limb's skin carries the whole limb.
                limbs.carried = Some((id, run.clone()));
                state.start_carry(part, run.clone());
            }
        }
    }
    mouse.clear_just_pressed(MouseButton::Left);
}

/// Joint drag, section presses, the joint / thickness wheel, Ctrl joins,
/// hover and joint ball visuals (after the handle manipulators).
#[allow(clippy::too_many_arguments)]
pub fn manipulate(
    mut commands: Commands,
    (mouse, keys, time): (Res<ButtonInput<MouseButton>>, Res<ButtonInput<KeyCode>>, Res<Time>),
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    players: Query<&Transform, With<Player>>,
    mut visuals: Query<(Entity, &JointVisual, &mut Transform, &MeshMaterial3d<StandardMaterial>), Without<Player>>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut editor: ResMut<CreatureEditor>,
    mut state: ResMut<EditorState>,
    pointer: Res<crate::spui::view::SpuiPointer>,
    mut limbs: ResMut<LimbState>,
    mut run: ResMut<RunProgress>,
    (originals, data): (Res<Originals>, Option<Res<CreatureEditorData>>),
    mut assets: Local<Option<(Handle<Mesh>, [Handle<StandardMaterial>; 3])>>,
) {
    let data = data.as_deref();
    let build = editor.mode == Mode::Build && !editor.options_open && !editor.naming;
    let notches: f32 = wheel
        .read()
        .map(|w| match w.unit {
            bevy::input::mouse::MouseScrollUnit::Line => w.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => w.y / 40.0,
        })
        .sum();
    let (Ok(window), Ok((camera, cam)), Ok(pt)) = (windows.single(), cameras.single(), players.single()) else { return };
    let pt = *pt;
    let cursor = window.cursor_position();
    let ray = cursor.and_then(|c| camera.viewport_to_world(cam, c).ok());
    let units = skin_units(&originals, &run.body);
    if limbs.tunings.is_none() {
        limbs.tunings = Some(Tunings::load(&originals.resource_index));
    }
    let tunings = limbs.tunings.unwrap_or_default();
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let over_palette = cursor.is_some_and(|c| state.retail_layout.is_some_and(|l| l.over_palette(c)));

    // A carried limb (root ball, skin or split) is dropped: sold on the
    // palette, with Ctrl joined onto a free limb end or branched onto any
    // other joint of a limb, one undo step.
    if let Some((id, before)) = limbs.carried.clone() {
        if state.moving_part().is_none() {
            limbs.carried = None;
            let index = run.parts.iter().position(|p| p.edits.limb.as_ref().is_some_and(|l| l.id == id));
            if let (Some(i), true) = (index, over_palette) {
                run.focused_part = i as u32;
                state.queued.push(Action::Remove);
                if before.parts != run.parts {
                    state.record_external(before);
                }
            } else {
                if ctrl {
                    let body = run.body.clone();
                    let roots = |p: &PartInstance| root_of(&originals, &body, p);
                    let reach = MAX_BALL * skin::BALL_CONNECTOR_RADIUS * units * 2.0;
                    let value = index.and_then(|i| run.parts.get(i)).map_or(0.0, |p| limb_value(data, p));
                    match attach_limb(&mut run, id, &roots, reach, value) {
                        Some(Attach::Joined) => state.message = "Limbs joined".into(),
                        Some(Attach::Branched) => state.message = "Limb attached".into(),
                        None => {}
                    }
                }
                if before.parts != run.parts {
                    state.record_external(before);
                    run.request_save();
                }
                state.visuals_dirty = true;
            }
        }
    }

    // A Ctrl press on a section: a click removes it, a drag splits there.
    if let Some(p) = limbs.pending.clone() {
        let moved = cursor.is_some_and(|c| c.distance(p.at) > DRAG_PX);
        if !build {
            limbs.pending = None;
        } else if !mouse.pressed(MouseButton::Left) {
            limbs.pending = None;
            let refund = run.parts.get(p.part).map_or(0.0, |q| section_refund(data, q, p.section));
            if remove_limb_section(&mut run, p.part, p.section) {
                // The segment block's price comes back (retail: the creation
                // costs the sum of its blocks); the limb is worth that less.
                run.editor_dna += refund;
                if let Some(l) = run.parts.get_mut(p.part).and_then(|q| q.edits.limb.as_mut()) {
                    l.extra_value -= refund;
                }
                crate::creature_handles::sync_twins(&mut run.parts, p.part);
                state.record_external(p.before);
                state.visuals_dirty = true;
                run.request_save();
            } else {
                state.message = "The shoulder or hip section cannot be removed".into();
            }
        } else if moved {
            limbs.pending = None;
            if let Some(index) = split_limb(&mut run, p.part, p.section) {
                if let Some(id) = run.parts[index].edits.limb.as_ref().map(|l| l.id) {
                    limbs.carried = Some((id, p.before));
                    state.start_carry(index, run.clone());
                    state.visuals_dirty = true;
                }
            }
        }
    }

    // The joint drag.
    if let Some(drag) = limbs.drag.take() {
        if !build || !mouse.pressed(MouseButton::Left) {
            if drag.before.parts != run.parts {
                state.record_external(drag.before);
                run.request_save();
            }
        } else {
            if let Some(r) = ray {
                let (o, d) = local_ray(&pt, r);
                let lengths = run.parts.get(drag.part).and_then(|p| p.edits.limb.as_ref()).map_or((MIN_SEGMENT, MAX_SEGMENT), Limb::lengths);
                let lengths = (lengths.0 * units, lengths.1 * units);
                if let Some(target) = drag_target(drag.kind, &drag.press, drag.joint, o + drag.grab, d, &tunings, units, lengths) {
                    let ball = run.parts.get(drag.part).and_then(|p| p.edits.limb.as_ref()).and_then(|l| l.balls.get(drag.joint).copied()).unwrap_or(1.0)
                        * skin::BALL_CONNECTOR_RADIUS
                        * units;
                    let joints = drag_joints(&drag.press, drag.joint, drag.kind, target, drag.has_feet, units, lengths, ball);
                    if let Some(limb) = run.parts.get_mut(drag.part).and_then(|p| p.edits.limb.as_mut()) {
                        let rel: Vec<[f32; 3]> = joints.iter().map(|j| (*j - drag.root).to_array()).collect();
                        if rel != limb.joints {
                            limb.joints = rel;
                            crate::creature_handles::sync_twins(&mut run.parts, drag.part);
                            state.visuals_dirty = true;
                        }
                    }
                }
            }
            limbs.drag = Some(drag);
        }
    }
    editor.manipulating |= limbs.held();

    // Joint balls and sections of every limb (Build mode).
    limbs.joints.clear();
    limbs.sections.clear();
    if build && !state.dragging() {
        for (i, p) in run.parts.iter().enumerate() {
            let Some(limb) = &p.edits.limb else { continue };
            let root = root_of(&originals, &run.body, p);
            let radius = |k: usize| limb.balls.get(k).copied().unwrap_or(1.0) * skin::BALL_CONNECTOR_RADIUS * units * pt.scale.x;
            for k in 0..limb.joints.len() {
                // ADAPTED: a translucent ball a little larger than the joint's
                // ball connector, so it shows around the skin.
                limbs.joints.push((i, k, to_world(&pt, root + limb.joint(k)), (radius(k) * 0.8).max(JOINT_RADIUS)));
            }
            for k in 0..limb.segment_count() {
                let r = radius(k).max(radius(k + 1)) * if limb.is_rigid(k) { 1.5 } else { 1.0 };
                limbs.sections.push((i, k, to_world(&pt, root + limb.joint(k)), to_world(&pt, root + limb.joint(k + 1)), r));
            }
        }
    }
    let over_ui = pointer.over_ui || cursor.is_some_and(|c| state.retail_layout.is_some_and(|l| l.over_ui(c)));
    limbs.hover = limbs.drag.as_ref().map(|d| LimbHover::Joint(d.part, d.joint)).or_else(|| {
        let c = cursor.filter(|_| !over_ui)?;
        let joint = limbs
            .joints
            .iter()
            .filter_map(|(i, k, at, _)| {
                let q = camera.world_to_viewport(cam, *at).ok()?;
                let d = q.distance(c);
                (d < PICK_PX).then_some((*i, *k, d))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(i, k, _)| LimbHover::Joint(i, k));
        joint.or_else(|| {
            let r = ray?;
            limbs
                .sections
                .iter()
                .filter_map(|(i, k, a, b, rad)| {
                    let (dist, t) = ray_segment(r.origin, *r.direction, *a, *b);
                    (dist < *rad).then_some((*i, *k, t))
                })
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(i, k, _)| LimbHover::Section(i, k))
        })
    });

    // The joint wheel (ball scale) and the thickness wheel (section).
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let hover_now = limbs.hover;
    let dt = time.delta_secs();
    let expired = limbs.wheel.as_mut().is_some_and(|(_, idle, target)| {
        *idle += dt;
        *idle > WHEEL_TIMEOUT || hover_now != Some(*target) || !build
    });
    if expired {
        let (before, _, _) = limbs.wheel.take().unwrap();
        if before.parts != run.parts {
            state.record_external(before);
            run.request_save();
        }
    }
    if build && notches != 0.0 && !shift && !over_ui && !limbs.held() {
        if let Some(h) = limbs.hover {
            let before = run.clone();
            let (part, changed) = match h {
                LimbHover::Joint(i, k) => (
                    i,
                    run.parts.get_mut(i).and_then(|p| p.edits.limb.as_mut()).is_some_and(|l| {
                        let (lo, hi) = l.balls_range();
                        match l.balls.get_mut(k) {
                            Some(b) => {
                                let v = (*b + BALL_STEP * notches).clamp(lo, hi);
                                let changed = (v - *b).abs() > 1e-6;
                                *b = v;
                                changed
                            }
                            None => false,
                        }
                    }),
                ),
                LimbHover::Section(i, k) => (
                    i,
                    run.parts.get_mut(i).and_then(|p| p.edits.limb.as_mut()).filter(|l| !l.is_rigid(k)).is_some_and(|l| match l.muscle.get_mut(k) {
                        Some(m) => {
                            let v = (*m + MUSCLE_STEP * notches).clamp(0.0, 1.0);
                            let changed = (v - *m).abs() > 1e-6;
                            *m = v;
                            changed
                        }
                        None => false,
                    }),
                ),
            };
            editor.wheel_consumed = true;
            if changed {
                crate::creature_handles::sync_twins(&mut run.parts, part);
                state.visuals_dirty = true;
                match limbs.wheel.as_mut() {
                    Some(w) => w.1 = 0.0,
                    None => limbs.wheel = Some((before, 0.0, h)),
                }
            }
        }
    }

    let (sphere, mats) = assets
        .get_or_insert_with(|| {
            let mut mat = |c: [f32; 3], a: f32| {
                materials.add(StandardMaterial {
                    base_color: Color::srgba(c[0].min(1.0), c[1].min(1.0), c[2].min(1.0), a),
                    emissive: LinearRgba::rgb(c[0] * 0.6, c[1] * 0.6, c[2] * 0.6),
                    alpha_mode: AlphaMode::Blend,
                    unlit: true,
                    ..default()
                })
            };
            (
                meshes.add(Sphere::new(JOINT_RADIUS)),
                [
                    mat([HANDLE_COLOR[0], HANDLE_COLOR[1], HANDLE_COLOR[2]], 0.45),
                    mat(HANDLE_ROLLOVER, 0.6),
                    mat(HANDLE_ACTIVE, 0.7),
                ],
            )
        })
        .clone();
    let hovered = |i: usize, k: usize| match limbs.hover {
        Some(LimbHover::Joint(a, b)) => a == i && b == k,
        Some(LimbHover::Section(a, b)) => a == i && (b == k || b + 1 == k),
        None => false,
    };
    let mut seen: Vec<(usize, usize)> = Vec::new();
    for (e, v, mut tf, mat) in &mut visuals {
        match limbs.joints.iter().find(|j| j.0 == v.0 && j.1 == v.1) {
            Some(&(i, k, at, r)) if !seen.contains(&(i, k)) => {
                seen.push((i, k));
                tf.translation = at;
                tf.scale = Vec3::splat(r / JOINT_RADIUS);
                let m = &mats[if limbs.drag.as_ref().is_some_and(|d| d.part == i && d.joint == k) {
                    2
                } else if hovered(i, k) {
                    1
                } else {
                    0
                }];
                if mat.0 != *m {
                    commands.entity(e).insert(MeshMaterial3d(m.clone()));
                }
            }
            _ => commands.entity(e).despawn(),
        }
    }
    for &(i, k, at, r) in &limbs.joints {
        if !seen.contains(&(i, k)) {
            commands.spawn((
                StageOwned,
                CreatureEditorOwned,
                JointVisual(i, k),
                Mesh3d(sphere.clone()),
                MeshMaterial3d(mats[0].clone()),
                Transform::from_translation(at).with_scale(Vec3::splat(r / JOINT_RADIUS)),
                bevy::light::NotShadowCaster,
            ));
        }
    }
}

/// The creature's lowest point (player frame): the torso skin, the limb ends
/// (less their end ball) and the mounted parts' mesh boxes.
pub fn lowest_point(originals: &Originals, run: &RunProgress, meshes: &Assets<Mesh>, units: f32) -> f32 {
    let mut low = crate::baked_model::body_triangles(originals, &run.body).iter().flat_map(|t| t.iter().map(|v| v.y)).fold(f32::MAX, f32::min);
    for p in &run.parts {
        if let Some(limb) = &p.edits.limb {
            let root = root_of(originals, &run.body, p);
            let ball = limb.balls.last().copied().unwrap_or(1.0) * skin::BALL_CONNECTOR_RADIUS * units;
            low = low.min(root.y + limb.end()[1] - ball);
            continue;
        }
        let Some(model) = crate::editor::model_for_part(originals, p) else { continue };
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = meshes.get(&model.draw_mesh()).and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION)) else {
            continue;
        };
        let at = root_of(originals, &run.body, p);
        let (rot, scale) = (p.rotation(), p.mesh_scale(model.scale * 0.5 * p.scale));
        let part_low = pos.iter().step_by(4).map(|v| at.y + (rot * (scale * Vec3::from_array(*v))).y).fold(f32::MAX, f32::min);
        low = low.min(part_low);
    }
    low
}

/// The creator stands a creature with limbs on its lowest point (retail
/// places the creation on the ground, as CellToCreature does with -minZ):
/// the torso (and everything on it) moves vertically.
pub fn stand(originals: &Originals, run: &mut RunProgress, meshes: &Assets<Mesh>) -> bool {
    let units = skin_units(originals, &run.body);
    let low = lowest_point(originals, run, meshes, units);
    let dy = crate::creature_cell_convert::GROUND_Y - low;
    if !dy.is_finite() || dy.abs() < 0.01 {
        return false;
    }
    run.body.edit_spine(|spine| {
        for p in spine.iter_mut() {
            p.position[1] += dy;
        }
        true
    })
}

/// End parts follow their limbs (in the creator), and the skin's limb list
/// follows the creature (every stage).
#[allow(clippy::too_many_arguments)]
pub fn sync_limbs(
    stage: Option<Res<State<GameStage>>>,
    editor: Option<Res<CreatureEditor>>,
    mut state: ResMut<EditorState>,
    mut run: ResMut<RunProgress>,
    originals: Res<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut last: Local<Vec<LimbGeom>>,
    mut had_limbs: Local<bool>,
) {
    let editing = editor.as_ref().filter(|e| e.active);
    let creating = editing.is_some() && stage.is_some_and(|s| crate::creature_editor::routes(s.get()));
    if creating {
        let mut parts = run.parts.clone();
        let units = skin_units(&originals, &run.body);
        let base = segment_base(&originals, units);
        if settle_ends(&mut parts, state.moving_part().map(|m| m.0), state.moving_part().is_none(), &base) {
            run.parts = parts;
            state.visuals_dirty = true;
        }
        // Stand on the ground once nothing is held (creatures with limbs, and
        // once more after the last limb is gone).
        let limbs = run.parts.iter().any(|p| p.edits.limb.is_some());
        let held = editing.is_some_and(|e| e.manipulating) || state.dragging();
        if (limbs || *had_limbs) && !held {
            *had_limbs = limbs;
            let mut r = run.clone();
            if stand(&originals, &mut r, &meshes) {
                run.body = r.body;
                state.visuals_dirty = true;
            }
        }
    }
    let now = geoms(&originals, &run);
    if now != *last {
        *SKIN_LIMBS.lock().unwrap_or_else(|e| e.into_inner()) = now.clone();
        *last = now;
        if let Some(template) = &originals.baked_body {
            crate::baked_model::update_body(template, &run.body, &mut meshes);
        }
    }
}

/// Capture option `--limbs-demo`: in the creator, buy the first limb assembly
/// of the session's palette (`--limbs-demo-exo`: the first rigid exo leg),
/// mount it on the side of the torso (a mirrored pair) and bend its knee
/// forward with the retail knee rule (the foot stays); `--limbs-demo-drag`
/// instead drags the foot out and forward (the retail foot rule re-places the
/// knee), `--limbs-demo-wheel` turns the wheel 8 notches up over the knee ball
/// and the first section; `--limbs-demo-branch` also drops a second pair onto
/// the knees as branches.
pub fn demo(
    editor: Option<Res<CreatureEditor>>,
    data: Option<Res<CreatureEditorData>>,
    mut state: ResMut<EditorState>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
    mut done: Local<bool>,
) {
    let (Some(editor), Some(data)) = (editor, data) else { return };
    if *done || !editor.active || !data.loaded {
        return;
    }
    *done = true;
    let args: Vec<String> = std::env::args().collect();
    let has = |a: &str| args.iter().any(|x| x == a);
    if !has("--limbs-demo") {
        return;
    }
    let Some(palette) = data.parts_palette(editor.kind) else { return };
    let exo = has("--limbs-demo-exo");
    let asm = if exo {
        (1..=12).find_map(|n| assembly(&originals.resource_index, crate::world_objects::name_id(&format!("limb_assembly_exo_l_{n:02}"))))
    } else {
        palette.pages.iter().flat_map(|p| p.items.iter()).find_map(|i| assembly(&originals.resource_index, i.part))
    };
    let Some(asm) = asm else {
        warn!("limbs demo: no limb assembly");
        return;
    };
    let mut models: Vec<u32> = asm.end.map(|(_, i)| i).into_iter().collect();
    if asm.rigid {
        models.extend(&asm.segment_parts);
    }
    for m in models {
        if let Some(def) = originals.part_catalog.by_definition_in(CREATURE_PARTS, m).cloned() {
            let _ = crate::editor::ensure_model(&mut originals, &def, &mut meshes, &mut images, &mut materials);
        }
    }
    let price = data.parts.get(&asm.item).map_or(0, |i| i.price) as f32;
    let Some(index) = buy(&mut run, &originals, &asm, price) else { return };
    // On the side of the torso, below its middle.
    let body = run.body.clone();
    let n = body.spine.len();
    let (centre, radius) = crate::baked_model::spine_center(&originals, &body, n / 2);
    let side = crate::baked_model::nearest_body_surface(&originals, &body, centre + Vec3::new(radius * 1.5, -radius * 0.6, 0.0));
    let slot = crate::baked_model::attachment_point(&originals, &body, run.parts[index].bone);
    run.parts[index].offset = crate::game::clamp_offset((side - slot).to_array(), crate::game::OFFSET_LIMIT);
    run.parts[index].pair_id = run.parts.iter().map(|p| p.pair_id).max().unwrap_or(0) + 1;
    let mut twin = run.parts[index].clone();
    let mirrored = Vec3::new(-side.x, side.y, side.z);
    twin.offset = crate::game::clamp_offset((mirrored - slot).to_array(), crate::game::OFFSET_LIMIT);
    twin.mirrored = true;
    twin.edits = run.parts[index].edits.mirrored();
    run.parts.push(twin);
    let units = skin_units(&originals, &run.body);
    let end = end_kind(&run.parts, run.parts[index].edits.limb.as_ref().map_or(0, |l| l.id), Some(&data));
    let root = root_of(&originals, &run.body, &run.parts[index]);
    if let Some(limb) = run.parts[index].edits.limb.as_mut() {
        let press: Vec<Vec3> = limb.joints.iter().map(|j| root + Vec3::from_array(*j)).collect();
        let len = limb.end().length();
        let last = press.len() - 1;
        let lengths = limb.lengths();
        let lengths = (lengths.0 * units, lengths.1 * units);
        let joints = if has("--limbs-demo-drag") {
            // The foot out and forward: the knee follows proportionally.
            let target = press[last] + Vec3::new(0.35 * len, 0.0, -0.3 * len);
            drag_joints(&press, last, joint_kind(last, press.len(), end), target, end == EndKind::Foot, units, lengths, 0.0)
        } else {
            // The knee turned 35 degrees forward about the hip -> foot line
            // (both segment lengths kept, as the knee tuning prefers).
            let axis = (press[last] - press[0]).normalize_or(Vec3::NEG_Y);
            let turn = Quat::from_axis_angle(axis, if axis.cross(Vec3::NEG_Z).dot(press[1] - press[0]) >= 0.0 { 0.6 } else { -0.6 });
            let mut target = press[0] + turn * (press[1] - press[0]);
            if target.z > press[1].z {
                target = press[0] + turn.inverse() * (press[1] - press[0]);
            }
            drag_joints(&press, 1, joint_kind(1, press.len(), end), target, end == EndKind::Foot, units, lengths, 0.0)
        };
        limb.joints = joints.iter().map(|j| (*j - root).to_array()).collect();
        if has("--limbs-demo-wheel") {
            let (lo, hi) = limb.balls_range();
            if let Some(b) = limb.balls.get_mut(1) {
                *b = (*b + 8.0 * BALL_STEP).clamp(lo, hi);
            }
            if let Some(m) = limb.muscle.get_mut(0) {
                *m = (*m + 8.0 * MUSCLE_STEP).clamp(0.0, 1.0);
            }
        }
    }
    crate::creature_handles::sync_twins(&mut run.parts, index);
    let base = segment_base(&originals, units);
    settle_ends(&mut run.parts, None, true, &base);
    if has("--limbs-demo-branch") {
        // A second limb dropped with Ctrl on the leg's knee (a branch), turned
        // to point outward; its twin hangs from the twin leg's knee.
        let knee = root_of(&originals, &run.body, &run.parts[index]) + run.parts[index].edits.limb.as_ref().map_or(Vec3::ZERO, |l| l.joint(1));
        if let Some(b) = buy(&mut run, &originals, &asm, price) {
            let slot = crate::baked_model::attachment_point(&originals, &run.body, run.parts[b].bone);
            run.parts[b].offset = crate::game::clamp_offset((knee - slot).to_array(), crate::game::OFFSET_LIMIT);
            if let Some(l) = run.parts[b].edits.limb.as_mut() {
                let turn = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2 * 0.8);
                l.joints = l.joints.iter().map(|j| (turn * Vec3::from_array(*j)).to_array()).collect();
            }
            let id = run.parts[b].edits.limb.as_ref().map_or(0, |l| l.id);
            let body = run.body.clone();
            let roots = |p: &PartInstance| root_of(&originals, &body, p);
            let attached = attach_limb(&mut run, id, &roots, 0.05 * units, 0.0);
            if let Some(b) = run.parts.iter().position(|p| p.edits.limb.as_ref().is_some_and(|l| l.id == id)) {
                run.parts[b].pair_id = run.parts.iter().map(|p| p.pair_id).max().unwrap_or(0) + 1;
                let mut twin = run.parts[b].clone();
                twin.offset[0] = -twin.offset[0];
                twin.mirrored = true;
                twin.edits = run.parts[b].edits.mirrored();
                run.parts.push(twin);
            }
            settle_ends(&mut run.parts, None, true, &base);
            info!("limbs demo: branch {attached:?} {:?}", run.parts.iter().filter_map(|p| p.edits.limb.as_ref()).map(|l| (l.id, l.parent, l.parent_joint)).collect::<Vec<_>>());
        }
    }
    state.visuals_dirty = true;
    info!(
        "limbs demo: {} joints {:?}, balls {:?}, rigid {}, end {end:?} part {:?}, units {units}",
        run.parts[index].edits.limb.as_ref().map_or(0, |l| l.joints.len()),
        run.parts[index].edits.limb.as_ref().map(|l| l.joints.clone()),
        run.parts[index].edits.limb.as_ref().map(|l| l.balls.clone()),
        asm.rigid,
        asm.end
    );
    for p in &run.parts {
        debug!("  part {:08X} def {:08X?} bone {} offset {:?} on_limb {:x} seg {} scale {} limb {:?} model {}", p.instance, p.definition_id, p.bone, p.offset, p.edits.on_limb, p.edits.segment, p.scale, p.edits.limb.as_ref().map(|l| l.id), crate::editor::model_for_part(&originals, p).is_some());
    }
}

/// Run condition of every creator-limbs system: the resources they use exist
/// (headless test apps add only some plugins).
#[allow(clippy::too_many_arguments)]
pub fn ready(
    (a, b, c, d): (Option<Res<Originals>>, Option<Res<RunProgress>>, Option<Res<EditorState>>, Option<Res<crate::spui::view::SpuiPointer>>),
    (e, f, g, h): (
        Option<Res<ButtonInput<MouseButton>>>,
        Option<Res<Assets<Mesh>>>,
        Option<Res<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>>,
        Option<Res<Assets<Image>>>,
    ),
    (i, j, k, l): (Option<Res<Assets<StandardMaterial>>>, Option<Res<CreatureEditor>>, Option<Res<crate::creature_handles::HandleState>>, Option<Res<LimbState>>),
    (m, n, o, q): (
        Option<Res<crate::creature_handles::HandleCache>>,
        Option<Res<crate::creature_blocks::DriveState>>,
        Option<Res<ButtonInput<KeyCode>>>,
        Option<Res<Messages<bevy::input::mouse::MouseWheel>>>,
    ),
) -> bool {
    a.is_some()
        && b.is_some()
        && c.is_some()
        && d.is_some()
        && e.is_some()
        && f.is_some()
        && g.is_some()
        && h.is_some()
        && i.is_some()
        && j.is_some()
        && k.is_some()
        && l.is_some()
        && m.is_some()
        && n.is_some()
        && o.is_some()
        && q.is_some()
}

/// Systems of the creator-limbs modules.
pub struct CreatorLimbsPlugin;

impl Plugin for CreatorLimbsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::creature_handles::HandleState>()
            .init_resource::<crate::creature_handles::HandleCache>()
            .init_resource::<LimbState>()
            .init_resource::<crate::creature_blocks::DriveState>()
            .add_systems(
                Update,
                (crate::creature_cell_convert::capture_hook, intercept, demo, crate::creature_handles::press, press)
                    .chain()
                    .run_if(ready)
                    .before(crate::editor::input)
                    .before(crate::editor::drag),
            )
            .add_systems(
                Update,
                (crate::creature_handles::demo_part, crate::creature_blocks::ensure_part_models)
                    .run_if(ready)
                    .before(crate::editor::refresh_parts),
            )
            .add_systems(
                Update,
                crate::creature_blocks::test_drive
                    .run_if(ready)
                    .after(crate::creature_editor_paint::paint_ui)
                    .after(crate::creature::accessory_follow)
                    .after(crate::editor::refresh_parts),
            )
            .add_systems(Update, sync_limbs.run_if(ready).after(crate::creature_editor_paint::paint_ui))
            .add_systems(Update, crate::creature_handles::apply_posed_meshes.run_if(ready).after(crate::editor::refresh_parts));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leg() -> Limb {
        Limb {
            id: 1,
            joints: vec![[0.0; 3], [0.0, -0.5, 0.0], [0.0, -1.0, 0.1]],
            balls: vec![0.5, 1.0, 1.1],
            muscle: vec![0.5, 0.5],
            segments: vec![7, 8],
            tentacle: true,
            ..Default::default()
        }
    }

    fn three() -> Limb {
        Limb {
            id: 1,
            joints: vec![[0.0; 3], [0.0, -0.4, 0.0], [0.0, -0.8, -0.1], [0.0, -1.2, 0.0]],
            balls: vec![0.5, 1.0, 1.1, 0.9],
            muscle: vec![0.5, 0.6, 0.7],
            segments: vec![7, 8, 9],
            rigid: true,
            rest: vec![[0.0; 3], [0.0, -0.4, 0.0], [0.0, -0.8, -0.1], [0.0, -1.2, 0.0]],
            frames: vec![[0.0, 0.0, 0.0, 1.0]; 3],
            ..Default::default()
        }
    }

    /// A rigid limb part with its segment parts and a foot.
    fn rigid_parts() -> Vec<PartInstance> {
        let mut limb = PartInstance::new(0, 3);
        limb.offset = [0.4, 0.0, 0.0];
        limb.edits.limb = Some(three());
        let mut parts = vec![limb];
        for k in 1..=3 {
            let mut s = PartInstance::new(20 + k, 0);
            s.edits.on_limb = 1;
            s.edits.segment = k;
            parts.push(s);
        }
        let mut foot = PartInstance::new(9, 0);
        foot.edits.on_limb = 1;
        parts.push(foot);
        settle_ends(&mut parts, None, true, &|_| 1.0);
        parts
    }

    #[test]
    fn rigid_segments_ride_their_joints() {
        let parts = rigid_parts();
        for k in 1..=3usize {
            let s = &parts[k];
            let l = parts[0].edits.limb.as_ref().unwrap();
            assert!((Vec3::from_array(s.offset) - (Vec3::new(0.4, 0.0, 0.0) + l.joint(k - 1))).length() < 1e-5);
            assert!((s.scale - 1.0).abs() < 1e-5, "rest pose at the skin's size: {}", s.scale);
        }
        // Bending section 1 turns its mesh onto the new direction and stretches it.
        let mut l = three();
        l.joints[2] = [0.0, -0.4, -0.8];
        let (_, rot, scale) = segment_pose(&l, 1).unwrap();
        let rest = Vec3::from_array(l.rest[2]) - Vec3::from_array(l.rest[1]);
        assert!((rot * rest.normalize() - Vec3::NEG_Z).length() < 1e-4);
        assert!((scale - 0.8 / rest.length()).abs() < 1e-3, "{scale}");
        // A twin limb's segments are the mirror image.
        let m = l.mirrored();
        let (_, mrot, _) = segment_pose(&m, 1).unwrap();
        let v = Vec3::new(0.3, 0.2, 0.1);
        let a = rot * v;
        let b = mrot * Vec3::new(-v.x, v.y, v.z);
        assert!((Vec3::new(-a.x, a.y, a.z) - b).length() < 1e-4);
    }

    #[test]
    fn ctrl_split_remove_and_join() {
        // Split at section 1: the stub keeps joints 0..=1, the new limb takes
        // sections 1.. with segment parts 2, 3 (renumbered 1, 2) and the foot.
        let mut run = RunProgress::default();
        run.parts = rigid_parts();
        let index = split_limb(&mut run, 0, 1).expect("split");
        let stub = run.parts[0].edits.limb.clone().unwrap();
        let tail = run.parts[index].edits.limb.clone().unwrap();
        assert_eq!(stub.joints.len(), 2);
        assert_eq!(tail.joints.len(), 3);
        assert_eq!(tail.joints[0], [0.0; 3]);
        assert_ne!(tail.id, stub.id);
        assert_eq!(run.parts.iter().filter(|p| p.edits.on_limb == tail.id).count(), 3, "two segments and the foot");
        assert!(run.parts.iter().any(|p| p.edits.on_limb == tail.id && p.edits.segment == 2));
        assert!(run.parts.iter().any(|p| p.edits.on_limb == stub.id && p.edits.segment == 1));
        assert!((Vec3::from_array(run.parts[index].offset) - Vec3::new(0.4, -0.4, 0.0)).length() < 1e-5);
        // Joining puts it back onto the stub's free end.
        let roots = |p: &PartInstance| Vec3::from_array(p.offset);
        assert!(join_limbs(&mut run, tail.id, &roots, 0.05));
        let joined = run.parts.iter().find_map(|p| p.edits.limb.clone()).unwrap();
        assert_eq!(joined.joints.len(), 4);
        assert!((joined.end() - three().end()).length() < 1e-5);
        assert_eq!(run.parts.iter().filter(|p| p.edits.limb.is_some()).count(), 1);
        let segs: Vec<u32> = run.parts.iter().filter(|p| p.edits.on_limb == joined.id && p.edits.segment > 0).map(|p| p.edits.segment).collect();
        assert_eq!(segs.len(), 3);
        assert!(segs.contains(&3));
        // Removing section 1: the joints below move up, segment part 2 goes.
        let before = joined.end();
        assert!(remove_limb_section(&mut run, 0, 1));
        let l = run.parts[0].edits.limb.clone().unwrap();
        assert_eq!(l.joints.len(), 3);
        let shift = three().joint(1) - three().joint(2);
        assert!((l.end() - (before + shift)).length() < 1e-5);
        assert_eq!(run.parts.iter().filter(|p| p.edits.segment > 0).count(), 2);
        assert!(!remove_limb_section(&mut run, 0, 0), "the shoulder / hip section stays");
        // A Ctrl press on the hip section carries the whole limb.
        assert_eq!(split_limb(&mut run, 0, 0), Some(0));
    }

    #[test]
    fn ctrl_drop_on_a_joint_branches_and_the_branch_rides_it() {
        // A leg with a foot and a short limb dropped (Ctrl) on its knee.
        let mut run = RunProgress::default();
        let mut leg_part = PartInstance::new(0, 3);
        leg_part.offset = [0.4, 0.0, 0.0];
        leg_part.edits.limb = Some(leg());
        let mut foot = PartInstance::new(5, 3);
        foot.edits.on_limb = 1;
        let mut arm = PartInstance::new(0, 3);
        arm.offset = [0.41, -0.5, 0.0];
        arm.edits.limb = Some(Limb { id: 2, joints: vec![[0.0; 3], [0.3, 0.0, 0.0]], balls: vec![1.0, 1.0], muscle: vec![0.5], ..Default::default() });
        run.parts = vec![leg_part, foot, arm];
        let roots = |p: &PartInstance| Vec3::from_array(p.offset);
        assert_eq!(attach_limb(&mut run, 2, &roots, 0.05, 10.0), Some(Attach::Branched));
        let branch = run.parts[2].edits.limb.clone().unwrap();
        assert_eq!((branch.parent, branch.parent_joint), (1, 1));
        assert_eq!(branch.joints.len(), 2, "not merged into the leg");
        assert_eq!(branch_of_twin(&branch), (1 ^ TWIN_BIT, 1));
        // The branch root rides the knee when the knee moves.
        run.parts[0].edits.limb.as_mut().unwrap().joints[1] = [0.1, -0.6, 0.0];
        settle_ends(&mut run.parts, None, true, &|_| 1.0);
        assert!((Vec3::from_array(run.parts[2].offset) - Vec3::new(0.5, -0.6, 0.0)).length() < 1e-5);
        // Grabbing the branch takes it off the leg.
        let mut held = run.parts.clone();
        settle_ends(&mut held, Some(2), false, &|_| 1.0);
        assert_eq!(held[2].edits.limb.as_ref().unwrap().parent, 0);
        // Skin records: parents first; the branch hangs from the thigh (the
        // record ending at the knee).
        let geom = |p: &PartInstance, parent| {
            let l = p.edits.limb.as_ref().unwrap();
            let root = Vec3::from_array(p.offset);
            LimbGeom { joints: l.joints.iter().map(|j| root + Vec3::from_array(*j)).collect(), balls: l.balls.clone(), muscle: l.muscle.clone(), tentacle: false, skinned: Vec::new(), parent }
        };
        let recs = limb_records(&[geom(&run.parts[2], Some((1, 1))), geom(&run.parts[0], None)], &[], 1.0);
        assert_eq!(recs.len(), 3);
        assert_eq!((recs[0].parent, recs[1].parent, recs[2].parent), (-1, 0, 0));
        assert!((recs[2].position - recs[0].segment_end()).length() < 1e-5, "the branch starts at the knee");
        // Removing the shin moves a branch on the foot joint up to the knee.
        run.parts[2].edits.limb.as_mut().unwrap().parent_joint = 2;
        assert_eq!(section_refund(None, &run.parts[0], 1), SEGMENT_PRICE);
        assert!(remove_limb_section(&mut run, 0, 1));
        assert_eq!(run.parts[2].edits.limb.as_ref().unwrap().parent_joint, 1);
        // Selling the leg sells its branch too.
        assert!(sell(&mut run, 0, 0.0));
        assert!(run.parts.iter().all(|p| p.edits.limb.is_none() && p.edits.on_limb == 0), "{:?}", run.parts.len());
    }

    fn branch_of_twin(l: &Limb) -> (u32, u32) {
        let m = l.mirrored();
        (m.parent, m.parent_joint)
    }

    #[test]
    fn ray_segment_distance() {
        let (d, t) = ray_segment(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z, Vec3::new(-1.0, 0.1, 0.0), Vec3::new(1.0, 0.1, 0.0));
        assert!((d - 0.1).abs() < 1e-5);
        assert!((t - 5.0).abs() < 1e-5);
    }

    /// The retail drag of a leg in player space: the knee under the camera
    /// ray keeps its depth, the foot stays planted; the foot slides on its
    /// height and re-places the knee.
    #[test]
    fn retail_leg_drag() {
        let units = 0.5;
        let press = vec![Vec3::new(0.4, -0.5, 0.0), Vec3::new(0.5, -0.8, -0.1), Vec3::new(0.5, -1.25, 0.0)];
        let lengths = (MIN_SEGMENT * units, MAX_SEGMENT * units);
        let cam = Vec3::new(0.5, -0.6, 4.0);
        let aim = press[1] + Vec3::new(0.1, 0.05, 0.0);
        let d = (aim - cam).normalize();
        let t = drag_target(JointKind::Knee, &press, 1, cam, d, &Tunings::default(), units, lengths).unwrap();
        assert!((t.z - press[1].z).abs() < 0.08, "{t}");
        let j = drag_joints(&press, 1, JointKind::Knee, t, true, units, lengths, 0.05);
        assert_eq!(j[2], press[2]);
        // The foot: on the plane of its height.
        let aim = press[2] + Vec3::new(0.2, 0.0, -0.2);
        let d = (aim - cam).normalize();
        let f = drag_target(JointKind::Foot, &press, 2, cam, d, &Tunings::default(), units, lengths).unwrap();
        assert!((f.y - press[2].y).abs() < 1e-4);
        let j = drag_joints(&press, 2, JointKind::Foot, f, true, units, lengths, 0.05);
        assert!(j[1].distance(press[1]) > 1e-3, "the knee moves with the foot");
        assert!(j[2].y >= crate::creature_cell_convert::GROUND_Y - 1e-5);
    }

    /// Installed data: the rigid exo leg `exo_l_01` is a `.bem` assembly of
    /// two `ce_limb_exo_*` segments and a foot.
    #[test]
    fn installed_exo_assembly() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        let asm = assembly(&index, crate::world_objects::name_id("limb_assembly_exo_l_01")).expect("exo assembly");
        assert!(asm.rigid);
        assert_eq!(asm.segments.len(), 2);
        assert!(asm.end.is_some());
        let limb = limb_of_with(&asm, 0.5, 1, |seg| catalog.by_definition_in(CREATURE_PARTS, seg).map(mesh_basis));
        assert_eq!(limb.frames.len(), 2);
        assert_eq!(limb.rest, limb.joints);
        for (k, seg) in asm.segment_parts.iter().enumerate() {
            let def = catalog.by_definition_in(CREATURE_PARTS, *seg).expect("segment part");
            let handles = crate::creature_handles::load_part_handles(&index, def);
            let q = Quat::from_array(limb.frames[k]);
            // The mesh's socket axis (model -Y) points down the segment.
            let axis = q * (mesh_basis(def) * Vec3::NEG_Y);
            let dir = (limb.joint(k + 1) - limb.joint(k)).normalize();
            println!(
                "segment {k}: {} scale {} rot {:?} offset {:?} length {:.3} axis.dir {:.4} handles {:?} weights {:?}",
                def.name,
                def.model_scale,
                def.model_rotation,
                def.model_offset,
                limb.joint(k + 1).distance(limb.joint(k)),
                axis.dot(dir),
                handles.handles.iter().map(|h| (format!("{:08X}", h.id), h.start, h.end, h.default)).collect::<Vec<_>>(),
                asm.segment_handles.get(k)
            );
            assert!(axis.dot(dir) > 0.999, "the mesh points down its segment");
            // The mesh's extent in model coordinates (exploration).
            if let Ok(bytes) = index.read(def.model) {
                let mut stats = crate::rw4::MeshStats::default();
                for m in crate::rw4::extract_meshes(&bytes, &mut stats) {
                    let (lo, hi) = m.positions.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), p| (a.min(Vec3::from_array(*p)), b.max(Vec3::from_array(*p))));
                    println!("  mesh section {}: {} vertices, model box {lo} .. {hi}", m.source_section, m.positions.len());
                }
            }
        }
        let foot = catalog.by_definition_in(CREATURE_PARTS, asm.end.unwrap().1).expect("foot");
        println!("end {} scale {}", foot.name, asm.end_scale);
        // Every exo item of the Creature palette is now an assembly.
        let data = CreatureEditorData::load(&index);
        let palette = data.parts_palette(crate::creature_editor_data::SetupKind::Creature).expect("palette");
        let ok = palette.pages.iter().flat_map(|p| p.items.iter()).filter(|i| assembly(&index, i.part).is_some()).count();
        println!("creature palette limb assemblies: {ok}");
        assert!(ok >= 48, "{ok}");
    }

    #[test]
    fn joint_drag_reaims_the_parent_segment() {
        let mut l = leg();
        let end_before = l.end() - l.joint(1);
        assert!(drag_joint(&mut l, 1, Vec3::new(0.0, -0.4, -0.3), 0.06, 6.0));
        // Length of the parent segment kept within range, children rigid.
        assert!((l.joint(1).length() - 0.5).abs() < 1e-5);
        assert!((l.end() - l.joint(1) - end_before).length() < 1e-5);
        // Clamped to the socket range.
        let target = l.joint(1) + Vec3::new(0.0, -10.0, 0.0);
        assert!(drag_joint(&mut l, 2, target, 0.06, 1.0));
        assert!((l.end().distance(l.joint(1)) - 1.0).abs() < 1e-5);
        assert!(!drag_joint(&mut l, 0, Vec3::ONE, 0.06, 1.0), "the root moves with the part");
        let m = l.mirrored();
        assert_eq!(m.id, 1 | TWIN_BIT);
        assert_eq!(m.joints[1][0], -l.joints[1][0]);
    }

    #[test]
    fn limb_records_hang_from_the_nearest_vertebra() {
        let units = 2.0;
        let vertebrae: Vec<SkinRecord> = (0..3)
            .map(|i| SkinRecord {
                kind: BlockKind::Vertebra,
                flags: skin::flag::USES_SKIN,
                position: Vec3::new(0.0, i as f32 * 0.3, 0.2),
                parent: i - 1,
                ..Default::default()
            })
            .collect();
        let root = spore_to_bevy(Vec3::new(0.2, 0.6, 0.0)) * units;
        let geom = LimbGeom { joints: leg().joints.iter().map(|j| root + Vec3::from_array(*j)).collect(), balls: vec![0.5, 1.0, 1.1], muscle: vec![0.5, 0.5], tentacle: true, skinned: Vec::new(), parent: None };
        let recs = limb_records(&[geom.clone()], &vertebrae, units);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].parent, 2, "nearest vertebra (y 0.6)");
        assert_eq!(recs[1].parent, 3);
        // Segment ends land on the next joints.
        for (k, r) in recs.iter().enumerate() {
            let e = spore_to_bevy(r.segment_end()) * units;
            assert!(e.distance(geom.joints[k + 1]) < 1e-4);
            assert_eq!(r.next_muscle_scale, geom.balls[k + 1]);
        }
        // The whole set builds a skin with a limb chain.
        let mut all = vertebrae.clone();
        all.extend(recs);
        let ordered = skin::order_records(all);
        let prims = skin::primitives(&ordered);
        assert_eq!(prims.chains.iter().filter(|c| c.limb).count(), 1);
    }

    #[test]
    fn rigid_sections_are_bones_without_skin() {
        let units = 2.0;
        let vertebrae: Vec<SkinRecord> = (0..3)
            .map(|i| SkinRecord {
                kind: BlockKind::Vertebra,
                flags: skin::flag::USES_SKIN,
                position: Vec3::new(0.0, i as f32 * 0.3, 0.2),
                parent: i - 1,
                ..Default::default()
            })
            .collect();
        let root = spore_to_bevy(Vec3::new(0.2, 0.6, 0.0)) * units;
        let joints: Vec<Vec3> = leg().joints.iter().map(|j| root + Vec3::from_array(*j)).collect();
        let skinned = LimbGeom { joints: joints.clone(), balls: vec![0.5, 1.0, 1.1], muscle: vec![0.5, 0.5], tentacle: false, skinned: Vec::new(), parent: None };
        let exo = LimbGeom { skinned: vec![false, false], ..skinned.clone() };
        let mixed = LimbGeom { skinned: vec![false, true], ..skinned.clone() };
        // A fully rigid (exo) leg: one chained record per section, no skin.
        let recs = limb_records(&[exo], &vertebrae, units);
        assert_eq!(recs.len(), 2);
        assert!(recs.iter().all(|r| r.kind == BlockKind::Limb && !r.uses_skin()));
        assert_eq!((recs[0].parent, recs[1].parent), (2, 3), "the second section hangs from the first");
        // The skin ignores them: the same field as the vertebrae alone.
        let mut all = vertebrae.clone();
        all.extend(recs);
        let prims = skin::primitives(&skin::order_records(all));
        assert_eq!(prims.chains.iter().filter(|c| c.limb).count(), 0);
        let base = skin::primitives(&skin::order_records(vertebrae.clone()));
        assert_eq!(prims.field.balls.len(), base.field.balls.len());
        // A mixed chain: the skinned section still makes a limb chain.
        let recs = limb_records(&[mixed], &vertebrae, units);
        assert_eq!(recs.iter().map(|r| r.uses_skin()).collect::<Vec<_>>(), vec![false, true]);
        let mut all = vertebrae.clone();
        all.extend(recs);
        let prims = skin::primitives(&skin::order_records(all));
        assert_eq!(prims.chains.iter().filter(|c| c.limb).count(), 1);
    }

    #[test]
    fn end_parts_ride_their_limbs_and_twin() {
        let mut limb = PartInstance::new(0, 3);
        limb.offset = [0.4, 0.0, 0.0];
        limb.edits.limb = Some(leg());
        let mut foot = PartInstance::new(9, 0);
        foot.edits.on_limb = 1;
        let mut parts = vec![limb.clone(), foot];
        assert!(settle_ends(&mut parts, None, true, &|_| 1.0));
        assert_eq!(parts[1].bone, 3);
        assert!((Vec3::from_array(parts[1].offset) - Vec3::new(0.4, -1.0, 0.1)).length() < 1e-5);
        // A limb twin gets a mirrored end part, paired with the original.
        let mut twin = limb.clone();
        twin.offset = [-0.4, 0.0, 0.0];
        twin.edits = limb.edits.mirrored();
        parts.push(twin);
        assert!(settle_ends(&mut parts, None, true, &|_| 1.0));
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[3].edits.on_limb, 1 | TWIN_BIT);
        assert!(parts[3].mirrored);
        assert_eq!(parts[1].pair_id, parts[3].pair_id);
        assert!((Vec3::from_array(parts[3].offset) - Vec3::new(-0.4, -1.0, 0.1)).length() < 1e-5);
        assert!(!settle_ends(&mut parts, None, true, &|_| 1.0), "stable");
        // Selling the limb removes its twin and both end parts.
        parts[0].pair_id = 5;
        parts[2].pair_id = 5;
        let mut run = RunProgress::default();
        run.parts = parts;
        run.editor_dna = 0.0;
        assert!(sell(&mut run, 0, 50.0));
        assert!(run.parts.is_empty(), "{:?}", run.parts);
        assert_eq!(run.editor_dna, 50.0);
        // A grabbed end part leaves its limb; pruning removes orphans.
        let mut parts = vec![limb.clone(), PartInstance { edits: crate::creature_handles::PartEdits { on_limb: 1, ..default() }, ..PartInstance::new(9, 0) }];
        settle_ends(&mut parts, Some(1), false, &|_| 1.0);
        assert_eq!(parts[1].edits.on_limb, 0);
        let mut orphan = vec![PartInstance { edits: crate::creature_handles::PartEdits { on_limb: 4, ..default() }, ..PartInstance::new(9, 0) }];
        assert!(settle_ends(&mut orphan, None, true, &|_| 1.0));
        assert!(orphan.is_empty());
    }

    /// Installed data: the Cell-to-Creature legs are assemblies of skinned
    /// tentacle-type segments ending in a foot part; `x_leg_09` matches the
    /// copy in `notes/skin-re`.
    #[test]
    fn installed_leg_assemblies() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        let item = crate::world_objects::name_id("limb_assembly_x_leg_09");
        let asm = assembly(&index, item).expect("x_leg_09 assembly");
        println!("x_leg_09: {} segments, parts {:08X?}, end {:08X?}", asm.segments.len(), asm.segment_parts, asm.end);
        assert_eq!(asm.segments.len(), 2);
        for s in &asm.segments {
            println!("  seg pos {} socket {} muscle {} ball {} next {} scale {} flags {:#x}", s.position, s.socket_distance, s.muscle_scale, s.base_muscle_scale, s.next_muscle_scale, s.scale, s.flags);
        }
        let gap = asm.segments[0].segment_end().distance(asm.segments[1].position);
        println!("  segment 0 end to segment 1 start: {gap}");
        let (_, end) = asm.end.expect("foot");
        let foot = catalog.by_definition_in(CREATURE_PARTS, end).expect("foot part in the catalog");
        println!("  foot {} model scale {}", foot.name, foot.model_scale);
        if let Ok(bytes) = index.read(foot.model) {
            let mut stats = crate::rw4::MeshStats::default();
            for m in crate::rw4::extract_meshes(&bytes, &mut stats) {
                let (lo, hi) = m.positions.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), p| (a.min(Vec3::from_array(*p)), b.max(Vec3::from_array(*p))));
                println!("  foot mesh section {}: {} vertices, model box {lo} .. {hi}", m.source_section, m.positions.len());
            }
        }
        let limb = limb_of(&asm, 2.0, 1);
        assert_eq!(limb.joints.len(), 3);
        assert!(limb.tentacle);
        assert!(limb.end()[1] < 0.0, "the leg hangs down: {:?}", limb.joints);
        // The limb items of the Creature palette: assemblies with a chain.
        let data = CreatureEditorData::load(&index);
        let palette = data.parts_palette(crate::creature_editor_data::SetupKind::Creature).expect("creature palette");
        let (mut ok, mut keyed, mut failed) = (0, 0, Vec::new());
        for item in palette.pages.iter().flat_map(|p| p.items.iter()) {
            if assembly_key(&index, item.part).is_none() {
                continue;
            }
            keyed += 1;
            match assembly(&index, item.part) {
                Some(a) if !a.segments.is_empty() => ok += 1,
                _ => failed.push(data.parts.get(&item.part).map_or(format!("{:08X}", item.part), |i| i.name.clone())),
            }
        }
        println!("creature palette: {keyed} assembly items, {ok} with a limb chain; without: {failed:?}");
        // The other half are rigid (exo) limbs: unskinned segment parts (limb
        // type 2), not offered yet.
        assert!(ok >= 20, "{ok}");
    }
}
