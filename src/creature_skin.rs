//! The retail creature skin: the metaball body Spore builds from a creature's
//! rigblocks (`notes/skin-re-notes.md` section 1).
//!
//! Pipeline (all VERIFIED unless marked; addresses SDK/march2017):
//! 1. records (`cEditorSkinMeshBase::FromRigblocks` 0x4C7460): one
//!    [`SkinRecord`] per rigblock (type, flags, bone number, scale,
//!    orientation, position, socket distance, muscle / ball-connector scales),
//!    spine first;
//! 2. primitives (FUN_00413DD0, FUN_004128F0, FUN_004FCD90): a ball per
//!    vertebra, link spheres between consecutive vertebrae, balls along every
//!    skinned limb segment;
//! 3. field `f(p) = sum w (1 - |p - c|^2 / R^2)^4 - 0.01` (FUN_004F91A0 /
//!    FUN_004F8E40), inside where `f >= 0`;
//! 4. polygonizer (FUN_004142D0 -> FUN_005400A0): surface-following cube
//!    marcher on a grid of cell `res` aligned to multiples of `res`, edge
//!    crossings found by bisection and merged into one vertex per owning
//!    grid corner (the characteristic Spore skin topology), analytic gradient
//!    normals, retry at `res * 1.2` on failure; `res` = 0.042 while editing,
//!    0.032 otherwise. The Creature polygonizes with the fans of [`cube_table`]; the Cell body
//!    uses retail's own walk of the cube polygons (`polygonize_retail`, tables of 0x53D2B0,
//!    0x53FE30, the cube queue order, bisection and averaging arithmetic), which reproduces the
//!    baked starter body bit for bit (`cell_body::tests`);
//! 5. bone weights (FUN_0052B340): field contribution per primitive group,
//!    shared between the bones of a segment / joint, the 4 largest kept;
//! 6. UVs: ADAPTED for the Creature (the retail flood and packing, `unwrap_retail` below, are
//!    ported for the Cell body; the Creature keeps one chart per side of every chain (spine,
//!    limb chains), cylindrical around the chain).
//!
//! Everything here is pure (no Bevy world access) and works in Spore creature
//! space (Z up, front -Y). `creature_skin_view` converts to the player frame.

// The test-vector loaders and the records' limb data serve the tests and the
// later limb / Test Drive work.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::OnceLock;

use bevy::math::Vec3;

// ---------------------------------------------------------------------------
// Retail constants
// ---------------------------------------------------------------------------

/// Field threshold `T` (FUN_004F91A0).
pub const FIELD_THRESHOLD: f32 = 0.01;
/// `editorSkinResolutionRealtime` (while editing / dragging).
pub const RES_REALTIME: f32 = 0.042;
/// `editorSkinResolutionHighQuality` (Paint, Test Drive, bakes, idle).
pub const RES_HIGH_QUALITY: f32 = 0.032;
/// FUN_004142D0: a failed attempt retries at `res * 1.2` while `res < 0.5`.
pub const RETRY_FACTOR: f32 = 1.2;
pub const MAX_RES: f32 = 0.5;
/// Link blend `b` (FUN_00413DD0 / FUN_004128F0: 0x3E99999A).
pub const LINK_BLEND: f32 = 0.3;
/// Vertebra ball: centre offset along the vertebra's local up, times scale
/// (FUN_004A6180) and radius factor (FUN_004A6110: `scale * 0.378 * 0.7`,
/// at least 0.04).
pub const VERTEBRA_BALL_OFFSET: f32 = -0.159_931_78;
pub const VERTEBRA_RADIUS_FACTOR: f32 = 0.378_027_68 * 0.7;
pub const VERTEBRA_MIN_RADIUS: f32 = 0.04;
/// `modelMinBallConnectorScale` default (property 0xFE0926F6) and the 0.1
/// factor of FUN_004128F0.
pub const MIN_BALL_CONNECTOR_SCALE: f32 = 0.5;
pub const BALL_CONNECTOR_RADIUS: f32 = 0.1;
/// Balls of a tentacle limb segment (FUN_004128F0 `local_fc = 15.0`).
pub const TENTACLE_BALLS: usize = 15;
/// Each grid axis must have 1..=255 cells (FUN_004F96C0).
pub const MAX_AXIS_CELLS: i32 = 255;
/// Edge-crossing cache capacity (FUN_0053F960: index < 0x8000, else the
/// attempt fails).
pub const EDGE_CACHE: usize = 0x8000;
/// Bisection of an edge crossing (FUN_004FC120): at most 20 steps, stop when
/// `|f| <= min(0.001, 0.1 * T)`.
pub const ROOT_STEPS: usize = 20;
/// Seed walk along +X (FUN_0053E690).
pub const SEED_STEPS: i32 = 128;
/// Bones addressable by the skin weights (FUN_0052B340: index < 0x40).
pub const MAX_BONES: u32 = 64;
/// Group-id bit of a joint ball.
pub const JOINT: u32 = 0x8000_0000;

/// `RigblockData::mType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Null = 0,
    Baked = 1,
    Standard = 2,
    Limb = 3,
    PlantRoot = 4,
    Vertebra = 5,
}

impl BlockKind {
    pub fn from_i8(v: i8) -> Self {
        match v {
            1 => Self::Baked,
            2 => Self::Standard,
            3 => Self::Limb,
            4 => Self::PlantRoot,
            5 => Self::Vertebra,
            _ => Self::Null,
        }
    }
}

/// `RigblockData::mFlags` bits (section 1.1).
pub mod flag {
    /// Bool attribute bit 10 `UseSkin` (ModAPI's `kFlagNotUseSkin` is wrong).
    pub const USES_SKIN: u16 = 1;
    /// Starts a new bone chain (no parent, or a limb whose parent is not a limb).
    pub const CHAIN_START: u16 = 2;
    /// Bool attribute bit 31 (FUN_004128F0 reads the parent's position as the
    /// tentacle's first control point when set).
    pub const BALL_JOINT: u16 = 4;
    pub const BAKED: u16 = 8;
    /// `mLimbType == 1`: tentacle-style limb (15-ball cubic tube).
    pub const TENTACLE: u16 = 0x10;
    pub const FOOT: u16 = 0x20;
}

/// One rigblock as the skin builder sees it (`RigblockData`, 0x8C bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct SkinRecord {
    pub index: i16,
    /// `field_2`: bone (joint) number.
    pub bone: i16,
    pub parent: i16,
    pub flags: u16,
    pub kind: BlockKind,
    pub scale: f32,
    /// `mTotalOrientation` rows: the block's local axes in creature space.
    pub orientation: [Vec3; 3],
    pub position: Vec3,
    /// `mSocketConnectorDistance`: limb segment length.
    pub socket_distance: f32,
    /// `mMuscleScale` (thickness wheel 0..1).
    pub muscle_scale: f32,
    /// `mBaseMuscleScale`: this joint's ball-connector scale.
    pub base_muscle_scale: f32,
    /// `field_7C`: the next joint's ball-connector scale (< 0 = 1).
    pub next_muscle_scale: f32,
    /// `modelMinBallConnectorScale` of the part.
    pub min_ball_scale: f32,
}

impl Default for SkinRecord {
    fn default() -> Self {
        Self {
            index: 0,
            bone: 0,
            parent: -1,
            flags: 0,
            kind: BlockKind::Standard,
            scale: 1.0,
            orientation: [Vec3::X, Vec3::Y, Vec3::Z],
            position: Vec3::ZERO,
            socket_distance: 0.0,
            muscle_scale: 0.0,
            base_muscle_scale: 1.0,
            next_muscle_scale: 1.0,
            min_ball_scale: MIN_BALL_CONNECTOR_SCALE,
        }
    }
}

impl SkinRecord {
    pub fn uses_skin(&self) -> bool {
        self.flags & flag::USES_SKIN != 0
    }
    /// `v * mTotalOrientation` (row-vector convention: rows are the axes).
    pub fn rotate(&self, v: Vec3) -> Vec3 {
        self.orientation[0] * v.x + self.orientation[1] * v.y + self.orientation[2] * v.z
    }
    /// Limb segment end: `S + Rot * (0, -L, 0)` (FUN_004128F0).
    pub fn segment_end(&self) -> Vec3 {
        self.position + self.rotate(Vec3::new(0.0, -self.socket_distance, 0.0))
    }
    /// The vertebra ball (FUN_004A6180 / FUN_004A6110): centre and support radius.
    pub fn vertebra_ball(&self) -> (Vec3, f32) {
        (
            self.position + self.orientation[2] * (VERTEBRA_BALL_OFFSET * self.scale),
            (self.scale * VERTEBRA_RADIUS_FACTOR).max(VERTEBRA_MIN_RADIUS),
        )
    }
}

/// Retail bone numbering (`field_2`, section 1.1): consecutive, a chain start
/// takes two numbers (the extra one is the chain's root joint). Records must be
/// in skin order (spine first).
pub fn assign_bones(records: &mut [SkinRecord]) {
    let mut next = 0i16;
    for r in records.iter_mut() {
        if r.flags & flag::CHAIN_START != 0 {
            next += 1;
        }
        r.bone = next;
        next += 1;
    }
}

/// Records from a serialized `cCreatureDataResource` (`.creaturedata`, the
/// installed test vectors): the `RigblockData` fields at their ModAPI offsets.
pub fn records_from_creature_data(data: &crate::creature_data::CreatureData) -> Vec<SkinRecord> {
    data.rigblocks
        .iter()
        .map(|b| {
            let raw = &b.raw_bytes;
            let f = |o: usize| f32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
            let v = |o: usize| Vec3::new(f(o), f(o + 4), f(o + 8));
            SkinRecord {
                index: b.index(),
                bone: i16::from_le_bytes([raw[2], raw[3]]),
                parent: b.parent_index(),
                flags: b.flags(),
                kind: BlockKind::from_i8(b.block_type()),
                scale: f(0x2C),
                orientation: [v(0x30), v(0x3C), v(0x48)],
                position: v(0x54),
                socket_distance: f(0x70),
                muscle_scale: f(0x74),
                base_muscle_scale: f(0x78),
                next_muscle_scale: f(0x7C),
                min_ball_scale: MIN_BALL_CONNECTOR_SCALE,
            }
        })
        .collect()
}

/// Records from an editor model `.crt` (XML `sporemodel`, e.g. the startup
/// creatures `B_00..`): `is_vertebra(group, instance)` names the vertebra part
/// (`ce_vertebra` / `cl_vertebra`, part bool `IsVertebra`). Vertebrae use skin;
/// other blocks are rigid parts here (limb assemblies come from their baked
/// `.creaturedata`, section 3).
pub fn records_from_crt(xml: &str, is_vertebra: impl Fn(u32, u32) -> bool) -> Vec<SkinRecord> {
    fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
        let open = format!("<{name}>");
        let close = format!("</{name}>");
        let a = s.find(&open)? + open.len();
        let b = a + s[a..].find(&close)?;
        Some(s[a..b].trim())
    }
    fn nums(s: &str) -> Vec<f32> {
        s.split(',').filter_map(|v| v.trim().parse::<f32>().ok()).collect()
    }
    fn hex(s: &str) -> u32 {
        let s = s.trim();
        u32::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).unwrap_or(0)
    }
    let vec3 = |s: Option<&str>, d: Vec3| {
        s.map(nums).filter(|v| v.len() >= 3).map_or(d, |v| Vec3::new(v[0], v[1], v[2]))
    };
    let mut blocks = Vec::new();
    let mut children: Vec<Vec<usize>> = Vec::new();
    let mut rest = xml;
    while let Some(a) = rest.find("<blockref>") {
        let Some(b) = rest[a..].find("</blockref>") else { break };
        let block = &rest[a..a + b];
        rest = &rest[a + b + 11..];
        let ids: Vec<u32> = tag(block, "blockid").map(|s| s.split(',').map(hex).collect()).unwrap_or_default();
        let (group, instance) = (ids.first().copied().unwrap_or(0), ids.get(1).copied().unwrap_or(0));
        let scale = tag(block, "scale").and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0);
        let orientation = [
            vec3(tag(block, "row0"), Vec3::X),
            vec3(tag(block, "row1"), Vec3::Y),
            vec3(tag(block, "row2"), Vec3::Z),
        ];
        let position = vec3(tag(block, "position"), Vec3::ZERO);
        let mut kids = Vec::new();
        let mut c = block;
        while let Some(i) = c.find("<childid>") {
            let j = i + 9;
            let Some(k) = c[j..].find("</childid>") else { break };
            if let Ok(n) = c[j..j + k].trim().parse::<usize>() {
                kids.push(n);
            }
            c = &c[j + k..];
        }
        let vertebra = is_vertebra(group, instance);
        blocks.push(SkinRecord {
            index: blocks.len() as i16,
            flags: if vertebra { flag::USES_SKIN } else { 0 },
            kind: if vertebra { BlockKind::Vertebra } else { BlockKind::Standard },
            scale,
            orientation,
            position,
            ..Default::default()
        });
        children.push(kids);
    }
    for (p, kids) in children.iter().enumerate() {
        for &k in kids {
            if let Some(b) = blocks.get_mut(k) {
                b.parent = p as i16;
            }
        }
    }
    order_records(blocks)
}

/// Section 1.1 order (spine first: roots, vertebrae in chain order, then
/// limbs, then the rest) with parents re-indexed, chain-start flags and bone
/// numbers. ADAPTED: a stable sort by kind rank keeps chain order for
/// well-formed creations (the retail sort FUN_00461B50 is over (parent, type)
/// pairs).
pub fn order_records(mut blocks: Vec<SkinRecord>) -> Vec<SkinRecord> {
    let rank = |k: BlockKind| match k {
        BlockKind::Vertebra => 0,
        BlockKind::Limb => 1,
        _ => 2,
    };
    let mut order: Vec<usize> = (0..blocks.len()).collect();
    order.sort_by_key(|&i| rank(blocks[i].kind));
    let mut new_index = vec![0i16; blocks.len()];
    for (n, &o) in order.iter().enumerate() {
        new_index[o] = n as i16;
    }
    let old = blocks.clone();
    for (n, &o) in order.iter().enumerate() {
        let mut r = old[o].clone();
        r.index = n as i16;
        r.parent = if r.parent >= 0 { new_index[r.parent as usize] } else { -1 };
        blocks[n] = r;
    }
    for i in 0..blocks.len() {
        let parent = blocks[i].parent;
        let start = parent < 0
            || (blocks[i].kind == BlockKind::Limb && blocks[parent as usize].kind != BlockKind::Limb)
            || (blocks[i].kind == BlockKind::Vertebra && blocks[parent as usize].kind != BlockKind::Vertebra);
        if start {
            blocks[i].flags |= flag::CHAIN_START;
        } else {
            blocks[i].flags &= !flag::CHAIN_START;
        }
    }
    assign_bones(&mut blocks);
    blocks
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// A field ball: centre, support radius `R`, weight and primitive group
/// (segment = rigblock index, joint ball = joint number | [`JOINT`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ball {
    pub center: Vec3,
    pub radius: f32,
    pub weight: f32,
    pub group: u32,
}

