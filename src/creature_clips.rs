//! Creature animation clips on any creature's skeleton (`briefs/clip-player.md`,
//! `notes/skin-re-notes.md` section 5, `notes/clip-player-notes.md`).
//!
//! ORIGINAL data and rules used here:
//! * clips are the installed `.animation` resources, reached through the TLSA
//!   action lists (action id -> first choice -> its clips) like
//!   `cinematics::actors`; the TLSA `blend_in_time` is the fade-in;
//! * every channel addresses blocks by its primary context (capability tag +
//!   spatial selection flags, `retarget::Rig::select`); positions are
//!   specialized by `retarget::specialize_position` (rest-relative 0x20,
//!   bounds 0x40 / limb-length 0x80 scale, ground-relative movement 0x10);
//!   curves are the native evaluator of `animation_sampling`;
//! * the block list is the creature's own (`creature_blocks` for the player's
//!   skin, the `.creaturedata` rigblocks and their capability table for the
//!   installed creatures), the skin is deformed by the retail bone weights.
//!
//! The installed clips show what the goals mean: at rest every rest-relative
//! key is zero (`csa_idle_gen_editor`), the punch moves the root, the mouth and
//! the striking hand forward together, a walk cycle slides the planted foot
//! back under the body. So all goals are read in the creature (character)
//! frame. ADAPTED (the retail spine solver, limb IK and procedural gait are not
//! traced, see the notes):
//! * the root goal moves the whole spine rigidly about the root block; the
//!   main mouth's goal bends the chain root -> head vertebra (translation and
//!   rotation spread along the chain by arc length, at most 0.6 of its length);
//! * a limb chain ending in a foot (or a pseudo-foot) or a grasper reaches its
//!   end goal with two-bone IK (CCD for other lengths, aim for one segment);
//!   feet without a goal stay planted, other limbs ride their vertebra; end
//!   parts take their rotation goal (rest-relative, creature frame);
//! * pseudo-feet answer `foot` queries; when one channel selects several feet
//!   of a side (four or more legs), every second rank (front to back) runs half
//!   a foot cycle later, so quadrupeds trot on diagonals;
//! * a looping gait clip plays at the creature's ground speed divided by the
//!   clip's own planted-foot speed (limb lengths per second, measured from its
//!   foot keys) times the creature's leg length;
//! * clips cross-fade as a layer stack (newest on top).
//!
//! ORIGINAL since `creature-look` (`creature_look`, `creature_morph`): channels
//! with the secondary movement flag key their goals in the frame toward the
//! secondary point (the external look target when the secondary context
//! selects nothing): positions FUN_009B0D60, aimed rotations FUN_009B06A0;
//! the movement flags 2 / 8 alone leave a channel on the regular path; the
//! deform components (mouth open, blinks, hands, feet, ears) come out per block
//! for the part rigs (`pose_full`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use bevy::math::{Affine3A, Mat3A, Quat, Vec3, Vec3A, Vec4};

use crate::animation::{Animation, InfoKey, Keys, PositionKey};
use crate::animation_sampling::{sample, RotationCurve};
use crate::creature_skin::BlockKind;
use crate::retarget::{specialize_position, Body, Rig};

/// Fade-in of a clip whose TLSA group gives none (seconds).
pub const DEFAULT_BLEND: f32 = 0.25;
/// A foot key this close to the channel's lowest key (generalized units) is
/// planted.
const PLANTED: f32 = 0.05;

// ---------------------------------------------------------------------------
// Clips
// ---------------------------------------------------------------------------

/// A decoded clip and what the player needs to know about it.
#[derive(Debug)]
pub struct Clip {
    /// The requested action (TLSA group) id, or the clip id itself.
    pub id: u32,
    pub anim: Animation,
    /// Header flag 8: the last key repeats the first (loop seam).
    pub cyclic: bool,
    /// Play length: a cyclic clip loops over `length - 1` frames.
    pub seconds: f32,
    pub blend_in: f32,
    /// Planted-foot speed of a gait cycle and its scale mode (0x80 limb
    /// lengths, 0x40 bounds, 0 creature units) per second.
    pub stance_speed: Option<(f32, u32)>,
    /// Frames of one foot cycle (the foot keys repeat after it).
    pub foot_cycle: Option<f32>,
    /// TLSA `allow_locomotion`.
    pub allow_locomotion: bool,
}

impl Clip {
    pub fn new(id: u32, anim: Animation, blend_in: f32) -> Self {
        let cyclic = anim.flags & 8 != 0;
        let frames = if cyclic { (anim.length_frames - 1.0).max(1.0) } else { anim.length_frames.max(1.0) };
        let seconds = (frames * anim.frame_length).max(1e-3);
        let (stance_speed, foot_cycle) = gait_of(&anim);
        Self { id, cyclic, seconds, blend_in, stance_speed, foot_cycle, allow_locomotion: false, anim }
    }

    /// The frame at `t` seconds (`looped`: wraps, else holds the end).
    pub fn frame_at(&self, t: f32, looped: bool) -> f32 {
        let t = if looped { t.rem_euclid(self.seconds) } else { t.clamp(0.0, self.seconds) };
        (t / self.anim.frame_length).clamp(0.0, self.anim.length_frames)
    }
}

fn info(ch: &crate::animation::Channel) -> Option<&Vec<InfoKey>> {
    ch.components.iter().find_map(|c| if let Keys::Info(t) = &c.keys { Some(t) } else { None })
}

fn positions(ch: &crate::animation::Channel) -> Option<(&Vec<PositionKey>, u32)> {
    ch.components.iter().find_map(|c| match &c.keys {
        Keys::Position(k) if c.flags & 0x10 != 0 => Some((k, c.flags)),
        _ => None,
    })
}

/// Planted-foot speed and foot cycle of a gait clip, from its foot keys: the
/// backward slide between consecutive keys near the lowest foot height over
/// their time (in the channel's scale: limb lengths 0x80, bounds 0x40 or
/// creature units); the cycle is the shortest repeat of the key values (the
/// seam key excepted). Slides slower than 0.3 units per second are no gait.
pub fn gait_of(anim: &Animation) -> (Option<(f32, u32)>, Option<f32>) {
    let (mut dist, mut frames, mut cycle, mut scale) = (0.0f32, 0.0f32, None, 0u32);
    for ch in anim.channels.iter().filter(|c| c.primary.capability.trim_end_matches('\0') == "foot") {
        let (Some(times), Some((keys, flags))) = (info(ch), positions(ch)) else { continue };
        if flags & 0x20 == 0 {
            continue;
        }
        scale = flags & 0xC0;
        let n = keys.len().min(times.len());
        let low = keys.iter().map(|k| k.position[2]).fold(f32::MAX, f32::min) + PLANTED;
        for i in 1..n {
            let (a, b) = (keys[i - 1].position, keys[i].position);
            let df = times[i].frame as f32 - times[i - 1].frame as f32;
            if a[2] <= low && b[2] <= low && b[1] > a[1] && df > 0.0 {
                dist += b[1] - a[1];
                frames += df;
            }
        }
        if cycle.is_none() {
            cycle = (2..=n / 2).find_map(|m| {
                let same = (0..(n - m - 1).max(1)).all(|i| (0..3).all(|a| (keys[i].position[a] - keys[i + m].position[a]).abs() < 1e-3));
                let p = times[m].frame as f32 - times[0].frame as f32;
                (same && p > 0.0).then_some(p)
            });
        }
    }
    let speed = (frames > 0.0 && dist > 0.0).then(|| dist / (frames * anim.frame_length)).filter(|s| *s > 0.3);
    (speed.map(|s| (s, scale)), cycle)
}

static LIBRARY: OnceLock<Mutex<HashMap<u32, Option<Arc<Clip>>>>> = OnceLock::new();

/// The clip of an action id (TLSA group -> first choice -> first decodable
/// clip), else the clip with that instance id; decoded once.
pub fn clip(originals: &crate::Originals, id: u32) -> Option<Arc<Clip>> {
    let lib = LIBRARY.get_or_init(Default::default);
    if let Some(c) = lib.lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
        return c.clone();
    }
    let loaded = load(originals, id).map(Arc::new);
    lib.lock().unwrap_or_else(|e| e.into_inner()).insert(id, loaded.clone());
    loaded
}

fn load(originals: &crate::Originals, id: u32) -> Option<Clip> {
    let group = originals.action_lists.lists.iter().flat_map(|(_, l)| &l.groups).find(|g| g.id == id);
    let mut candidates: Vec<u32> = group.and_then(|g| g.choices.first()).map(|c| c.animations.iter().map(|a| a.id).collect()).unwrap_or_default();
    candidates.push(id);
    for c in candidates {
        let Ok(key) = originals.animation_catalog.resolve(c, None) else { continue };
        let Ok(bytes) = originals.animation_catalog.read(&originals.resource_index, key) else { continue };
        let Ok(anim) = crate::animation::parse(&bytes) else { continue };
        if anim.channels.is_empty() || anim.length_frames <= 0.0 {
            continue;
        }
        let blend = group.map_or(0.0, |g| g.blend_in_time);
        let mut clip = Clip::new(id, anim, if blend > 0.0 { blend.clamp(0.05, 1.0) } else { DEFAULT_BLEND });
        clip.allow_locomotion = group.is_some_and(|g| g.allow_locomotion);
        return Some(clip);
    }
    None
}

// ---------------------------------------------------------------------------
// Rig
// ---------------------------------------------------------------------------

/// One block of a clip rig (Spore creature space: +X left, +Y back, +Z up).
#[derive(Clone, Debug)]
pub struct Joint {
    pub kind: BlockKind,
    pub parent: Option<usize>,
    /// Rest rotation pivot (a limb segment: its start joint).
    pub pivot: Vec3,
    /// Where selection queries see the block (a limb segment: its end).
    pub at: Vec3,
    pub caps: Vec<[u8; 4]>,
}

impl Joint {
    fn has(&self, tag: &[u8; 4]) -> bool {
        self.caps.iter().any(|c| c == tag)
    }
}

/// A limb chain: segments from the body outward, the rest joint points
/// (segment starts, then the last end) and the end part.
#[derive(Clone, Debug)]
struct Chain {
    segs: Vec<usize>,
    points: Vec<Vec3>,
    end: Option<usize>,
    foot: bool,
}

/// The skeleton a clip plays on.
#[derive(Clone, Debug)]
pub struct ClipRig {
    pub joints: Vec<Joint>,
    rig: Rig,
    order: Vec<usize>,
    pub root: usize,
    /// Root .. head vertebra with arc-length fractions.
    spine: Vec<(usize, f32)>,
    spine_reach: f32,
    mouth: Option<usize>,
    chains: Vec<Chain>,
    chain_start: Vec<Option<usize>>,
    pub ground_z: f32,
    /// Mean limb length of the feet (creature units), for speed matching.
    pub leg_length: f32,
    /// Rest orientation of every block in the creature frame (the look frame
    /// keeps a block's rest up axis; identity when unknown).
    rests: Vec<Quat>,
}

/// Deform tracks per block (`ClipRig::pose_full`).
pub type BlockDeforms = Vec<Vec<crate::creature_morph::Track>>;

