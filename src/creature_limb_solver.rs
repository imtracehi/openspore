//! The retail limb joint drag (`notes/limb-re-notes.md` sections 1-3,
//! VERIFIED unless marked): what replaces creator-limbs' one-segment re-aim.
//!
//! The Limb manipulator (0x8665F54D) picks a mode from the grabbed block's node
//! type (`FUN_0048C5F0`): the foot ball slides on the floor and the joints
//! between the limb root and the foot are re-placed proportionally
//! (`FUN_0048AB90`); a hand, knee or elbow ball is put at the best of 400
//! points along the mouse ray (`FUN_005AEE80`), "best" being the weighted
//! score of `FUN_005A7DC0` with the weights of the installed
//! `editor_limbTuning~` PROPs (knee / elbow / hand / hand2); the joints below
//! a hand or an elbow follow it rigidly (`FUN_004894E0`), those below a knee
//! stay (the foot stays planted). The root ball re-pins the limb on the body
//! (the creator's ordinary part carry).
//!
//! All maths here works in OpenSpore's player frame (Y up, the plane of
//! symmetry x = 0, the creature's front/back axis Z): every feature is a
//! distance or an angle, so the minimum is the same as in retail's creature
//! space; the two axis-bound terms map Spore X -> X (the symmetry normal) and
//! Spore Y -> Z (the front axis of the side-view rule).

use bevy::prelude::*;

use crate::asset_catalog::ResourceIndex;
use crate::cell_editor_data::{number, prop_key};

/// `FUN_005AEE80`'s sample count (argument 400 at 0x5B0B84).
pub const SAMPLES: usize = 400;
/// `editor_limbTuning~`.
pub const TUNING_GROUP: u32 = 0x4060_6500;
pub const HAND_TUNING: u32 = 0xC606_4659;
pub const HAND_TUNING2: u32 = 0x14E0_BE29;
pub const KNEE_TUNING: u32 = 0x29B2_3B97;
pub const ELBOW_TUNING: u32 = 0x34D6_1CCD;
/// Penalty factor of the plane-of-symmetry rule and of invalid lengths
/// (`[0x1473BF0]` = 10.0).
pub const SYMMETRY_PENALTY: f32 = 10.0;

/// Weight property ids (`FUN_005A79F0`, push order 0x5A7A47..0x5A7D6F).
pub mod prop {
    pub const DISTANCE_FROM_ORIGINAL_POSITION: u32 = 0x0241_149F;
    pub const CHANGE_IN_PREVIOUS_LIMB: u32 = 0x0241_204B;
    pub const CHANGE_IN_NEXT_LIMB: u32 = 0x0241_2051;
    pub const DISTANCE_FROM_CAMERA_PLANE: u32 = 0x0242_175B;
    pub const CHANGE_IN_DISTANCE_FROM_PARALLEL_PLANE: u32 = 0x0242_1763;
    pub const ARM_ANGLE: u32 = 0x0242_1767;
    pub const CHANGE_IN_DISTANCE_FROM_SYMMETRY_PLANE: u32 = 0x0242_1776;
    pub const PLANE_OF_SYMMETRY_ALIGNMENT: u32 = 0x0242_177B;
    pub const ARM_ANGLE_CHANGE: u32 = 0x0242_359A;
    pub const CAMERA_ANGLE: u32 = 0x0242_359E;
    pub const INVALID_LENGTH: u32 = 0x0242_35A4;
    pub const LOWER_LIMB_LENGTH: u32 = 0x0242_3FE1;
    pub const UPPER_LIMB_LENGTH: u32 = 0x0242_3FDA;
}

/// The weights of one scorer (`editor_limbTuning~` PROP).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tuning {
    pub original: f32,
    pub previous: f32,
    pub next: f32,
    pub camera_plane: f32,
    pub parallel_plane: f32,
    pub arm_angle: f32,
    pub symmetry_plane: f32,
    pub symmetry_alignment: f32,
    pub arm_angle_change: f32,
    pub camera_angle: f32,
    pub invalid_length: f32,
    pub upper_length: f32,
    pub lower_length: f32,
}