/// `sqrt(1 - (T / w)^(1/4))`: visible ("iso") radius over support radius of a
/// lone ball (0.8269 for w = 1).
pub fn iso_factor(weight: f32) -> f32 {
    (1.0 - (FIELD_THRESHOLD / weight).powf(0.25)).max(0.0).sqrt()
}

/// The primitive set of one skin, with an 8x8x8 bin grid for lookups
/// (performance only, as retail).
#[derive(Clone, Debug, Default)]
pub struct Field {
    pub balls: Vec<Ball>,
    min: Vec3,
    max: Vec3,
    bin_size: Vec3,
    bins: Vec<Vec<u32>>,
}

const BINS: usize = 8;

impl Field {
    fn add(&mut self, center: Vec3, weight: f32, radius: f32, group: u32) -> usize {
        self.balls.push(Ball { center, radius: radius.max(0.0), weight, group });
        self.balls.len() - 1
    }

    /// Link spheres between balls `a` and `b` (FUN_004FCD30 -> FUN_004FCD90):
    /// iso radius varying linearly from A's to B's, spacing proportional to the
    /// local radius.
    fn link(&mut self, a: usize, b: usize, blend: f32, group: u32) {
        let (ba, bb) = (self.balls[a], self.balls[b]);
        let delta = bb.center - ba.center;
        let len = delta.length();
        if len < 1e-6 {
            return;
        }
        let dir = delta / len;
        let f = iso_factor(ba.weight);
        if f <= 0.0 {
            return;
        }
        let sa = f * ba.radius + 1e-6;
        let sb = iso_factor(bb.weight) * bb.radius + 1e-6;
        let k = (sa - sb) / len;
        if k.abs() <= 1e-6 {
            let n = (((blend * k + 1.0) * len) / (blend * 2.0 * sa)).ceil();
            if !n.is_finite() || n < 1.0 {
                return;
            }
            let s = len / (sa * 2.0 * n - k * len);
            for i in 1..(n as usize).min(4096) {
                let t = (s * 2.0 * sa * i as f32) / (s * k + 1.0);
                self.add(ba.center + dir * t, ba.weight, sa / f, group);
            }
        } else {
            let q0 = (1.0 - blend * k) / (blend * k + 1.0);
            let n = ((sb / sa).ln() / q0.ln()).ceil();
            if !n.is_finite() || n < 1.0 {
                return;
            }
            // INFERRED: the refit ratio lands the progression exactly on B
            // (`q^n = sB / sA`; the decompiled `s` / `q` refit is equivalent).
            let q = (sb / sa).powf(1.0 / n);
            for i in 1..(n as usize).min(4096) {
                let qi = q.powi(i as i32);
                let t = (1.0 - qi) * sa / k;
                self.add(ba.center + dir * t, ba.weight, qi * sa / f, group);
            }
        }
    }

    fn finish(&mut self) {
        let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for b in &self.balls {
            min = min.min(b.center - Vec3::splat(b.radius));
            max = max.max(b.center + Vec3::splat(b.radius));
        }
        if self.balls.is_empty() {
            (min, max) = (Vec3::ZERO, Vec3::ZERO);
        }
        self.min = min;
        self.max = max;
        self.bin_size = ((max - min) / BINS as f32).max(Vec3::splat(1e-4));
        self.bins = vec![Vec::new(); BINS * BINS * BINS];
        for (i, b) in self.balls.iter().enumerate() {
            let lo = self.bin_coord(b.center - Vec3::splat(b.radius));
            let hi = self.bin_coord(b.center + Vec3::splat(b.radius));
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        self.bins[(x * BINS + y) * BINS + z].push(i as u32);
                    }
                }
            }
        }
    }

    fn bin_coord(&self, p: Vec3) -> [usize; 3] {
        let c = ((p - self.min) / self.bin_size).floor();
        [c.x, c.y, c.z].map(|v| (v.max(0.0) as usize).min(BINS - 1))
    }

    fn bin(&self, p: Vec3) -> &[u32] {
        if p.cmplt(self.min).any() || p.cmpgt(self.max).any() || self.bins.is_empty() {
            return &[];
        }
        let c = self.bin_coord(p);
        &self.bins[(c[0] * BINS + c[1]) * BINS + c[2]]
    }

    /// Bounds of all supports.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        (self.min, self.max)
    }

    /// `f(p)`.
    pub fn value(&self, p: Vec3) -> f32 {
        let mut sum = 0.0;
        for &i in self.bin(p) {
            let b = &self.balls[i as usize];
            let d2 = (p - b.center).length_squared();
            let r2 = b.radius * b.radius;
            if d2 < r2 {
                let u = 1.0 - d2 / r2;
                sum += b.weight * (u * u) * (u * u);
            }
        }
        sum - FIELD_THRESHOLD
    }

    /// Analytic gradient (FUN_004F9270).
    pub fn gradient(&self, p: Vec3) -> Vec3 {
        let mut g = Vec3::ZERO;
        for &i in self.bin(p) {
            let b = &self.balls[i as usize];
            let d = p - b.center;
            let d2 = d.length_squared();
            let r2 = b.radius * b.radius;
            if d2 < r2 {
                let u = 1.0 - d2 / r2;
                g += d * (b.weight * 4.0 * u * u * u * (-2.0 / r2));
            }
        }
        g
    }

    /// Field contribution per primitive group at `p` (FUN_004F9590).
    pub fn contributions(&self, p: Vec3, out: &mut Vec<(u32, f32)>) {
        out.clear();
        for &i in self.bin(p) {
            let b = &self.balls[i as usize];
            let d2 = (p - b.center).length_squared();
            let r2 = b.radius * b.radius;
            if d2 < r2 {
                let u = 1.0 - d2 / r2;
                let c = b.weight * (u * u) * (u * u);
                match out.iter_mut().find(|e| e.0 == b.group) {
                    Some(e) => e.1 += c,
                    None => out.push((b.group, c)),
                }
            }
        }
    }

    /// Move `p` onto the surface (Newton steps along the gradient), with the
    /// outward normal there. Used by the painter's crawling brush particles.
    pub fn project(&self, mut p: Vec3) -> (Vec3, Vec3) {
        for _ in 0..8 {
            let f = self.value(p);
            let g = self.gradient(p);
            let g2 = g.length_squared();
            if g2 <= 1e-12 {
                break;
            }
            p -= g * (f / g2);
            if f.abs() < 1e-4 {
                break;
            }
        }
        let g = self.gradient(p);
        (p, (-g).normalize_or(Vec3::Z))
    }
}

/// A bone of the skin skeleton: rigblock (output bone index) and its two joints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkinBone {
    pub block: u32,
    pub joint_a: u32,
    pub joint_b: u32,
}

/// Joints by number (`field_2`), with position and the bones meeting there.
#[derive(Clone, Debug, Default)]
pub struct Skeleton {
    pub bones: HashMap<u32, SkinBone>,
    pub joints: HashMap<u32, (Vec3, Vec<u32>)>,
}

impl Skeleton {
    fn joint(&mut self, id: u32, pos: Vec3) {
        self.joints.entry(id).or_insert((pos, Vec::new())).0 = pos;
    }
    fn bone(&mut self, block: u32, a: u32, b: u32) {
        self.bones.insert(block, SkinBone { block, joint_a: a, joint_b: b });
        for j in [a, b] {
            let e = self.joints.entry(j).or_insert((Vec3::ZERO, Vec::new()));
            if !e.1.contains(&block) {
                e.1.push(block);
            }
        }
    }
}

/// A chain of the body (spine or a limb chain) as a polyline of centres with
/// iso radii; the ADAPTED unwrap parameterizes around it.
#[derive(Clone, Debug, Default)]
pub struct Chain {
    pub points: Vec<Vec3>,
    pub radii: Vec<f32>,
    pub limb: bool,
    /// Reference "up" for the angle around the chain.
    pub up: Vec3,
}

/// Primitives, skeleton and chains of a set of records.
#[derive(Clone, Debug, Default)]
pub struct Primitives {
    pub field: Field,
    pub skeleton: Skeleton,
    pub chains: Vec<Chain>,
    /// Chain of each rigblock index (absent = no skin).
    pub block_chain: HashMap<u32, usize>,
}

/// Cubic through the four tentacle control points, between `p1` (t = 0) and
/// `p2` (t = 1). INFERRED: FUN_00422470 is a Catmull-Rom segment (it passes
/// through the start and end balls).
fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1) + (-p0 + p2) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Build the implicit surface of `records` (FUN_00413DD0). Empty when no
/// rigblock uses skin.
pub fn primitives(records: &[SkinRecord]) -> Primitives {
    let mut out = Primitives::default();
    if !records.iter().any(SkinRecord::uses_skin) {
        return out;
    }
    let iso = iso_factor(1.0);
    // Vertebrae: the leading type-5 run, in order.
    let mut spine = Chain { up: Vec3::Z, ..Default::default() };
    let mut prev: Option<(usize, i16)> = None;
    let mut vertebrae = Vec::new();
    for r in records.iter().take_while(|r| r.kind == BlockKind::Vertebra) {
        let (c, radius) = r.vertebra_ball();
        let id = out.field.add(c, 1.0, radius, r.index as u32);
        if let Some((p, prev_bone)) = prev {
            out.field.link(p, id, LINK_BLEND, prev_bone as u32 | JOINT);
        }
        prev = Some((id, r.bone));
        spine.points.push(c);
        spine.radii.push(radius * iso);
        vertebrae.push(r);
        out.block_chain.insert(r.index as u32, 0);
    }
    // Spine skeleton: vertebra i spans joint (bone - 1) .. (bone); a joint
    // between two vertebrae sits at the midpoint of their balls (section 5);
    // the two chain ends mirror the half segment.
    let n = spine.points.len();
    for (i, r) in vertebrae.iter().enumerate() {
        let c = spine.points[i];
        let back = if i > 0 {
            (spine.points[i - 1] + c) * 0.5
        } else if n > 1 {
            c - (spine.points[1] - c) * 0.5
        } else {
            c
        };
        let front = if i + 1 < n {
            (spine.points[i + 1] + c) * 0.5
        } else if n > 1 {
            c + (c - spine.points[n - 2]) * 0.5
        } else {
            c
        };
        let (a, b) = ((r.bone - 1).max(0) as u32, r.bone as u32);
        out.skeleton.joint(a, back);
        out.skeleton.joint(b, front);
        out.skeleton.bone(r.index as u32, a, b);
    }
    if let (Some(first), Some(last)) = (spine.points.first(), spine.points.last()) {
        // ADAPTED unwrap reference: an upright torso measures its angle from
        // the front (-Y) instead of the back.
        if (*last - *first).normalize_or(Vec3::Y).z.abs() > 0.7 {
            spine.up = Vec3::NEG_Y;
        }
    }
    if !spine.points.is_empty() {
        out.chains.push(spine);
    }
    // Limbs: the type-3 run after the vertebrae.
    let start = vertebrae.len();
    let mut last_ball: HashMap<usize, usize> = HashMap::new();
    for c in start..records.len() {
        let r = &records[c];
        if r.kind != BlockKind::Limb {
            break;
        }
        if !r.uses_skin() {
            continue;
        }
        let parent = (r.parent >= 0).then(|| r.parent as usize).filter(|&p| p < records.len());
        let (start_joint, prev_ball) = match parent.filter(|&p| records[p].kind == BlockKind::Limb) {
            Some(p) => (records[p].bone as u32, last_ball.get(&p).copied()),
            None => ((r.bone - 1).max(0) as u32, None),
        };
        let end = add_limb(&mut out, records, c, start_joint, r.bone as u32, prev_ball);
        last_ball.insert(c, end);
    }
    out.field.finish();
    out
}

/// One skinned limb segment (FUN_004128F0); returns the id of its end ball.
fn add_limb(out: &mut Primitives, records: &[SkinRecord], c: usize, start_joint: u32, end_joint: u32, prev_ball: Option<usize>) -> usize {
    let r = &records[c];
    let s = r.position;
    let e = r.segment_end();
    let segment = r.index as u32;
    let min_r = r.min_ball_scale * BALL_CONNECTOR_RADIUS;
    let next = if r.next_muscle_scale < 0.0 { 1.0 } else { r.next_muscle_scale };
    out.skeleton.joint(start_joint, s);
    out.skeleton.joint(end_joint, e);
    out.skeleton.bone(segment, start_joint, end_joint);
    // The chain this segment belongs to (a limb chain starts at a segment
    // whose parent is not a limb).
    let parent_chain = (r.parent >= 0)
        .then(|| out.block_chain.get(&(r.parent as u32)).copied())
        .flatten()
        .filter(|&ch| out.chains[ch].limb)
        // A branch (a second child of a limb segment: the chain already goes
        // on past its parent) starts a chain of its own.
        .filter(|&ch| records.get(r.parent as usize).is_some_and(|p| out.chains[ch].points.last() == Some(&p.segment_end())));
    let chain = match parent_chain {
        Some(ch) => ch,
        None => {
            let up = if (e - s).normalize_or(Vec3::Z).dot(Vec3::Z).abs() > 0.9 { Vec3::NEG_Y } else { Vec3::Z };
            out.chains.push(Chain { points: vec![s], radii: vec![0.0], limb: true, up });
            out.chains.len() - 1
        }
    };
    out.block_chain.insert(segment, chain);
    let iso = iso_factor(1.0);
    let mut chain_radius = 0.0f32;
    let end_ball = if r.flags & flag::TENTACLE != 0 {
        // Control points: the parent's position (when it has a ball joint),
        // S, E and the child segment's end.
        let mut p0 = match (r.parent >= 0).then(|| &records[r.parent as usize]) {
            Some(p) if p.flags & flag::BALL_JOINT != 0 => p.position,
            _ => s,
        };
        let mut p3 = records
            .iter()
            .skip(c + 1)
            .take_while(|k| k.kind == BlockKind::Limb && k.parent >= c as i16)
            .find(|k| k.parent == c as i16)
            .map_or(e, SkinRecord::segment_end);
        if s.x.abs() < f32::MIN_POSITIVE && e.x.abs() < f32::MIN_POSITIVE {
            p0.x = 0.0;
            p3.x = 0.0;
        }
        let rs = (r.base_muscle_scale * BALL_CONNECTOR_RADIUS).max(min_r);
        let re = (next * BALL_CONNECTOR_RADIUS).max(min_r);
        let mut prev = prev_ball.unwrap_or_else(|| out.field.add(s, 1.0, rs, start_joint | JOINT));
        let steps = TENTACLE_BALLS as f32;
        for i in 1..TENTACLE_BALLS {
            let t = i as f32 / steps;
            let id = out.field.add(catmull_rom(p0, s, e, p3, t), 1.0, rs + (re - rs) * t, segment);
            out.field.link(prev, id, LINK_BLEND, segment);
            prev = id;
        }
        let end = out.field.add(e, 1.0, re, end_joint | JOINT);
        out.field.link(prev, end, LINK_BLEND, segment);
        chain_radius = chain_radius.max(rs.max(re) * iso);
        end
    } else {
        // Muscle limbs need the bake manager's muscle profile tables (not
        // traced); the retail rule for blocks without muscle files is three
        // balls S (3a), midpoint (2.7a), E (2a). ADAPTED: `a` = a quarter of
        // the smaller cross extent of the limb box (FUN_00413D50 not decoded).
        let extent = 0.06 * (r.base_muscle_scale + next) * 0.5 * 2.0;
        let a = (extent * 0.25).max(0.01);
        let first = match prev_ball {
            Some(p) => p,
            None => out.field.add(s, 1.0, 3.0 * a, start_joint | JOINT),
        };
        let mid = out.field.add((s + e) * 0.5, 1.0, 2.7 * a, segment);
        let end = out.field.add(e, 1.0, 2.0 * a, end_joint | JOINT);
        out.field.link(first, mid, LINK_BLEND, segment);
        out.field.link(mid, end, LINK_BLEND, segment);
        chain_radius = chain_radius.max(3.0 * a * iso);
        end
    };
    let ch = &mut out.chains[chain];
    ch.points.push(e);
    ch.radii.push(chain_radius);
    if ch.radii[0] == 0.0 {
        ch.radii[0] = chain_radius;
    }
    end_ball
}