impl ClipRig {
    /// Build from blocks (`ground_z`: the creature's lowest point).
    pub fn new(joints: Vec<Joint>, ground_z: f32) -> Result<Self, String> {
        let n = joints.len();
        if n == 0 || n > 2048 {
            return Err("clip rig needs 1..2048 blocks".into());
        }
        let parent = |i: usize| joints[i].parent.filter(|&p| p < n && p != i);
        let root = (0..n)
            .find(|&i| joints[i].has(b"root"))
            .or_else(|| (0..n).find(|&i| parent(i).is_none() && joints[i].kind == BlockKind::Vertebra))
            .unwrap_or(0);
        let mut children = vec![Vec::new(); n];
        for i in 0..n {
            if let Some(p) = parent(i) {
                children[p].push(i);
            }
        }
        let mut order: Vec<usize> = (0..n).filter(|&i| parent(i).is_none()).collect();
        let mut k = 0;
        while k < order.len() {
            let c = children[order[k]].clone();
            order.extend(c);
            k += 1;
        }
        if order.len() != n {
            return Err("cyclic clip rig hierarchy".into());
        }
        let bodies: Vec<Body> = joints
            .iter()
            .enumerate()
            .map(|(i, j)| {
                let mut caps: Vec<String> = j.caps.iter().map(|c| String::from_utf8_lossy(c).trim().to_string()).collect();
                // ADAPTED: a pseudo-foot answers the clips' `foot` channels.
                if j.has(b"psft") && !j.has(b"foot") {
                    caps.push("foot".into());
                }
                Body { capabilities: caps, parent: parent(i), position: j.at, rotation: Quat::IDENTITY }
            })
            .collect();
        let (mut min, mut max) = joints.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), j| (a.min(j.at).min(j.pivot), b.max(j.at).max(j.pivot)));
        if ground_z.is_finite() {
            min.z = min.z.min(ground_z);
        }
        if !min.is_finite() || !max.is_finite() {
            return Err("non-finite clip rig".into());
        }
        max = max.max(min);
        let rig = Rig { bodies, root, min, max };
        rig.validate()?;
        // Limb chains.
        let mut chains = Vec::new();
        let mut chain_start = vec![None; n];
        for &i in &order {
            if joints[i].kind != BlockKind::Limb || parent(i).is_some_and(|p| joints[p].kind == BlockKind::Limb) {
                continue;
            }
            let mut segs = vec![i];
            while let Some(&next) = children[*segs.last().unwrap()].iter().find(|&&c| joints[c].kind == BlockKind::Limb) {
                if segs.contains(&next) {
                    break;
                }
                segs.push(next);
            }
            let last = *segs.last().unwrap();
            let ends: Vec<usize> = children[last].iter().copied().filter(|&c| joints[c].kind != BlockKind::Limb).collect();
            let end = ends.iter().copied().find(|&c| joints[c].has(b"foot") || joints[c].has(b"grsp")).or_else(|| ends.first().copied());
            let mut points = vec![joints[i].pivot];
            points.extend(segs.iter().map(|&s| joints[s].at));
            let foot = end.is_some_and(|e| joints[e].has(b"foot")) || joints[last].has(b"psft");
            chain_start[i] = Some(chains.len());
            chains.push(Chain { segs, points, end, foot });
        }
        // The main mouth and the spine that bends toward it: from the root
        // to the mouth's vertebra; when the root is that vertebra (the
        // root block can be the head end), from the vertebra carrying the
        // limb farthest from the head (else the farthest vertebra).
        let mouth = (0..n).filter(|&i| joints[i].has(b"mout")).min_by(|&a, &b| joints[a].at.y.total_cmp(&joints[b].at.y));
        let vertebra_of = |mut v: Option<usize>| {
            while let Some(x) = v.filter(|&x| joints[x].kind != BlockKind::Vertebra) {
                v = parent(x);
            }
            v
        };
        let mut spine = Vec::new();
        if let Some(head) = mouth.and_then(|m| vertebra_of(parent(m))) {
            let far = |list: &mut dyn Iterator<Item = usize>| list.max_by(|&a, &b| joints[a].pivot.distance(joints[head].pivot).total_cmp(&joints[b].pivot.distance(joints[head].pivot)));
            let anchor = if head != root {
                Some(root)
            } else {
                far(&mut chains.iter().filter_map(|c| vertebra_of(parent(c.segs[0]))))
                    .or_else(|| far(&mut (0..n).filter(|&i| joints[i].kind == BlockKind::Vertebra)))
            };
            if let Some(anchor) = anchor.filter(|&x| x != head) {
                spine = tree_path(anchor, head, &parent).into_iter().map(|x| (x, 0.0)).collect();
            }
        }
        let mut arc = 0.0;
        let mut lengths = vec![0.0];
        for w in spine.windows(2) {
            arc += joints[w[0].0].pivot.distance(joints[w[1].0].pivot);
            lengths.push(arc);
        }
        for (s, l) in spine.iter_mut().zip(&lengths) {
            s.1 = if arc > 1e-5 { l / arc } else { 0.0 };
        }
        let feet: Vec<f32> = chains
            .iter()
            .filter(|c| c.foot)
            .filter_map(|c| rig.limb_length(c.end.unwrap_or(*c.segs.last().unwrap())).ok())
            .filter(|l| *l > 1e-3)
            .collect();
        let leg_length = if feet.is_empty() { ((max.z - min.z) * 0.5).max(0.1) } else { feet.iter().sum::<f32>() / feet.len() as f32 };
        Ok(Self { rests: vec![Quat::IDENTITY; n], joints, rig, order, root, spine, spine_reach: 0.6 * arc, mouth, chains, chain_start, ground_z, leg_length })
    }

    pub fn len(&self) -> usize {
        self.joints.len()
    }

    /// Set the blocks' rest orientations (creature frame); entries that are
    /// missing or invalid stay identity.
    pub fn set_rests(&mut self, rests: &[Quat]) {
        for (r, q) in self.rests.iter_mut().zip(rests) {
            if q.is_finite() && q.length_squared() > 1e-8 {
                *r = q.normalize();
            }
        }
    }

    /// The default external look target (creature frame): straight ahead on
    /// the ground, FUN_00C0D3A0's (0, -10 x scale, 0) local point.
    pub fn default_look(&self) -> Vec3 {
        Vec3::new(0.0, -crate::creature_look::DEFAULT_AHEAD, self.ground_z)
    }

    /// The Creature Creator's default look point (FUN_0059B390).
    pub fn editor_look(&self) -> Vec3 {
        Vec3::new(0.0, -crate::creature_look::EDITOR_AHEAD, self.ground_z + crate::creature_look::EDITOR_UP)
    }

    /// The secondary frame point of a channel (FUN_009B05F0): the external
    /// target when its secondary context selects nothing (flags 2), else the
    /// selected block's rest point (ADAPTED: retail its posed point).
    fn secondary_point(&self, ch: &crate::animation::Channel, look: Vec3) -> Vec3 {
        let flags = ch.secondary.flags;
        if flags & 0x10_0003 == 2 {
            return look;
        }
        let cap = ch.secondary.capability.trim_end_matches('\0').trim();
        self.rig.select(flags, cap).ok().and_then(|s| s.first().copied()).map_or(look, |i| self.joints[i].at)
    }

    /// FUN_009B0B80: a position component's scale along `axis` (bounds 0x40,
    /// limb length 0x80, else 1).
    fn measure(&self, j: usize, flags: u32, axis: Vec3) -> f32 {
        match flags & 0xC0 {
            0x40 => axis.dot(self.rig.max - self.rig.min) + 1e-8,
            0x80 => self.rig.limb_length(j).ok().filter(|l| *l > 1e-6).unwrap_or(1.0),
            _ => 1.0,
        }
    }

    /// The ground speed (creature units per second) a gait clip shows on
    /// this rig: its planted-foot speed in the foot channel's scale.
    pub fn natural_speed(&self, clip: &Clip) -> Option<f32> {
        let (s, scale) = clip.stance_speed?;
        Some(match scale {
            0x80 => s * self.leg_length,
            0x40 => s * (self.rig.max.y - self.rig.min.y),
            _ => s,
        })
    }

    /// Bind a clip: the blocks of every channel (selection once per clip).
    pub fn bind(&self, clip: Arc<Clip>) -> Bound {
        let mut channels = Vec::new();
        for (ci, ch) in clip.anim.channels.iter().enumerate() {
            let cap = ch.primary.capability.trim_end_matches('\0').trim();
            let Ok(selected) = self.rig.select(ch.primary.flags, cap) else { continue };
            if selected.is_empty() {
                continue;
            }
            let mut joints: Vec<(usize, f32)> = selected.iter().map(|&j| (j, 0.0)).collect();
            // ADAPTED: several feet of one side alternate rank by rank.
            if cap == "foot" && joints.len() > 1 {
                if let Some(cycle) = clip.foot_cycle {
                    joints.sort_by(|a, b| self.joints[a.0].at.y.total_cmp(&self.joints[b.0].at.y));
                    for (r, j) in joints.iter_mut().enumerate() {
                        j.1 = if r % 2 == 1 { cycle * 0.5 } else { 0.0 };
                    }
                }
            }
            channels.push(BoundChannel { channel: ci, joints, movement: ch.movement_flags });
        }
        Bound { clip, channels }
    }

    /// Skinning matrices (rest -> posed, creature space) for a layer mix
    /// `(bound clip, weight, frame)`.
    pub fn pose(&self, layers: &[(&Bound, f32, f32)]) -> Vec<Affine3A> {
        self.pose_with(layers, true)
    }

    /// `pose`, optionally without the root channel (a cinematic actor whose
    /// root offset the cinematic host applies to the whole actor).
    pub fn pose_with(&self, layers: &[(&Bound, f32, f32)], root: bool) -> Vec<Affine3A> {
        self.pose_full(layers, root, None, None)
    }

    /// `pose_with` with an external look target (creature frame; `None` =
    /// `default_look`) for the secondary frames, and optionally the deform
    /// tracks of every block (value and weight blended over the layers).
    pub fn pose_full(&self, layers: &[(&Bound, f32, f32)], root: bool, look: Option<Vec3>, deforms: Option<&mut BlockDeforms>) -> Vec<Affine3A> {
        self.pose_layered(layers, &[], root, look, deforms)
    }

    /// `pose_full` with overlay layers `(bound clip, weight, frame)` laid over the
    /// stack: an overlay only covers the joints its channels animate (the
    /// bundle grasper's `platter_grasper_overlay` 0x05F08F08 lays the carry pose
    /// on the arms of a walking citizen, T9K-1); on those joints the goal of the
    /// layers below blends toward the overlay's by its weight, every other
    /// joint keeps the stack's pose.
    pub fn pose_layered(
        &self,
        layers: &[(&Bound, f32, f32)],
        overlays: &[(&Bound, f32, f32)],
        root: bool,
        look: Option<Vec3>,
        mut deforms: Option<&mut BlockDeforms>,
    ) -> Vec<Affine3A> {
        let n = self.joints.len();
        let look = look.filter(|l| l.is_finite()).unwrap_or_else(|| self.default_look());
        let mut pos = vec![(Vec3::ZERO, 0.0f32); n];
        let mut rot = vec![(Vec4::ZERO, 0.0f32); n];
        // Deform sums per block (`AddDeform` 0x99C3B0 / 0x99C450): (id, value
        // x weight or cos x weight, weight, sin x weight, cyclic).
        let mut sums: Vec<Vec<(u32, f32, f32, f32, bool)>> = if deforms.is_some() { vec![Vec::new(); n] } else { Vec::new() };
        for (overlay, &(bound, alpha, frame)) in layers.iter().map(|l| (false, l)).chain(overlays.iter().map(|l| (true, l))) {
            if alpha <= 1e-4 {
                continue;
            }
            let anim = &bound.clip.anim;
            let period = (anim.length_frames - 1.0).max(1.0);
            for bc in &bound.channels {
                let Some(ch) = anim.channels.get(bc.channel) else { continue };
                let secondary = bc.movement & 1 != 0;
                let point = secondary.then(|| self.secondary_point(ch, look));
                let mut cached: Option<(f32, crate::animation::Sample)> = None;
                for &(j, offset) in &bc.joints {
                    if !root && j == self.root {
                        continue;
                    }
                    let f = if offset != 0.0 { (frame + offset).rem_euclid(period) } else { frame };
                    if cached.as_ref().is_none_or(|(o, _)| *o != offset) {
                        let Ok(s) = sample(ch, f, anim.length_frames, bound.clip.cyclic, RotationCurve::Spherical) else { break };
                        cached = Some((offset, s));
                    }
                    let s = &cached.as_ref().unwrap().1;
                    // The block's position in this channel (rest unless keyed):
                    // the look frame starts there.
                    let mut at = self.joints[j].at;
                    if let Some((p, w, flags)) = s.position {
                        let w = (w * alpha).clamp(0.0, 1.0);
                        if w > 1e-4 {
                            let key = Vec3::from_array(p);
                            let goal = match point {
                                Some(sp) => Some(crate::creature_look::secondary_position(self.joints[j].at, sp, key, bc.movement, &|axis| self.measure(j, flags, axis), self.ground_z)),
                                None => specialize_position(&self.rig, j, key, flags, bc.movement & 0x10, self.ground_z, false).ok(),
                            };
                            if let Some(g) = goal.filter(|g| g.is_finite()) {
                                if overlay {
                                    let (s0, w0) = pos[j];
                                    let rest = self.joints[j].at;
                                    let base = if w0 >= 1.0 { s0 / w0 } else { rest * (1.0 - w0) + s0 };
                                    pos[j] = (base * (1.0 - w) + g * w, 1.0);
                                } else {
                                    pos[j].0 += g * w;
                                    pos[j].1 += w;
                                }
                                at = g;
                            }
                        }
                    }
                    if let Some((q, w, rflags)) = s.rotation {
                        let w = (w * alpha).clamp(0.0, 1.0);
                        let key = Vec4::from_array(q);
                        let goal = if key.length_squared() > 1e-8 && w > 1e-4 {
                            let key = Quat::from_vec4(key.normalize());
                            match point {
                                None => Some(key),
                                Some(sp) => match bc.movement & 6 {
                                    0 => Some(key),
                                    // Aimed at the secondary point (relative to
                                    // that look frame with flag 0x40); goals are
                                    // turns of the block from its rest.
                                    2 => {
                                        let rest = self.rests[j];
                                        let aim = crate::creature_look::look_frame(rest, sp, at);
                                        let abs = if rflags & 0x60 == 0x40 { aim * key } else { aim };
                                        Some((abs * rest.inverse()).normalize())
                                    }
                                    _ => None,
                                },
                            }
                        } else {
                            None
                        };
                        if let Some(q) = goal {
                            let mut q = Vec4::from(q);
                            if q.w < 0.0 {
                                q = -q;
                            }
                            if overlay {
                                let (s0, w0) = rot[j];
                                let mut base = if w0 >= 1.0 {
                                    s0 / w0
                                } else if w0 > 1e-4 {
                                    s0 + Vec4::from(Quat::IDENTITY) * (1.0 - w0)
                                } else {
                                    Vec4::from(Quat::IDENTITY)
                                };
                                if base.dot(q) < 0.0 {
                                    base = -base;
                                }
                                rot[j] = (base * (1.0 - w) + q * w, 1.0);
                            } else {
                                rot[j].0 += q * w;
                                rot[j].1 += w;
                            }
                        }
                    }
                    if let Some(sum) = sums.get_mut(j) {
                        for &(id, value, weight) in &s.deforms {
                            // Records of weight 0 stay (they zero the slot);
                            // component flag 0x80 is skipped by the model
                            // update, flag 0x40 blends as an angle (0x99DE00).
                            let w = (weight * alpha).max(0.0);
                            let flags = ch.components.iter().find(|c| c.id == id && c.flags & 0xF == 3).map_or(0, |c| c.flags);
                            if id == 0 || !w.is_finite() || !value.is_finite() || flags & 0x80 != 0 {
                                continue;
                            }
                            let cyclic = flags & 0x40 != 0;
                            let (x, y) = if cyclic {
                                let a = value * std::f32::consts::TAU;
                                (a.cos() * w, a.sin() * w)
                            } else {
                                (value * w, 0.0)
                            };
                            match sum.iter_mut().find(|t| t.0 == id) {
                                Some(t) => {
                                    t.1 += x;
                                    t.2 += w;
                                    t.3 += y;
                                    t.4 |= cyclic;
                                }
                                None => sum.push((id, x, w, y, cyclic)),
                            }
                        }
                    }
                }
            }
        }
        if let Some(out) = deforms.as_deref_mut() {
            // FUN_009C5BD0's phase of each record, with its summed weight.
            *out = sums
                .into_iter()
                .map(|v| {
                    v.into_iter()
                        .map(|(id, x, w, y, cyclic)| (id, if cyclic { crate::creature_morph::cyclic_phase(x, y) } else { crate::creature_morph::phase_of(x, w) }, w))
                        .collect()
                })
                .collect();
        }
        let goal_pos = |j: usize, default: Vec3| -> Option<Vec3> {
            let (s, w) = pos[j];
            (w > 1e-4).then(|| if w >= 1.0 { s / w } else { default * (1.0 - w) + s })
        };
        let goal_rot = |j: usize, default: Quat| -> Option<Quat> {
            let (s, w) = rot[j];
            if w <= 1e-4 {
                return None;
            }
            let mut d = Vec4::from(default);
            if d.dot(s) < 0.0 {
                d = -d;
            }
            let q = if w >= 1.0 { s / w } else { s + d * (1.0 - w) };
            (q.length_squared() > 1e-8).then(|| Quat::from_vec4(q.normalize()))
        };
        let r = self.root;
        let rest_root = self.joints[r].at;
        let target_root = goal_pos(r, rest_root).unwrap_or(rest_root);
        let q_root = goal_rot(r, Quat::IDENTITY).unwrap_or(Quat::IDENTITY);
        let body = Affine3A::from_translation(target_root) * Affine3A::from_quat(q_root) * Affine3A::from_translation(-rest_root);
        let mut m = vec![body; n];
        let mut done = vec![false; n];
        // The spine bends so the main mouth meets its goal.
        if let (Some(mouth), true) = (self.mouth, self.spine.len() >= 2) {
            let rest = self.joints[mouth].at;
            let default = body.transform_point3(rest);
            let target = goal_pos(mouth, default).unwrap_or(default);
            let q_m = goal_rot(mouth, q_root).unwrap_or(q_root);
            let q_c = (q_m * q_root.inverse()).normalize();
            let head = self.spine.last().unwrap().0;
            let hp = body.transform_point3(self.joints[head].pivot);
            let c = (target - (hp + q_c * (default - hp))).clamp_length_max(self.spine_reach);
            for &(v, s) in &self.spine {
                let piv = body.transform_point3(self.joints[v].pivot);
                let qs = Quat::IDENTITY.slerp(q_c, s);
                m[v] = Affine3A::from_translation(piv + c * s) * Affine3A::from_quat(qs) * Affine3A::from_translation(-piv) * body;
                done[v] = true;
            }
        }
        for &j in &self.order {
            if done[j] {
                continue;
            }
            let parent_m = self.joints[j].parent.filter(|&p| p < n && p != j).map_or(body, |p| m[p]);
            if let Some(c) = self.chain_start[j] {
                self.solve(&self.chains[c], parent_m, &goal_pos, &goal_rot, &mut m, &mut done);
                continue;
            }
            m[j] = match self.joints[j].kind {
                BlockKind::Vertebra if j == r => body,
                BlockKind::Vertebra | BlockKind::Limb => parent_m,
                // A part rides its block, turned by its own rotation goal
                // (the main mouth already turned the head).
                _ if Some(j) == self.mouth && self.spine.len() >= 2 => parent_m,
                _ => match goal_rot(j, rotation_of(&parent_m)) {
                    Some(q) => {
                        let pivot = self.joints[j].pivot;
                        Affine3A::from_translation(parent_m.transform_point3(pivot)) * Affine3A::from_quat(q) * Affine3A::from_translation(-pivot)
                    }
                    None => parent_m,
                },
            };
            done[j] = true;
        }
        m
    }

    fn solve(
        &self,
        c: &Chain,
        parent_m: Affine3A,
        goal_pos: &dyn Fn(usize, Vec3) -> Option<Vec3>,
        goal_rot: &dyn Fn(usize, Quat) -> Option<Quat>,
        m: &mut [Affine3A],
        done: &mut [bool],
    ) {
        let rv = rotation_of(&parent_m);
        let last = *c.segs.last().unwrap();
        let tip = *c.points.last().unwrap();
        let posed: Vec<Vec3> = c.points.iter().map(|p| parent_m.transform_point3(*p)).collect();
        // Feet stay planted (creature frame), other limbs ride the body.
        let default = if c.foot { tip } else { *posed.last().unwrap() };
        let goal = c
            .end
            .and_then(|e| goal_pos(e, default + (self.joints[e].at - tip)).map(|g| g - (self.joints[e].at - tip)))
            .or_else(|| goal_pos(last, default));
        let degenerate = c.points.windows(2).any(|w| w[0].distance(w[1]) < 1e-4);
        let mut new = posed.clone();
        let target = if c.foot { goal.or(Some(default)) } else { goal };
        if let (Some(target), false) = (target, degenerate) {
            match c.segs.len() {
                1 => {
                    let len = posed[0].distance(posed[1]);
                    let d = (target - posed[0]).try_normalize().unwrap_or_else(|| (posed[1] - posed[0]) / len);
                    new[1] = posed[0] + d * len;
                }
                2 => {
                    let (knee, foot) = crate::creature_blocks::two_bone(posed[0], posed[1], posed[2], target);
                    new[1] = knee;
                    new[2] = foot;
                }
                _ => crate::creature_blocks::ccd(&mut new, target, 10),
            }
        }
        let mut last_rot = rv;
        for (k, &s) in c.segs.iter().enumerate() {
            let r = arc(rv * (c.points[k + 1] - c.points[k]), new[k + 1] - new[k]) * rv;
            m[s] = Affine3A::from_translation(new[k]) * Affine3A::from_quat(r) * Affine3A::from_translation(-c.points[k]);
            done[s] = true;
            last_rot = r;
        }
        if let Some(e) = c.end {
            let r = goal_rot(e, last_rot).unwrap_or(last_rot);
            m[e] = Affine3A::from_translation(*new.last().unwrap()) * Affine3A::from_quat(r) * Affine3A::from_translation(-tip);
            done[e] = true;
        }
    }
}