impl Tuning {
    /// Installed `kneetuning` (fallback when the install is not read).
    pub const KNEE: Tuning = Tuning { original: 1.0, previous: 1.0, next: 1.0, parallel_plane: 1.0, ..Tuning::ZERO };
    /// Installed `elbowtuning`.
    pub const ELBOW: Tuning = Tuning { original: 3.0, parallel_plane: 2.0, ..Tuning::ZERO };
    /// Installed `handtuning` ("for limb not aligned with camera").
    pub const HAND: Tuning = Tuning { original: 1.0, parallel_plane: 1.0, ..Tuning::ZERO };
    /// Installed `handtuning2` ("for limb aligned with camera").
    pub const HAND2: Tuning = Tuning { original: 1.0, previous: 1.0, ..Tuning::ZERO };
    pub const ZERO: Tuning = Tuning {
        original: 0.0,
        previous: 0.0,
        next: 0.0,
        camera_plane: 0.0,
        parallel_plane: 0.0,
        arm_angle: 0.0,
        symmetry_plane: 0.0,
        symmetry_alignment: 0.0,
        arm_angle_change: 0.0,
        camera_angle: 0.0,
        invalid_length: 0.0,
        upper_length: 0.0,
        lower_length: 0.0,
    };

    pub fn load(index: &ResourceIndex, instance: u32) -> Option<Tuning> {
        let p = index.properties(prop_key(TUNING_GROUP, instance)).ok()?;
        let w = |id: u32| number(&p, id).unwrap_or(0.0);
        Some(Tuning {
            original: w(prop::DISTANCE_FROM_ORIGINAL_POSITION),
            previous: w(prop::CHANGE_IN_PREVIOUS_LIMB),
            next: w(prop::CHANGE_IN_NEXT_LIMB),
            camera_plane: w(prop::DISTANCE_FROM_CAMERA_PLANE),
            parallel_plane: w(prop::CHANGE_IN_DISTANCE_FROM_PARALLEL_PLANE),
            arm_angle: w(prop::ARM_ANGLE),
            symmetry_plane: w(prop::CHANGE_IN_DISTANCE_FROM_SYMMETRY_PLANE),
            symmetry_alignment: w(prop::PLANE_OF_SYMMETRY_ALIGNMENT),
            arm_angle_change: w(prop::ARM_ANGLE_CHANGE),
            camera_angle: w(prop::CAMERA_ANGLE),
            invalid_length: w(prop::INVALID_LENGTH),
            upper_length: w(prop::UPPER_LIMB_LENGTH),
            lower_length: w(prop::LOWER_LIMB_LENGTH),
        })
    }

    /// The weighted sum (`FUN_005A8490` / `FUN_005A86B0` sums).
    pub fn sum(&self, f: &Features, hand: bool) -> f32 {
        let mut s = self.original * f.from_original
            + self.previous * f.previous_change
            + self.next * f.next_change
            + self.camera_plane * f.camera_plane
            + self.parallel_plane * f.parallel_change
            + self.arm_angle * f.arm_angle
            + self.symmetry_plane * f.symmetry_change
            + self.symmetry_alignment * f.symmetry_alignment
            + self.invalid_length * f.invalid_length
            + self.camera_angle * f.camera_angle
            + self.arm_angle_change * f.arm_angle_change;
        if hand {
            // Only the grasper scorer adds the two length terms.
            s += self.upper_length * f.upper + self.lower_length * f.lower;
        }
        s
    }
}

/// The four scorers' weights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tunings {
    pub knee: Tuning,
    pub elbow: Tuning,
    pub hand: Tuning,
    pub hand2: Tuning,
}

impl Default for Tunings {
    fn default() -> Self {
        Self { knee: Tuning::KNEE, elbow: Tuning::ELBOW, hand: Tuning::HAND, hand2: Tuning::HAND2 }
    }
}

impl Tunings {
    pub fn load(index: &ResourceIndex) -> Self {
        let d = Self::default();
        Self {
            knee: Tuning::load(index, KNEE_TUNING).unwrap_or(d.knee),
            elbow: Tuning::load(index, ELBOW_TUNING).unwrap_or(d.elbow),
            hand: Tuning::load(index, HAND_TUNING).unwrap_or(d.hand),
            hand2: Tuning::load(index, HAND_TUNING2).unwrap_or(d.hand2),
        }
    }
}