// ---------------------------------------------------------------------------
// Polygonizer
// ---------------------------------------------------------------------------

// Bloomenthal's cube numbering (corner bit 2 = x, bit 1 = y, bit 0 = z), the
// edge-tracing table the retail marcher uses (`DAT_015E2658`).
const LBN: usize = 0;
const LBF: usize = 1;
const LTN: usize = 2;
const LTF: usize = 3;
const RBN: usize = 4;
const RBF: usize = 5;
const RTN: usize = 6;
const RTF: usize = 7;
const FL: u8 = 0;
const FR: u8 = 1;
const FB: u8 = 2;
const FT: u8 = 3;
const FN: u8 = 4;
const FF: u8 = 5;
const LB: usize = 0;
const LT: usize = 1;
const LN: usize = 2;
const LF: usize = 3;
const RB: usize = 4;
const RT: usize = 5;
const RN: usize = 6;
const RF: usize = 7;
const BN: usize = 8;
const BF: usize = 9;
const TN: usize = 10;
const TF: usize = 11;
const CORNER1: [usize; 12] = [LBN, LTN, LBN, LBF, RBN, RTN, RBN, RBF, LBN, LBF, LTN, LTF];
const CORNER2: [usize; 12] = [LBF, LTF, LTN, LTF, RBF, RTF, RTN, RTF, RBN, RBF, RTN, RTF];
const LEFTFACE: [u8; 12] = [FB, FL, FL, FF, FR, FT, FN, FR, FN, FB, FT, FF];
const RIGHTFACE: [u8; 12] = [FL, FT, FN, FL, FB, FR, FR, FF, FB, FF, FN, FT];

fn next_cw_edge(edge: usize, face: u8) -> usize {
    match edge {
        LB => if face == FL { LF } else { BN },
        LT => if face == FL { LN } else { TF },
        LN => if face == FL { LB } else { TN },
        LF => if face == FL { LT } else { BF },
        RB => if face == FR { RN } else { BF },
        RT => if face == FR { RF } else { TN },
        RN => if face == FR { RT } else { BN },
        RF => if face == FR { RB } else { TF },
        BN => if face == FB { RB } else { LN },
        BF => if face == FB { LB } else { RF },
        TN => if face == FT { LT } else { RN },
        _ => if face == FT { RT } else { LF },
    }
}

fn other_face(edge: usize, face: u8) -> u8 {
    if face == LEFTFACE[edge] {
        RIGHTFACE[edge]
    } else {
        LEFTFACE[edge]
    }
}

/// Polygons (lists of cube edges) per corner-sign configuration.
fn cube_table() -> &'static Vec<Vec<Vec<u8>>> {
    static TABLE: OnceLock<Vec<Vec<Vec<u8>>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..256usize)
            .map(|i| {
                let pos = |c: usize| (i >> c) & 1 == 1;
                let mut done = [false; 12];
                let mut polys = Vec::new();
                for e in 0..12 {
                    if done[e] || pos(CORNER1[e]) == pos(CORNER2[e]) {
                        continue;
                    }
                    let mut poly = Vec::new();
                    let mut edge = e;
                    let mut face = if pos(CORNER1[e]) { RIGHTFACE[e] } else { LEFTFACE[e] };
                    for _ in 0..64 {
                        edge = next_cw_edge(edge, face);
                        done[edge] = true;
                        if pos(CORNER1[edge]) != pos(CORNER2[edge]) {
                            poly.push(edge as u8);
                            if edge == e {
                                break;
                            }
                            face = other_face(edge, face);
                        }
                    }
                    polys.push(poly);
                }
                polys
            })
            .collect()
    })
}

/// Why a polygonization attempt failed (the retail retry conditions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshFailure {
    /// A grid axis needs more than 255 (or no) cells.
    Grid,
    /// The edge-crossing cache (0x8000) or 16-bit vertex range overflowed.
    Overflow,
    /// No inside corner found from any primitive (degenerate field).
    NoSeed,
}

/// A polygonized skin in creature space.
#[derive(Clone, Debug, Default)]
pub struct SkinMesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Counter-clockwise seen from outside (creature space is right-handed).
    pub triangles: Vec<[u32; 3]>,
    /// Up to 4 bone (rigblock) indices and normalized weights per vertex.
    pub bones: Vec<[u8; 4]>,
    pub weights: Vec<[f32; 4]>,
    /// Resolution actually used (after retries).
    pub resolution: f32,
}

struct Grid<'a> {
    field: &'a Field,
    res: f32,
    lo: [i32; 3],
    n: [i32; 3],
    corners: HashMap<u32, f32>,
    edges: HashMap<u32, u32>,
    owner: HashMap<u32, u32>,
    sums: Vec<(Vec3, u32)>,
    triangles: Vec<[u32; 3]>,
    overflow: bool,
    /// Walk the cube polygons with retail's tables ([`RetailCube`], the Cell body) instead of
    /// the fans of [`cube_table`] (the Creature).
    retail: bool,
}

impl Grid<'_> {
    fn corner_id(&self, c: [i32; 3]) -> u32 {
        ((c[0] as u32 * (self.n[1] as u32 + 1)) + c[1] as u32) * (self.n[2] as u32 + 1) + c[2] as u32
    }
    fn corner_pos(&self, c: [i32; 3]) -> Vec3 {
        if self.retail {
            // 0x53EB10: `origin + cell x res` with the origin `floor(min / res) x res` of 0x4F96C0, each
            // product and the sum rounded to float.
            let at = |k: usize| self.lo[k] as f32 * self.res + c[k] as f32 * self.res;
            return Vec3::new(at(0), at(1), at(2));
        }
        Vec3::new((self.lo[0] + c[0]) as f32, (self.lo[1] + c[1]) as f32, (self.lo[2] + c[2]) as f32) * self.res
    }
    fn value(&mut self, c: [i32; 3]) -> f32 {
        let id = self.corner_id(c);
        if let Some(v) = self.corners.get(&id) {
            return *v;
        }
        let v = self.field.value(self.corner_pos(c));
        self.corners.insert(id, v);
        v
    }
    fn inside(&mut self, c: [i32; 3]) -> bool {
        self.value(c) >= 0.0
    }

    /// Bisection between an inside and an outside point (FUN_004FC120).
    fn root(&self, mut inside: Vec3, mut outside: Vec3) -> Vec3 {
        let tolerance = 0.001f32.min(0.1 * FIELD_THRESHOLD);
        let mut m = (inside + outside) * 0.5;
        for _ in 0..ROOT_STEPS {
            m = (inside + outside) * 0.5;
            let f = self.field.value(m);
            if f.abs() <= tolerance {
                break;
            }
            if f >= 0.0 {
                inside = m;
            } else {
                outside = m;
            }
        }
        m
    }

    /// Retail's bisection (0x4FC120, the walk's Cell path): the parameter `t` of `first + (second - first) t`
    /// is halved (`t = (lo + hi) 0.5`, `f < 0` moves `hi`, else `lo`) up to 20 times until `|f|` is within the
    /// tolerance; the point is `first + dir t` with the vector products of 0x41DC60 / 0x41DBD0.
    fn root_retail(&self, first: Vec3, second: Vec3) -> Vec3 {
        let tolerance = 0.001f32.min(0.1 * FIELD_THRESHOLD);
        let dir = second - first;
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        let mut p = first;
        for _ in 0..ROOT_STEPS {
            let t = (lo + hi) * 0.5;
            p = first + dir * t;
            let f = self.field.value(p);
            if f < 0.0 {
                hi = t;
            } else {
                lo = t;
            }
            if f.abs() <= tolerance {
                break;
            }
        }
        p
    }

    /// The vertex of the crossing on edge (a inside, b outside) (FUN_0053F960):
    /// the half of the edge holding the crossing names the owning corner, all
    /// crossings of one corner merge into one vertex.
    fn edge_vertex(&mut self, a: [i32; 3], b: [i32; 3]) -> u32 {
        let (ia, ib) = (self.corner_id(a), self.corner_id(b));
        let axis = (0..3).find(|&k| a[k] != b[k]).unwrap_or(0) as u32;
        let key = ia.min(ib) * 3 + axis;
        if let Some(&v) = self.edges.get(&key) {
            return v;
        }
        if self.edges.len() >= EDGE_CACHE {
            self.overflow = true;
        }
        let (pa, pb) = (self.corner_pos(a), self.corner_pos(b));
        let mid = (pa + pb) * 0.5;
        let (p, owner) = if self.field.value(mid) >= 0.0 {
            (if self.retail { self.root_retail(mid, pb) } else { self.root(mid, pb) }, ib)
        } else {
            (if self.retail { self.root_retail(pa, mid) } else { self.root(pa, mid) }, ia)
        };
        let v = match self.owner.get(&owner) {
            Some(&v) => {
                let s = &mut self.sums[v as usize];
                s.0 += p;
                s.1 += 1;
                v
            }
            None => {
                let v = self.sums.len() as u32;
                if v > 0xFFFF {
                    self.overflow = true;
                }
                self.sums.push((p, 1));
                self.owner.insert(owner, v);
                v
            }
        };
        self.edges.insert(key, v);
        v
    }

    fn cube(&mut self, c: [i32; 3]) -> u8 {
        let mut index = 0u8;
        for k in 0..8 {
            let p = [c[0] + ((k >> 2) & 1), c[1] + ((k >> 1) & 1), c[2] + (k & 1)];
            if self.inside(p) {
                index |= 1 << k;
            }
        }
        index
    }

    /// Emit the polygons of one cube as fans, skipping collapsed triangles
    /// (FUN_0053FE30).
    fn polygonize(&mut self, c: [i32; 3], index: u8) {
        let corner = |k: usize| [c[0] + ((k >> 2) & 1) as i32, c[1] + ((k >> 1) & 1) as i32, c[2] + (k & 1) as i32];
        for poly in &cube_table()[index as usize] {
            let (mut a, mut b) = (u32::MAX, u32::MAX);
            for &e in poly {
                let (c1, c2) = (corner(CORNER1[e as usize]), corner(CORNER2[e as usize]));
                let (inside, outside) = if (index >> CORNER1[e as usize]) & 1 == 1 { (c1, c2) } else { (c2, c1) };
                let v = self.edge_vertex(inside, outside);
                if a == u32::MAX {
                    a = v;
                } else if b == u32::MAX {
                    b = v;
                } else {
                    if v != a && v != b && a != b {
                        self.triangles.push([a, b, v]);
                    }
                    b = v;
                }
            }
        }
    }
}

/// One polygonization attempt at `res` (FUN_005400A0).
pub fn polygonize_once(field: &Field, res: f32) -> Result<SkinMesh, MeshFailure> {
    polygonize_once_with(field, res, false)
}