/// The path from `a` to `b` in a parent tree (both included).
fn tree_path(a: usize, b: usize, parent: &dyn Fn(usize) -> Option<usize>) -> Vec<usize> {
    let up = |mut x: usize| {
        let mut out = vec![x];
        while let Some(p) = parent(x) {
            if out.contains(&p) {
                break;
            }
            out.push(p);
            x = p;
        }
        out
    };
    let (ua, ub) = (up(a), up(b));
    let Some(meet) = ua.iter().position(|x| ub.contains(x)) else { return Vec::new() };
    let lca = ua[meet];
    let mut path: Vec<usize> = ua[..=meet].to_vec();
    let down = ub.iter().position(|&x| x == lca).unwrap();
    path.extend(ub[..down].iter().rev());
    path
}

/// The rotation from `a` to `b` (identity for degenerate vectors).
fn arc(a: Vec3, b: Vec3) -> Quat {
    match (a.try_normalize(), b.try_normalize()) {
        (Some(a), Some(b)) => Quat::from_rotation_arc(a, b),
        _ => Quat::IDENTITY,
    }
}

/// The rotation of a rigid affine map.
pub fn rotation_of(m: &Affine3A) -> Quat {
    Quat::from_mat3a(&m.matrix3).normalize()
}

/// A creature-space map in a Bevy frame of `unit` frame units per creature
/// unit: the basis change (-x, z, y) is its own inverse and a rotation.
pub fn to_frame(m: &Affine3A, unit: f32) -> Affine3A {
    let c = Mat3A::from_cols(Vec3A::new(-1.0, 0.0, 0.0), Vec3A::new(0.0, 0.0, 1.0), Vec3A::new(0.0, 1.0, 0.0));
    Affine3A { matrix3: c * m.matrix3 * c, translation: c * m.translation * unit }
}

/// Spore creature space from a Bevy frame point (the inverse of
/// `creature_editor::spore_to_bevy`, which is an involution).
pub fn bevy_to_spore(v: Vec3) -> Vec3 {
    Vec3::new(-v.x, v.z, v.y)
}

/// A clip bound to one rig.
#[derive(Debug)]
pub struct Bound {
    pub clip: Arc<Clip>,
    channels: Vec<BoundChannel>,
}

#[derive(Debug)]
struct BoundChannel {
    channel: usize,
    /// Selected blocks and their frame offsets.
    joints: Vec<(usize, f32)>,
    movement: u32,
}

impl Bound {
    /// Channels that drive at least one block.
    pub fn driven_channels(&self) -> usize {
        self.channels.len()
    }
}

// ---------------------------------------------------------------------------
// Player (layer stack)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Layer {
    pub bound: Arc<Bound>,
    pub time: f32,
    pub rate: f32,
    pub looped: bool,
    pub weight: f32,
    fade: f32,
}

impl Layer {
    pub fn frame(&self) -> f32 {
        self.bound.clip.frame_at(self.time, self.looped)
    }
}

/// Cross-fading clip layers, newest last.
#[derive(Debug, Clone, Default)]
pub struct ClipPlayer {
    pub layers: Vec<Layer>,
}