/// What ends a limb chain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EndKind {
    #[default]
    None,
    /// `modelActsLikeFoot`.
    Foot,
    /// `modelActsLikeGrasper` (or any other end part).
    Grasper,
}

/// The node types of `FUN_0048C5F0` (0..5) = the drag modes 0..5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointKind {
    /// 0: the foot's ball.
    Foot,
    /// 1: the grasper's ball.
    Grasper,
    /// 2: a mid joint of a chain ending in a foot.
    Knee,
    /// 3: a mid joint of any other chain.
    Elbow,
    /// 4 / 5: the chain root (shoulder / hip) of a leg / an arm.
    LegRoot,
    ArmRoot,
}

/// The kind of joint `k` of a chain of `joints` joints (joint 0 = root, the
/// last = the end block's ball). ADAPTED: a chain without an end block shows a
/// ball at its socket end too, dragged like an elbow (retail's socket end has
/// no ball-connector handle).
pub fn joint_kind(k: usize, joints: usize, end: EndKind) -> JointKind {
    let leg = end == EndKind::Foot;
    if k == 0 {
        return if leg { JointKind::LegRoot } else { JointKind::ArmRoot };
    }
    if k + 1 == joints {
        return match end {
            EndKind::Foot => JointKind::Foot,
            EndKind::Grasper => JointKind::Grasper,
            EndKind::None => JointKind::Elbow,
        };
    }
    if leg {
        JointKind::Knee
    } else {
        JointKind::Elbow
    }
}

/// The features of `FUN_005A7DC0` for a candidate `p`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Features {
    pub from_original: f32,
    pub upper: f32,
    pub lower: f32,
    pub previous_change: f32,
    pub next_change: f32,
    pub invalid_length: f32,
    pub camera_plane: f32,
    pub parallel_change: f32,
    pub arm_angle: f32,
    pub symmetry_change: f32,
    pub symmetry_alignment: f32,
    pub camera_angle: f32,
    pub arm_angle_change: f32,
}

/// The fixed context of one drag: N = the joint at press, A = its parent
/// joint at press, C = its child joint at press (or the segment's socket),
/// G = the grandparent joint, the camera position, the socket length range.
#[derive(Clone, Copy, Debug)]
pub struct ScoreCtx {
    pub n: Vec3,
    pub a: Vec3,
    pub c: Vec3,
    pub g: Option<Vec3>,
    pub cam: Vec3,
    pub min_len: f32,
    pub max_len: f32,
    /// The joint is snapped to the plane of symmetry (bool bit 15).
    pub on_plane: bool,
}

fn cos(a: Vec3, b: Vec3) -> f32 {
    a.normalize_or_zero().dot(b.normalize_or_zero())
}

/// `FUN_005A7DC0` (VERIFIED, disassembly `limb-re/dec/LimbScore_Features_5a7dc0.asm.txt`).
pub fn features(ctx: &ScoreCtx, p: Vec3, dir: Vec3) -> Features {
    let (n, a, c) = (ctx.n, ctx.a, ctx.c);
    let upper = p.distance(a);
    let lower = p.distance(c);
    let invalid = upper > ctx.max_len || lower > ctx.max_len || upper < ctx.min_len || lower < ctx.min_len;
    // The plane through A facing the camera: normal -D.
    let normal = -dir;
    let camera_plane = normal.dot(p - a);
    let s = Vec3::X;
    let arm_angle = ctx.g.map_or(0.0, |g| cos(p - a, g - a));
    Features {
        from_original: p.distance(n),
        upper,
        lower,
        previous_change: (upper - n.distance(a)).abs(),
        next_change: (lower - n.distance(c)).abs(),
        invalid_length: if invalid { (lower * SYMMETRY_PENALTY + upper).abs() } else { 0.0 },
        camera_plane,
        parallel_change: normal.dot(n - p).abs(),
        arm_angle,
        symmetry_change: s.dot(n - p).abs(),
        symmetry_alignment: (ctx.cam - n).normalize_or_zero().dot(s),
        camera_angle: cos(ctx.cam - a, n - a),
        arm_angle_change: (ctx.g.map_or(0.0, |g| cos(n - a, g - a)) - arm_angle).abs(),
    }
}