/// [`polygonize_once`] with the Creature's polygon fans (`retail` false) or retail's own polygon
/// walk of 0x53FE30 (`retail` true, [`polygonize_retail`]).
fn polygonize_once_with(field: &Field, res: f32, retail: bool) -> Result<SkinMesh, MeshFailure> {
    if field.balls.is_empty() {
        return Ok(SkinMesh { resolution: res, ..Default::default() });
    }
    let (min, max) = field.bounds();
    let lo = [(min.x / res).floor() as i32, (min.y / res).floor() as i32, (min.z / res).floor() as i32];
    let hi = [(max.x / res).ceil() as i32, (max.y / res).ceil() as i32, (max.z / res).ceil() as i32];
    let n = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    if n.iter().any(|&k| !(1..=MAX_AXIS_CELLS).contains(&k)) {
        return Err(MeshFailure::Grid);
    }
    let mut g = Grid {
        field,
        res,
        lo,
        n,
        corners: HashMap::new(),
        edges: HashMap::new(),
        owner: HashMap::new(),
        sums: Vec::new(),
        triangles: Vec::new(),
        overflow: false,
        retail,
    };
    // Seed (FUN_0053E690): from the first ball whose cell has an inside
    // corner, walk +X to the surface. Retail's step is the packed cell (1, 0, 0) of
    // 0x53E9A6, whose first byte is Z (0x53EB10): its walk runs along +Z, and the
    // seed cube starts at the last inside corner (the Creature keeps +X).
    let walk = if retail { 2 } else { 0 };
    let mut seed = None;
    'balls: for b in &field.balls {
        let cell = [
            ((b.center.x / res).floor() as i32 - lo[0]).clamp(0, n[0] - 1),
            ((b.center.y / res).floor() as i32 - lo[1]).clamp(0, n[1] - 1),
            ((b.center.z / res).floor() as i32 - lo[2]).clamp(0, n[2] - 1),
        ];
        for k in 0..8 {
            let mut p = [cell[0] + ((k >> 2) & 1), cell[1] + ((k >> 1) & 1), cell[2] + (k & 1)];
            if !g.inside(p) {
                continue;
            }
            for _ in 0..SEED_STEPS {
                if p[walk] >= n[walk] {
                    break;
                }
                p[walk] += 1;
                if !g.inside(p) {
                    p[walk] -= 1;
                    seed = Some([p[0].min(n[0] - 1), p[1].min(n[1] - 1), p[2].min(n[2] - 1)]);
                    break 'balls;
                }
            }
        }
    }
    let Some(seed) = seed else {
        return Err(MeshFailure::NoSeed);
    };
    // Surface following over face neighbours with a sign change. The Creature's order is -x, +x, -y,
    // +y, -z, +z; retail's queue (0x53EDA0) visits the faces in the order of the face table 0x53D2B0
    // builds, whose first stored byte is Z: -z, +z, -y, +y, -x, +x (this decides the order of the
    // cubes, hence of the vertices and triangles).
    const FACES: [([usize; 4], [i32; 3]); 6] = [
        ([0, 1, 2, 3], [-1, 0, 0]),
        ([4, 5, 6, 7], [1, 0, 0]),
        ([0, 1, 4, 5], [0, -1, 0]),
        ([2, 3, 6, 7], [0, 1, 0]),
        ([0, 2, 4, 6], [0, 0, -1]),
        ([1, 3, 5, 7], [0, 0, 1]),
    ];
    const FACES_RETAIL: [([usize; 4], [i32; 3]); 6] = [
        ([0, 2, 4, 6], [0, 0, -1]),
        ([1, 3, 5, 7], [0, 0, 1]),
        ([0, 1, 4, 5], [0, -1, 0]),
        ([2, 3, 6, 7], [0, 1, 0]),
        ([0, 1, 2, 3], [-1, 0, 0]),
        ([4, 5, 6, 7], [1, 0, 0]),
    ];
    let faces = if retail { FACES_RETAIL } else { FACES };
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(seed);
    queue.push_back(seed);
    while let Some(c) = queue.pop_front() {
        let index = g.cube(c);
        if index == 0 || index == 255 {
            continue;
        }
        if retail {
            g.polygonize_retail(c, index);
        } else {
            g.polygonize(c, index);
        }
        if g.overflow {
            return Err(MeshFailure::Overflow);
        }
        for (corners, d) in faces {
            let bits: Vec<bool> = corners.iter().map(|&k| (index >> k) & 1 == 1).collect();
            if bits.iter().all(|&b| b) || bits.iter().all(|&b| !b) {
                continue;
            }
            let nc = [c[0] + d[0], c[1] + d[1], c[2] + d[2]];
            if (0..3).any(|k| nc[k] < 0 || nc[k] >= n[k]) {
                continue;
            }
            if visited.insert(nc) {
                queue.push_back(nc);
            }
        }
    }
    // Finalize (FUN_0053F4E0): average the merged crossings; normals from the
    // analytic gradient, re-projected only where it vanishes (FUN_004FBCE0).
    let mut mesh = SkinMesh { resolution: res, ..Default::default() };
    for (sum, count) in &g.sums {
        // Retail multiplies the sum by the float reciprocal of the count (0x53F4E0: `fild; fld1; fdivrp`, 0x41DB60).
        let mut p = if retail { *sum * (1.0 / *count as f32) } else { *sum / *count as f32 };
        let mut grad = field.gradient(p);
        if grad.length_squared() <= 1e-8 {
            p = field.project(p).0;
            grad = field.gradient(p);
        }
        mesh.positions.push(p);
        mesh.normals.push((-grad).normalize_or(Vec3::Z));
    }
    // Orientation: counter-clockwise around the outward normal. The retail walk emits its triangles
    // in the order of the table walk (0x53FE30 appends `v0, v1, v` as they come: no flip step).
    if !retail {
        for t in &mut g.triangles {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            let n = mesh.normals[t[0] as usize] + mesh.normals[t[1] as usize] + mesh.normals[t[2] as usize];
            if (b - a).cross(c - a).dot(n) < 0.0 {
                t.swap(1, 2);
            }
        }
    }
    mesh.triangles = g.triangles;
    Ok(mesh)
}

/// Polygonize with the retail retry (FUN_004142D0): `res * 1.2` while the
/// attempt fails and `res < 0.5`.
pub fn polygonize(field: &Field, res: f32) -> Result<SkinMesh, MeshFailure> {
    polygonize_with(field, res, false)
}