impl ClipPlayer {
    pub fn current(&self) -> Option<&Layer> {
        self.layers.last()
    }
    pub fn current_id(&self) -> Option<u32> {
        self.layers.last().map(|l| l.bound.clip.id)
    }
    /// Play `bound` unless it already plays on top.
    pub fn play(&mut self, bound: Arc<Bound>, looped: bool) {
        if self.layers.last().is_some_and(|l| l.bound.clip.id == bound.clip.id && l.looped == looped) {
            return;
        }
        self.restart(bound, looped);
    }
    /// Start `bound` from its beginning, fading in over its blend time.
    pub fn restart(&mut self, bound: Arc<Bound>, looped: bool) {
        let fade = bound.clip.blend_in.max(0.05);
        let weight = if self.layers.is_empty() { 1.0 } else { 0.0 };
        self.layers.push(Layer { bound, time: 0.0, rate: 1.0, looped, weight, fade });
        if self.layers.len() > 4 {
            self.layers.remove(0);
        }
    }
    /// Put the top clip at `t` seconds (a cinematic job's clock).
    pub fn set_time(&mut self, t: f32) {
        if let Some(l) = self.layers.last_mut() {
            l.time = t.max(0.0);
        }
    }
    pub fn set_rate(&mut self, rate: f32) {
        if let Some(l) = self.layers.last_mut() {
            l.rate = rate;
        }
    }
    pub fn advance(&mut self, dt: f32) {
        for l in &mut self.layers {
            l.time += dt.max(0.0) * l.rate;
        }
        if let Some(top) = self.layers.last_mut() {
            top.weight = (top.weight + dt.max(0.0) / top.fade).min(1.0);
            if top.weight >= 1.0 {
                let keep = self.layers.len() - 1;
                self.layers.drain(..keep);
            }
        }
    }
    /// The top clip plays once and has reached its end.
    pub fn finished(&self) -> bool {
        self.layers.last().is_some_and(|l| !l.looped && l.time >= l.bound.clip.seconds)
    }
    /// Layer weights for `ClipRig::pose` (stack blend: each layer covers what
    /// is below it by its weight).
    pub fn mix(&self) -> Vec<(&Bound, f32, f32)> {
        let mut out = Vec::with_capacity(self.layers.len());
        let mut left = 1.0;
        for l in self.layers.iter().rev() {
            let a = l.weight * left;
            left *= 1.0 - l.weight;
            out.push((&*l.bound, a, l.frame()));
        }
        out
    }
    /// Re-bind every layer to a new rig (after an edit).
    pub fn rebind(&mut self, rig: &ClipRig) {
        for l in &mut self.layers {
            l.bound = Arc::new(rig.bind(l.bound.clip.clone()));
        }
    }
}

/// Playback rate of a gait clip for a ground speed (creature units per
/// second) on `rig`.
pub fn gait_rate(clip: &Clip, speed: f32, rig: &ClipRig) -> f32 {
    match rig.natural_speed(clip) {
        Some(s) if s > 1e-3 => (speed / s).clamp(0.35, 2.5),
        _ => 1.0,
    }
}

// ---------------------------------------------------------------------------
// Action ids (ORIGINAL TLSA groups; the ability -> group mapping is by name)
// ---------------------------------------------------------------------------

/// `csa_idle_gen_editor` (the Test Drive idle).
pub const IDLE: u32 = 0x0433_0667;
/// `BP1_cycle_walk` / `BP1_cycle_run` (the Test Drive Walk / Run cycles).
pub const WALK: u32 = 0x06F8_A609;
pub const RUN: u32 = 0x06F8_A61B;
/// `mot_fruit_eat_mouth`.
pub const EAT: u32 = 0x0327_6473;

/// The clip of a Creature-stage ability (`creature_actions::SOCIAL` /
/// `COMBAT` slot order) at `rank` 1..5: `soc_sing/dance/charm/pose_lvl0N`,
/// `com_bite_lvl0N`, `com_charge_rush_hit`, `com_strike_lvl0N`, `com_spit`.
/// VERIFIED against the installed ability props (`CreatureAbilities!bite1..5`
/// etc., `creatureAbilityAnimationIDs`): the same ids. The game reads them
/// from the props (`creature_actions::ability_clip`); this table is the
/// fallback when the catalog is not loaded.
pub fn ability_action(social: bool, slot: usize, rank: u8) -> Option<u32> {
    let r = rank.clamp(1, 5) as u32 - 1;
    Some(match (social, slot) {
        (true, 0) => 0x042A_1CAF + r,
        (true, 1) => 0x042A_1CA0 + r,
        (true, 2) => 0x042A_1CA5 + r,
        (true, 3) => 0x042A_1CAA + r,
        (false, 0) => 0x044D_DEA6 + r,
        (false, 1) => 0x04A3_B511,
        (false, 2) => 0x044D_DD75 + r,
        (false, 3) => 0x044D_DEAF,
        _ => return None,
    })
}

/// Follow a cinematic job: its action's clip at its time (a looping clip loops).
pub fn follow_script(player: &mut ClipPlayer, originals: &crate::Originals, rig: &ClipRig, id: u32, t: f32) {
    if player.current_id() != Some(id) {
        if let Some(c) = clip(originals, id) {
            // A cut: the script's clock owns the clip.
            let looped = c.cyclic;
            *player = ClipPlayer::default();
            player.restart(Arc::new(rig.bind(c)), looped);
        }
    }
    if player.current_id() == Some(id) {
        player.set_time(t);
        player.advance(0.0);
    }
}

/// Switch: `OPENSPORE_CLIPS=0` turns the clip player off (`player`: only the
/// player's custom skin off, `native`: only the installed models off).
pub fn enabled(part: &str) -> bool {
    match std::env::var("OPENSPORE_CLIPS").as_deref() {
        Ok("0") => false,
        Ok(v) => v != part,
        Err(_) => true,
    }
}

/// Per-app clip-player state shared with other systems.
#[derive(bevy::prelude::Resource, Default)]
pub struct ClipDrive {
    /// The player's custom skin is posed by clips (`drive_player`).
    pub body: bool,
    /// The installed starter's model is (`locomotion_preview`).
    pub native: bool,
    /// Clip rigs of skins by their inverse bind poses (cinematic copies of a
    /// body are found by them), with frame units per creature unit.
    skins: HashMap<bevy::asset::AssetId<bevy::mesh::skinning::SkinnedMeshInverseBindposes>, (Arc<ClipRig>, f32)>,
    /// Morph joints (part rig bones) of skins that have them.
    morphs: HashMap<bevy::asset::AssetId<bevy::mesh::skinning::SkinnedMeshInverseBindposes>, Arc<crate::creature_morph::MorphSet>>,
}

impl ClipDrive {
    pub fn register_morph(&mut self, id: bevy::asset::AssetId<bevy::mesh::skinning::SkinnedMeshInverseBindposes>, morph: Arc<crate::creature_morph::MorphSet>) {
        self.morphs.insert(id, morph);
    }

    /// The player's body is posed by clips now: whole-body procedural motion
    /// (bob, lean, Test Drive poses) stands aside.
    pub fn drives_body(&self) -> bool {
        self.body || self.native
    }

    pub fn register_skin(&mut self, id: bevy::asset::AssetId<bevy::mesh::skinning::SkinnedMeshInverseBindposes>, rig: Arc<ClipRig>, unit: f32) {
        self.skins.insert(id, (rig, unit));
    }

    /// The morph joints (part rig bones) of a registered skin by its inverse
    /// bind poses (copies of an installed body pose them too).
    pub fn morph_of_skin(&self, id: bevy::asset::AssetId<bevy::mesh::skinning::SkinnedMeshInverseBindposes>) -> Option<Arc<crate::creature_morph::MorphSet>> {
        self.morphs.get(&id).cloned()
    }

    /// The clip rig and frame unit of a registered skin by its inverse bind
    /// poses: copies of the player's body (the nest mates) find theirs this way.
    pub fn rig_of_skin(&self, id: bevy::asset::AssetId<bevy::mesh::skinning::SkinnedMeshInverseBindposes>) -> Option<(Arc<ClipRig>, f32)> {
        self.skins.get(&id).cloned()
    }
}

/// `ClipDrive::drives_body` for systems that may run without the resource.
pub fn drives_body(drive: Option<&ClipDrive>) -> bool {
    drive.is_some_and(|d| d.drives_body())
}

// ---------------------------------------------------------------------------
// The player's skinned body (Test Drive and the Creature stage)
// ---------------------------------------------------------------------------

use bevy::{
    camera::visibility::DynamicSkinnedMeshBounds,
    mesh::{
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
        VertexAttributeValues,
    },
    prelude::{
        Assets, ChildOf, Commands, Component, Entity, Handle, Has, Mat4, Mesh, Mesh3d, Query, Res, ResMut, Resource, State, Time, Transform, With, Without,
    },
};

use crate::{
    creature::Accessory,
    creature_editor::{CreatureEditor, Mode},
    creature_editor_data::{PlayAnimKind, PLAY_ANIM_BUTTONS},
    game::{Hunger, PartInstance, Player, RunProgress},
    stages::GameStage,
    Originals,
};

/// A skinning joint of the player's body (one per skin record).
#[derive(Component)]
pub struct DriveJoint;

/// The player's clip-driven body (per app).
#[derive(Resource, Default)]
pub struct DriveState {
    active: Option<Avatar>,
    /// (our skinned copy, the body's own mesh) to put back when the copy
    /// returns on the root (`player_appearance` saved it while the installed
    /// starter's model was shown).
    pending: Option<(Handle<Mesh>, Handle<Mesh>)>,
}

impl DriveState {
    /// The player's body and its clip layers (`anim_audio` plays their events).
    pub fn avatar_clips(&self) -> Option<(Entity, &ClipPlayer)> {
        self.active.as_ref().map(|a| (a.player, &a.clips))
    }

    /// This frame's deform tracks of the avatar's parts (part index ->
    /// tracks), while clips pose the avatar (`creature_morph`).
    pub fn part_tracks(&self) -> Option<&HashMap<usize, Vec<crate::creature_morph::Track>>> {
        self.active.as_ref().map(|a| &a.part_tracks)
    }
}

struct Avatar {
    player: Entity,
    editor: bool,
    joints: Vec<Entity>,
    original_mesh: Handle<Mesh>,
    skinned_mesh: Handle<Mesh>,
    inverse: bevy::asset::AssetId<SkinnedMeshInverseBindposes>,
    rig: Arc<ClipRig>,
    unit: f32,
    /// Part index -> (block, rest transform in the player frame).
    parts: HashMap<usize, (usize, Transform)>,
    body: crate::body::BodyProfile,
    run_parts: Vec<PartInstance>,
    clips: ClipPlayer,
    last_pos: Vec3,
    speed: f32,
    actions: AvatarActions,
    last_elapsed: f32,
    /// The smoothed external look target (creature frame).
    look: Option<Vec3>,
    part_tracks: HashMap<usize, Vec<crate::creature_morph::Track>>,
    /// A requested clip sequence (PoseClip): its serial and the action.
    pose: (u32, Option<u32>),
    /// Test Drive's idle after a walk (`StopWalk`).
    stop: StopWalk,
}

/// Clip rig joints of the player's blocks (`creature_blocks::blocks_of`:
/// skin records, then the mounted parts).
fn joints_of_blocks(blocks: &[crate::creature_blocks::Block], built: &crate::creature_skin_view::Built) -> Vec<Joint> {
    blocks
        .iter()
        .map(|bl| match bl.record.and_then(|r| built.records.get(r)) {
            Some(r) => Joint {
                kind: r.kind,
                parent: bl.parent,
                pivot: r.position,
                at: if r.kind == BlockKind::Limb { r.segment_end() } else { r.position },
                caps: bl.caps.clone(),
            },
            None => {
                let p = bevy_to_spore(bl.position) / built.unit.max(1e-4);
                Joint { kind: BlockKind::Standard, parent: bl.parent, pivot: p, at: p, caps: bl.caps.clone() }
            }
        })
        .collect()
}