/// The score of a candidate (`FUN_005A8490` knee / elbow, `FUN_005A86B0`
/// hand). Lower is better.
pub fn score(kind: JointKind, t: &Tunings, ctx: &ScoreCtx, p: Vec3, dir: Vec3) -> f32 {
    let f = features(ctx, p, dir);
    let s = match kind {
        JointKind::Grasper => {
            let s1 = t.hand.sum(&f, true);
            let s2 = t.hand2.sum(&f, true);
            (s2 - s1) * f.camera_angle + s1
        }
        JointKind::Knee => t.knee.sum(&f, false),
        _ => t.elbow.sum(&f, false),
    };
    if !ctx.on_plane {
        return s;
    }
    // A joint on the plane of symmetry: the more the view looks along the
    // creature's side, the more it keeps the joint on the plane.
    let h = Vec2::new(dir.x, dir.z).normalize_or_zero();
    let m = h.y.abs();
    let pen = p.x.abs() * SYMMETRY_PENALTY;
    (s - pen) * m + pen
}

/// `FUN_005AEE80`: the best of `samples` points on the segment `a..b` (the
/// mouse ray between the camera side `a` and the far side `b`); `view` = the
/// ray direction. Sample 0 is where the segment crosses x = 0 when its ends
/// are on both sides (or the joint is on the plane), scored with the segment
/// direction; the rest run evenly from `b` to `a`. The first minimum wins.
pub fn line_search(a: Vec3, b: Vec3, samples: usize, on_plane: bool, view: Vec3, mut f: impl FnMut(Vec3, Vec3) -> f32) -> Option<Vec3> {
    let mut best: Option<(f32, Vec3)> = None;
    let mut first = 0;
    let side = |x: f32| if x >= 0.0 { 1 } else { -1 };
    if side(a.x) != side(b.x) || on_plane {
        let u = (b - a).normalize_or_zero();
        if u.x.abs() > 1e-9 {
            let p = a + u * (-a.x / u.x);
            best = Some((f(p, u), p));
        }
        first = 1;
    }
    let rest = samples.saturating_sub(first);
    for i in 0..rest {
        let t = if rest > 1 { i as f32 / (rest - 1) as f32 } else { 0.0 };
        let p = b + (a - b) * t;
        let s = f(p, view);
        if best.is_none_or(|(bs, _)| s < bs) {
            best = Some((s, p));
        }
    }
    best.map(|(_, p)| p)
}

/// `FUN_0048AB90`: the joints strictly between the root (index 0) and the
/// moved joint `m` re-placed from their press positions: their position
/// along the root -> moved axis scales with its length and turns with it, the
/// offset from the axis is kept.
pub fn proportional(press: &[Vec3], m: usize, target: Vec3) -> Vec<Vec3> {
    let mut out = press.to_vec();
    if m == 0 || m >= press.len() {
        return out;
    }
    let r0 = press[0];
    let axis0 = press[m] - r0;
    let len0 = axis0.length() + 1e-12;
    let u = axis0 / len0;
    let now = target - r0;
    let ratio = (now.length() + 1e-12) / len0;
    let v = now.normalize_or_zero();
    for (i, o) in out.iter_mut().enumerate().take(m).skip(1) {
        let e = press[i] - r0;
        let c = e.dot(u);
        let perp = e - u * c;
        *o = r0 + v * c * ratio + perp;
    }
    out[m] = target;
    out
}

/// The joints after dragging joint `k` (kind `kind`) of a chain whose press
/// positions are `press` to `target` (`FUN_005AF300` per mode): the foot
/// re-places the joints above it; a hand or an elbow carries the joints below
/// it when the chain has no foot (`has_feet`); a knee leaves them.
pub fn place(press: &[Vec3], k: usize, kind: JointKind, target: Vec3, has_feet: bool) -> Vec<Vec3> {
    if k == 0 || k >= press.len() {
        return press.to_vec();
    }
    match kind {
        JointKind::Foot => proportional(press, k, target),
        JointKind::LegRoot | JointKind::ArmRoot => press.to_vec(),
        _ => {
            let mut out = press.to_vec();
            out[k] = target;
            if !has_feet && kind != JointKind::Knee {
                for i in k + 1..out.len() {
                    out[i] = target + (press[i] - press[k]);
                }
            }
            out
        }
    }
}