fn polygonize_with(field: &Field, res: f32, retail: bool) -> Result<SkinMesh, MeshFailure> {
    let mut res = res;
    loop {
        match polygonize_once_with(field, res, retail) {
            Ok(m) => return Ok(m),
            Err(MeshFailure::NoSeed) => return Err(MeshFailure::NoSeed),
            Err(e) => {
                res *= RETRY_FACTOR;
                if res >= MAX_RES {
                    return Err(e);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Retail marching-cubes polygon walk (the Cell body; helper `cell-mcubes`)
// ---------------------------------------------------------------------------
//
// The retail marcher (0x53EDA0, called from 0x5400A0) hands every surface cube to 0x53FE30
// with the cube's base cell and an 8-bit corner mask. The tables it reads live in the data
// segment at 0x15E2658 .. 0x15E2D08 and are filled at run time by the constructor 0x53D2B0
// (the code image holds garbage there); [`RetailCube`] rebuilds them the same way from the one
// piece of real data, the edge / corner table at 0x150CB8C.
//
// A cell is packed in a dword of three bytes (b0, b1, b2) and 0x53EB10 turns it into a position
// with b0 = Z, b1 = Y, b2 = X, so retail's corner numbering `k = b0 | b1 << 1 | b2 << 2` (the offsets
// at 0x15E2D08, built by 0x53E190) is `k = x << 2 | y << 1 | z`: the crate's own numbering
// ([`cube_table`]). Retail's corner mask has bit `k` set when the field is NEGATIVE there (0x53EDA0:
// `0 > value`), i.e. outside this module's `f >= 0` convention (the crate's masks are bit set =
// inside).

/// `.rdata` 0x150CB8C: the corner pair of every cube edge (edge `e`: corners `[2e]`, `[2e + 1]`).
/// ORIGINAL data (read from the code image).
const RETAIL_EDGE_CORNERS: [u8; 24] = [0, 4, 4, 6, 2, 6, 0, 2, 0, 1, 4, 5, 6, 7, 2, 3, 1, 5, 5, 7, 3, 7, 1, 3];

/// The six cube faces in the order 0x53D2B0 builds them (the 0x53E1D0 constructor arguments; face `f` is
/// the `b0`, `b1`, `b2` = 0, 1 face for `f` = 0, 1 ...: -z, +z, -y, +y, -x, +x): the four corner offsets
/// `(b0, b1, b2)` of each, in the stored order.
const RETAIL_FACES: [[[u8; 3]; 4]; 6] = [
    [[0, 0, 0], [0, 1, 0], [0, 1, 1], [0, 0, 1]],
    [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]],
    [[0, 0, 0], [0, 0, 1], [1, 0, 1], [1, 0, 0]],
    [[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]],
    [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]],
    [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
];

/// Retail's cube tables (constructor 0x53D2B0), addresses SDK / march2017. The tables are indexed by
/// corner `k = x << 2 | y << 1 | z` (see above); a face's corner list is in stored (b0, b1, b2) order.
///
/// A *directed edge* is `2 e + dir`: edge `e` (0 .. 11) walked from corner `corner[d]` to
/// corner `corner[d ^ 1]` (the table 0x15E28E8 holds the id of every corner pair).
pub struct RetailCube {
    /// 0x15E2658: for every corner mask the edges whose two corners differ (bit `e` = edge `e`).
    pub mask: [u16; 256],
    /// 0x15E2858: the corner bit (`1 << k`) at the start of every directed edge's pair.
    pub corner_bit: [u8; 24],
    /// 0x15E28D0: the start corner of every directed edge.
    pub corner: [u8; 24],
    /// 0x15E2928 / 0x15E25F8 / 0x15E2870 (each word = edge bit, byte at +2 = directed id): per
    /// directed edge `I0 -> I1` of a face, the edges the walk tries next in this order: the
    /// edge `I2 -> I1` reversed, `I3 -> I2` reversed, `I0 -> I3` (the last is taken unchecked).
    pub next: [[(u16, u8); 3]; 24],
}

fn build_retail_cube() -> RetailCube {
    let ec = RETAIL_EDGE_CORNERS;
    // 0x15E28E8: directed edge id of corner a -> corner b (and `^ 1` backwards).
    let mut pair = [0xFFu8; 64];
    for i in 0..12 {
        let (a, b) = (ec[2 * i] as usize, ec[2 * i + 1] as usize);
        let v = if a < b { 2 * i } else { 2 * i + 1 } as u8;
        pair[a * 8 + b] = v;
        pair[b * 8 + a] = v ^ 1;
    }
    let mut corner_bit = [0u8; 24];
    for i in 0..12 {
        corner_bit[2 * i] = 1 << ec[2 * i];
        corner_bit[2 * i + 1] = 1 << ec[2 * i + 1];
    }
    // 0x15E28D0[i] = table[(i & 1) + (i >> 1) * 2] = table[i].
    let corner = ec;
    // The faces (0x53E1D0), walked the other way round on every second face (0x53DC26: `s`).
    let mut next = [[(0u16, 0u8); 3]; 24];
    for (f, face) in RETAIL_FACES.iter().enumerate() {
        let s: i32 = if f & 1 == 1 { 1 } else { -1 };
        let corner_of = |i: i32| {
            let c = face[i.rem_euclid(4) as usize];
            c[0] as usize | (c[1] as usize) << 1 | (c[2] as usize) << 2
        };
        for j in 0..4i32 {
            let (c0, c1, c2, c3) = (corner_of(j), corner_of(j + s), corner_of(j + 2 * s), corner_of(j + 3 * s));
            let e01 = pair[c0 * 8 + c1] as usize;
            let (e21, e32, e03) = (pair[c2 * 8 + c1], pair[c3 * 8 + c2], pair[c0 * 8 + c3]);
            next[e01] = [(1 << (e21 >> 1), e21), (1 << (e32 >> 1), e32), (1 << (e03 >> 1), e03)];
        }
    }
    // 0x15E2658: edges with exactly one of their two corners in the mask.
    let mut mask = [0u16; 256];
    for (i, m) in mask.iter_mut().enumerate() {
        for e in 0..12 {
            if (corner_bit[2 * e] as usize & i != 0) != (corner_bit[2 * e + 1] as usize & i != 0) {
                *m |= 1 << e;
            }
        }
    }
    RetailCube { mask, corner_bit, corner, next }
}

/// The tables of [`RetailCube`], built once.
pub fn retail_cube() -> &'static RetailCube {
    static CUBE: OnceLock<RetailCube> = OnceLock::new();
    CUBE.get_or_init(build_retail_cube)
}

/// 0x53FE30 on a corner mask in retail's convention (bit `k` set = field negative = outside at corner
/// `k`): the polygons of the cube as directed-edge walks. Each walk starts at the lowest edge
/// still in the mask (directed from the inside corner to the outside one) and follows
/// [`RetailCube::next`] until it returns to its first directed edge.
pub fn retail_polygons(retail_index: u8) -> Vec<Vec<u8>> {
    let t = retail_cube();
    let mut mask = t.mask[retail_index as usize];
    let mut polys = Vec::new();
    while mask != 0 {
        let e = mask.trailing_zeros() as usize;
        let start = if retail_index & t.corner_bit[2 * e] != 0 { 2 * e + 1 } else { 2 * e };
        let mut poly = Vec::new();
        let mut cur = start;
        for _ in 0..64 {
            poly.push(cur as u8);
            let n = &t.next[cur];
            let (bit, dir) = if n[0].0 & mask != 0 {
                n[0]
            } else if n[1].0 & mask != 0 {
                n[1]
            } else {
                n[2]
            };
            mask ^= bit;
            cur = dir as usize;
            if cur == start {
                break;
            }
        }
        polys.push(poly);
    }
    polys
}

impl Grid<'_> {
    /// Emit the polygons of one cube with retail's walk (0x53FE30): each polygon is walked from
    /// its lowest edge and fanned from its FIRST vertex as the vertices come (`v0, v1, v`),
    /// collapsed triangles skipped, the triangles kept in the walk's own winding. `index` is the
    /// crate's corner mask (bit set = inside, [`Grid::cube`]).
    fn polygonize_retail(&mut self, c: [i32; 3], index: u8) {
        let t = retail_cube();
        // Retail's mask has the bit set where the field is negative (outside).
        let retail_index = !index;
        let corner = |r: u8| [c[0] + ((r >> 2) & 1) as i32, c[1] + ((r >> 1) & 1) as i32, c[2] + (r & 1) as i32];
        let mut mask = t.mask[retail_index as usize];
        while mask != 0 {
            let e = mask.trailing_zeros() as usize;
            let start = if retail_index & t.corner_bit[2 * e] != 0 { 2 * e + 1 } else { 2 * e };
            let mut cur = start;
            let (mut v0, mut v1) = (u32::MAX, u32::MAX);
            for _ in 0..64 {
                // The directed edge runs from the inside corner to the outside corner (0x53F960).
                let v = self.edge_vertex(corner(t.corner[cur]), corner(t.corner[cur ^ 1]));
                if v0 == u32::MAX {
                    v0 = v;
                } else if v1 == u32::MAX {
                    v1 = v;
                } else {
                    if v != v1 && v != v0 && v1 != v0 {
                        self.triangles.push([v0, v1, v]);
                    }
                    v1 = v;
                }
                let n = &t.next[cur];
                let (bit, dir) = if n[0].0 & mask != 0 {
                    n[0]
                } else if n[1].0 & mask != 0 {
                    n[1]
                } else {
                    n[2]
                };
                mask ^= bit;
                cur = dir as usize;
                if cur == start {
                    break;
                }
            }
        }
    }
}

/// The skin polygonizer with retail's own polygon walk ([`RetailCube`], 0x53FE30), for the Cell
/// body: the same marcher, vertex merging and retry as [`polygonize`], but every cube polygon
/// is walked and fanned the way retail does, so the triangles (and their vertex order) are the
/// retail skin's. The Creature keeps [`polygonize`].
pub fn polygonize_retail(field: &Field, res: f32) -> Result<SkinMesh, MeshFailure> {
    polygonize_with(field, res, true)
}

// ---------------------------------------------------------------------------
// Bone weights (FUN_004CB3B0 -> FUN_0052B340)
// ---------------------------------------------------------------------------

/// Weights for every vertex: per group contribution; a segment's share goes
/// `2t` to its bone and the rest equally to the bones at the near joint, a
/// joint ball's share equally to the joint's bones; the 4 largest kept and
/// normalized (`skinnerWeightSpineTuning` and `skinnerWeightSmoothing` are 0).
pub fn bone_weights(prims: &Primitives, mesh: &mut SkinMesh) {
    let mut contrib = Vec::new();
    mesh.bones.clear();
    mesh.weights.clear();
    for &v in &mesh.positions {
        prims.field.contributions(v, &mut contrib);
        let mut acc: Vec<(u32, f32)> = Vec::new();
        let mut add = |bone: u32, w: f32| {
            if bone >= MAX_BONES || w <= 0.0 {
                return;
            }
            match acc.iter_mut().find(|e| e.0 == bone) {
                Some(e) => e.1 += w,
                None => acc.push((bone, w)),
            }
        };
        for &(group, c) in &contrib {
            if group & JOINT == 0 {
                let Some(bone) = prims.skeleton.bones.get(&group) else {
                    add(group, c);
                    continue;
                };
                let a = prims.skeleton.joints.get(&bone.joint_a).map_or(v, |j| j.0);
                let b = prims.skeleton.joints.get(&bone.joint_b).map_or(v, |j| j.0);
                let ab = b - a;
                let t = if ab.length_squared() > 0.0 { ((v - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
                let (near, w) = if t > 0.5 { (bone.joint_b, 2.0 * (1.0 - t)) } else { (bone.joint_a, 2.0 * t) };
                add(group, c * w);
                let rest = c * (1.0 - w);
                if let Some((_, bones)) = prims.skeleton.joints.get(&near) {
                    if !bones.is_empty() && rest > 0.0 {
                        for &bb in bones {
                            add(bb, rest / bones.len() as f32);
                        }
                    }
                }
            } else if let Some((_, bones)) = prims.skeleton.joints.get(&(group & !JOINT)) {
                for &bb in bones {
                    add(bb, c / bones.len().max(1) as f32);
                }
            }
        }
        acc.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        acc.truncate(4);
        let sum: f32 = acc.iter().map(|e| e.1).sum();
        let mut bones = [0u8; 4];
        let mut weights = [0f32; 4];
        if sum > 0.0 {
            for (k, (b, w)) in acc.iter().enumerate() {
                bones[k] = *b as u8;
                weights[k] = w / sum;
            }
        } else {
            weights[0] = 1.0;
        }
        mesh.bones.push(bones);
        mesh.weights.push(weights);
    }
}

/// Records -> primitives -> mesh (+ weights) at `res` (with the retry).
pub fn build(records: &[SkinRecord], res: f32) -> Result<(Primitives, SkinMesh), MeshFailure> {
    let prims = primitives(records);
    let mut mesh = polygonize(&prims.field, res)?;
    bone_weights(&prims, &mut mesh);
    Ok((prims, mesh))
}

/// [`build`] with retail's own polygon walk ([`polygonize_retail`]): the Cell body's entry point
/// (the Creature keeps [`build`]).
pub fn build_retail(records: &[SkinRecord], res: f32) -> Result<(Primitives, SkinMesh), MeshFailure> {
    let prims = primitives(records);
    let mut mesh = polygonize_retail(&prims.field, res)?;
    bone_weights(&prims, &mut mesh);
    Ok((prims, mesh))
}

// ---------------------------------------------------------------------------
// UV unwrap (ADAPTED)
// ---------------------------------------------------------------------------

/// Texture size of the skin paint (`skinpaintTextureSize`, `max(64, 512)`).
pub const PAINT_SIZE: u32 = 512;
/// ADAPTED: empty texels around each chart (the retail colour dilate fills
/// 8 texels).
const GUTTER: f32 = 3.0;

/// Where a skin point sits on its chain.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChainParam {
    /// Arc length along the chain (negative / beyond the length on the caps).
    pub s: f32,
    /// Angle around the chain, 0 = the chain's up, + = the side `T x U`.
    pub theta: f32,
    /// Chain length.
    pub length: f32,
}

/// Parameterize `v` on `chain`: closest polyline point, caps unrolled by angle.
pub fn chain_param(chain: &Chain, v: Vec3) -> ChainParam {
    let pts = &chain.points;
    if pts.len() < 2 {
        let d = v - pts.first().copied().unwrap_or(Vec3::ZERO);
        return ChainParam { s: d.dot(Vec3::Y), theta: d.x.atan2(d.z), length: 0.0 };
    }
    let mut cum = 0.0;
    let mut best = (f32::MAX, 0usize, 0.0f32, 0.0f32, 0.0f32);
    for k in 0..pts.len() - 1 {
        let d = pts[k + 1] - pts[k];
        let len2 = d.length_squared().max(1e-12);
        let raw = (v - pts[k]).dot(d) / len2;
        let t = raw.clamp(0.0, 1.0);
        let dist = (pts[k] + d * t - v).length_squared();
        if dist < best.0 {
            best = (dist, k, t, raw, cum);
        }
        cum += len2.sqrt();
    }
    let length = cum;
    let (_, k, t, raw, before) = best;
    let d = pts[k + 1] - pts[k];
    let len = d.length().max(1e-6);
    let tangent = d / len;
    let (center, s) = if k == 0 && raw < 0.0 {
        let rel = v - pts[0];
        let axial = -rel.dot(tangent);
        let radial = (rel + tangent * axial).length();
        (pts[0], -axial.atan2(radial) * rel.length())
    } else if k == pts.len() - 2 && raw > 1.0 {
        let rel = v - pts[k + 1];
        let axial = rel.dot(tangent);
        let radial = (rel - tangent * axial).length();
        (pts[k + 1], length + axial.atan2(radial) * rel.length())
    } else {
        (pts[k] + d * t, before + t * len)
    };
    let up = (chain.up - tangent * chain.up.dot(tangent)).normalize_or(Vec3::X);
    let side = tangent.cross(up);
    let rel = v - center;
    ChainParam { s, theta: rel.dot(side).atan2(rel.dot(up)), length }
}

/// Direction of the chain segment closest to `v` (head to tail for the spine).
pub fn chain_tangent(chain: &Chain, v: Vec3) -> Vec3 {
    let pts = &chain.points;
    if pts.len() < 2 {
        return Vec3::Y;
    }
    let mut best = (f32::MAX, Vec3::Y);
    for k in 0..pts.len() - 1 {
        let d = pts[k + 1] - pts[k];
        let t = ((v - pts[k]).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0);
        let dist = (pts[k] + d * t - v).length_squared();
        if dist < best.0 {
            best = (dist, d.normalize_or(Vec3::Y));
        }
    }
    best.1
}

/// A UV chart: chain and side (0 = theta >= 0, 1 = theta < 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChartId {
    pub chain: usize,
    pub side: u8,
}

/// Chart coordinates of a chain parameter (continuous across the seams of the
/// chart's own side).
fn chart_xy(p: ChainParam, side: u8, radius: f32) -> (f32, f32) {
    use std::f32::consts::{FRAC_PI_2, TAU};
    let theta = if side == 0 {
        if p.theta < -FRAC_PI_2 { p.theta + TAU } else { p.theta }
    } else if p.theta > FRAC_PI_2 {
        TAU - p.theta
    } else {
        -p.theta
    };
    (p.s, theta * radius)
}

/// The unwrapped render mesh: vertices split per chart, UVs in 0..1.
#[derive(Clone, Debug, Default)]
pub struct Unwrapped {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<[f32; 2]>,
    /// Source vertex of each split vertex.
    pub source: Vec<u32>,
    pub triangles: Vec<[u32; 3]>,
    /// Chart per split vertex.
    pub chart: Vec<ChartId>,
    pub charts: Vec<ChartId>,
    /// Texels per creature unit.
    pub texel_density: f32,
}

/// ADAPTED unwrap (the Creature's; the Cell body uses [`unwrap_retail`], retail's own
/// flood and packing, which the Creature editor runs too: follow-up): every triangle goes
/// to the chart of its dominant chain and
/// side; charts are packed in shelves at one texel density into the 512^2
/// paint texture.
pub fn unwrap(prims: &Primitives, mesh: &SkinMesh) -> Unwrapped {
    let mut out = Unwrapped::default();
    if mesh.triangles.is_empty() || prims.chains.is_empty() {
        return out;
    }
    let chain_of = |v: usize| -> usize {
        mesh.bones
            .get(v)
            .and_then(|b| prims.block_chain.get(&(b[0] as u32)).copied())
            .unwrap_or(0)
            .min(prims.chains.len() - 1)
    };
    let radius: Vec<f32> = prims.chains.iter().map(|c| c.radii.iter().copied().fold(0.01, f32::max)).collect();
    // Triangle -> chart.
    let mut tri_chart = Vec::with_capacity(mesh.triangles.len());
    for t in &mesh.triangles {
        let chains = t.map(|i| chain_of(i as usize));
        let chain = if chains[1] == chains[2] { chains[1] } else { chains[0] };
        let centroid = t.iter().map(|&i| mesh.positions[i as usize]).sum::<Vec3>() / 3.0;
        let p = chain_param(&prims.chains[chain], centroid);
        tri_chart.push(ChartId { chain, side: if p.theta >= 0.0 { 0 } else { 1 } });
    }
    // Split vertices per chart and collect chart coordinates.
    let mut split: HashMap<(u32, ChartId), u32> = HashMap::new();
    let mut xy: Vec<(f32, f32)> = Vec::new();
    for (t, chart) in mesh.triangles.iter().zip(&tri_chart) {
        let mut tri = [0u32; 3];
        for (k, &v) in t.iter().enumerate() {
            tri[k] = *split.entry((v, *chart)).or_insert_with(|| {
                let p = chain_param(&prims.chains[chart.chain], mesh.positions[v as usize]);
                xy.push(chart_xy(p, chart.side, radius[chart.chain]));
                out.positions.push(mesh.positions[v as usize]);
                out.normals.push(mesh.normals[v as usize]);
                out.source.push(v);
                out.chart.push(*chart);
                out.positions.len() as u32 - 1
            });
        }
        out.triangles.push(tri);
    }
    // Chart rectangles.
    let mut rects: Vec<(ChartId, f32, f32, f32, f32)> = Vec::new();
    for (i, c) in out.chart.iter().enumerate() {
        let (x, y) = xy[i];
        match rects.iter_mut().find(|r| r.0 == *c) {
            Some(r) => {
                r.1 = r.1.min(x);
                r.2 = r.2.min(y);
                r.3 = r.3.max(x);
                r.4 = r.4.max(y);
            }
            None => rects.push((*c, x, y, x, y)),
        }
    }
    rects.sort_by(|a, b| (b.4 - b.2).total_cmp(&(a.4 - a.2)).then((a.0.chain, a.0.side).cmp(&(b.0.chain, b.0.side))));
    let size = PAINT_SIZE as f32;
    let pack = |k: f32| -> Option<Vec<(f32, f32)>> {
        let (mut x, mut y, mut row) = (0.0f32, 0.0f32, 0.0f32);
        let mut at = Vec::new();
        for r in &rects {
            let (w, h) = ((r.3 - r.1) * k + 2.0 * GUTTER, (r.4 - r.2) * k + 2.0 * GUTTER);
            if w > size || h > size {
                return None;
            }
            if x + w > size {
                x = 0.0;
                y += row;
                row = 0.0;
            }
            if y + h > size {
                return None;
            }
            at.push((x + GUTTER, y + GUTTER));
            x += w;
            row = row.max(h);
        }
        Some(at)
    };
    let (mut lo, mut hi) = (1.0f32, 100_000.0f32);
    for _ in 0..40 {
        let mid = (lo * hi).sqrt();
        if pack(mid).is_some() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let k = lo.floor().max(1.0);
    let at = pack(k).unwrap_or_else(|| vec![(GUTTER, GUTTER); rects.len()]);
    out.texel_density = k;
    out.charts = rects.iter().map(|r| r.0).collect();
    out.uvs = out
        .chart
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let r = rects.iter().position(|r| r.0 == *c).unwrap();
            let (x, y) = xy[i];
            [(at[r].0 + (x - rects[r].1) * k) / size, (at[r].1 + (y - rects[r].2) * k) / size]
        })
        .collect();
    out
}

// ---------------------------------------------------------------------------
// Retail chart flood and atlas packing (the Cell body; helper `cell-seams`)
// ---------------------------------------------------------------------------
//
// `cEditor` 0x4CB890 (the editor's skin finish step) runs, on the skin output
// object (tag 'Skin'): adjacency 0x5094F0 (edge neighbours of the corners by
// the sorted pair of vertex indices; 0x509940 pairs the corners of non-manifold
// edges), the reset 0x509BC0, then mode 2 = the chart flood 0x50A140, the
// packing 0x50AB80(0.006), the tangents 0x508850 and mode 3 = 0x509070 (chart
// edge lists, not needed for the geometry). Everything below follows that
// code (decode trail: `cell-seams\re\`, notes\cell-body-notes.md). The Creature
// path keeps [`unwrap`]; the Cell body calls [`unwrap_retail`].

/// Padding added to every chart rectangle before it is packed (the argument
/// 0x4CB890 passes to 0x50AB80, `.rdata` 0x13F0574).
pub const RETAIL_CHART_PAD: f32 = 0.006;
/// A packing row is filled up to this factor times the square root of the total
/// chart area (0x50AB80, `.rdata` 0x13EF52C).
const RETAIL_ROW_SLACK: f32 = 1.1;
/// Edge-neighbour value of a corner without a neighbour (boundary).
const NO_NEIGHBOUR: u32 = u32::MAX;

/// A chart of the flood (the 0x94-byte `Skin` / `Edit` object: +0x00 index,
/// +0x04 group key, +0x08 triangles, +0x58 / +0x5C / +0x60 / +0x64 the UV
/// rectangle minU / maxU / minV / maxV, +0x68 bit 0 = turned a quarter by the
/// packing).
#[derive(Clone, Debug, Default)]
struct RetailChart {
    id: u32,
    key: u8,
    tris: Vec<u32>,
    min_u: f32,
    max_u: f32,
    min_v: f32,
    max_v: f32,
    turned: bool,
}

impl RetailChart {
    fn empty(id: u32, key: u8) -> Self {
        // 0x50A3E5: the rectangle starts inverted (FLT_MAX / -FLT_MAX).
        RetailChart { id, key, tris: Vec::new(), min_u: f32::MAX, max_u: -f32::MAX, min_v: f32::MAX, max_v: -f32::MAX, turned: false }
    }
    /// An `Edit` rectangle of a part rigblock (0x4CB890: (0, 0) .. (s, s)).
    fn part(id: u32, size: f32) -> Self {
        RetailChart { id, key: 0, tris: Vec::new(), min_u: 0.0, max_u: size, min_v: 0.0, max_v: size, turned: false }
    }
    /// 0x509E40: grow the rectangle by a UV.
    fn grow(&mut self, uv: [f32; 2]) {
        self.min_u = if self.min_u > uv[0] { uv[0] } else { self.min_u };
        self.max_u = if uv[0] > self.max_u { uv[0] } else { self.max_u };
        self.min_v = if self.min_v > uv[1] { uv[1] } else { self.min_v };
        self.max_v = if uv[1] > self.max_v { uv[1] } else { self.max_v };
    }
    fn width(&self) -> f32 {
        self.max_u - self.min_u
    }
    fn height(&self) -> f32 {
        self.max_v - self.min_v
    }
    /// 0x50BD40: turn the rectangle (swap the U and V bounds, toggle the flag).
    fn turn(&mut self) {
        self.turned = !self.turned;
        std::mem::swap(&mut self.max_u, &mut self.max_v);
        std::mem::swap(&mut self.min_u, &mut self.min_v);
    }
}

/// Group key of a triangle (0x50A7D0): the face normal `(p1 - p0) x (p2 - p0)`'s
/// dominant axis times two, plus one when that component is negative (six
/// directions, a cube map).
fn retail_group_key(p: [Vec3; 3]) -> u8 {
    let n = (p[1] - p[0]).cross(p[2] - p[0]);
    let a = n.abs();
    // 0x4FDE40: strict comparisons, ties go to the later axis.
    let axis = if a.x > a.y {
        if a.x > a.z { 0 } else { 2 }
    } else if a.y > a.z {
        1
    } else {
        2
    };
    2 * axis as u8 + u8::from(n[axis] < 0.0)
}

/// Planar projection of a point for a group key (0x509F60): drop the key's axis,
/// U = the next axis (negated for a negative direction), V = the one after.
fn retail_project(p: Vec3, key: u8) -> [f32; 2] {
    let axis = (key >> 1) as usize;
    let u = p[(axis + 1) % 3];
    let v = p[(axis + 2) % 3];
    [if key & 1 != 0 { -u } else { u }, v]
}

/// Edge neighbours of the corners (0x5094F0 + 0x509940): corner `3 t + k` starts the edge
/// `v[k] -> v[k + 1]` of triangle `t`; the edges are registered by the sorted vertex pair in the
/// order corner 2, 0, 1 of every triangle. The first corner to meet a pair points to itself, the
/// second becomes its neighbour (mutually, whatever the orientation), a third and later corner is
/// inserted into the ring behind the first (`adj[first] = new`, `adj[new] = old adj[first]`), and
/// the corner that closes a ring of three is put on the non-manifold list. 0x509940 then walks
/// each listed ring (starting at the listed corner, following the neighbours), pairs the first
/// corner of the ring with the first later one whose edge runs the other way (`v[c] < v[c + 1]`
/// differs), removes both and repeats; what is left points to itself. A corner pointing to itself
/// (a boundary) is [`NO_NEIGHBOUR`] here. VERIFIED against both functions (cell-mcubes); the
/// result depends on the triangle order, which is the walk's (`build_retail`).
fn retail_adjacency(triangles: &[[u32; 3]]) -> Vec<u32> {
    let n = triangles.len() * 3;
    let vertex = |c: usize| triangles[c / 3][c % 3];
    let next = |c: usize| c / 3 * 3 + (c % 3 + 1) % 3;
    let mut adj = vec![0u32; n];
    let mut first: HashMap<(u32, u32), u32> = HashMap::new();
    let mut rings: Vec<u32> = Vec::new();
    for t in 0..triangles.len() {
        for k in [2usize, 0, 1] {
            let a = t * 3 + k;
            let (v0, v1) = (vertex(a), vertex(next(a)));
            let key = (v0.min(v1), v0.max(v1));
            match first.get(&key) {
                None => {
                    adj[a] = a as u32;
                    first.insert(key, a as u32);
                }
                Some(&f) => {
                    let old = adj[f as usize];
                    adj[f as usize] = a as u32;
                    adj[a] = old;
                    if old != f && adj[old as usize] == f {
                        rings.push(old);
                    }
                }
            }
        }
    }
    for &c0 in &rings {
        let mut ring: Vec<(u32, bool)> = Vec::new();
        let mut c = c0;
        loop {
            ring.push((c, vertex(c as usize) < vertex(next(c as usize))));
            c = adj[c as usize];
            if c == c0 || ring.len() > n {
                break;
            }
        }
        while ring.len() >= 2 {
            let flag = ring[0].1;
            let Some(j) = (1..ring.len()).find(|&j| ring[j].1 != flag) else { break };
            let (a, b) = (ring[0].0, ring[j].0);
            adj[a as usize] = b;
            adj[b as usize] = a;
            ring.remove(j);
            ring.remove(0);
        }
        while let Some((c, _)) = ring.pop() {
            adj[c as usize] = c;
        }
    }
    for (c, a) in adj.iter_mut().enumerate() {
        if *a == c as u32 {
            *a = NO_NEIGHBOUR;
        }
    }
    adj
}

/// The packed atlas of [`unwrap_retail`].
#[derive(Clone, Debug, Default)]
pub struct RetailUnwrap {
    pub unwrapped: Unwrapped,
    /// Group key and triangle count of every skin chart, in flood order.
    pub charts: Vec<(u8, usize)>,
    /// Packed rectangle `(u0, v0, u1, v1)` of every part rectangle given to
    /// [`unwrap_retail`], same order (retail copies them to +0x90 of the editor
    /// model, 4CB890 0x4CBD00).
    pub part_rects: Vec<[f32; 4]>,
}

/// One packing row (the 0x64-byte strip of 0x50AB80: +0x00 the chart vector,
/// +0x58 filled width, +0x5C height, +0x60 vertical offset of the nested pair).
#[derive(Clone, Debug, Default)]
struct Strip {
    charts: Vec<usize>,
    fill: f32,
    height: f32,
    offset: f32,
}

/// The retail unwrap of a skin mesh: chart flood 0x50A140 and packing 0x50AB80
/// (VERIFIED against the code, see the section comment). `part_sizes` are the
/// side lengths of the `Edit` rectangles of the part rigblocks that share the
/// atlas (0x4CB890: `0.4 x |bbox| x scale + 0.08`, a mouth times 2.2 before
/// the 0.4); they are flooded first so the skin charts take the later indices.
/// UVs are in the skin's own units projected per chart and normalised by the
/// packing into 0 .. 1.
pub fn unwrap_retail(mesh: &SkinMesh, part_sizes: &[f32]) -> RetailUnwrap {
    let mut out = RetailUnwrap::default();
    let ntri = mesh.triangles.len();
    if ntri == 0 || mesh.positions.is_empty() {
        return out;
    }
    let adj = retail_adjacency(&mesh.triangles);
    let corner_vertex = |c: usize| mesh.triangles[c / 3][c % 3] as usize;
    let key_of = |t: usize| retail_group_key(mesh.triangles[t].map(|i| mesh.positions[i as usize]));

    // 0x509BC0: triangle state 0, corner UVs -1, no charts but the part rectangles.
    let mut state = vec![0u32; ntri];
    let mut corner_uv = vec![u32::MAX; ntri * 3];
    // Per vertex: (chart, UV index) of the last chart that placed it (+0x1B8).
    let mut record = vec![(u32::MAX, 0u32); mesh.positions.len()];
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut charts: Vec<RetailChart> = part_sizes.iter().enumerate().map(|(i, s)| RetailChart::part(i as u32, *s)).collect();
    let parts = charts.len();

    // 0x509D20 with index -1: a new UV for a corner's vertex, projected for the chart's key.
    let create_uv = |corner: usize, chart: &mut RetailChart, uvs: &mut Vec<[f32; 2]>, record: &mut Vec<(u32, u32)>, corner_uv: &mut Vec<u32>| {
        let v = corner_vertex(corner);
        let uv = retail_project(mesh.positions[v], chart.key);
        uvs.push(uv);
        let index = uvs.len() as u32 - 1;
        record[v] = (chart.id, index);
        corner_uv[corner] = index;
        chart.grow(uv);
    };
    // 0x509FF0: put a triangle in the chart, entered through `edge` (its corners `edge`
    // and `edge + 1` are already placed; the third one is placed or shared).
    let place = |tri: usize,
                 edge: usize,
                 chart: &mut RetailChart,
                 state: &mut Vec<u32>,
                 uvs: &mut Vec<[f32; 2]>,
                 record: &mut Vec<(u32, u32)>,
                 corner_uv: &mut Vec<u32>| {
        chart.tris.push(tri as u32);
        let c = tri * 3 + edge;
        let c1 = tri * 3 + (edge + 1) % 3;
        let c2 = tri * 3 + (edge + 2) % 3;
        let v2 = corner_vertex(c2);
        if record[v2].0 == chart.id {
            corner_uv[c2] = record[v2].1;
        } else {
            create_uv(c2, chart, uvs, record, corner_uv);
        }
        corner_uv[c] = record[corner_vertex(c)].1;
        corner_uv[c1] = record[corner_vertex(c1)].1;
        state[tri] = !chart.id;
    };

    // 0x50A140: flood until every triangle has a chart.
    let mut start = 0usize;
    let mut queue: Vec<(usize, usize)> = Vec::new();
    loop {
        let n = charts.len() as u32;
        let mut t = start;
        if state[t] > n {
            t = 0;
            while t < ntri && state[t] > n {
                t += 1;
            }
        }
        if t == ntri {
            break;
        }
        let key = key_of(t);
        let mut chart = RetailChart::empty(n, key);
        queue.clear();
        create_uv(t * 3, &mut chart, &mut uvs, &mut record, &mut corner_uv);
        create_uv(t * 3 + 1, &mut chart, &mut uvs, &mut record, &mut corner_uv);
        place(t, 0, &mut chart, &mut state, &mut uvs, &mut record, &mut corner_uv);
        // 0x0050A9C0 (the crossing test) compares the normal indices of the shared edge's
        // ends; the Skinner's normals are per vertex, so it holds whenever the edge is
        // shared; 0x52E640 (the overlap test) is a stub returning false.
        for k in 0..3 {
            let nb = adj[t * 3 + k];
            if nb == NO_NEIGHBOUR {
                continue;
            }
            let (nt, ne) = (nb as usize / 3, nb as usize % 3);
            if state[nt] <= n && key_of(nt) == key {
                state[nt] = 0x7FFF_FFFF;
                queue.push((nt, ne));
            }
        }
        let mut cursor = 0;
        while cursor < queue.len() {
            let (qt, qe) = queue[cursor];
            cursor += 1;
            place(qt, qe, &mut chart, &mut state, &mut uvs, &mut record, &mut corner_uv);
            for k in 0..3 {
                if k == qe {
                    continue;
                }
                let nb = adj[qt * 3 + k];
                if nb == NO_NEIGHBOUR {
                    continue;
                }
                let (nt, ne) = (nb as usize / 3, nb as usize % 3);
                if state[nt] > n {
                    continue;
                }
                if key_of(nt) == key {
                    state[nt] = 0x7FFF_FFFF;
                    queue.push((nt, ne));
                } else {
                    // Left for the next chart, which starts here (0x50A6BB).
                    state[nt] = n + 1;
                    start = nt;
                }
            }
        }
        charts.push(chart);
    }

    // 0x50AB80: pad, turn to portrait, sort by height, fill rows, nest row pairs, normalise.
    let mut area = 0.0f32;
    for chart in &mut charts {
        chart.max_u += RETAIL_CHART_PAD;
        chart.min_u -= RETAIL_CHART_PAD;
        chart.max_v += RETAIL_CHART_PAD;
        chart.min_v -= RETAIL_CHART_PAD;
        let (w, h) = (chart.width(), chart.height());
        area = w * h + area;
        if w > h {
            chart.turn();
        }
    }
    let side = area.sqrt();
    let limit = side * RETAIL_ROW_SLACK;
    // The sort (std::sort 0x50ECA0: an insertion sort up to 28 elements, so stable there): tallest first.
    // ADAPTED: beyond 28 charts retail's introsort is not stable, the equal heights may come in
    // another order; this sort stays stable.
    let mut sorted: Vec<Option<usize>> = (0..charts.len()).map(Some).collect();
    sorted.sort_by(|a, b| charts[b.unwrap()].height().total_cmp(&charts[a.unwrap()].height()));
    let mut strips = vec![Strip::default()];
    let mut i = 0usize;
    while i < sorted.len() {
        while sorted[i].is_none() && i + 1 < sorted.len() {
            i += 1;
        }
        let Some(c) = sorted[i] else { break };
        let w = charts[c].width();
        let at = strips.len() - 1;
        if limit > strips[at].fill + w {
            let strip = &mut strips[at];
            strip.charts.push(c);
            strip.fill += w;
            strip.height = if strip.height > charts[c].height() { strip.height } else { charts[c].height() };
            i += 1;
        } else {
            // The row is full: smaller charts that still fit go in first, then a new row.
            for j in i + 1..sorted.len() {
                if let Some(d) = sorted[j] {
                    let wd = charts[d].width();
                    if limit > strips[at].fill + wd {
                        let strip = &mut strips[at];
                        strip.charts.push(d);
                        strip.fill += wd;
                        strip.height = if strip.height > charts[d].height() { strip.height } else { charts[d].height() };
                        sorted[j] = None;
                    }
                }
            }
            strips.push(Strip::default());
        }
    }
    // Rows are nested in pairs: the first reversed (short to tall), the second as sorted (tall
    // to short), the second pushed up against the first as far as the charts allow.
    let mut max_width = 0.0f32;
    let mut total_height = 0.0f32;
    for pair in 0..strips.len() / 2 {
        let (head, tail) = strips.split_at_mut(2 * pair + 1);
        let (a, b) = (&mut head[2 * pair], &mut tail[0]);
        a.charts.reverse();
        let mut offset = -(a.height + b.height);
        let (mut ia, mut ib, mut xa, mut xb) = (0usize, 0usize, 0.0f32, 0.0f32);
        while ia < a.charts.len() && ib < b.charts.len() {
            let (ca, cb) = (&charts[a.charts[ia]], &charts[b.charts[ib]]);
            let (wa, ha, wb, hb) = (ca.width(), ca.height(), cb.width(), cb.height());
            let room = a.height + b.height - hb + offset;
            if ha > room {
                offset = ha - room + offset;
            }
            let (da, db) = (wa - xa, wb - xb);
            if db > da {
                ia += 1;
                xa = 0.0;
                xb += da;
            } else if da > db {
                ib += 1;
                xb = 0.0;
                xa += db;
            } else {
                ia += 1;
                ib += 1;
                xa = 0.0;
                xb = 0.0;
            }
        }
        b.offset = offset;
        let wide = if a.fill > b.fill { a.fill } else { b.fill };
        max_width = if max_width < wide { wide } else { max_width };
        let both = a.height + b.height + b.offset;
        total_height += if a.height > both { a.height } else { both };
    }
    if strips.len() % 2 == 1 {
        let last = &strips[strips.len() - 1];
        max_width = if max_width < last.fill { last.fill } else { max_width };
        total_height += last.height;
    }
    let extent = if total_height > max_width { total_height } else { max_width };
    let scale = 1.0 / extent;
    let mut y = 0.0f32;
    for (index, strip) in strips.iter().enumerate() {
        let mut x = 0.0f32;
        for &c in &strip.charts {
            let chart = &mut charts[c];
            let (w, h) = (chart.width(), chart.height());
            let dx = x - chart.min_u;
            let mut dy = y - chart.min_v;
            if index % 2 == 1 {
                dy = strip.height - h + strip.offset + dy;
            }
            let mut used: Vec<u32> = Vec::with_capacity(chart.tris.len() * 3);
            for &t in &chart.tris {
                used.extend_from_slice(&corner_uv[t as usize * 3..t as usize * 3 + 3]);
            }
            used.sort_unstable();
            used.dedup();
            let sum = chart.max_u + chart.min_u;
            for &u in &used {
                let uv = &mut uvs[u as usize];
                if chart.turned {
                    let v = uv[1];
                    uv[1] = uv[0];
                    uv[0] = sum - v;
                }
                uv[0] = (uv[0] + dx) * scale;
                uv[1] = (uv[1] + dy) * scale;
            }
            chart.min_u = (chart.min_u + dx) * scale;
            chart.max_u = (chart.max_u + dx) * scale;
            chart.min_v = (chart.min_v + dy) * scale;
            chart.max_v = (chart.max_v + dy) * scale;
            chart.turned = false;
            x += w;
        }
        y += strip.height + strip.offset;
    }

    // The render mesh: one vertex per (skin vertex, UV).
    let un = &mut out.unwrapped;
    let mut split: HashMap<(u32, u32), u32> = HashMap::new();
    let chart_of_tri: Vec<usize> = {
        let mut v = vec![0usize; ntri];
        for (ci, chart) in charts.iter().enumerate() {
            for &t in &chart.tris {
                v[t as usize] = ci;
            }
        }
        v
    };
    for (t, tri) in mesh.triangles.iter().enumerate() {
        let id = ChartId { chain: 0, side: (chart_of_tri[t] - parts).min(255) as u8 };
        let mut new = [0u32; 3];
        for k in 0..3 {
            let (v, uv) = (tri[k], corner_uv[t * 3 + k]);
            new[k] = *split.entry((v, uv)).or_insert_with(|| {
                un.positions.push(mesh.positions[v as usize]);
                un.normals.push(mesh.normals[v as usize]);
                un.uvs.push(uvs[uv as usize]);
                un.source.push(v);
                un.chart.push(id);
                un.positions.len() as u32 - 1
            });
        }
        un.triangles.push(new);
    }
    un.charts = (0..charts.len() - parts).map(|i| ChartId { chain: 0, side: i.min(255) as u8 }).collect();
    un.texel_density = PAINT_SIZE as f32 * scale;
    out.charts = charts[parts..].iter().map(|c| (c.key, c.tris.len())).collect();
    out.part_rects = charts[..parts].iter().map(|c| [c.min_u, c.min_v, c.max_u, c.max_v]).collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_catalog::ResourceIndex;
    use crate::dbpf::ResourceKey;

    fn index() -> Option<&'static ResourceIndex> {
        static INDEX: OnceLock<Option<ResourceIndex>> = OnceLock::new();
        INDEX
            .get_or_init(|| crate::spore_install::discover().ok().map(|i| ResourceIndex::load(&i.packages)))
            .as_ref()
            .filter(|i| !i.records.is_empty())
    }

    /// `creature_editorModel~!B_00.crt` (40626200!99D79CD8, type 0x2B978C46).
    fn b00() -> Option<Vec<SkinRecord>> {
        let bytes = index()?.read(ResourceKey { type_id: 0x2B97_8C46, group_id: 0x4062_6200, instance_id: 0x99D7_9CD8 }).ok()?;
        let xml = String::from_utf8_lossy(&bytes).into_owned();
        let vertebra = crate::world_objects::name_id("ce_vertebra");
        Some(records_from_crt(&xml, |_, i| i == vertebra))
    }

    /// `palette_editorModel~!x_leg_09.creaturedata` (406B6200!97780B04).
    fn x_leg() -> Option<Vec<SkinRecord>> {
        let bytes = index()?
            .read(ResourceKey { type_id: crate::creature_data::TYPE, group_id: 0x406B_6200, instance_id: 0x9778_0B04 })
            .ok()?;
        Some(records_from_creature_data(&crate::creature_data::parse(&bytes).ok()?))
    }

    fn sphere_records(scale: f32) -> Vec<SkinRecord> {
        order_records(vec![SkinRecord { kind: BlockKind::Vertebra, flags: flag::USES_SKIN, scale, ..Default::default() }])
    }

    fn check_mesh(mesh: &SkinMesh) {
        assert!(!mesh.triangles.is_empty());
        for t in &mesh.triangles {
            assert!(t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
            assert!(t.iter().all(|&i| (i as usize) < mesh.positions.len()));
        }
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            assert!(p.is_finite() && n.is_finite());
            assert!((n.length() - 1.0).abs() < 1e-3);
        }
        for w in &mesh.weights {
            let s: f32 = w.iter().sum();
            assert!((s - 1.0).abs() < 1e-4, "weights sum {s}");
            assert!(w.windows(2).all(|p| p[0] >= p[1]), "weights sorted {w:?}");
        }
    }

    #[test]
    fn cube_table_matches_bloomenthal() {
        let t = cube_table();
        assert!(t[0].is_empty() && t[255].is_empty());
        // One inside corner: one triangle through its three edges.
        assert_eq!(t[1].len(), 1);
        assert_eq!(t[1][0].len(), 3);
        // Two diagonally opposite corners: two separate triangles.
        assert_eq!(t[(1 << LBN) | (1 << RTF)].len(), 2);
        // Every edge with a sign change appears exactly once per config.
        for (i, polys) in t.iter().enumerate() {
            let crossing = (0..12).filter(|&e| ((i >> CORNER1[e]) & 1) != ((i >> CORNER2[e]) & 1)).count();
            assert_eq!(polys.iter().map(Vec::len).sum::<usize>(), crossing, "config {i}");
        }
    }

    #[test]
    fn lone_ball_iso_radius_and_vertebra_constants() {
        assert!((iso_factor(1.0) - 0.8269).abs() < 1e-3);
        let r = sphere_records(1.0);
        let (c, radius) = r[0].vertebra_ball();
        assert!((radius - 0.264_619_4).abs() < 1e-5);
        assert!((c.z + 0.159_931_78).abs() < 1e-6);
        assert_eq!(sphere_records(0.05)[0].vertebra_ball().1, VERTEBRA_MIN_RADIUS);
    }

    #[test]
    fn sphere_mesh_is_closed_outward_and_on_the_iso_radius() {
        let records = sphere_records(1.0);
        let (prims, mesh) = build(&records, RES_HIGH_QUALITY).unwrap();
        check_mesh(&mesh);
        let (c, radius) = records[0].vertebra_ball();
        let iso = radius * iso_factor(1.0);
        // Clustered vertices are averages of crossings: within a cell of the
        // iso sphere (they are not re-projected).
        for p in &mesh.positions {
            assert!(((*p - c).length() - iso).abs() < RES_HIGH_QUALITY, "{}", (*p - c).length());
        }
        // Outward winding and a closed surface (every edge used twice).
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &mesh.triangles {
            let [a, b, cc] = t.map(|i| mesh.positions[i as usize]);
            assert!((b - a).cross(cc - a).dot((a + b + cc) / 3.0 - c) > 0.0);
            for k in 0..3 {
                let (u, v) = (t[k], t[(k + 1) % 3]);
                *edges.entry((u.min(v), u.max(v))).or_default() += 1;
            }
        }
        let open = edges.values().filter(|&&n| n != 2).count();
        assert!(open * 10 <= edges.len(), "{open} of {} edges not shared by two triangles", edges.len());
        assert!(prims.field.value(c) > 0.9);
        // Deterministic.
        let (_, again) = build(&records, RES_HIGH_QUALITY).unwrap();
        assert_eq!(mesh.positions, again.positions);
        assert_eq!(mesh.triangles, again.triangles);
    }

    #[test]
    fn link_spheres_fill_the_gap_with_radius_progression() {
        let mut f = Field::default();
        let a = f.add(Vec3::ZERO, 1.0, 0.3, 0);
        let b = f.add(Vec3::new(0.0, 1.0, 0.0), 1.0, 0.1, 1);
        f.link(a, b, LINK_BLEND, JOINT);
        let extra = &f.balls[2..];
        assert!(extra.len() >= 3);
        let iso = iso_factor(1.0);
        let mut last_y = 0.0;
        let mut last_r = 0.3 * iso;
        for e in extra {
            assert!(e.center.y > last_y && e.center.y < 1.0);
            let r = e.radius * iso;
            assert!(r < last_r && r > 0.1 * iso * 0.99);
            assert_eq!(e.group, JOINT);
            last_y = e.center.y;
            last_r = r;
        }
        // Equal radii: evenly spaced.
        let mut g = Field::default();
        let a = g.add(Vec3::ZERO, 1.0, 0.2, 0);
        let b = g.add(Vec3::X, 1.0, 0.2, 0);
        g.link(a, b, LINK_BLEND, 0);
        let xs: Vec<f32> = g.balls[2..].iter().map(|e| e.center.x).collect();
        let step = xs[0];
        for (i, x) in xs.iter().enumerate() {
            assert!((x - step * (i + 1) as f32).abs() < 1e-4);
        }
        assert!((step * (xs.len() + 1) as f32 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn b00_startup_creature_skin() {
        let Some(records) = b00() else { return };
        assert_eq!(records.len(), 8);
        assert!(records.iter().all(|r| r.kind == BlockKind::Vertebra && r.uses_skin()));
        assert_eq!(records[0].flags & flag::CHAIN_START, flag::CHAIN_START);
        assert_eq!(records.iter().map(|r| r.bone).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
        for res in [RES_REALTIME, RES_HIGH_QUALITY] {
            let (prims, mesh) = build(&records, res).unwrap();
            check_mesh(&mesh);
            assert_eq!(mesh.resolution, res);
            assert_eq!(prims.chains.len(), 1);
            let (min, max) = mesh.positions.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
            // The torso: symmetric about x = 0, long along y, balls below the
            // vertebra positions.
            assert!((min.x + max.x).abs() < res, "{min} {max}");
            assert!(max.y - min.y > 0.7 && max.y - min.y < 1.1, "{min} {max}");
            assert!(max.x - min.x > 0.35 && max.x - min.x < 0.6, "{min} {max}");
            assert!(max.z < 0.66 && min.z > -0.1, "{min} {max}");
            // Weights: spine bones only (rigblock indices 0..8).
            for (b, w) in mesh.bones.iter().zip(&mesh.weights) {
                for k in 0..4 {
                    if w[k] > 0.0 {
                        assert!(b[k] < 8);
                    }
                }
            }
            let n = mesh.positions.len();
            if res == RES_HIGH_QUALITY {
                assert!((300..8000).contains(&n), "{n} vertices");
            }
            // The head-end vertices weigh mostly on the first vertebra.
            let front = (0..n).min_by(|&a, &b| mesh.positions[a].y.total_cmp(&mesh.positions[b].y)).unwrap();
            assert_eq!(mesh.bones[front][0], 0);
            let back = (0..n).max_by(|&a, &b| mesh.positions[a].y.total_cmp(&mesh.positions[b].y)).unwrap();
            assert_eq!(mesh.bones[back][0], 7);
            // UVs inside the texture and every triangle in one chart.
            let un = unwrap(&prims, &mesh);
            assert_eq!(un.triangles.len(), mesh.triangles.len());
            assert_eq!(un.charts.len(), 2);
            for uv in &un.uvs {
                assert!(uv.iter().all(|v| (0.0..=1.0).contains(v)), "{uv:?}");
            }
            for t in &un.triangles {
                assert!(un.chart[t[0] as usize] == un.chart[t[1] as usize] && un.chart[t[1] as usize] == un.chart[t[2] as usize]);
            }
            assert!(un.texel_density > 100.0, "{}", un.texel_density);
        }
        // Coarser resolution, fewer vertices.
        let coarse = build(&records, RES_REALTIME).unwrap().1.positions.len();
        let fine = build(&records, RES_HIGH_QUALITY).unwrap().1.positions.len();
        assert!(coarse < fine);
    }

    #[test]
    fn x_leg_09_limb_assembly_skin() {
        let Some(records) = x_leg() else { return };
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].kind, BlockKind::Limb);
        assert_eq!(records[0].flags, 0x1F);
        assert!((records[0].socket_distance - 0.2605).abs() < 1e-3);
        assert!((records[0].muscle_scale - 0.475).abs() < 1e-3);
        assert!((records[0].base_muscle_scale - 0.5).abs() < 1e-3);
        assert!((records[0].next_muscle_scale - 1.1).abs() < 1e-3);
        assert_eq!(records[1].parent, 0);
        assert_eq!(records[2].kind, BlockKind::Standard);
        assert!(!records[2].uses_skin());
        let (prims, mesh) = build(&records, RES_HIGH_QUALITY).unwrap();
        check_mesh(&mesh);
        // Two tentacle segments: start ball + 14 + end ball, then 14 + end.
        let main: Vec<_> = prims.field.balls.iter().filter(|b| b.group == 0 || b.group == 1 || b.group & JOINT != 0).collect();
        assert!(main.len() >= 2 * TENTACLE_BALLS);
        assert_eq!(prims.chains.len(), 1);
        assert!(prims.chains[0].limb);
        assert_eq!(prims.chains[0].points.len(), 3);
        // The tube follows the two segments: its extent covers both lengths.
        let (min, max) = mesh.positions.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
        let span = (max - min).length();
        let length = records[0].socket_distance + records[1].socket_distance;
        assert!(span > length * 0.6 && span < length + 0.6, "span {span} length {length}");
        // Weights on the two segments only (the foot has no skin).
        for (b, w) in mesh.bones.iter().zip(&mesh.weights) {
            for k in 0..4 {
                if w[k] > 1e-6 {
                    assert!(b[k] <= 1, "{b:?} {w:?}");
                }
            }
        }
        // The start of the limb weighs on segment 0, its far end on segment 1.
        let s = records[0].position;
        let near = (0..mesh.positions.len()).min_by(|&a, &b| mesh.positions[a].distance(s).total_cmp(&mesh.positions[b].distance(s))).unwrap();
        assert_eq!(mesh.bones[near][0], 0);
        let e = records[1].segment_end();
        let far = (0..mesh.positions.len()).min_by(|&a, &b| mesh.positions[a].distance(e).total_cmp(&mesh.positions[b].distance(e))).unwrap();
        assert_eq!(mesh.bones[far][0], 1);
    }

    #[test]
    fn grid_overflow_retries_coarser() {
        // A body too long for 255 cells at 0.032 succeeds after retries.
        let mut blocks = Vec::new();
        for i in 0..12 {
            blocks.push(SkinRecord {
                kind: BlockKind::Vertebra,
                flags: flag::USES_SKIN,
                scale: 1.0,
                position: Vec3::new(0.0, i as f32 * 0.9, 0.0),
                parent: i as i16 - 1,
                ..Default::default()
            });
        }
        let records = order_records(blocks);
        assert_eq!(polygonize_once(&primitives(&records).field, RES_HIGH_QUALITY).unwrap_err(), MeshFailure::Grid);
        let (_, mesh) = build(&records, RES_HIGH_QUALITY).unwrap();
        assert!(mesh.resolution > RES_HIGH_QUALITY);
        check_mesh(&mesh);
    }

    /// A closed cube (side 2, counter-clockwise from outside) as a skin mesh.
    fn cube_mesh() -> SkinMesh {
        let positions = vec![
            Vec3::new(-1.0, -1.0, -1.0),
            Vec3::new(1.0, -1.0, -1.0),
            Vec3::new(1.0, 1.0, -1.0),
            Vec3::new(-1.0, 1.0, -1.0),
            Vec3::new(-1.0, -1.0, 1.0),
            Vec3::new(1.0, -1.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(-1.0, 1.0, 1.0),
        ];
        let quads = [[0u32, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [3, 7, 6, 2], [0, 4, 7, 3], [1, 2, 6, 5]];
        let triangles = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
        SkinMesh { normals: vec![Vec3::Z; 8], positions, triangles, ..Default::default() }
    }

    #[test]
    fn retail_group_keys_are_the_six_cube_directions() {
        let tri = |a: Vec3, b: Vec3, c: Vec3| retail_group_key([a, b, c]);
        // 0x50A7D0: key = 2 x dominant axis + (that component negative).
        assert_eq!(tri(Vec3::ZERO, Vec3::Y, Vec3::Z), 0);
        assert_eq!(tri(Vec3::ZERO, Vec3::Z, Vec3::Y), 1);
        assert_eq!(tri(Vec3::ZERO, Vec3::Z, Vec3::X), 2);
        assert_eq!(tri(Vec3::ZERO, Vec3::X, Vec3::Z), 3);
        assert_eq!(tri(Vec3::ZERO, Vec3::X, Vec3::Y), 4);
        assert_eq!(tri(Vec3::ZERO, Vec3::Y, Vec3::X), 5);
        // 0x4FDE40 compares strictly: a tie goes to the later axis.
        assert_eq!(tri(Vec3::ZERO, Vec3::new(1.0, 0.0, -1.0), Vec3::new(0.0, 1.0, 0.0)), 4);
    }

    #[test]
    fn retail_projection_drops_the_key_axis() {
        let p = Vec3::new(1.0, 2.0, 3.0);
        // 0x509F60: U = the next axis (negated for a negative key), V = the one after.
        assert_eq!(retail_project(p, 0), [2.0, 3.0]);
        assert_eq!(retail_project(p, 1), [-2.0, 3.0]);
        assert_eq!(retail_project(p, 2), [3.0, 1.0]);
        assert_eq!(retail_project(p, 3), [-3.0, 1.0]);
        assert_eq!(retail_project(p, 4), [1.0, 2.0]);
        assert_eq!(retail_project(p, 5), [-1.0, 2.0]);
    }

    #[test]
    fn retail_adjacency_pairs_the_corners_of_shared_edges() {
        let mesh = cube_mesh();
        let adj = retail_adjacency(&mesh.triangles);
        for (c, &n) in adj.iter().enumerate() {
            assert_ne!(n, NO_NEIGHBOUR, "a closed mesh has no boundary corner");
            assert_eq!(adj[n as usize] as usize, c, "neighbours are mutual");
            assert_ne!(n as usize / 3, c / 3, "the neighbour is another triangle");
            // The neighbour's edge runs the same two vertices backwards.
            let (t, k) = (c / 3, c % 3);
            let (u, j) = (n as usize / 3, n as usize % 3);
            assert_eq!((mesh.triangles[t][k], mesh.triangles[t][(k + 1) % 3]), (mesh.triangles[u][(j + 1) % 3], mesh.triangles[u][j]));
        }
        // An open edge has no neighbour.
        let open = retail_adjacency(&mesh.triangles[..1]);
        assert!(open.iter().all(|&n| n == NO_NEIGHBOUR));
    }

    #[test]
    fn retail_unwrap_of_a_cube_is_six_separate_charts_in_the_unit_square() {
        let mesh = cube_mesh();
        let out = unwrap_retail(&mesh, &[]);
        let un = &out.unwrapped;
        assert_eq!(out.charts.len(), 6);
        assert!(out.charts.iter().all(|c| c.1 == 2), "{:?}", out.charts);
        let mut keys: Vec<u8> = out.charts.iter().map(|c| c.0).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec![0, 1, 2, 3, 4, 5]);
        // Every corner of every triangle keeps its position; corners of different charts are different vertices.
        assert_eq!(un.triangles.len(), 12);
        assert_eq!(un.positions.len(), 24, "each cube corner is split by the three charts that meet there");
        assert!(un.uvs.iter().flatten().all(|v| (0.0..=1.0).contains(v)));
        // Chart rectangles are disjoint squares of the same size (side 2 + 2 x 0.006 before the normalisation).
        let mut rects: Vec<[f32; 4]> = vec![[f32::MAX, f32::MAX, f32::MIN, f32::MIN]; 6];
        for (u, c) in un.uvs.iter().zip(&un.chart) {
            let r = &mut rects[c.side as usize];
            *r = [r[0].min(u[0]), r[1].min(u[1]), r[2].max(u[0]), r[3].max(u[1])];
        }
        let side = rects[0][2] - rects[0][0];
        for r in &rects {
            assert!((r[2] - r[0] - side).abs() < 1e-5 && (r[3] - r[1] - side).abs() < 1e-5, "{r:?}");
        }
        for i in 0..6 {
            for j in i + 1..6 {
                let (a, b) = (rects[i], rects[j]);
                assert!(a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1], "charts {i} and {j} overlap: {a:?} {b:?}");
            }
        }
        // The part rectangles share the atlas and keep clear of the charts.
        let with = unwrap_retail(&mesh, &[1.0, 0.5]);
        assert_eq!(with.part_rects.len(), 2);
        let (p, q) = (with.part_rects[0], with.part_rects[1]);
        assert!(p[2] - p[0] > q[2] - q[0], "the larger rectangle stays larger: {p:?} {q:?}");
        let mut body = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for u in &with.unwrapped.uvs {
            body = [body[0].min(u[0]), body[1].min(u[1]), body[2].max(u[0]), body[3].max(u[1])];
        }
        assert!(body[2] <= 1.0 && body[3] <= 1.0);
        assert!(with.unwrapped.texel_density > 0.0);
    }

    // --- the retail polygon walk (0x53D2B0 tables, 0x53FE30) ---

    /// The tables the constructor 0x53D2B0 builds (transcribed from a literal run of its code in
    /// `re\mc_tables.py`: the next-edge triples of the 24 directed edges and the edge-mask table).
    #[test]
    fn retail_cube_tables_are_the_constructors() {
        let t = retail_cube();
        let golden: [[(u16, u8); 3]; 24] = [
            [(2, 3), (4, 4), (8, 6)],
            [(16, 9), (256, 17), (32, 10)],
            [(4, 4), (8, 6), (1, 1)],
            [(32, 11), (512, 19), (64, 12)],
            [(64, 13), (1024, 20), (128, 14)],
            [(8, 6), (1, 1), (2, 3)],
            [(128, 15), (2048, 22), (16, 8)],
            [(1, 1), (2, 3), (4, 4)],
            [(256, 17), (32, 10), (1, 0)],
            [(8, 7), (128, 15), (2048, 22)],
            [(512, 19), (64, 12), (2, 2)],
            [(1, 0), (16, 9), (256, 17)],
            [(1024, 20), (128, 14), (4, 5)],
            [(2, 2), (32, 11), (512, 19)],
            [(2048, 22), (16, 8), (8, 7)],
            [(4, 5), (64, 13), (1024, 20)],
            [(32, 10), (1, 0), (16, 9)],
            [(2048, 23), (1024, 21), (512, 18)],
            [(64, 12), (2, 2), (32, 11)],
            [(256, 16), (2048, 23), (1024, 21)],
            [(512, 18), (256, 16), (2048, 23)],
            [(128, 14), (4, 5), (64, 13)],
            [(1024, 21), (512, 18), (256, 16)],
            [(16, 8), (8, 7), (128, 15)],
        ];
        assert_eq!(t.next, golden);
        assert_eq!(&t.mask[..8], &[0, 25, 2320, 2313, 140, 149, 2460, 2437]);
        assert_eq!(t.mask[255], 0);
        assert_eq!(t.mask.iter().map(|&m| m as u32).sum::<u32>(), 524_160);
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for m in t.mask {
            for b in m.to_le_bytes() {
                h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
            }
        }
        assert_eq!(h, 0xec0a_d4fa_b718_bd05);
        // Every directed edge is the first step of exactly one face walk and leads to a different edge.
        for d in 0..24 {
            assert_eq!(t.corner[d] as usize, RETAIL_EDGE_CORNERS[d] as usize);
            for (bit, dir) in t.next[d] {
                assert_eq!(bit, 1 << (dir >> 1));
                assert_ne!(dir >> 1, d as u8 >> 1);
            }
        }
    }

    /// Retail's walk finds the same polygons as the crate's own table (`cube_table`) for all 256
    /// corner masks (the surface topology is the same, including the ambiguous cases): only the
    /// start edge and the direction differ, which is what the fans below depend on.
    #[test]
    fn retail_polygons_are_the_crates_polygons_up_to_rotation() {
        let edge_of = |e: usize| {
            let (a, b) = (CORNER1[e] as u8, CORNER2[e] as u8);
            (0..12).find(|&r| (RETAIL_EDGE_CORNERS[2 * r], RETAIL_EDGE_CORNERS[2 * r + 1]) == (a.min(b), a.max(b))).unwrap() as u8
        };
        let canon = |mut p: Vec<u8>| {
            let mut best: Option<Vec<u8>> = None;
            for _ in 0..2 {
                for r in 0..p.len() {
                    let mut q = p.clone();
                    q.rotate_left(r);
                    if best.as_ref().is_none_or(|b| q < *b) {
                        best = Some(q);
                    }
                }
                p.reverse();
            }
            best.unwrap_or_default()
        };
        for index in 0..256usize {
            let mut ours: Vec<Vec<u8>> = cube_table()[index].iter().map(|p| canon(p.iter().map(|&e| edge_of(e as usize)).collect())).collect();
            let retail_index = !(index as u8);
            let mut theirs: Vec<Vec<u8>> = retail_polygons(retail_index).into_iter().map(|p| canon(p.iter().map(|&d| d >> 1).collect())).collect();
            ours.sort();
            theirs.sort();
            assert_eq!(ours, theirs, "corner mask {index:#04x}");
        }
    }

    /// The walk starts every polygon at the lowest remaining edge, visits every crossing edge
    /// exactly once and runs from the inside corner to the outside one (bit set = outside).
    #[test]
    fn retail_walk_visits_every_crossing_edge_once_inside_to_outside() {
        let t = retail_cube();
        assert!(retail_polygons(0).is_empty() && retail_polygons(255).is_empty());
        for index in 0..256usize {
            let polys = retail_polygons(index as u8);
            let mut seen = 0u16;
            let mut last_start = None;
            for p in &polys {
                let start_edge = p[0] >> 1;
                assert!(last_start.is_none_or(|l| l < start_edge), "polygons start at ascending edges");
                last_start = Some(start_edge);
                for &d in p {
                    let (from, to) = (t.corner[d as usize] as usize, t.corner[d as usize ^ 1] as usize);
                    assert_eq!((index >> from) & 1, 0, "mask {index:#04x}: directed edge {d} starts outside");
                    assert_eq!((index >> to) & 1, 1, "mask {index:#04x}: directed edge {d} ends inside");
                    assert_eq!(seen & (1 << (d >> 1)), 0, "mask {index:#04x}: edge {} walked twice", d >> 1);
                    seen |= 1 << (d >> 1);
                }
            }
            assert_eq!(seen, t.mask[index], "mask {index:#04x}");
        }
    }

    /// The Cell's polygonizer builds the same surface as the Creature's (same vertices, closed,
    /// outward) with retail's triangulation, and the Creature's own mesh is unchanged.
    #[test]
    fn retail_polygonizer_builds_the_same_closed_outward_surface() {
        let records = sphere_records(1.0);
        let (c, _) = records[0].vertebra_ball();
        let (prims, ours) = build(&records, RES_HIGH_QUALITY).unwrap();
        let (_, retail) = build_retail(&records, RES_HIGH_QUALITY).unwrap();
        check_mesh(&retail);
        assert_eq!(ours.positions.len(), retail.positions.len());
        // The same vertices up to the float order of retail's bisection and averaging (the numbering
        // follows the walk order).
        let key = |p: &Vec3| p.to_array().map(f32::to_bits);
        for p in &retail.positions {
            let near = ours.positions.iter().map(|q| q.distance(*p)).fold(f32::MAX, f32::min);
            assert!(near < 1e-4, "retail vertex {p:?} is {near} from the nearest Creature vertex");
        }
        assert!(prims.field.value(c) > 0.9);
        // Retail's winding is outward with no flip step, and the surface is closed.
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &retail.triangles {
            let [p, q, r] = t.map(|i| retail.positions[i as usize]);
            assert!((q - p).cross(r - p).dot((p + q + r) / 3.0 - c) > 0.0, "retail triangle {t:?} faces inwards");
            for k in 0..3 {
                let (u, v) = (t[k], t[(k + 1) % 3]);
                *edges.entry((u.min(v), u.max(v))).or_default() += 1;
            }
        }
        let open = edges.values().filter(|&&n| n != 2).count();
        assert!(open * 10 <= edges.len(), "{open} of {} edges not shared by two triangles", edges.len());
        // The same triangle count (a polygon of n corners gives n - 2 triangles either way), but a
        // different triangulation of some polygons.
        let set = |m: &SkinMesh| {
            let mut s: Vec<[[u32; 3]; 3]> = m
                .triangles
                .iter()
                .map(|t| {
                    let mut k = t.map(|i| key(&m.positions[i as usize]));
                    k.sort_unstable();
                    k
                })
                .collect();
            s.sort_unstable();
            s
        };
        assert_eq!(ours.triangles.len(), retail.triangles.len());
        assert_ne!(set(&ours), set(&retail), "the walks fan some polygons from different vertices");
        // Deterministic, and the Creature entry point still gives the Creature's mesh.
        let (_, again) = build_retail(&records, RES_HIGH_QUALITY).unwrap();
        assert_eq!(retail.triangles, again.triangles);
        let (_, creature_again) = build(&records, RES_HIGH_QUALITY).unwrap();
        assert_eq!(ours.triangles, creature_again.triangles);
    }
}