/// A block's rest orientation from its `mTotalOrientation` rows (the local
/// axes in the creature frame); a mirrored (left-handed) frame flips its X
/// axis first (ADAPTED: a rotation is needed for the look frame).
pub fn rest_rotation(rows: &[Vec3; 3]) -> Quat {
    let (x, y, z) = (rows[0].normalize_or_zero(), rows[1].normalize_or_zero(), rows[2].normalize_or_zero());
    let x = if x.cross(y).dot(z) < 0.0 { -x } else { x };
    let q = Quat::from_mat3(&bevy::math::Mat3::from_cols(x, y, z));
    if q.is_finite() && q.length_squared() > 1e-8 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// A Bevy-frame rotation in the Spore creature frame (the basis change
/// (-x, z, y) of `to_frame`).
pub fn spore_rotation(q: Quat) -> Quat {
    let c = bevy::math::Mat3::from_cols(Vec3::NEG_X, Vec3::Z, Vec3::Y);
    let m = c * bevy::math::Mat3::from_quat(q) * c;
    let r = Quat::from_mat3(&m);
    if r.is_finite() && r.length_squared() > 1e-8 {
        r.normalize()
    } else {
        Quat::IDENTITY
    }
}

#[allow(clippy::too_many_arguments)]
fn build_avatar(
    originals: &Originals,
    run: &RunProgress,
    entity: Entity,
    editor: bool,
    mesh3d: &mut Mesh3d,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
) -> Option<Avatar> {
    let (blocks, b) = crate::creature_blocks::creature_blocks(originals, run)?;
    let ground = b.mesh.positions.iter().map(|p| p.z).fold(f32::MAX, f32::min);
    let rig = match ClipRig::new(joints_of_blocks(&blocks, &b), ground) {
        Ok(mut r) => {
            let rests: Vec<Quat> = blocks
                .iter()
                .map(|bl| match (bl.record.and_then(|i| b.records.get(i)), bl.part.and_then(|i| run.parts.get(i))) {
                    (Some(rec), _) => rest_rotation(&rec.orientation),
                    (None, Some(p)) => spore_rotation(p.rotation()),
                    _ => Quat::IDENTITY,
                })
                .collect();
            r.set_rests(&rests);
            Arc::new(r)
        }
        Err(e) => {
            bevy::log::warn!("clip rig: {e}");
            return None;
        }
    };
    let mut mesh = crate::creature_skin_view::render_mesh(&b);
    let src = &b.unwrapped.source;
    let joints_attr: Vec<[u16; 4]> = src.iter().map(|&s| b.mesh.bones[s as usize].map(|x| x as u16)).collect();
    let weights: Vec<[f32; 4]> = src.iter().map(|&s| b.mesh.weights[s as usize]).collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joints_attr));
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
    mesh.generate_skinned_mesh_bounds().ok()?;
    // Joints are posed in the player frame: every bind pose is the identity.
    let inverse = bindposes.add(vec![Mat4::IDENTITY; b.records.len()]);
    let inverse_id = inverse.id();
    let joints: Vec<Entity> = (0..b.records.len())
        .map(|_| {
            let mut e = commands.spawn((DriveJoint, Transform::IDENTITY, ChildOf(entity)));
            if editor {
                e.insert(crate::creature_editor::CreatureEditorOwned);
            } else {
                e.insert(crate::stages::StageOwned);
            }
            e.id()
        })
        .collect();
    let mut parts: HashMap<usize, (usize, Transform)> = blocks
        .iter()
        .enumerate()
        .filter_map(|(i, bl)| {
            let index = bl.part?;
            let p = run.parts.get(index)?;
            let rest = Transform::from_translation(crate::creature_limbs::root_of(originals, &run.body, p)).with_rotation(p.rotation());
            Some((index, (i, rest)))
        })
        .collect();
    // Rigid limb segment parts (`PartEdits::segment` k rides section k - 1,
    // placed at its start joint) follow the joint of their section: the
    // non-skin limb record that starts where the part sits.
    let rigid_sections: Vec<(usize, Vec3)> = blocks
        .iter()
        .enumerate()
        .filter_map(|(i, bl)| {
            let r = b.records.get(bl.record?)?;
            (r.kind == BlockKind::Limb && !r.uses_skin()).then(|| (i, crate::creature_editor::spore_to_bevy(r.position) * b.unit))
        })
        .collect();
    for (index, p) in run.parts.iter().enumerate().filter(|(_, p)| p.edits.segment != 0 && p.edits.on_limb != 0) {
        let at = crate::creature_limbs::root_of(originals, &run.body, p);
        let nearest = rigid_sections.iter().min_by(|a, c| a.1.distance_squared(at).total_cmp(&c.1.distance_squared(at)));
        if let Some(&(j, _)) = nearest.filter(|(_, start)| start.distance(at) < 0.05 * b.unit.max(1e-3) + 1e-3) {
            parts.insert(index, (j, Transform::from_translation(at).with_rotation(p.rotation())));
        }
    }
    if std::env::var_os("OPENSPORE_CLIP_DEBUG").is_some() {
        for (i, (j, bl)) in rig.joints.iter().zip(&blocks).enumerate() {
            let caps: Vec<String> = j.caps.iter().map(|c| String::from_utf8_lossy(c).to_string()).collect();
            bevy::log::info!("clip block {i}: {:?} parent {:?} part {:?} pivot {:.2} at {:.2} {caps:?}", j.kind, j.parent, bl.part, j.pivot, j.at);
        }
        for p in &run.parts {
            bevy::log::info!("part {:08X} limb {:?} on_limb {:08X}", p.instance, p.edits.limb.as_ref().map(|l| l.id), p.edits.on_limb);
        }
    }
    let skinned_mesh = meshes.add(mesh);
    let original_mesh = std::mem::replace(&mut mesh3d.0, skinned_mesh.clone());
    commands.entity(entity).insert((SkinnedMesh { inverse_bindposes: inverse, joints: joints.clone() }, DynamicSkinnedMeshBounds));
    bevy::log::info!(
        "clip player: {} blocks, {} skin joints, {} limb chains ({} feet), spine {}, leg length {:.2}",
        rig.len(),
        b.records.len(),
        rig.chains.len(),
        rig.chains.iter().filter(|c| c.foot).count(),
        rig.spine.len(),
        rig.leg_length
    );
    Some(Avatar {
        player: entity,
        editor,
        joints,
        original_mesh,
        skinned_mesh,
        inverse: inverse_id,
        rig,
        unit: b.unit,
        parts,
        body: run.body.clone(),
        run_parts: run.parts.clone(),
        clips: ClipPlayer::default(),
        last_pos: Vec3::ZERO,
        speed: 0.0,
        actions: AvatarActions::default(),
        last_elapsed: 0.0,
        look: None,
        part_tracks: HashMap::new(),
        pose: (0, None),
        stop: StopWalk::Idle,
    })
}

/// Walk or run by ground speed (creature units per second), with hysteresis
/// between the two cycles' natural speeds on `rig`; idle below 0.15 leg
/// lengths per second.
pub fn locomotion_request(originals: &crate::Originals, speed: f32, rig: &ClipRig, running: &mut bool) -> u32 {
    let natural = |id: u32, fallback: f32| clip(originals, id).and_then(|c| rig.natural_speed(&c)).unwrap_or(fallback * rig.leg_length);
    let mid = 0.5 * (natural(WALK, 2.0) + natural(RUN, 4.0));
    *running = if *running { speed > 0.9 * mid } else { speed > 1.1 * mid };
    if speed > 0.15 * rig.leg_length {
        if *running {
            RUN
        } else {
            WALK
        }
    } else {
        *running = false;
        IDLE
    }
}

/// What the avatar (custom or the installed starter) plays in the Creature
/// stage: an ability that was just used (its cooldown started), eating
/// (hunger rose), else walk / run / idle by ground speed.
#[derive(Clone, Debug, Default)]
pub struct AvatarActions {
    cooldowns: Option<[f32; 4]>,
    food: Option<f32>,
    action: Option<u32>,
    /// The clip that follows the current one (charge: the run-up, then the hit).
    then: Option<u32>,
    running: bool,
}

impl AvatarActions {
    #[allow(clippy::too_many_arguments)]
    pub fn request(
        &mut self,
        clips: &ClipPlayer,
        originals: &Originals,
        run: &RunProgress,
        encounter: Option<&crate::creature_actions::Encounter>,
        hunger: Option<&Hunger>,
        speed: f32,
        rig: &ClipRig,
    ) -> (u32, bool, bool) {
        use crate::creature_actions::EncounterMode;
        let mut restart = false;
        if let Some(enc) = encounter {
            if let Some(before) = self.cooldowns {
                for i in 0..4 {
                    if enc.cooldowns[i] > before[i] + 0.25 {
                        let social = enc.mode == EncounterMode::Social;
                        let ranks = crate::creature_actions::levels(crate::creature_actions::player_abilities(run, originals), enc.mode);
                        // The ability prop's own clip (`creatureAbilityAnimationIDs`).
                        if let Some(id) = crate::creature_actions::ability_clip(&originals.creature_abilities, enc.mode, i, ranks[i].max(1)) {
                            bevy::log::info!("clip player: ability {i} ({}) plays {id:08X}", if social { "social" } else { "combat" });
                            // Charge runs up first (`creatureAbilityRushingAnimationID`).
                            let lead = crate::creature_actions::ability_lead_clip(&originals.creature_abilities, enc.mode, i, ranks[i].max(1))
                                .filter(|lead| clip(originals, *lead).is_some());
                            self.action = Some(lead.unwrap_or(id));
                            self.then = lead.map(|_| id);
                            restart = true;
                        }
                    }
                }
            }
            self.cooldowns = Some(enc.cooldowns);
        }
        if let Some(h) = hunger {
            if self.food.is_some_and(|f| h.food > f + 1e-3) && self.action.is_none() {
                self.action = Some(EAT);
                restart = clips.current_id() != Some(EAT) || clips.finished();
            }
            self.food = Some(h.food);
        }
        if !restart && self.action.is_some() && clips.current_id() == self.action && clips.finished() {
            self.action = self.then.take();
            restart = self.action.is_some();
        }
        if let Some(id) = self.action {
            return (id, false, restart);
        }
        if crate::creature_support::swimming() {
            // In water: the swim stroke when moving, treading water at rest.
            self.running = false;
            let id = if speed > 0.15 * rig.leg_length {
                crate::creature_support::SWIM
            } else {
                crate::creature_support::SWIM_IN_PLACE
            };
            return (id, true, false);
        }
        (locomotion_request(originals, speed, rig, &mut self.running), true, false)
    }
}

/// A requested clip sequence on the avatar (`PoseClip`, as
/// `locomotion_preview` plays it on installed models): the action once, then
/// the idle looped; a new serial starts it again.
fn pose_request(state: &mut (u32, Option<u32>), clips: &ClipPlayer, originals: &Originals, p: &crate::locomotion_preview::PoseClip) -> (u32, bool, bool) {
    let action = p.action.filter(|a| clip(originals, *a).is_some());
    if state.0 != p.serial {
        state.0 = p.serial;
        state.1 = action;
        return (action.unwrap_or(p.idle), action.is_none(), true);
    }
    if state.1.is_some() && clips.current_id() == state.1 && clips.finished() {
        state.1 = None;
    }
    match state.1 {
        Some(a) => (a, false, false),
        None => (p.idle, true, false),
    }
}

/// `csa_idle_walk_lookat_start` (TLSA 0x0452B634) and `_loop` (0x0452B64A,
/// `creature_look::LOOK_IDLE`): the idle the editor creature settles into when
/// a walk ends.
pub const LOOKAT_START: u32 = 0x0452_B634;
pub const LOOKAT_LOOP: u32 = crate::creature_look::LOOK_IDLE;

/// Test Drive's stop-walking idle (the editor creature controller's
/// FUN_006388D0, run when the walk ends): `csa_idle_walk_lookat_start` plays
/// once (not interrupting a `csa_sit_stop` that is still running), then
/// `csa_idle_walk_lookat_loop` loops after it; both carry the `Head lookat`
/// channel that follows the editor look target (look mode 2). VERIFIED in the
/// code; the arms-moving walk clip it layers on top
/// (`csa_idle_walk_normal_armsmoving` / `_fast_` by the gait) is not played.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StopWalk {
    /// The ordinary idle (`csa_idle_gen_editor`).
    #[default]
    Idle,
    Walking,
    Start,
    Loop,
}

/// Test Drive's request (`creature_editor::PlayState`): click-to-walk, the
/// button animation (Sit holds its end while sitting), the stop-walking idle
/// after a walk, else the idle. `look_clips` says whether the stop-walking
/// clips exist (a missing clip would be replaced by the idle every frame).
pub fn test_drive_request(
    play: &crate::creature_editor::PlayState,
    last_elapsed: &mut f32,
    stop: &mut StopWalk,
    clips: &ClipPlayer,
    look_clips: bool,
) -> (u32, bool, bool) {
    if let Some((_, run)) = play.walk_to {
        *stop = StopWalk::Walking;
        return (if run { RUN } else { WALK }, true, false);
    }
    // A button animation ends the stop-walking idle (the idle comes back
    // after it as usual).
    if play.anim.as_ref().is_some_and(|a| a.id != IDLE) || !look_clips {
        *stop = StopWalk::Idle;
    }
    match *stop {
        StopWalk::Walking => {
            *stop = StopWalk::Start;
            return (LOOKAT_START, false, true);
        }
        StopWalk::Start => {
            if clips.current_id() == Some(LOOKAT_START) && !clips.finished() {
                return (LOOKAT_START, false, false);
            }
            *stop = StopWalk::Loop;
            return (LOOKAT_LOOP, true, true);
        }
        StopWalk::Loop => return (LOOKAT_LOOP, true, false),
        StopWalk::Idle => {}
    }
    let Some(anim) = &play.anim else { return (IDLE, true, false) };
    let restart = anim.kind != PlayAnimKind::Loop && anim.elapsed + 1e-4 < *last_elapsed;
    *last_elapsed = anim.elapsed;
    if play.sitting && anim.id == IDLE {
        if let Some(sit) = PLAY_ANIM_BUTTONS.iter().find(|b| b.kind == PlayAnimKind::Toggle) {
            return (sit.anims[0], false, false);
        }
    }
    let looped = anim.kind == PlayAnimKind::Loop || anim.id == IDLE;
    (anim.id, looped, restart)
}

fn teardown(a: Avatar, entity: Entity, mesh3d: &mut Mesh3d, commands: &mut Commands, state: &mut crate::editor::EditorState) {
    if a.player == entity {
        mesh3d.0 = a.original_mesh;
        commands.entity(entity).remove::<(SkinnedMesh, DynamicSkinnedMeshBounds)>();
        for j in a.joints {
            commands.entity(j).despawn();
        }
        if a.editor {
            state.visuals_dirty = true;
        }
    }
}