/// INFERRED (the rigblock socket range, `FUN_0048A1F0` re-aims each segment at
/// its child): the moved joint's parent segment keeps its length within
/// `[min, max]`; the joints that moved with it shift along.
pub fn clamp_length(joints: &mut [Vec3], press: &[Vec3], k: usize, min: f32, max: f32) {
    if k == 0 || k >= joints.len() {
        return;
    }
    let v = joints[k] - joints[k - 1];
    let len = v.length();
    if len < 1e-9 || (min..=max).contains(&len) {
        return;
    }
    let fixed = joints[k - 1] + v / len * len.clamp(min, max);
    let delta = fixed - joints[k];
    // Only the joints that followed the moved one (moved by the same offset
    // from their press positions) shift.
    let moved = joints[k] - press.get(k).copied().unwrap_or(joints[k]);
    for i in k..joints.len() {
        if i == k || press.get(i).is_some_and(|p| (joints[i] - *p).distance(moved) < 1e-5) {
            joints[i] += delta;
        }
    }
}

/// `FUN_00486E10` (shape INFERRED): a joint stays above its floor height and
/// inside the editor bounds (a vertical cylinder of radius `bound` around the
/// creature's axis, below `top`).
pub fn clamp_bounds(p: Vec3, floor: f32, top: f32, bound: f32) -> Vec3 {
    let mut out = p;
    out.y = out.y.clamp(floor, top.max(floor));
    let h = Vec2::new(out.x, out.z);
    if h.length() > bound {
        let k = h.normalize() * bound;
        out.x = k.x;
        out.z = k.y;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leg() -> Vec<Vec3> {
        vec![Vec3::new(0.5, 0.0, 0.0), Vec3::new(0.6, -0.5, -0.1), Vec3::new(0.6, -1.0, 0.0)]
    }

    #[test]
    fn node_types_follow_the_chain_end() {
        assert_eq!(joint_kind(0, 3, EndKind::Foot), JointKind::LegRoot);
        assert_eq!(joint_kind(1, 3, EndKind::Foot), JointKind::Knee);
        assert_eq!(joint_kind(2, 3, EndKind::Foot), JointKind::Foot);
        assert_eq!(joint_kind(0, 3, EndKind::Grasper), JointKind::ArmRoot);
        assert_eq!(joint_kind(1, 3, EndKind::Grasper), JointKind::Elbow);
        assert_eq!(joint_kind(2, 3, EndKind::Grasper), JointKind::Grasper);
        assert_eq!(joint_kind(1, 3, EndKind::None), JointKind::Elbow);
    }

    #[test]
    fn foot_drag_places_the_knee_proportionally() {
        let press = leg();
        let target = Vec3::new(0.9, -1.0, -0.4);
        let out = place(&press, 2, JointKind::Foot, target, true);
        assert_eq!(out[0], press[0]);
        assert_eq!(out[2], target);
        // The knee keeps its offset from the root -> foot axis and its relative
        // position along it.
        let (u0, u1) = ((press[2] - press[0]).normalize(), (out[2] - out[0]).normalize());
        let c0 = (press[1] - press[0]).dot(u0) / press[2].distance(press[0]);
        let c1 = (out[1] - out[0] - ((press[1] - press[0]) - u0 * (press[1] - press[0]).dot(u0))).dot(u1) / out[2].distance(out[0]);
        assert!((c0 - c1).abs() < 1e-4, "{c0} {c1}");
        // Not moving the foot leaves everything.
        let same = place(&press, 2, JointKind::Foot, press[2], true);
        for (a, b) in same.iter().zip(&press) {
            assert!(a.distance(*b) < 1e-5);
        }
    }

    #[test]
    fn knee_leaves_the_foot_and_elbow_carries_the_hand() {
        let press = leg();
        let t = Vec3::new(0.7, -0.4, -0.3);
        let knee = place(&press, 1, JointKind::Knee, t, true);
        assert_eq!(knee[2], press[2], "the foot stays planted");
        let elbow = place(&press, 1, JointKind::Elbow, t, false);
        assert!((elbow[2] - elbow[1] - (press[2] - press[1])).length() < 1e-6, "the hand follows rigidly");
        // A hand on a chain with feet (another limb of the helper) does not drag
        // anything.
        let hand = place(&press, 1, JointKind::Elbow, t, true);
        assert_eq!(hand[2], press[2]);
    }

    #[test]
    fn length_clamp_moves_the_followers() {
        let press = leg();
        let mut j = place(&press, 1, JointKind::Elbow, Vec3::new(0.5, -5.0, 0.0), false);
        clamp_length(&mut j, &press, 1, 0.03, 1.0);
        assert!((j[1].distance(j[0]) - 1.0).abs() < 1e-5);
        assert!((j[2] - j[1] - (press[2] - press[1])).length() < 1e-5);
    }

    /// The knee scorer prefers, along the mouse ray, the point that keeps the
    /// joint where it was and both segment lengths.
    #[test]
    fn knee_line_search_finds_the_original_depth() {
        let press = leg();
        let t = Tunings::default();
        let cam = Vec3::new(0.6, -0.5, 5.0);
        let ctx = ScoreCtx { n: press[1], a: press[0], c: press[2], g: None, cam, min_len: 0.03, max_len: 3.0, on_plane: false };
        // A ray from the camera through the knee: the best sample is the knee.
        let dir = (press[1] - cam).normalize();
        let far = cam + dir * 10.0;
        let best = line_search(cam, far, SAMPLES, false, dir, |p, d| score(JointKind::Knee, &t, &ctx, p, d)).unwrap();
        assert!(best.distance(press[1]) < 0.03, "{best} vs {}", press[1]);
        // A ray passing 0.2 to the side: the result stays at the knee's depth.
        let aim = press[1] + Vec3::X * 0.2;
        let dir = (aim - cam).normalize();
        let best = line_search(cam, cam + dir * 10.0, SAMPLES, false, dir, |p, d| score(JointKind::Knee, &t, &ctx, p, d)).unwrap();
        assert!((best.z - press[1].z).abs() < 0.05, "{best}");
    }

    #[test]
    fn plane_crossing_sample_and_symmetry_rule() {
        let a = Vec3::new(-1.0, 0.0, 0.0);
        let b = Vec3::new(1.0, 0.0, 0.0);
        let mut seen = Vec::new();
        line_search(a, b, 5, false, Vec3::Z, |p, _| {
            seen.push(p);
            p.x.abs()
        });
        assert_eq!(seen.len(), 5);
        assert!(seen[0].x.abs() < 1e-6, "sample 0 on the plane");
        assert!(seen[1].distance(b) < 1e-6, "then from b to a");
        // Seen from the side (ray along X), a joint on the plane is held on it.
        let t = Tunings::default();
        let ctx = ScoreCtx { n: Vec3::ZERO, a: Vec3::Y, c: -Vec3::Y, g: None, cam: Vec3::X * 5.0, min_len: 0.0, max_len: 9.0, on_plane: true };
        let side = score(JointKind::Elbow, &t, &ctx, Vec3::new(0.3, 0.0, 0.0), -Vec3::X);
        assert!((side - 3.0).abs() < 1e-4, "10 |x| from the side: {side}");
    }

    #[test]
    fn hand_blends_its_two_tunings_by_camera_angle() {
        let t = Tunings::default();
        let n = Vec3::new(0.0, 0.0, 1.0);
        let a = Vec3::ZERO;
        // Limb pointing at the camera: cos = 1 -> handtuning2 only.
        let ctx = ScoreCtx { n, a, c: n + Vec3::Z * 0.1, g: None, cam: Vec3::Z * 5.0, min_len: 0.0, max_len: 9.0, on_plane: false };
        let p = Vec3::new(0.0, 0.0, 1.5);
        let f = features(&ctx, p, -Vec3::Z);
        let s = score(JointKind::Grasper, &t, &ctx, p, -Vec3::Z);
        assert!((s - t.hand2.sum(&f, true)).abs() < 1e-5);
    }

    #[test]
    fn bounds_clamp() {
        let p = clamp_bounds(Vec3::new(4.0, -9.0, 0.0), -1.0, 2.0, 3.0);
        assert_eq!(p, Vec3::new(3.0, -1.0, 0.0));
    }

    /// Installed tuning PROPs equal the fallback constants.
    #[test]
    fn installed_tunings() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let t = Tunings::load(&index);
        println!("{t:?}");
        assert_eq!(t, Tunings::default());
    }
}