/// Pose the player's skinned body with clips: Test Drive (its buttons, walk
/// and run) and the Creature stage (idle, speed-matched locomotion, ability
/// and eating actions). Parts ride their blocks.
#[allow(clippy::too_many_arguments)]
pub fn drive_player(
    mut commands: Commands,
    time: Res<Time>,
    stage: Option<Res<State<GameStage>>>,
    editor: Option<Res<CreatureEditor>>,
    encounter: Option<Res<crate::creature_actions::Encounter>>,
    scene: Option<Res<crate::cinematics::CinematicScene>>,
    mut shared: Option<ResMut<ClipDrive>>,
    mut drive: ResMut<DriveState>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    mut state: ResMut<crate::editor::EditorState>,
    (mut meshes, mut bindposes): (ResMut<Assets<Mesh>>, ResMut<Assets<SkinnedMeshInverseBindposes>>),
    mut players: Query<(Entity, &Transform, Option<&mut Mesh3d>, Option<&Hunger>, Has<crate::baked_model::NativePlayerSource>, (Option<&crate::creature_look::LookAt>, Option<&crate::locomotion_preview::PoseClip>)), With<Player>>,
    mut joint_tf: Query<&mut Transform, (With<DriveJoint>, Without<Player>, Without<Accessory>)>,
    mut accessories: Query<(&Accessory, &mut Transform), (Without<Player>, Without<DriveJoint>)>,
    (cursor, creatures): (Option<Res<crate::creature_look::CursorGround>>, Query<(&crate::persistence::WorldId, &bevy::prelude::GlobalTransform, Option<&crate::creature_look::PosedHead>), Without<Player>>),
) {
    let editing = editor.as_ref().is_some_and(|e| e.active);
    let test_drive = editing && editor.as_ref().is_some_and(|e| e.mode == Mode::Play);
    // Not while an editor shapes the body (the legacy stage editor included).
    let in_stage = !editing && !state.open && stage.as_ref().is_some_and(|s| *s.get() == GameStage::Creature);
    let skinned = originals.baked_body.as_ref().is_some_and(|t| t.skin.is_some());
    let Ok((entity, pt, mesh3d, hunger, native, (look_at, pose_clip))) = players.single_mut() else {
        if drive.active.take().is_some() {
            if let Some(d) = shared.as_mut() {
                d.body = false;
            }
        }
        return;
    };
    let Some(mut mesh3d) = mesh3d else {
        // The installed starter's model replaced the root mesh: drop the
        // skeleton now, the mesh when it comes back.
        if let Some(a) = drive.active.take() {
            if a.player == entity {
                commands.entity(entity).remove::<(SkinnedMesh, DynamicSkinnedMeshBounds)>();
                for j in a.joints {
                    commands.entity(j).despawn();
                }
                drive.pending = Some((a.skinned_mesh, a.original_mesh));
            }
            if let Some(d) = shared.as_mut() {
                d.body = false;
            }
        }
        return;
    };
    if let Some((copy, original)) = drive.pending.take() {
        if mesh3d.0 == copy {
            mesh3d.0 = original;
        }
    }
    let want = skinned && !native && (test_drive || in_stage) && enabled("player");
    let stale = drive.active.as_ref().is_some_and(|a| !want || a.player != entity || a.editor != test_drive || a.body != run.body || a.run_parts != run.parts);
    let mut carried = None;
    if stale {
        let a = drive.active.take().unwrap();
        if want && a.player == entity && a.editor == test_drive {
            carried = Some(a.clips.clone());
        }
        teardown(a, entity, &mut mesh3d, &mut commands, &mut state);
        if let Some(d) = shared.as_mut() {
                d.body = false;
            }
    }
    if !want {
        return;
    }
    let pt = *pt;
    if drive.active.is_none() {
        let Some(mut a) = build_avatar(&originals, &run, entity, test_drive, &mut mesh3d, &mut commands, &mut meshes, &mut bindposes) else { return };
        if let Some(mut clips) = carried {
            clips.rebind(&a.rig);
            a.clips = clips;
        }
        a.last_pos = pt.translation;
        drive.active = Some(a);
    }
    if let Some(d) = shared.as_mut() {
        d.body = true;
        if let Some(a) = &drive.active {
            if !d.skins.contains_key(&a.inverse) {
                d.register_skin(a.inverse, a.rig.clone(), a.unit);
            }
        }
    }
    let a = drive.active.as_mut().unwrap();
    let dt = time.delta_secs();
    // Ground speed in creature units per second.
    let frame_unit = (pt.scale.x.abs() * a.unit).max(1e-4);
    let step = (pt.translation - a.last_pos).with_y(0.0).length();
    a.last_pos = pt.translation;
    let v = if dt > 0.0 && step < 5.0 * frame_unit { step / dt / frame_unit } else { 0.0 };
    a.speed += (v - a.speed) * (1.0 - (-8.0 * dt).exp());
    // A cinematic `playAnimation` on the avatar wins (the cinematic host
    // moves the whole actor by the clip's root channel).
    let scripted = scene.as_ref().and_then(|s| s.actor_clip(entity));
    let (id, looped, restart) = if let Some((id, _)) = scripted {
        (id, false, false)
    } else if let Some(p) = pose_clip {
        pose_request(&mut a.pose, &a.clips, &originals, p)
    } else if test_drive {
        let look_clips = clip(&originals, LOOKAT_START).is_some() && clip(&originals, LOOKAT_LOOP).is_some();
        test_drive_request(&editor.as_ref().unwrap().play, &mut a.last_elapsed, &mut a.stop, &a.clips, look_clips)
    } else {
        a.actions.request(&a.clips, &originals, &run, encounter.as_deref(), hunger, a.speed, &a.rig)
    };
    if let Some((id, t)) = scripted {
        follow_script(&mut a.clips, &originals, &a.rig, id, t);
    } else if restart || a.clips.current_id() != Some(id) || a.clips.current().is_some_and(|l| l.looped != looped) {
        if let Some(c) = clip(&originals, id).or_else(|| clip(&originals, IDLE)) {
            let bound = Arc::new(a.rig.bind(c));
            if restart {
                a.clips.restart(bound, looped);
            } else {
                a.clips.play(bound, looped);
            }
        }
    }
    if let Some(top) = a.clips.current() {
        if top.looped && top.bound.clip.stance_speed.is_some() {
            let rate = gait_rate(&top.bound.clip, a.speed, &a.rig);
            a.clips.set_rate(rate);
        }
    }
    if scripted.is_none() {
        a.clips.advance(dt);
    }
    let frame = pt.to_matrix();
    // The external look target (FUN_00C17F40): the creature the avatar is
    // engaged with (its head), else the cursor's ground point; Test Drive the
    // Creature Creator's default point; a cinematic the default ahead.
    let unit = a.unit.max(1e-4);
    let inverse = frame.inverse();
    let to_rig = |w: Vec3| bevy_to_spore(inverse.transform_point3(w)) / unit;
    let target = if scripted.is_some() {
        None
    } else if test_drive {
        Some(a.rig.editor_look())
    } else {
        let engaged = encounter.as_ref().and_then(|e| e.target).and_then(|id| creatures.iter().find(|(w, _, _)| w.0 == id)).map(|(_, gt, head)| crate::creature_look::head_of(gt, head));
        engaged.or_else(|| cursor.as_ref().and_then(|c| c.0)).map(to_rig)
    };
    let target = look_at.map(|l| to_rig(l.0)).or(target);
    let look = match target {
        Some(t) => Some(crate::creature_look::smooth(&mut a.look, t, dt)),
        None => {
            a.look = None;
            None
        }
    };
    let mut deforms = BlockDeforms::new();
    let pose = a.rig.pose_full(&a.clips.mix(), scripted.is_none(), look, crate::creature_morph::enabled().then_some(&mut deforms));
    a.part_tracks = a.parts.iter().map(|(&part, &(j, _))| (part, deforms.get(j).cloned().unwrap_or_default())).collect();
    if look_at.is_some() && crate::creature_look::debug() {
        let heads: Vec<String> = (0..pose.len())
            .filter(|&j| a.rig.joints[j].caps.iter().any(|c| c == b"mout"))
            .map(|j| format!("{j}:{:.2}", rotation_of(&pose[j]) * Vec3::NEG_Y))
            .collect();
        let tracks: usize = a.part_tracks.values().map(|t| t.len()).sum();
        bevy::log::info!("look debug avatar: clip {:08X?} look {:.2?} heads {heads:?} part tracks {tracks}", a.clips.current_id(), look);
    }
    for (j, &e) in a.joints.iter().enumerate() {
        if let (Ok(mut tf), Some(m)) = (joint_tf.get_mut(e), pose.get(j)) {
            *tf = Transform::from_matrix(Mat4::from(to_frame(m, a.unit)));
        }
    }
    for (acc, mut tf) in &mut accessories {
        let Some((j, rest)) = a.parts.get(&acc.part) else { continue };
        let Some(m) = pose.get(*j) else { continue };
        let world = Transform::from_matrix(frame * Mat4::from(to_frame(m, a.unit)) * rest.to_matrix());
        tf.translation = world.translation;
        tf.rotation = world.rotation;
    }
}

/// Clip state of a cinematic actor copy (`addActor` copies the avatar's
/// hierarchy, its skinned body and joints included).
pub struct ActorPose {
    rig: Arc<ClipRig>,
    unit: f32,
    joints: Vec<Entity>,
    clips: ClipPlayer,
    posed: bool,
    morph: Option<Arc<crate::creature_morph::MorphSet>>,
    morph_state: crate::creature_morph::MorphState,
}

/// Pose the skinned bodies of cinematic actor copies from their
/// `playAnimation` clip (limbs, head, feet; the root offset stays the
/// cinematic host's, `cinematics::actors`).
#[allow(clippy::too_many_arguments)]
pub fn drive_cinematic_actors(
    originals: Res<Originals>,
    scene: Option<Res<crate::cinematics::CinematicScene>>,
    shared: Option<Res<ClipDrive>>,
    actors: Query<Entity, bevy::prelude::With<crate::cinematics::CinematicActor>>,
    children: Query<&bevy::prelude::Children>,
    skins: Query<&SkinnedMesh>,
    mut transforms: Query<&mut Transform, Without<crate::cinematics::CinematicActor>>,
    mut states: bevy::prelude::Local<HashMap<Entity, Option<ActorPose>>>,
) {
    states.retain(|e, _| actors.contains(*e));
    let Some(scene) = scene else { return };
    for actor in &actors {
        let state = states.entry(actor).or_insert_with(|| {
            let mut stack = vec![actor];
            while let Some(e) = stack.pop() {
                if let Ok(skin) = skins.get(e) {
                    if let Some((rig, unit)) = shared.as_ref().and_then(|d| d.skins.get(&skin.inverse_bindposes.id()).cloned()) {
                        if skin.joints.len() >= rig.len() && !skin.joints.contains(&Entity::PLACEHOLDER) {
                            let morph = shared.as_ref().and_then(|d| d.morphs.get(&skin.inverse_bindposes.id()).cloned());
                            return Some(ActorPose { rig, unit, joints: skin.joints.clone(), clips: ClipPlayer::default(), posed: false, morph, morph_state: Default::default() });
                        }
                    }
                }
                if let Ok(c) = children.get(e) {
                    stack.extend(c.iter());
                }
            }
            None
        });
        let Some(st) = state.as_mut() else { continue };
        match scene.actor_clip(actor) {
            Some((id, t)) => {
                follow_script(&mut st.clips, &originals, &st.rig, id, t);
                if st.clips.current_id() != Some(id) {
                    continue;
                }
                // ADAPTED: the cinematic `-target` of playAnimation is not
                // traced; actors look at the default point ahead.
                let mut deforms = BlockDeforms::new();
                let pose = st.rig.pose_full(&st.clips.mix(), false, None, st.morph.is_some().then_some(&mut deforms));
                for (j, &e) in st.joints.iter().enumerate().take(st.rig.len()) {
                    if let (Ok(mut tf), Some(m)) = (transforms.get_mut(e), pose.get(j)) {
                        *tf = Transform::from_matrix(Mat4::from(to_frame(m, st.unit)));
                    }
                }
                if let Some(morph) = &st.morph {
                    let mut out = Vec::new();
                    morph.pose(&pose, &deforms, &mut st.morph_state, &mut out);
                    for (j, m) in out {
                        if let Some(Ok(mut tf)) = st.joints.get(j).map(|&e| transforms.get_mut(e)) {
                            *tf = Transform::from_matrix(Mat4::from(to_frame(&m, st.unit)));
                        }
                    }
                }
                st.posed = true;
            }
            None if st.posed => {
                for &e in st.joints.iter() {
                    if let Ok(mut tf) = transforms.get_mut(e) {
                        *tf = Transform::IDENTITY;
                    }
                }
                st.posed = false;
                st.clips = ClipPlayer::default();
            }
            None => {}
        }
    }
}

/// Systems of the clip player outside the player's body (which runs as
/// `creature_blocks::test_drive` in `CreatorLimbsPlugin`).
pub struct ClipPlayerPlugin;

impl bevy::prelude::Plugin for ClipPlayerPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        use bevy::prelude::*;
        app.init_resource::<ClipDrive>()
            .init_resource::<crate::creature_look::CursorGround>()
            .init_resource::<crate::creature_morph::PartMorphCache>()
            .add_systems(bevy::prelude::Update, drive_cinematic_actors.run_if(bevy::prelude::resource_exists::<Originals>))
            .add_systems(bevy::prelude::Update, crate::creature_look::track_cursor.before(drive_player))
            .add_systems(bevy::prelude::Update, crate::creature_look::demo_update.before(drive_player))
            .add_systems(bevy::prelude::Last, crate::creature_look::demo_camera)
            .add_systems(bevy::prelude::Last, crate::creature_morph::demo_camera)
            .add_systems(
                bevy::prelude::Update,
                crate::creature_morph::pose_player_parts
                    .after(drive_player)
                    .run_if(bevy::prelude::resource_exists::<Originals>)
                    .run_if(bevy::prelude::resource_exists::<RunProgress>)
                    .run_if(bevy::prelude::resource_exists::<Assets<Mesh>>)
                    .run_if(bevy::prelude::resource_exists::<Assets<SkinnedMeshInverseBindposes>>),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{Channel, Component, Context, Interpolator, RotationKey};

    #[test]
    fn test_drive_settles_into_the_look_idle_when_a_walk_ends() {
        use crate::creature_editor::{PlayAnim, PlayState};
        let clips = ClipPlayer::default();
        let mut elapsed = 0.0;
        let mut stop = StopWalk::Idle;
        let mut play = PlayState::default();
        // Standing before any walk: the editor idle.
        assert_eq!(test_drive_request(&play, &mut elapsed, &mut stop, &clips, true), (IDLE, true, false));
        // Walking: the walk cycle.
        play.walk_to = Some((Vec3::X, false));
        assert_eq!(test_drive_request(&play, &mut elapsed, &mut stop, &clips, true), (WALK, true, false));
        play.walk_to = Some((Vec3::X, true));
        assert_eq!(test_drive_request(&play, &mut elapsed, &mut stop, &clips, true), (RUN, true, false));
        // The walk ends (FUN_006388D0): the look idle's start plays once...
        play.walk_to = None;
        assert_eq!(test_drive_request(&play, &mut elapsed, &mut stop, &clips, true), (LOOKAT_START, false, true));
        assert_eq!(stop, StopWalk::Start);
        // ...and, nothing playing it any more, its loop follows and stays.
        assert_eq!(test_drive_request(&play, &mut elapsed, &mut stop, &clips, true), (LOOKAT_LOOP, true, true));
        assert_eq!(test_drive_request(&play, &mut elapsed, &mut stop, &clips, true), (LOOKAT_LOOP, true, false));
        assert_eq!((LOOKAT_START, LOOKAT_LOOP), (0x0452_B634, 0x0452_B64A));
        // A button animation takes over; the ordinary idle returns after it.
        play.anim = Some(PlayAnim { id: 0x03FC_4304, kind: PlayAnimKind::Action, elapsed: 0.0, duration: 1.0 });
        let (id, _, _) = test_drive_request(&play, &mut elapsed, &mut stop, &clips, true);
        assert_eq!(id, 0x03FC_4304);
        assert_eq!(stop, StopWalk::Idle);
        play.anim = None;
        assert_eq!(test_drive_request(&play, &mut elapsed, &mut stop, &clips, true), (IDLE, true, false));
        // Without the look clips the walk simply ends in the idle.
        play.walk_to = Some((Vec3::X, false));
        test_drive_request(&play, &mut elapsed, &mut stop, &clips, false);
        play.walk_to = None;
        assert_eq!(test_drive_request(&play, &mut elapsed, &mut stop, &clips, false), (IDLE, true, false));
    }

    fn joint(kind: BlockKind, parent: Option<usize>, pivot: Vec3, at: Vec3, caps: &[&[u8; 4]]) -> Joint {
        Joint { kind, parent, pivot, at, caps: caps.iter().map(|c| **c).collect() }
    }

    /// Spine of three vertebrae (root in the middle), a mouth at the front,
    /// two two-segment legs with feet.
    fn creature() -> ClipRig {
        let v = |y: f32| Vec3::new(0.0, y, 1.0);
        let mut j = vec![
            joint(BlockKind::Vertebra, None, v(0.0), v(0.0), &[b"spin", b"root"]),
            joint(BlockKind::Vertebra, Some(0), v(-0.4), v(-0.4), &[b"spin"]),
            joint(BlockKind::Vertebra, Some(0), v(0.4), v(0.4), &[b"spin"]),
            joint(BlockKind::Standard, Some(1), Vec3::new(0.0, -0.6, 1.0), Vec3::new(0.0, -0.6, 1.0), &[b"mout"]),
        ];
        for x in [0.3f32, -0.3] {
            let base = j.len();
            j.push(joint(BlockKind::Limb, Some(2), Vec3::new(x, 0.4, 0.9), Vec3::new(x, 0.35, 0.45), &[b"limb"]));
            j.push(joint(BlockKind::Limb, Some(base), Vec3::new(x, 0.35, 0.45), Vec3::new(x, 0.4, 0.0), &[b"limb"]));
            j.push(joint(BlockKind::Standard, Some(base + 1), Vec3::new(x, 0.4, 0.0), Vec3::new(x, 0.4, 0.0), &[b"foot"]));
        }
        ClipRig::new(j, 0.0).unwrap()
    }

    fn info_keys(frames: &[u32]) -> Component {
        Component {
            flags: 0,
            authored_flags: 0,
            id: 0,
            authored_id: 0,
            index: 0,
            reserved: [0; 12],
            keys: Keys::Info(frames.iter().map(|f| InfoKey { frame: *f, ..Default::default() }).collect()),
        }
    }

    fn channel(cap: &str, flags: u32, movement: u32, pos: Option<(u32, Vec<[f32; 3]>)>, rot: Option<Vec<[f32; 4]>>, frames: &[u32]) -> Channel {
        let mut components = vec![info_keys(frames)];
        if let Some((f, keys)) = pos {
            components.push(Component {
                flags: f,
                authored_flags: f,
                id: 0,
                authored_id: 0,
                index: 1,
                reserved: [0; 12],
                keys: Keys::Position(keys.into_iter().map(|p| PositionKey { position: p, weight: 1.0, interpolators: [Interpolator { next_mode: 1, previous_mode: 1, ..Default::default() }; 4] }).collect()),
            });
        }
        if let Some(keys) = rot {
            components.push(Component {
                flags: 0x32,
                authored_flags: 0x32,
                id: 0,
                authored_id: 0,
                index: 2,
                reserved: [0; 12],
                keys: Keys::Rotation(
                    keys.into_iter()
                        .map(|q| RotationKey { rotation: q, weight: 1.0, next_mode: 0, previous_mode: 0, weight_interpolator: Interpolator { next_mode: 1, previous_mode: 1, ..Default::default() }, raw: vec![] })
                        .collect(),
                ),
            });
        }
        Channel {
            name: cap.into(),
            movement_flags: movement,
            primary: Context { flags, capability: cap.into(), id: 0, fields: [0; 2] },
            secondary: Context { flags: 2, ..Default::default() },
            bind_flags: 0,
            reserved: vec![],
            components,
        }
    }

    fn anim(channels: Vec<Channel>, frames: f32, flags: u32) -> Animation {
        Animation { version: 0x19, declared_size: 0, id: 0, source_path: String::new(), flags, length_frames: frames, frame_length: 1.0 / 30.0, predicate: [0; 2], channels, events: vec![] }
    }

    #[test]
    fn rest_keys_give_the_rest_pose_and_root_goals_move_the_body() {
        let rig = creature();
        assert_eq!(rig.root, 0);
        assert_eq!(rig.spine.iter().map(|s| s.0).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(rig.chains.len(), 2);
        assert!(rig.chains.iter().all(|c| c.foot));
        let zero = vec![[0.0; 3], [0.0; 3]];
        let rest = anim(
            vec![
                channel("", 1, 0, Some((0x31, zero.clone())), Some(vec![[0.0, 0.0, 0.0, 1.0]; 2]), &[0, 30]),
                channel("foot", 0, 0, Some((0xB1, zero.clone())), None, &[0, 30]),
                channel("mout", 0, 0, Some((0x31, zero)), None, &[0, 30]),
            ],
            31.0,
            0,
        );
        let bound = rig.bind(Arc::new(Clip::new(1, rest, 0.2)));
        assert_eq!(bound.driven_channels(), 3);
        let m = rig.pose(&[(&bound, 1.0, 10.0)]);
        for (i, a) in m.iter().enumerate() {
            assert!(a.abs_diff_eq(Affine3A::IDENTITY, 1e-4), "joint {i} moved at rest: {a:?}");
        }
        // The root crouches 0.2 down and leans 0.3 rad: the spine follows,
        // feet stay on their rest points, the mouth rides the head.
        let crouch = anim(
            vec![
                channel("", 1, 0, Some((0x31, vec![[0.0, 0.0, -0.2]; 2])), Some(vec![Quat::from_rotation_x(0.3).to_array(); 2]), &[0, 30]),
                channel("foot", 0, 0, Some((0xB1, vec![[0.0; 3]; 2])), None, &[0, 30]),
            ],
            31.0,
            0,
        );
        let bound = rig.bind(Arc::new(Clip::new(2, crouch, 0.2)));
        let m = rig.pose(&[(&bound, 1.0, 5.0)]);
        assert!((m[0].transform_point3(rig.joints[0].pivot) - Vec3::new(0.0, 0.0, 0.8)).length() < 1e-4);
        assert!(rotation_of(&m[1]).angle_between(Quat::from_rotation_x(0.3)) < 1e-3);
        for c in &rig.chains {
            let tip = *c.points.last().unwrap();
            let foot = m[c.end.unwrap()].transform_point3(tip);
            assert!(foot.distance(tip) < 1e-3, "foot planted: {foot} vs {tip}");
            // Segment lengths are kept.
            let knee = m[c.segs[1]].transform_point3(c.points[1]);
            let hip = m[c.segs[0]].transform_point3(c.points[0]);
            assert!((knee.distance(hip) - c.points[0].distance(c.points[1])).abs() < 1e-3);
            assert!((foot.distance(knee) - c.points[1].distance(c.points[2])).abs() < 1e-3);
        }
        // Half a blend gives half the crouch.
        let m = rig.pose(&[(&bound, 0.5, 5.0)]);
        assert!((m[0].transform_point3(rig.joints[0].pivot).z - 0.9).abs() < 1e-3);
    }

    #[test]
    fn feet_reach_limb_length_goals_and_the_mouth_bends_the_spine() {
        let rig = creature();
        // Right foot (local -x side) steps 0.2 limb lengths forward and lifts.
        let step = anim(
            vec![
                channel("foot", 0x20004, 0, Some((0xB1, vec![[0.0, -0.2, 0.1]; 2])), Some(vec![Quat::from_rotation_x(0.4).to_array(); 2]), &[0, 30]),
                channel("mout", 0, 0, Some((0x31, vec![[0.0, -0.1, -0.1]; 2])), None, &[0, 30]),
            ],
            31.0,
            0,
        );
        let bound = rig.bind(Arc::new(Clip::new(3, step, 0.2)));
        let m = rig.pose(&[(&bound, 1.0, 0.0)]);
        let right = rig.chains.iter().find(|c| c.points[0].x < 0.0).unwrap();
        let left = rig.chains.iter().find(|c| c.points[0].x > 0.0).unwrap();
        let len = rig.rig.limb_length(right.end.unwrap()).unwrap();
        let tip = *right.points.last().unwrap();
        let foot = m[right.end.unwrap()].transform_point3(tip);
        assert!(foot.distance(tip + Vec3::new(0.0, -0.2, 0.1) * len) < 2e-3, "right foot {foot}");
        assert!(rotation_of(&m[right.end.unwrap()]).angle_between(Quat::from_rotation_x(0.4)) < 1e-3);
        let ltip = *left.points.last().unwrap();
        assert!(m[left.end.unwrap()].transform_point3(ltip).distance(ltip) < 1e-3, "left foot planted");
        // The mouth reaches its goal; the root stays.
        let mouth = rig.joints[3].at;
        let got = m[3].transform_point3(mouth);
        assert!(got.distance(mouth + Vec3::new(0.0, -0.1, -0.1)) < 1e-3, "mouth {got}");
        assert!(m[0].abs_diff_eq(Affine3A::IDENTITY, 1e-5));
    }

    #[test]
    fn gait_speed_cycle_rank_offsets_and_layer_fades() {
        // A foot cycle of 10 frames: planted slide from -0.3 to +0.3 over
        // frames 0..6, swing back over 6..10, repeated twice.
        let ys = [-0.3f32, 0.0, 0.3, 0.0, -0.3, 0.0, 0.3, 0.0, -0.3];
        let zs = [0.0f32, 0.0, 0.0, 0.2, 0.0, 0.0, 0.0, 0.2, 0.0];
        let frames = [0u32, 3, 6, 8, 10, 13, 16, 18, 20];
        let keys: Vec<[f32; 3]> = ys.iter().zip(zs).map(|(y, z)| [0.0, *y, z]).collect();
        let a = anim(vec![channel("foot", 0x20004, 0, Some((0xB1, keys)), None, &frames)], 21.0, 8);
        let clip = Arc::new(Clip::new(4, a, 0.2));
        let (speed, scale) = clip.stance_speed.unwrap();
        assert_eq!(scale, 0x80);
        assert!((speed - 0.6 / (6.0 / 30.0)).abs() < 1e-3, "{speed}");
        assert_eq!(clip.foot_cycle, Some(10.0));
        assert!((clip.seconds - 20.0 / 30.0).abs() < 1e-6, "cyclic clips loop over length - 1");
        // Four legs on the right: ranks alternate half a cycle.
        let mut j = vec![joint(BlockKind::Vertebra, None, Vec3::Z, Vec3::Z, &[b"spin", b"root"])];
        for y in [-0.5f32, 0.5] {
            j.push(joint(BlockKind::Standard, Some(0), Vec3::new(-0.3, y, 0.0), Vec3::new(-0.3, y, 0.0), &[b"foot"]));
        }
        j.push(joint(BlockKind::Standard, Some(0), Vec3::new(0.3, 0.0, 0.0), Vec3::new(0.3, 0.0, 0.0), &[b"foot"]));
        let rig = ClipRig::new(j, 0.0).unwrap();
        let bound = rig.bind(clip.clone());
        assert_eq!(bound.channels[0].joints, vec![(1, 0.0), (2, 5.0)]);
        let natural = rig.natural_speed(&clip).unwrap();
        assert!((natural - speed * rig.leg_length).abs() < 1e-4);
        assert!((gait_rate(&clip, natural * 2.0, &rig) - 2.0).abs() < 1e-4);
        // Fades: the new layer takes over within its blend time.
        let mut player = ClipPlayer::default();
        player.play(Arc::new(rig.bind(clip.clone())), true);
        assert_eq!(player.mix()[0].1, 1.0);
        let other = Arc::new(Clip::new(5, anim(vec![], 10.0, 0), 0.2));
        player.play(Arc::new(rig.bind(other)), false);
        player.advance(0.1);
        let mix = player.mix();
        assert_eq!(mix.len(), 2);
        assert!((mix[0].1 - 0.5).abs() < 1e-4 && (mix[1].1 - 0.5).abs() < 1e-4);
        player.advance(0.2);
        assert_eq!(player.layers.len(), 1);
        assert_eq!(player.current_id(), Some(5));
        player.advance(1.0);
        assert!(player.finished());
        assert_eq!(ability_action(true, 1, 3), Some(0x042A_1CA2));
        assert_eq!(ability_action(false, 3, 5), Some(0x044D_DEAF));
    }

    #[test]
    fn secondary_channels_aim_at_the_look_target_and_deforms_come_out_per_block() {
        let rig = creature();
        let mouth = rig.joints[3].at;
        // `head sec`: secondary + directional only + look-at, no secondary
        // selection (the external target); rotation relative to the look frame.
        let mut head = channel("mout", 0, 0xB, None, Some(vec![[0.0, 0.0, 0.0, 1.0]; 2]), &[0, 30]);
        head.components[1].flags = 0x52;
        head.components.push(Component {
            flags: 0x13,
            authored_flags: 0x13,
            id: 0x89E0_6A31,
            authored_id: 0x89E0_6A31,
            index: 2,
            reserved: [0; 12],
            keys: Keys::Deform(vec![crate::animation::DeformKey { value: 0.6, weight: 1.0, interpolators: [Interpolator { next_mode: 1, previous_mode: 1, ..Default::default() }; 2] }; 2]),
        });
        let bound = rig.bind(Arc::new(Clip::new(9, anim(vec![head], 31.0, 0), 0.2)));
        let dir = Quat::from_rotation_z(0.5) * Vec3::NEG_Y;
        let mut deforms = BlockDeforms::new();
        let m = rig.pose_full(&[(&bound, 1.0, 5.0)], true, Some(mouth + dir * 3.0), Some(&mut deforms));
        let forward = rotation_of(&m[3]) * Vec3::NEG_Y;
        assert!(forward.distance(dir) < 1e-3, "head faces {forward}, target {dir}");
        assert_eq!(deforms.len(), rig.len());
        assert_eq!(deforms[3].len(), 1);
        let (id, v, w) = deforms[3][0];
        assert_eq!(id, 0x89E0_6A31);
        assert!((v - 0.6).abs() < 1e-5 && (w - 1.0).abs() < 1e-5);
        assert!(deforms[0].is_empty());
        // Without a target the default point (10 ahead on the ground) keeps
        // the head facing forward, slightly down.
        let m = rig.pose_full(&[(&bound, 1.0, 5.0)], true, None, None);
        let f = rotation_of(&m[3]) * Vec3::NEG_Y;
        assert!(f.x.abs() < 1e-3 && f.y < -0.99 && f.z < 0.0, "{f}");
        // Rest rotations from block axes and the Bevy -> Spore basis change.
        let q = rest_rotation(&[Vec3::NEG_X, Vec3::Y, Vec3::Z]);
        assert!((q * Vec3::Y).distance(Vec3::Y) < 1e-5 && (q * Vec3::Z).distance(Vec3::Z) < 1e-5);
        let b = Quat::from_rotation_y(0.4);
        let s = spore_rotation(b);
        let p = Vec3::new(0.2, 0.3, 0.4);
        assert!(bevy_to_spore(b * crate::creature_editor::spore_to_bevy(p)).distance(s * p) < 1e-5);
    }

    #[test]
    fn frame_maps_are_rotations() {
        let m = Affine3A::from_quat(Quat::from_rotation_z(0.5)) * Affine3A::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let p = Vec3::new(0.3, -0.2, 0.9);
        let f = to_frame(&m, 2.0);
        let expect = crate::creature_editor::spore_to_bevy(m.transform_point3(p)) * 2.0;
        assert!(f.transform_point3(crate::creature_editor::spore_to_bevy(p) * 2.0).distance(expect) < 1e-5);
        assert!((f.matrix3.determinant() - 1.0).abs() < 1e-5);
        assert_eq!(bevy_to_spore(crate::creature_editor::spore_to_bevy(p)), p);
    }

    /// Installed clips on the installed starter's rigblocks: every Test Drive
    /// and ability clip binds, poses finitely, and the gait cycles carry a
    /// stance speed.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_clips_pose_the_starter_rig() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut originals = crate::Originals::default();
        originals.animation_catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
        originals.action_lists = crate::animation_catalog::ActionLists::from_index(&index, &originals.animation_catalog);
        originals.resource_index = index;
        let key = crate::dbpf::ResourceKey {
            type_id: crate::creature_data::TYPE,
            group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
            instance_id: crate::baked_model::PLAYER_BASELINE_INSTANCE,
        };
        let data = crate::creature_data::parse(&originals.resource_index.read(key).unwrap()).unwrap();
        let rig = rig_of_creature_data(&data).unwrap();
        println!("starter rig: {} blocks, {} chains, spine {:?}, leg {:.3}", rig.len(), rig.chains.len(), rig.spine, rig.leg_length);
        for (i, j) in rig.joints.iter().enumerate() {
            println!("  {i:2} {:?} parent {:?} at {:.2} caps {:?}", j.kind, j.parent, j.at, j.caps.iter().map(|c| String::from_utf8_lossy(c).to_string()).collect::<Vec<_>>());
        }
        assert!(rig.chains.iter().filter(|c| c.foot).count() >= 2);
        let mut ids: Vec<u32> = crate::creature_editor_data::PLAY_ANIM_BUTTONS.iter().flat_map(|b| b.anims.iter().copied()).collect();
        ids.extend([IDLE, WALK, RUN, EAT]);
        ids.extend((0..4).flat_map(|s| [ability_action(true, s, 1).unwrap(), ability_action(false, s, 5).unwrap()]));
        for id in ids {
            let clip = clip(&originals, id).unwrap_or_else(|| panic!("clip {id:08X}"));
            let bound = rig.bind(clip.clone());
            assert!(bound.driven_channels() > 0, "{id:08X} drives nothing");
            for t in [0.0, 0.37, 0.5 * clip.seconds, clip.seconds] {
                let m = rig.pose(&[(&bound, 1.0, clip.frame_at(t, clip.cyclic))]);
                assert!(m.iter().all(|a| a.is_finite()), "{id:08X} at {t}");
            }
            println!("{id:08X} {} {:.2}s cyclic {} channels {}/{} stance {:?} cycle {:?}", clip.anim.source_path.rsplit(['\\', '/']).next().unwrap_or(""), clip.seconds, clip.cyclic, bound.driven_channels(), clip.anim.channels.len(), clip.stance_speed, clip.foot_cycle);
        }
        let walk = clip(&originals, WALK).unwrap();
        let run = clip(&originals, RUN).unwrap();
        let (w, r) = (rig.natural_speed(&walk).unwrap(), rig.natural_speed(&run).unwrap());
        println!("natural speeds on the starter: walk {w:.2}, run {r:.2} units/s");
        assert!(r > w && w > 0.0);
    }
}

/// The clip rig of an installed creature's `.creaturedata` (rigblocks in
/// order; capabilities from its capability table).
pub fn rig_of_creature_data(data: &crate::creature_data::CreatureData) -> Result<ClipRig, String> {
    let records = crate::creature_skin::records_from_creature_data(data);
    let joints: Vec<Joint> = records
        .iter()
        .zip(&data.rigblocks)
        .map(|(r, b)| {
            let start = b.capability_index().max(0) as usize;
            let count = b.capability_count().max(0) as usize;
            let caps: Vec<[u8; 4]> = data.capability_ids.iter().skip(start).take(count).map(|c| tag(*c)).collect();
            Joint {
                kind: r.kind,
                parent: (r.parent >= 0).then_some(r.parent as usize),
                pivot: r.position,
                at: if r.kind == BlockKind::Limb { r.segment_end() } else { r.position },
                caps,
            }
        })
        .collect();
    let mut joints = joints;
    if let Some(root) = joints.iter().position(|j| j.parent.is_none()) {
        if !joints[root].caps.iter().any(|c| c == b"root") {
            joints[root].caps.push(*b"root");
        }
    }
    let feet = joints.iter().any(|j| j.caps.iter().any(|c| c == b"foot"));
    let ground = joints.iter().map(|j| j.at.z.min(j.pivot.z)).fold(f32::MAX, f32::min);
    if !feet {
        mark_pseudo_feet(&mut joints, ground);
    }
    let mut rig = ClipRig::new(joints, ground)?;
    let rests: Vec<Quat> = records.iter().map(|r| rest_rotation(&r.orientation)).collect();
    rig.set_rests(&rests);
    Ok(rig)
}

/// A capability id of a `.creaturedata` as its 4-character tag (stored in
/// reading order, e.g. `limb foot cspd` in `x_leg_09.creaturedata`).
fn tag(raw: [u8; 4]) -> [u8; 4] {
    raw.map(|c| if c == 0 { b' ' } else { c })
}

/// FUN_009CBCE0 (the rule of `creature_blocks::mark_pseudo_feet`): limb-chain
/// ends within a tenth of the height above the lowest point.
fn mark_pseudo_feet(joints: &mut [Joint], ground: f32) {
    let top = joints.iter().map(|j| j.at.z).fold(f32::MIN, f32::max);
    let n = joints.len();
    for i in 0..n {
        let end = joints[i].kind == BlockKind::Limb && !(0..n).any(|c| joints[c].parent == Some(i) && joints[c].kind == BlockKind::Limb);
        if end && joints[i].at.z <= ground + (top - ground) * crate::creature_blocks::PSEUDO_FOOT_FRACTION {
            joints[i].caps.push(*b"psft");
        }
    }
}
