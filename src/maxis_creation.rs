//! Maxis creations editable: the creation file an installed Maxis PNG carries
//! (`front_end::png_creation`, a `<sporemodel>` XML = the retail `.crt` /
//! `.bld` / `.vcl` / `.ufo`) becomes the OpenSpore records the creators edit,
//! so Sporepedia's Edit (and the outfitters) open a Maxis creation on a
//! scratch copy that is kept as the player's own creation.
//!
//! The file (VERIFIED, `notes/creator-re-notes.md` 2.1, `limb-re-notes.md`
//! 5): one `<blockref>` per `EditorRigblock` with its part PROP (group,
//! instance), scale, position and total orientation (rows = the block's local
//! axes, row-vector convention; Spore model space: Z up, front -Y), the
//! `childlist` (parent -> children), the `symmetric` twin, limb blocks'
//! `limbtype` (0 muscle, 1 tentacle, 2 rigid), `musclescale` (segment
//! thickness) and `basemusclescale` (ball connector at the block's joint), the
//! morph `handles` and per-region `paintlist`; a creature's properties carry
//! its skin paint (`skineffect1..3`, seeds, colours).
//!
//! Rigid creations (buildings, vehicles, UFOs): each block is a
//! `RigidBlock` in the rigid editor's space (Y up, +Z the front; one unit =
//! one retail model unit, so `rigid_editor::editor_unit` draws it at
//! `modelScale x scale` as retail): position `C p`, rotation `C R B^-1` (C =
//! (x, z, -y), R = the orientation with the rows as columns, B = the part
//! mesh's import basis), its region paints and its twin (`pair`; a twin of the
//! same part is the reflected mesh). The editor class is the setup whose name
//! hashes to the creation's `modeltype` (e.g. `VehicleEconomicWater`
//! 0x2A5147A9, `BuildingEntertainment` 0x72C49181), else the class template
//! of the creation type.
//!
//! Creatures: the vertebrae (`ce_vertebra`) are the spine, every other block
//! a part on it, limb chains limbs (`creature_limbs::Limb`) with their foot /
//! hand as the end part, the skin paint the creature paint. Positions map
//! with the skin's own basis and units (`creature_skin_view::to_player`), the
//! creature's ground (z = 0) on the player frame's ground
//! (`creature_cell_convert::GROUND_Y`); part rotations are `S R B^-1` (S =
//! `spore_to_bevy`) as the rigid limb segments' frames; part scales draw the
//! mesh at the retail `modelScale x scale` creature units
//! (`creature_limbs::skin_scale`, with the importer's model scale computed
//! from the mesh, `editor::import_definition`).
//!
//! ADAPTED (OpenSpore's creature model is not a rigblock tree):
//! * the editable body is a `BodyProfile` deforming the starter's vertebra
//!   balls (`baked_model::BodyTemplate`): the profile is solved so the
//!   deformed balls follow the Maxis spine and its ball radii (one profile
//!   point per vertebra, at most 32), but the skin keeps the starter's ball
//!   count, so a long spine is a smoothed torso;
//! * parts hang from the nearest of the eight attachment slots with the rest
//!   as their offset (clamped to `game::OFFSET_LIMIT`, as every OpenSpore
//!   part); parts on limb segments other than the end stay where they are
//!   (they do not ride the limb);
//! * the limb part's palette item is `limb_assembly_x_leg_09` (skinned) or
//!   `limb_assembly_exo_l_01` (rigid): the file names segments, not the
//!   assembly they were bought as;
//! * the morph handles of rigid blocks are not kept (`RigidBlock` has none;
//!   the rigid editor's deform handles are its `stretch`).

use bevy::prelude::*;
use std::collections::{HashMap, VecDeque};

use crate::{
    asset_catalog::{PartCatalog, PartDefinition, ResourceIndex, CREATURE_PARTS},
    body::{BodyProfile, SpinePoint},
    creation_records::{BLD, UFO, VCL},
    creature_editor_data::SetupKind,
    creature_handles::TWIN_BIT,
    creature_limbs::Limb,
    dbpf::ResourceKey,
    game::{PartInstance, RunProgress, VehiclePart},
    rigid_editor::{RigidBlock, RigidDesign},
    rigid_paint::RegionPaint,
};

/// `ce_vertebra` (`creature_parts~`, part bool `IsVertebra`).
pub const VERTEBRA: u32 = 0xD05C_53A3;
/// The creature creation type (`crt`).
pub const CRT: u32 = 0x2B97_8C46;

// ---------------------------------------------------------------------------
// The creation file
// ---------------------------------------------------------------------------

/// One `<blockref>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub group: u32,
    pub instance: u32,
    pub scale: f32,
    /// Spore model space (Z up, front -Y).
    pub position: Vec3,
    /// The orientation's rows: the block's local axes.
    pub rows: [Vec3; 3],
    pub children: Vec<usize>,
    pub symmetric: Option<usize>,
    /// `limbtype` (limb segment blocks only).
    pub limb_type: Option<u32>,
    pub muscle: Option<f32>,
    pub ball: Option<f32>,
    pub handles: Vec<(u32, f32)>,
    pub paints: Vec<RegionPaint>,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            group: 0,
            instance: 0,
            scale: 1.0,
            position: Vec3::ZERO,
            rows: [Vec3::X, Vec3::Y, Vec3::Z],
            children: Vec::new(),
            symmetric: None,
            limb_type: None,
            muscle: None,
            ball: None,
            handles: Vec::new(),
            paints: Vec::new(),
        }
    }
}

impl Block {
    /// The orientation as a column-math rotation (columns = local axes).
    pub fn rotation(&self) -> Mat3 {
        Mat3::from_cols(self.rows[0], self.rows[1], self.rows[2])
    }
    pub fn key(&self) -> (u32, u32) {
        (self.group, self.instance)
    }
}

/// A parsed creation file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub model_type: u32,
    /// `skineffect1..3`, `skineffectseed1..3`, `skincolor1..3` (present).
    pub skin: Option<([u32; 3], [u32; 3], [[f32; 3]; 3])>,
    pub blocks: Vec<Block>,
}

/// The next `<name ...>` element of `s` (attributes allowed): its content
/// (empty for `<name/>`) and the rest of `s` after it.
fn element<'a>(s: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut from = 0;
    loop {
        let a = from + s[from..].find(&open)?;
        let after = a + open.len();
        match s[after..].chars().next()? {
            c if c == '>' || c == '/' || c.is_whitespace() => {
                let gt = after + s[after..].find('>')?;
                if s[..gt].ends_with('/') {
                    return Some(("", &s[gt + 1..]));
                }
                let b = gt + 1 + s[gt + 1..].find(&close)?;
                return Some((s[gt + 1..b].trim(), &s[b + close.len()..]));
            }
            _ => from = after,
        }
    }
}

fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    element(s, name).map(|(content, _)| content)
}

fn sections<'a>(s: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some((content, after)) = element(rest, name) {
        out.push(content);
        rest = after;
    }
    out
}

fn hex(s: &str) -> Option<u32> {
    let s = s.trim();
    u32::from_str_radix(s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s), 16).ok()
}

fn floats(s: &str) -> Vec<f32> {
    s.split(',').filter_map(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).collect()
}

fn vec3(s: Option<&str>) -> Option<Vec3> {
    let v = floats(s?);
    (v.len() >= 3).then(|| Vec3::new(v[0], v[1], v[2]))
}

/// Parse a `<sporemodel>` creation file (tolerant: unknown tags are skipped,
/// a block without a valid id, scale or position makes the file invalid).
pub fn parse(xml: &str) -> Option<Model> {
    let root = &xml[xml.find("<sporemodel")?..];
    let props = tag(root, "properties").unwrap_or("");
    let model_type = tag(props, "modeltype").and_then(hex).unwrap_or(0);
    let skin = (|| {
        let e = |i: u32| tag(props, &format!("skineffect{i}")).and_then(hex);
        let s = |i: u32| tag(props, &format!("skineffectseed{i}")).and_then(|v| v.parse::<u64>().ok()).map(|v| v as u32);
        let c = |i: u32| vec3(tag(props, &format!("skincolor{i}"))).map(|v| v.to_array());
        Some(([e(1)?, e(2)?, e(3)?], [s(1).unwrap_or(1234), s(2).unwrap_or(1234), s(3).unwrap_or(1234)], [c(1)?, c(2)?, c(3)?]))
    })();
    let mut blocks = Vec::new();
    for b in sections(root, "blockref") {
        let ids: Vec<u32> = tag(b, "blockid")?.split(',').filter_map(hex).collect();
        let (&group, &instance) = (ids.first()?, ids.get(1)?);
        let transform = tag(b, "transform")?;
        let scale = tag(transform, "scale")?.parse::<f32>().ok().filter(|s| s.is_finite() && *s > 0.0)?;
        let position = vec3(tag(transform, "position"))?;
        let orientation = tag(transform, "orientation").unwrap_or("");
        let rows = [
            vec3(tag(orientation, "row0")).unwrap_or(Vec3::X),
            vec3(tag(orientation, "row1")).unwrap_or(Vec3::Y),
            vec3(tag(orientation, "row2")).unwrap_or(Vec3::Z),
        ];
        let children = tag(b, "childlist").map(|c| sections(c, "childid").iter().filter_map(|v| v.parse().ok()).collect()).unwrap_or_default();
        let mut handles = Vec::new();
        if let Some(list) = tag(b, "handles") {
            let mut rest = list;
            while let Some(i) = rest.find("channel=\"") {
                let s = &rest[i + 9..];
                let Some(q) = s.find('"') else { break };
                let channel = hex(&s[..q]);
                let Some(gt) = s.find('>') else { break };
                let Some(lt) = s[gt..].find('<') else { break };
                let weight = s[gt + 1..gt + lt].trim().parse::<f32>().ok();
                if let (Some(c), Some(w)) = (channel, weight) {
                    handles.push((c, w));
                }
                rest = &s[gt + lt..];
            }
        }
        let paints = tag(b, "paintlist")
            .map(|list| {
                sections(list, "paint")
                    .iter()
                    .filter_map(|p| {
                        Some(RegionPaint {
                            region: tag(p, "paintregion").and_then(hex)?,
                            paint: tag(p, "paintid").and_then(hex)?,
                            color1: vec3(tag(p, "color1"))?.to_array(),
                            color2: vec3(tag(p, "color2"))?.to_array(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        blocks.push(Block {
            group,
            instance,
            scale,
            position,
            rows,
            children,
            symmetric: tag(b, "symmetric").and_then(|v| v.parse().ok()),
            limb_type: tag(transform, "limbtype").and_then(|v| v.parse().ok()),
            muscle: tag(transform, "musclescale").and_then(|v| v.parse().ok()),
            ball: tag(transform, "basemusclescale").and_then(|v| v.parse().ok()),
            handles,
            paints,
        });
    }
    let n = blocks.len();
    for b in &mut blocks {
        b.children.retain(|&c| c < n);
        b.symmetric = b.symmetric.filter(|&s| s < n);
    }
    Some(Model { model_type, skin, blocks })
}

impl Model {
    /// Parent of each block (from the child lists).
    pub fn parents(&self) -> Vec<Option<usize>> {
        let mut out = vec![None; self.blocks.len()];
        for (p, b) in self.blocks.iter().enumerate() {
            for &c in &b.children {
                if c != p && out[c].is_none() {
                    out[c] = Some(p);
                }
            }
        }
        out
    }
}

/// Rotation of a block's mesh in a frame: `frame R B^-1` (`basis` = the
/// part mesh's import rotation).
pub fn block_rotation(frame: Mat3, block: &Block, basis: Quat) -> Quat {
    let m = frame * block.rotation() * Mat3::from_quat(basis.inverse());
    let q = Quat::from_mat3(&m);
    if q.is_finite() && q.length_squared() > 1e-6 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// The part mesh's import rotation (`editor::assembled_mesh`).
fn basis_of(def: Option<&PartDefinition>) -> Quat {
    def.map_or(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2), crate::creature_limbs::mesh_basis)
}

/// Pair ids and mirror flags of symmetric twins: twins share a pair id; the
/// later twin of the same part is the reflected mesh (a left / right model
/// pair is two parts, neither reflected).
fn twins(model: &Model) -> Vec<(u32, bool)> {
    let mut out = vec![(0u32, false); model.blocks.len()];
    let mut next = 0;
    for (i, b) in model.blocks.iter().enumerate() {
        let Some(j) = b.symmetric.filter(|&j| j != i) else { continue };
        if j < i && out[j].0 != 0 {
            out[i] = (out[j].0, model.blocks[j].key() == b.key());
        } else if out[i].0 == 0 {
            next += 1;
            out[i].0 = next;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Rigid creations
// ---------------------------------------------------------------------------

/// Spore model space -> rigid editor space: (x, z, -y) (the front -Y goes to
/// +Z).
pub fn rigid_frame() -> Mat3 {
    Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y)
}

/// The rigid editor class of a creation: the setup whose name hashes to its
/// `modeltype`, else the creation type's class template.
pub fn rigid_kind(creation_type: u32, model_type: u32) -> Option<SetupKind> {
    let by_name = SetupKind::ALL
        .into_iter()
        .filter(|k| k.is_rigid())
        .find(|k| crate::world_objects::name_id(k.setup_name()) == model_type);
    by_name.or(match creation_type {
        VCL => Some(SetupKind::Vehicle),
        BLD => Some(SetupKind::Building),
        UFO => Some(SetupKind::Ufo),
        _ => None,
    })
}

/// What a conversion could not carry over.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub blocks: usize,
    /// Block parts with no installed definition (group, instance).
    pub unresolved: Vec<(u32, u32)>,
    pub vertebrae: usize,
    pub limbs: usize,
    pub parts: usize,
    /// Parts whose offset was clamped to the editor range.
    pub clamped: usize,
}

/// A rigid creation as the design of its editor class.
pub fn rigid_design(model: &Model, kind: SetupKind, catalog: &PartCatalog, name: &str) -> (RigidDesign, Report) {
    let group = kind.part_group();
    let frame = rigid_frame();
    let pairs = twins(model);
    let mut report = Report { blocks: model.blocks.len(), ..Default::default() };
    let mut parts = Vec::new();
    // `EditorRigblock::mpParent`: the file's `childlist`s give every block's parent (the block's id is its file
    // index + 1; a parent that has no installed definition is not in the design: its children are parentless).
    let resolved: Vec<bool> = model.blocks.iter().map(|b| b.group == group && catalog.by_definition_in(b.group, b.instance).is_some()).collect();
    let parents = model.parents();
    for (i, b) in model.blocks.iter().enumerate() {
        let Some(def) = catalog.by_definition_in(b.group, b.instance).filter(|_| b.group == group) else {
            report.unresolved.push(b.key());
            continue;
        };
        let (pair, mirrored) = pairs[i];
        let parent = parents[i].filter(|&p| resolved[p]).map_or(0, |p| p as u32 + 1);
        let block = RigidBlock {
            position: (frame * b.position).to_array(),
            rotation: block_rotation(frame, b, basis_of(Some(def))).to_array(),
            stretch: [1.0; 3],
            pair,
            mirrored,
            paints: b.paints.clone(),
            handles: b.handles.clone(),
            id: i as u32 + 1,
            parent,
        };
        parts.push(VehiclePart {
            instance: def.model.instance_id,
            definition_id: Some(def.key.instance_id),
            slot: 0,
            yaw: 0.0,
            scale: b.scale,
            offset: [0.0; 3],
            block: Some(block),
        });
    }
    report.parts = parts.len();
    (RigidDesign { name: name.to_string(), parts }, report)
}

/// Back to the file's block transform (position, rows) from a converted block
/// (the round trip of the tests).
#[cfg_attr(not(test), allow(dead_code))]
pub fn rigid_block_back(block: &RigidBlock, basis: Quat) -> (Vec3, [Vec3; 3]) {
    let frame = rigid_frame();
    let p = frame.transpose() * block.position();
    let r = frame.transpose() * Mat3::from_quat(block.rotation()) * Mat3::from_quat(basis);
    (p, [r.x_axis, r.y_axis, r.z_axis])
}

// ---------------------------------------------------------------------------
// Creatures
// ---------------------------------------------------------------------------

/// The skin the profile is solved for: player units per creature unit and the
/// starter's template and vertebra balls (`None`: the skin builds a ball on
/// every profile point, `creature_skin_view::records_from_profile`).
#[derive(Clone)]
pub struct SkinFit<'a> {
    pub units: f32,
    pub native: Option<NativeFit<'a>>,
}

#[derive(Clone)]
pub struct NativeFit<'a> {
    pub template: &'a crate::baked_model::BodyTemplate,
    pub balls: Vec<crate::creature_skin_view::NativeBall>,
}

impl SkinFit<'static> {
    /// The skin of a profile-built body (no installed starter).
    pub fn direct() -> Self {
        Self { units: crate::creature_skin_view::unit(), native: None }
    }
}

impl<'a> SkinFit<'a> {
    /// The running game's skin.
    pub fn of(originals: &'a crate::Originals) -> Self {
        let Some(template) = originals.baked_body.as_ref() else { return SkinFit::direct() };
        match template.skin.as_ref().filter(|s| !s.native.is_empty()) {
            Some(skin) => Self { units: skin.unit, native: Some(NativeFit { template, balls: skin.native.clone() }) },
            None => SkinFit::direct(),
        }
    }

    /// Creature space -> player frame.
    pub fn to_player(&self, v: Vec3) -> Vec3 {
        crate::creature_editor::spore_to_bevy(v) * self.units + Vec3::Y * crate::creature_cell_convert::GROUND_Y
    }
}

/// `profile_at` of `baked_model` (linear along the points by index).
fn profile_at(body: &BodyProfile, t: f32) -> (Vec3, f32) {
    let n = body.spine.len();
    if n == 0 {
        return (Vec3::ZERO, 1.0);
    }
    if n == 1 {
        return (Vec3::from_array(body.spine[0].position), body.spine[0].radius);
    }
    let f = t.clamp(0.0, 1.0) * (n - 1) as f32;
    let i = (f.floor() as usize).min(n - 2);
    let u = (f - i as f32).clamp(0.0, 1.0);
    (
        Vec3::from_array(body.spine[i].position).lerp(Vec3::from_array(body.spine[i + 1].position), u),
        body.spine[i].radius * (1.0 - u) + body.spine[i + 1].radius * u,
    )
}

/// `values` resampled to `n` evenly spaced points along their index.
fn resample<T: Copy>(values: &[T], n: usize, lerp: impl Fn(T, T, f32) -> T) -> Vec<T> {
    if values.len() == n || values.len() < 2 || n < 2 {
        return values.to_vec();
    }
    (0..n)
        .map(|k| {
            let f = k as f32 * (values.len() - 1) as f32 / (n - 1) as f32;
            let i = (f.floor() as usize).min(values.len() - 2);
            lerp(values[i], values[i + 1], f - i as f32)
        })
        .collect()
}

/// The spine: the longest chain of vertebrae (tail first: the end farther
/// back, +Y in Spore space).
pub fn spine_chain(model: &Model) -> Vec<usize> {
    let is_v = |i: usize| model.blocks[i].group == CREATURE_PARTS && model.blocks[i].instance == VERTEBRA;
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
    for (p, b) in model.blocks.iter().enumerate() {
        if !is_v(p) {
            continue;
        }
        adj.entry(p).or_default();
        for &c in &b.children {
            if is_v(c) && c != p {
                adj.entry(p).or_default().push(c);
                adj.entry(c).or_default().push(p);
            }
        }
    }
    let Some(&start) = adj.keys().min() else { return Vec::new() };
    let far = |from: usize| -> (usize, HashMap<usize, usize>) {
        let mut prev = HashMap::new();
        let mut seen = vec![from];
        let mut queue = VecDeque::from([from]);
        let mut last = from;
        while let Some(n) = queue.pop_front() {
            last = n;
            for &m in adj.get(&n).map(Vec::as_slice).unwrap_or(&[]) {
                if !seen.contains(&m) {
                    seen.push(m);
                    prev.insert(m, n);
                    queue.push_back(m);
                }
            }
        }
        (last, prev)
    };
    let (a, _) = far(start);
    let (b, prev) = far(a);
    let mut chain = vec![b];
    while let Some(&p) = prev.get(chain.last().unwrap()) {
        chain.push(p);
    }
    let y = |i: usize| model.blocks[i].position.y;
    if chain.len() > 1 && y(chain[0]) < y(*chain.last().unwrap()) {
        chain.reverse();
    }
    chain
}

/// The body profile whose skin follows the vertebrae (`centres` / `radii`:
/// the balls' support centres and radii in the player frame, tail first).
/// With the starter's template the skin builds one ball per profile point
/// (`creature_skin_view::native_samples`, at least the starter's nine): the
/// spine is resampled to that count, each point's radius is solved exactly
/// and its position by a few fixed-point steps on the template's own
/// deformation (`BodyTemplate::deform`), so every skin ball lands on its
/// vertebra's ball.
pub fn solve_profile(fit: &SkinFit, centres: &[Vec3], radii: &[f32], scales: &[f32]) -> BodyProfile {
    let max = crate::creature_editor_edit::MAX_VERTEBRAE;
    let default = BodyProfile::default();
    let mut body = match &fit.native {
        Some(native) => {
            let n = centres.len().clamp(native.balls.len().min(max), max);
            let centres = resample(centres, n, |a, b, u| a.lerp(b, u));
            let radii = resample(radii, n, |a, b, u| a + (b - a) * u);
            let samples = crate::creature_skin_view::native_samples(&native.balls, n);
            // Profile point k (tail first) carries the sample at t = k / (n - 1).
            let sample = |k: usize| &samples[n - 1 - k];
            let t = |k: usize| if n > 1 { k as f32 / (n - 1) as f32 } else { 0.5 };
            let mut spine: Vec<SpinePoint> = (0..n)
                .map(|k| {
                    let (old_c, old_r) = profile_at(&default, t(k));
                    SpinePoint {
                        position: (centres[k] - sample(k).centre + old_c).to_array(),
                        radius: radii[k] * old_r / sample(k).radius.max(1e-4),
                    }
                })
                .collect();
            for _ in 0..8 {
                let body = BodyProfile { spine: spine.clone() }.validated();
                if body.spine.len() != n {
                    break;
                }
                for (k, point) in spine.iter_mut().enumerate() {
                    let at = native.template.deform(sample(k).centre, t(k), &body);
                    let next = Vec3::from_array(point.position) + (centres[k] - at);
                    if next.is_finite() {
                        point.position = next.to_array();
                    }
                }
            }
            BodyProfile { spine }
        }
        None => {
            let n = centres.len().min(max);
            let pick: Vec<usize> =
                if centres.len() > n { (0..n).map(|k| k * (centres.len() - 1) / (n - 1).max(1)).collect() } else { (0..n).collect() };
            let spine = pick
                .iter()
                .map(|&i| SpinePoint { position: centres[i].to_array(), radius: scales[i] * crate::creature_editor_edit::VERTEBRA_UNIT_RADIUS })
                .collect();
            BodyProfile { spine }
        }
    };
    // Points closer than the profile allows are merged.
    body.spine.dedup_by(|b, a| Vec3::from_array(a.position).distance_squared(Vec3::from_array(b.position)) < 0.0001);
    body
}

/// What the converter needs of each part's import (`editor::import_definition`
/// draws a creature part at `model.scale x 0.5 x p.scale`).
pub type ImportScale<'a> = dyn Fn(&PartDefinition) -> Option<f32> + 'a;

/// `PartInstance::scale` drawing a part at `modelScale x scale` creature units
/// (`creature_limbs::skin_scale` with the import scale given).
fn part_scale(def: &PartDefinition, import: &ImportScale, units: f32, block_scale: f32) -> f32 {
    match import(def) {
        Some(s) if s > 0.0 => def.model_scale.max(1e-3) * block_scale * units / (s * 0.5),
        _ => block_scale,
    }
}

/// The creature of a creation file on a fresh record.
pub fn creature_record(
    model: &Model,
    name: &str,
    catalog: &PartCatalog,
    fit: &SkinFit,
    import: &ImportScale,
    slots: &dyn Fn(&BodyProfile) -> [Vec3; 8],
) -> Option<(RunProgress, Report)> {
    let blocks = &model.blocks;
    let mut report = Report { blocks: blocks.len(), ..Default::default() };
    let chain = spine_chain(model);
    if chain.len() < 2 {
        return None;
    }
    report.vertebrae = chain.len();
    let frame = crate::creature_limbs::spore_to_bevy_mat();
    // Spine.
    let records: Vec<crate::creature_skin::SkinRecord> = chain
        .iter()
        .map(|&i| crate::creature_skin::SkinRecord {
            scale: blocks[i].scale,
            orientation: blocks[i].rows,
            position: blocks[i].position,
            ..Default::default()
        })
        .collect();
    // ADAPTED: the creature is centred front to back on the stand (the
    // profile keeps its points within +-3 player units, `BodyProfile`; a long
    // creature such as Snoremal reaches past that from the file's origin).
    let (z0, z1) = records.iter().map(|r| fit.to_player(r.vertebra_ball().0).z).fold((f32::MAX, f32::MIN), |(a, b), z| (a.min(z), b.max(z)));
    let shift = Vec3::Z * -(z0 + z1) * 0.5;
    let to_player = |v: Vec3| fit.to_player(v) + shift;
    let centres: Vec<Vec3> = records.iter().map(|r| to_player(r.vertebra_ball().0)).collect();
    let radii: Vec<f32> = records.iter().map(|r| r.vertebra_ball().1 * fit.units).collect();
    let scales: Vec<f32> = records.iter().map(|r| r.scale).collect();
    let body = solve_profile(fit, &centres, &radii, &scales);
    if body.problem().is_some() || body.validated() == BodyProfile::default() {
        // Out of the profile's range: the skin would fall back to the starter.
        return None;
    }
    let slot = slots(&body);
    let place = |p: Vec3, report: &mut Report| -> (u32, [f32; 3]) {
        let bone = (0..8u32).min_by(|a, b| slot[*a as usize].distance_squared(p).total_cmp(&slot[*b as usize].distance_squared(p))).unwrap_or(0);
        let raw = p - slot[bone as usize];
        let offset = crate::game::clamp_offset(raw.to_array(), crate::game::OFFSET_LIMIT);
        if Vec3::from_array(offset).distance(raw) > 1e-4 {
            report.clamped += 1;
        }
        (bone, offset)
    };
    let pairs = twins(model);
    let parents = model.parents();
    let in_spine = |i: usize| chain.contains(&i) || (blocks[i].group == CREATURE_PARTS && blocks[i].instance == VERTEBRA);
    let is_limb = |i: usize| blocks[i].limb_type.is_some();
    let def_of = |i: usize| catalog.by_definition_in(blocks[i].group, blocks[i].instance);
    let mut used = vec![false; blocks.len()];
    let mut parts: Vec<PartInstance> = Vec::new();

    // Limbs: chains of limb blocks from a root whose parent is not a limb
    // block; extra limb children start branch limbs.
    let mut queue: VecDeque<(usize, u32, u32)> =
        (0..blocks.len()).filter(|&i| is_limb(i) && parents[i].is_none_or(|p| !is_limb(p))).map(|i| (i, 0, 0)).collect();
    let mut limb_ids: HashMap<usize, (u32, usize)> = HashMap::new(); // root block -> (id, part index)
    let mut next_id = 1u32;
    let item_skin = crate::world_objects::name_id("limb_assembly_x_leg_09");
    let item_rigid = crate::world_objects::name_id("limb_assembly_exo_l_01");
    while let Some((root, parent_id, parent_joint)) = queue.pop_front() {
        let mut segs = vec![root];
        loop {
            let cur = *segs.last().unwrap();
            let limb_children: Vec<usize> = blocks[cur].children.iter().copied().filter(|&c| is_limb(c) && !segs.contains(&c)).collect();
            let Some(&next) = limb_children.first() else { break };
            for &other in &limb_children[1..] {
                queue.push_back((other, u32::MAX, segs.len() as u32));
            }
            segs.push(next);
        }
        let last = *segs.last().unwrap();
        // The end part: the foot / hand child of the last segment.
        let end = blocks[last]
            .children
            .iter()
            .copied()
            .filter(|&c| !is_limb(c) && !in_spine(c))
            .max_by_key(|&c| def_of(c).map_or(0, |d| (d.abilities.foot > 0 || d.abilities.grasper > 0) as u32));
        let mut joints_c: Vec<Vec3> = segs.iter().map(|&s| blocks[s].position).collect();
        let end_joint = match end {
            Some(e) => blocks[e].position,
            None => {
                let len = if segs.len() > 1 { blocks[last].position.distance(blocks[segs[segs.len() - 2]].position) } else { 0.3 };
                blocks[last].position - blocks[last].rows[1] * len
            }
        };
        joints_c.push(end_joint);
        let root_p = to_player(joints_c[0]);
        let joints: Vec<[f32; 3]> = joints_c.iter().map(|j| (to_player(*j) - root_p).to_array()).collect();
        let mut balls: Vec<f32> = segs.iter().map(|&s| blocks[s].ball.unwrap_or(1.0)).collect();
        balls.push(end.and_then(|e| blocks[e].ball).unwrap_or(1.0));
        let rigid_mask: Vec<bool> = segs.iter().map(|&s| blocks[s].limb_type == Some(2)).collect();
        let any_rigid = rigid_mask.iter().any(|r| *r);
        // Twins: the root's symmetric twin, if already converted.
        let twin = blocks[root].symmetric.and_then(|t| limb_ids.get(&t).copied());
        let id = match twin {
            Some((tid, _)) => tid ^ TWIN_BIT,
            None => {
                let id = next_id;
                next_id += 1;
                id
            }
        };
        let parent = match parent_id {
            0 => 0,
            u32::MAX => parents[root].and_then(|p| limb_ids.iter().find(|(r, _)| {
                // The limb whose chain holds the parent segment.
                let mut s = **r;
                loop {
                    if s == p {
                        return true;
                    }
                    match blocks[s].children.iter().copied().find(|&c| is_limb(c)) {
                        Some(n) => s = n,
                        None => return false,
                    }
                }
            }).map(|(_, (id, _))| *id)).unwrap_or(0),
            other => other,
        };
        let mut limb = Limb {
            id,
            joints: joints.clone(),
            balls,
            muscle: segs.iter().map(|&s| blocks[s].muscle.unwrap_or(0.5)).collect(),
            segments: segs.iter().map(|&s| blocks[s].instance).collect(),
            tentacle: blocks[root].limb_type == Some(1),
            parent,
            parent_joint: if parent != 0 { parent_joint } else { 0 },
            ..Default::default()
        };
        if any_rigid {
            limb.rigid = true;
            if !rigid_mask.iter().all(|r| *r) {
                limb.rigid_segments = rigid_mask.clone();
            }
            limb.rest = joints;
            limb.frames = segs
                .iter()
                .map(|&s| if blocks[s].limb_type == Some(2) { block_rotation(frame, &blocks[s], basis_of(def_of(s))).to_array() } else { [0.0, 0.0, 0.0, 1.0] })
                .collect();
            limb.ball_range = [0.7, 1.5];
            limb.length_range = [0.035, 3.75];
        }
        let (bone, offset) = place(root_p, &mut report);
        let mut limb_part = PartInstance::new(0, bone).with_definition(if any_rigid { item_rigid } else { item_skin });
        limb_part.offset = offset;
        limb_part.edits.limb = Some(limb);
        if let Some((_, twin_index)) = twin {
            let pair = match parts[twin_index].pair_id {
                0 => parts.iter().map(|p| p.pair_id).max().unwrap_or(0).max(pairs.iter().map(|p| p.0).max().unwrap_or(0)) + 1,
                p => p,
            };
            parts[twin_index].pair_id = pair;
            limb_part.pair_id = pair;
            limb_part.mirrored = true;
        }
        limb_ids.insert(root, (id, parts.len()));
        parts.push(limb_part);
        report.limbs += 1;
        for &s in &segs {
            used[s] = true;
        }
        // Rigid segment parts ride their joints.
        for (k, &s) in segs.iter().enumerate() {
            if blocks[s].limb_type != Some(2) {
                continue;
            }
            let Some(def) = def_of(s) else {
                report.unresolved.push(blocks[s].key());
                continue;
            };
            let mut p = PartInstance::new(def.model.instance_id, bone).with_definition(def.key.instance_id);
            p.edits.on_limb = id;
            p.edits.segment = k as u32 + 1;
            p.edits.handles = blocks[s].handles.clone();
            p.scale = part_scale(def, import, fit.units, blocks[s].scale);
            parts.push(p);
        }
        if let Some(e) = end {
            used[e] = true;
            match def_of(e) {
                Some(def) => {
                    let mut p = PartInstance::new(def.model.instance_id, bone).with_definition(def.key.instance_id);
                    p.edits.on_limb = id;
                    p.edits.handles = blocks[e].handles.clone();
                    p.edits.set_ring(block_rotation(frame, &blocks[e], basis_of(Some(def))));
                    p.scale = part_scale(def, import, fit.units, blocks[e].scale);
                    p.mirrored = twin.is_some() && blocks[e].symmetric.is_some_and(|t| blocks[t].key() == blocks[e].key());
                    p.pair_id = pairs[e].0;
                    parts.push(p);
                }
                None => report.unresolved.push(blocks[e].key()),
            }
        }
    }

    // Every other block is a part on the body.
    for (i, b) in blocks.iter().enumerate() {
        if used[i] || in_spine(i) {
            continue;
        }
        let Some(def) = def_of(i) else {
            report.unresolved.push(b.key());
            continue;
        };
        let (bone, offset) = place(to_player(b.position), &mut report);
        let mut p = PartInstance::new(def.model.instance_id, bone).with_definition(def.key.instance_id);
        p.offset = offset;
        p.edits.handles = b.handles.clone();
        p.edits.set_ring(block_rotation(frame, b, basis_of(Some(def))));
        p.scale = part_scale(def, import, fit.units, b.scale);
        (p.pair_id, p.mirrored) = pairs[i];
        parts.push(p);
    }
    // End and rigid segment parts take their limb's slot and joints.
    let scale_of: HashMap<u32, f32> =
        parts.iter().filter(|p| p.edits.segment != 0).filter_map(|p| p.definition_id.map(|d| (d, p.scale))).collect();
    let base = |p: &PartInstance| p.definition_id.and_then(|d| scale_of.get(&d).copied()).unwrap_or(p.scale);
    crate::creature_limbs::settle_ends(&mut parts, None, false, &base);
    report.parts = parts.len();

    let mut run = RunProgress::default();
    run.body = body;
    run.parts = parts;
    run.creature_creation.name = name.to_string();
    if let Some((brushes, seeds, colors)) = model.skin {
        let paint = &mut run.creature_creation.paint;
        paint.painted = true;
        paint.brushes = brushes;
        paint.colors = colors;
        paint.seeds = Some(seeds);
    }
    run.sync_legacy_accessory();
    Some((run, report))
}

/// The import scale `editor::import_definition` gives a creature part (its
/// category's cap over the mesh's largest extent), read from the installed
/// model so a conversion needs no imported meshes.
pub fn import_scale(index: &ResourceIndex, def: &PartDefinition) -> Option<f32> {
    let mut scale = def.model_scale * 5.0;
    if def.key.group_id != CREATURE_PARTS {
        return Some(scale);
    }
    let cap = match def.category() {
        "Eyes" => 0.45,
        "Mouths" => 0.7,
        "Feet" => 0.7,
        "Hands" => 0.65,
        "Movement" => 1.0,
        _ => 0.75,
    };
    let bytes = index.read(def.model).ok()?;
    let mut stats = crate::rw4::MeshStats::default();
    let rotation = basis_of(Some(def));
    let offset = Vec3::from_array(def.model_offset);
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for m in crate::rw4::extract_meshes(&bytes, &mut stats) {
        for p in &m.positions {
            let v = rotation * (Vec3::from_array(*p) + offset);
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    let extent = (hi - lo).max_element();
    if extent.is_finite() && extent > 0.0 {
        scale = scale.min(cap * 2.0 / extent);
    }
    Some(scale)
}

// ---------------------------------------------------------------------------
// Sporepedia
// ---------------------------------------------------------------------------

/// A Maxis creation's file: (creation type, name, XML).
pub fn creation_file(index: &ResourceIndex, source: &crate::front_end::thumbs::Source, name: &str) -> Option<(u32, String, String)> {
    use crate::front_end::thumbs::Source;
    match source {
        Source::Png(key) if key.type_id != 0 => {
            let d = crate::front_end::png_creation::decode_installed(index, *key)?;
            let name = if d.header.name.trim().is_empty() { name.to_string() } else { d.header.name.clone() };
            Some((d.header.type_id, name, d.model))
        }
        Source::Installed(key) => {
            let bytes = index.read(*key).ok()?;
            Some((key.type_id, name.to_string(), String::from_utf8_lossy(&bytes).into_owned()))
        }
        _ => None,
    }
}

/// The editor and scratch record a creation file opens with.
pub fn scratch_of(originals: &crate::Originals, creation_type: u32, name: &str, xml: &str) -> Option<(SetupKind, RunProgress, Report)> {
    let model = parse(xml)?;
    let catalog = &originals.part_catalog;
    if creation_type == CRT {
        let (run, report) = creature_of(originals, &model, name)?;
        // Before the creator has prepared the starter's body (the galaxy
        // screen), the profile is fitted again once it has (`refit`).
        let pending = if SkinFit::of(originals).native.is_none() { Some((model, name.to_string(), run.parts.clone())) } else { None };
        *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = pending;
        return Some((SetupKind::Creature, run, report));
    }
    let kind = rigid_kind(creation_type, model.model_type)?;
    let (design, report) = rigid_design(&model, kind, catalog, name);
    if design.parts.is_empty() {
        return None;
    }
    let mut run = RunProgress::default();
    run.rigid.designs.insert(kind.setup_name().to_string(), design);
    Some((kind, run, report))
}

/// The creature of a creation file for the running game's skin.
fn creature_of(originals: &crate::Originals, model: &Model, name: &str) -> Option<(RunProgress, Report)> {
    let fit = SkinFit::of(originals);
    let index = &originals.resource_index;
    let cache = std::cell::RefCell::new(HashMap::<ResourceKey, Option<f32>>::new());
    let import = |def: &PartDefinition| *cache.borrow_mut().entry(def.key).or_insert_with(|| import_scale(index, def));
    let slots = |body: &BodyProfile| -> [Vec3; 8] { std::array::from_fn(|b| crate::baked_model::attachment_point(originals, body, b as u32)) };
    creature_record(model, name, &originals.part_catalog, &fit, &import, &slots)
}

/// A creature converted before the starter's body template existed: its
/// file, name and the parts it was given (the scratch is refitted only while
/// it is untouched).
static PENDING: std::sync::Mutex<Option<(Model, String, Vec<PartInstance>)>> = std::sync::Mutex::new(None);

/// Once the creator has prepared the starter's body (`baked_model::prepare_body`
/// gives it the skin's vertebra balls), convert a pending Maxis creature again
/// so its profile is solved for that skin (the profile is a deformation of the
/// starter's balls; a profile fitted without them misplaces the torso and the
/// limbs lose their connection to it).
pub fn refit(mut run: ResMut<RunProgress>, originals: Res<crate::Originals>, mut state: ResMut<crate::editor::EditorState>) {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let Some((_, name, parts)) = pending.as_ref() else { return };
    let same = |a: &[PartInstance], b: &[PartInstance]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.definition_id == y.definition_id);
    if !same(&run.parts, parts) || run.creature_creation.name != *name {
        // The scratch is gone or was edited: nothing to refit.
        if !crate::front_end::creators::menu_session() {
            *pending = None;
        }
        return;
    }
    if SkinFit::of(&originals).native.is_none() {
        return;
    }
    let (model, name, _) = pending.take().unwrap();
    let Some((fresh, report)) = creature_of(&originals, &model, &name) else { return };
    info!("Maxis creature \"{name}\" refitted to the starter's skin: {} parts, {} offsets clamped", report.parts, report.clamped);
    run.body = fresh.body;
    run.parts = fresh.parts;
    run.sync_legacy_accessory();
    state.visuals_dirty = true;
}

/// Edit on a Maxis creation: its editor on a scratch copy (accepting keeps it
/// as a new creation of the player's).
pub fn edit_launch(entry: &crate::front_end::pedia::Entry, originals: &crate::Originals) -> Option<crate::front_end::creators::MenuLaunch> {
    if !entry.is_maxis() {
        return None;
    }
    let (creation_type, name, xml) = creation_file(&originals.resource_index, &entry.source, &entry.name)?;
    let (kind, scratch, report) = scratch_of(originals, creation_type, &name, &xml)?;
    info!(
        "Maxis creation \"{name}\" -> {}: {} blocks, {} parts, {} limbs, {} vertebrae, {} unresolved, {} offsets clamped",
        kind.setup_name(),
        report.blocks,
        report.parts,
        report.limbs,
        report.vertebrae,
        report.unresolved.len(),
        report.clamped
    );
    Some(crate::front_end::creators::MenuLaunch { kind, scratch: Box::new(scratch), replace: None })
}

/// The outfitter on a Maxis creature.
pub fn outfit_launch(entry: &crate::front_end::pedia::Entry, kind: SetupKind, originals: &crate::Originals) -> Option<crate::front_end::creators::MenuLaunch> {
    let launch = edit_launch(entry, originals).filter(|l| l.kind == SetupKind::Creature)?;
    let scratch = crate::front_end::creators::outfit_scratch(*launch.scratch, &originals.part_catalog);
    Some(crate::front_end::creators::MenuLaunch { kind, scratch: Box::new(scratch), replace: None })
}

/// Whether Edit can open this Maxis entry (a creature, building, vehicle or
/// UFO with its creation file).
pub fn editable(entry: &crate::front_end::pedia::Entry) -> bool {
    use crate::front_end::thumbs::Source;
    entry.category <= 3 && matches!(entry.source, Source::Png(k) if k.type_id != 0) || matches!(entry.source, Source::Installed(_))
}

/// Capture option `--front-end maxis:<creatures|buildings|vehicles|spaceships>[:n]`:
/// the n-th installed Maxis creation of that type (PNG order) in its editor.
pub fn demo_launch(originals: &crate::Originals, arg: &str) -> Option<crate::front_end::creators::MenuLaunch> {
    let (kind, n) = arg.split_once(':').unwrap_or((arg, "0"));
    let group = match kind {
        "buildings" => 0x4063_6200,
        "vehicles" => 0x4064_6200,
        "spaceships" => 0x4065_6200,
        _ => 0x4062_6200,
    };
    let mut keys: Vec<ResourceKey> = originals
        .resource_index
        .records
        .keys()
        .filter(|k| k.type_id == crate::front_end::maxis::PNG_TYPE && k.group_id == group)
        .copied()
        .collect();
    keys.sort_by_key(|k| k.instance_id);
    let key = *keys.get(n.parse::<usize>().unwrap_or(0) % keys.len().max(1))?;
    let entry = crate::front_end::pedia::Entry {
        category: 0,
        name: String::new(),
        author: "Maxis".into(),
        kind: String::new(),
        description: String::new(),
        tags: Vec::new(),
        created: None,
        order: 0,
        parts: 0,
        source: crate::front_end::thumbs::Source::Png(key),
        thumb: String::new(),
        origin: crate::front_end::pedia::Origin::Maxis,
    };
    edit_launch(&entry, originals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_catalog::PartDefinition;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?><sporemodel><formatversion>16</formatversion><properties>
      <modeltype>0x9ea3031a</modeltype><skineffect1>0x0156ac06</skineffect1><skineffect2>0x52811f23</skineffect2>
      <skineffect3>0xf09862ba</skineffect3><skineffectseed1>1234</skineffectseed1><skineffectseed2>7</skineffectseed2>
      <skineffectseed3>1234</skineffectseed3><skincolor1>0.9,0.7,0.2</skincolor1><skincolor2>0.9,0.4,0.2</skincolor2>
      <skincolor3>1,1,1</skincolor3></properties><blocks count="7">
      <blockref><blockid>0x40626000, 0xd05c53a3</blockid><transform><scale>1</scale><position>0,0.2,0.8</position>
        <orientation><row0>1,0,0</row0><row1>0,1,0</row1><row2>0,0,1</row2></orientation></transform>
        <childlist count="3"><childid>1</childid><childid>2</childid><childid>5</childid></childlist></blockref>
      <blockref><blockid>0x40626000, 0xd05c53a3</blockid><transform><scale>1.2</scale><position>0,-0.1,0.85</position>
        <orientation><row0>1,0,0</row0><row1>0,1,0</row1><row2>0,0,1</row2></orientation></transform>
        <childlist count="1"><childid>6</childid></childlist></blockref>
      <blockref><blockid>0x40626000, 0x2051ae2d</blockid><transform><limbtype>1</limbtype><musclescale>0.35</musclescale>
        <basemusclescale>1.3</basemusclescale><scale>1</scale><position>0.2,0.2,0.7</position>
        <orientation><row0>1,0,0</row0><row1>0,0,1</row1><row2>0,-1,0</row2></orientation></transform>
        <childlist count="1"><childid>3</childid></childlist><symmetric>5</symmetric>
        <handles count="1"><weight channel="0xf320ba37">0.14</weight></handles></blockref>
      <blockref><blockid>0x40626000, 0x2051ae2d</blockid><transform><limbtype>1</limbtype><musclescale>0.3</musclescale>
        <basemusclescale>0.7</basemusclescale><scale>1</scale><position>0.3,0.2,0.4</position>
        <orientation><row0>1,0,0</row0><row1>0,0,1</row1><row2>0,-1,0</row2></orientation></transform>
        <childlist count="1"><childid>4</childid></childlist></blockref>
      <blockref><blockid>0x40626000, 0xc5d43f65</blockid><transform><basemusclescale>0.55</basemusclescale><scale>0.5</scale>
        <position>0.3,0.2,0.1</position><orientation><row0>1,0,0</row0><row1>0,1,0</row1><row2>0,0,1</row2></orientation>
        </transform></blockref>
      <blockref><blockid>0x40626000, 0x2051ae2d</blockid><transform><limbtype>1</limbtype><musclescale>0.35</musclescale>
        <basemusclescale>1.3</basemusclescale><scale>1</scale><position>-0.2,0.2,0.7</position>
        <orientation><row0>1,0,0</row0><row1>0,0,-1</row1><row2>0,1,0</row2></orientation></transform><symmetric>2</symmetric></blockref>
      <blockref><blockid>0x40626000, 0x06fa3d43</blockid><transform><scale>1</scale><position>0,-0.4,0.9</position>
        <orientation><row0>0,1,0</row0><row1>-1,0,0</row1><row2>0,0,1</row2></orientation></transform>
        <paintlist count="1"><paint><paintregion>0x4</paintregion><paintid>0xd39dbe3e</paintid><color1>0.5,0.4,0.3</color1>
        <color2>0.3,0.2,0.1</color2></paint></paintlist></blockref>
      </blocks></sporemodel>"#;

    fn def(group: u32, instance: u32, foot: bool) -> PartDefinition {
        let mut d = PartDefinition::raw_model(ResourceKey { type_id: 0, group_id: group, instance_id: instance ^ 0x5555 });
        d.key = ResourceKey { type_id: crate::asset_catalog::PROP, group_id: group, instance_id: instance };
        d.is_null = false;
        d.abilities.foot = foot as u8;
        d
    }

    #[test]
    fn parses_blocks_limbs_twins_handles_and_paint() {
        let m = parse(XML).expect("parses");
        assert_eq!(m.model_type, 0x9EA3_031A);
        let (brushes, seeds, colors) = m.skin.expect("skin");
        assert_eq!(brushes, [0x0156_AC06, 0x5281_1F23, 0xF098_62BA]);
        assert_eq!(seeds, [1234, 7, 1234]);
        assert_eq!(colors[0], [0.9, 0.7, 0.2]);
        assert_eq!(m.blocks.len(), 7);
        assert_eq!(m.blocks[0].children, vec![1, 2, 5]);
        assert_eq!(m.blocks[2].limb_type, Some(1));
        assert_eq!((m.blocks[2].muscle, m.blocks[2].ball), (Some(0.35), Some(1.3)));
        assert_eq!(m.blocks[2].symmetric, Some(5));
        assert_eq!(m.blocks[2].handles, vec![(0xF320_BA37, 0.14)]);
        assert_eq!(m.blocks[4].ball, Some(0.55));
        assert_eq!(m.blocks[4].scale, 0.5, "<scale> is not <musclescale>");
        assert_eq!(m.blocks[6].paints, vec![RegionPaint { region: 4, paint: 0xD39D_BE3E, color1: [0.5, 0.4, 0.3], color2: [0.3, 0.2, 0.1] }]);
        assert_eq!(m.blocks[6].rows[1], Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(m.parents()[3], Some(2));
        // The spine runs tail (+Y) first.
        assert_eq!(spine_chain(&m), vec![0, 1]);
    }

    #[test]
    fn rigid_blocks_round_trip_and_classes_follow_the_model_type() {
        assert_eq!(rigid_kind(VCL, crate::world_objects::name_id("VehicleEconomicWater")), Some(SetupKind::VehicleEconomicWater));
        assert_eq!(rigid_kind(BLD, 0x72C4_9181), Some(SetupKind::BuildingEntertainment));
        assert_eq!(rigid_kind(UFO, 0x98E0_3C0D), Some(SetupKind::Ufo));
        assert_eq!(rigid_kind(VCL, 1), Some(SetupKind::Vehicle));
        let xml = XML.replace("0x40626000", "0x40646000");
        let m = parse(&xml).unwrap();
        let mut catalog = PartCatalog::default();
        for b in &m.blocks {
            if catalog.by_definition_in(b.group, b.instance).is_none() {
                catalog.parts.push(def(b.group, b.instance, false));
            }
        }
        let (design, report) = rigid_design(&m, SetupKind::VehicleMilitaryLand, &catalog, "Boat");
        assert_eq!((design.parts.len(), report.unresolved.len()), (7, 0));
        assert_eq!(design.name, "Boat");
        // The file's child lists are the blocks' stored parents (id = file index + 1, 0 = parentless).
        let links: Vec<(u32, u32)> = design.parts.iter().map(|p| p.block.as_ref().map(|b| (b.id, b.parent)).unwrap()).collect();
        for (i, parent) in m.parents().iter().enumerate() {
            assert_eq!(links[i], (i as u32 + 1, parent.map_or(0, |p| p as u32 + 1)), "block {i}");
        }
        assert_eq!(links[0].1, 0, "the root has no parent");
        assert_eq!(links[3].1, 3, "block 3 is the child of block 2");
        for (b, p) in m.blocks.iter().zip(&design.parts) {
            let block = p.block.as_ref().unwrap();
            let (pos, rows) = rigid_block_back(block, basis_of(catalog.by_definition_in(b.group, b.instance)));
            assert!(pos.distance(b.position) < 1e-5);
            for k in 0..3 {
                assert!(rows[k].distance(b.rows[k]) < 1e-4, "{k}: {:?} vs {:?}", rows[k], b.rows[k]);
            }
            assert_eq!(p.scale, b.scale);
        }
        // Front -Y goes to the editor's +Z, up stays up.
        assert!(design.parts[6].block.as_ref().unwrap().position[2] > 0.0);
        assert!(design.parts[0].block.as_ref().unwrap().position[1] > 0.5);
        // Twins of one part: one pair, the second reflected.
        let (a, b) = (design.parts[2].block.as_ref().unwrap(), design.parts[5].block.as_ref().unwrap());
        assert!(a.pair != 0 && a.pair == b.pair);
        assert!(!a.mirrored && b.mirrored);
        assert_eq!(design.parts[6].block.as_ref().unwrap().paints.len(), 1);
        // The morph handle weights travel with the block (drawn morphed).
        assert_eq!(design.parts[2].block.as_ref().unwrap().handles, vec![(0xF320_BA37, 0.14)]);
        assert_eq!(design.parts[2].block.as_ref().unwrap().reflected().handles, vec![(0xF320_BA37, 0.14)]);
        let key = crate::rigid_editor::morph_key(ResourceKey { type_id: 0, group_id: 0, instance_id: 0 }, &[(9, 1.5), (3, -1.0)]).unwrap();
        assert_eq!(key.weights, vec![(3, 0.0f32.to_bits()), (9, 1.0f32.to_bits())], "clamped and sorted");
        assert!(crate::rigid_editor::morph_key(ResourceKey { type_id: 0, group_id: 0, instance_id: 0 }, &[]).is_none());
    }

    #[test]
    fn a_long_spine_keeps_its_ball_count() {
        use crate::creature_skin_view::{native_samples, NativeBall};
        let native: Vec<NativeBall> =
            (0..9).map(|i| NativeBall { centre: Vec3::new(0.0, 0.0, i as f32), radius: 1.0 + i as f32, t: 1.0 - i as f32 / 8.0 }).collect();
        assert_eq!(native_samples(&native, 5), native, "a short profile: the starter's balls");
        let long = native_samples(&native, 17);
        assert_eq!(long.len(), 17);
        assert_eq!((long[0].t, long[16].t), (1.0, 0.0));
        assert!(long[0].centre.distance(native[0].centre) < 1e-5 && long[16].centre.distance(native[8].centre) < 1e-5);
        assert!((long[1].radius - 1.5).abs() < 1e-5, "halfway between the first two balls");
        assert_eq!(resample(&[0.0f32, 1.0], 5, |a, b, u| a + (b - a) * u), vec![0.0, 0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn a_creature_becomes_spine_parts_and_limbs() {
        let m = parse(XML).unwrap();
        let mut catalog = PartCatalog::default();
        catalog.parts.push(def(CREATURE_PARTS, 0xC5D4_3F65, true));
        catalog.parts.push(def(CREATURE_PARTS, 0x06FA_3D43, false));
        let fit = SkinFit::direct();
        let import = |_: &PartDefinition| Some(1.0);
        let slots = |body: &BodyProfile| -> [Vec3; 8] { std::array::from_fn(|b| body.slot(b as u32)) };
        let (run, report) = creature_record(&m, "Groob", &catalog, &fit, &import, &slots).expect("converts");
        assert_eq!(report.vertebrae, 2);
        assert_eq!(report.limbs, 2);
        assert!(report.unresolved.is_empty(), "{:?}", report.unresolved);
        assert_eq!(run.creature_creation.name, "Groob");
        assert!(run.creature_creation.paint.painted);
        assert_eq!(run.creature_creation.paint.seeds, Some([1234, 7, 1234]));
        // Tail first, head toward -Z; one ball per vertebra.
        assert_eq!(run.body.spine.len(), 2);
        assert!(run.body.spine[0].position[2] > run.body.spine[1].position[2]);
        assert!((run.body.spine[1].radius / run.body.spine[0].radius - 1.2).abs() < 1e-4);
        // Limb part, its twin, the feet (one per limb), the mouth.
        let limbs: Vec<&PartInstance> = run.parts.iter().filter(|p| p.edits.limb.is_some()).collect();
        assert_eq!(limbs.len(), 2);
        let (l0, l1) = (limbs[0].edits.limb.as_ref().unwrap(), limbs[1].edits.limb.as_ref().unwrap());
        assert_eq!(l1.id, l0.id ^ TWIN_BIT);
        assert!(limbs[1].mirrored && limbs[0].pair_id != 0 && limbs[0].pair_id == limbs[1].pair_id);
        assert_eq!(l0.joints.len(), 3, "two segments and the foot's joint");
        assert!(l0.tentacle);
        assert_eq!(l0.balls, vec![1.3, 0.7, 0.55]);
        assert_eq!(l0.muscle, vec![0.35, 0.3]);
        // The joints keep the file's geometry (player frame: x flipped, z up -> y).
        let u = fit.units;
        assert!((Vec3::from_array(l0.joints[2]) - Vec3::new(-0.1, -0.6, 0.0) * u).length() < 1e-4, "{:?}", l0.joints);
        let feet: Vec<&PartInstance> = run.parts.iter().filter(|p| p.edits.on_limb != 0).collect();
        assert_eq!(feet.len(), 2, "the twin's foot is added by settle_ends");
        // The mouth sits at its file position.
        let mouth = run.parts.iter().find(|p| p.definition_id == Some(0x06FA_3D43)).unwrap();
        let at = run.body.slot(mouth.bone) + Vec3::from_array(mouth.offset);
        let tail = crate::creature_skin::SkinRecord { scale: 1.0, position: Vec3::new(0.0, 0.2, 0.8), ..Default::default() }.vertebra_ball().0;
        let expected = fit.to_player(Vec3::new(0.0, -0.4, 0.9)) - fit.to_player(tail) + Vec3::from_array(run.body.spine[0].position);
        assert!(at.distance(expected) < 1e-4, "{at} vs {expected}");
        // Its rotation maps back to the file's rows.
        let q = mouth.rotation();
        let r = crate::creature_limbs::spore_to_bevy_mat().transpose() * Mat3::from_quat(q) * Mat3::from_quat(basis_of(catalog.by_definition_in(CREATURE_PARTS, 0x06FA_3D43)));
        assert!(r.y_axis.distance(Vec3::new(-1.0, 0.0, 0.0)) < 1e-4, "{r:?}");
    }

    /// Installed data: the morph handles of a Maxis vehicle's blocks apply to
    /// their meshes (`MAXIS_DEBUG=<n>`: the n-th vehicle PNG, default 0 =
    /// "Yankee Clipper", whose mast height is a handle).
    #[test]
    #[ignore = "reads the installed packages"]
    fn installed_vehicle_morphs_apply() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        let catalog = PartCatalog::from_index(&index);
        let n: usize = std::env::var("MAXIS_DEBUG").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let mut keys: Vec<ResourceKey> =
            index.records.keys().filter(|k| k.type_id == crate::front_end::maxis::PNG_TYPE && k.group_id == 0x4064_6200).copied().collect();
        keys.sort_by_key(|k| k.instance_id);
        let d = crate::front_end::png_creation::decode_installed(&index, keys[n]).unwrap();
        let m = parse(&d.model).unwrap();
        let (mut ok, mut with) = (0, 0);
        for (i, b) in m.blocks.iter().enumerate() {
            let Some(def) = catalog.by_definition_in(b.group, b.instance) else { continue };
            if b.handles.is_empty() {
                continue;
            }
            with += 1;
            let bytes = index.read(def.model).unwrap();
            let result = crate::rw4::extract_morph_data(&bytes).and_then(|data| {
                let mut meshes = crate::rw4::extract_meshes(&bytes, &mut crate::rw4::MeshStats::default());
                let before: Vec<[f32; 3]> = meshes.iter().flat_map(|m| m.positions.clone()).collect();
                crate::morph::apply_creation_handles(&mut meshes, &data, &b.handles)?;
                let after: Vec<[f32; 3]> = meshes.iter().flat_map(|m| m.positions.clone()).collect();
                let moved = before.iter().zip(&after).map(|(a, b)| Vec3::from_array(*a).distance(Vec3::from_array(*b))).fold(0.0f32, f32::max);
                Ok((data.handles.iter().map(|h| h.id).collect::<Vec<_>>(), moved))
            });
            match &result {
                Ok(_) => ok += 1,
                Err(_) => {}
            }
            println!("{} block {i} {} handles {:08X?}: {result:08X?}", d.header.name, def.name, b.handles.iter().map(|h| h.0).collect::<Vec<_>>());
        }
        println!("{ok} of {with} blocks with handles morph");
        assert!(ok * 2 >= with, "{ok} of {with}");
    }

    /// One converted creature in numbers (`MAXIS_DEBUG=<n>`, default the
    /// third PNG, "Snoremal"); its limbs are skinned (profile-built skin).
    #[test]
    #[ignore = "reads the installed packages"]
    fn installed_creature_in_numbers() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        let catalog = PartCatalog::from_index(&index);
        let n: usize = std::env::var("MAXIS_DEBUG").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
        let mut keys: Vec<ResourceKey> =
            index.records.keys().filter(|k| k.type_id == crate::front_end::maxis::PNG_TYPE && k.group_id == 0x4062_6200).copied().collect();
        keys.sort_by_key(|k| k.instance_id);
        let d = crate::front_end::png_creation::decode_installed(&index, keys[n]).unwrap();
        let m = parse(&d.model).unwrap();
        let fit = SkinFit::direct();
        let import = |def: &PartDefinition| import_scale(&index, def);
        let slots = |body: &BodyProfile| -> [Vec3; 8] { std::array::from_fn(|b| body.slot(b as u32)) };
        let (run, report) = creature_record(&m, &d.header.name, &catalog, &fit, &import, &slots).unwrap();
        println!("{} units {} {report:?}", d.header.name, fit.units);
        for p in &run.body.spine {
            println!("  spine {:?} r {}", p.position, p.radius);
        }
        for p in &run.parts {
            let def = p.definition_id.and_then(|d| catalog.by_definition_in(CREATURE_PARTS, d));
            let at = run.body.slot(p.bone) + Vec3::from_array(p.offset);
            println!(
                "  part {:<28} at {at:.3} scale {:.3} import {:?} mirrored {} pair {} on_limb {:X} limb {:?}",
                def.map_or("?".to_string(), |d| d.name.clone()),
                p.scale,
                def.and_then(|d| import(d)),
                p.mirrored,
                p.pair_id,
                p.edits.on_limb,
                p.edits.limb.as_ref().map(|l| (l.id, l.tentacle, l.joints.clone(), l.balls.clone()))
            );
        }
        // The skin with the limbs: vertices near each limb's middle joint.
        let records = crate::creature_skin_view::records_from_profile(&run.body);
        let limbs: Vec<crate::creature_limbs::LimbGeom> = run
            .parts
            .iter()
            .filter_map(|p| {
                let l = p.edits.limb.as_ref()?;
                let root = run.body.slot(p.bone) + Vec3::from_array(p.offset);
                Some(crate::creature_limbs::LimbGeom {
                    joints: l.joints.iter().map(|j| root + Vec3::from_array(*j)).collect(),
                    balls: l.balls.clone(),
                    muscle: l.muscle.clone(),
                    tentacle: l.tentacle,
                    skinned: Vec::new(),
                    parent: None,
                })
            })
            .collect();
        let mut all = records.clone();
        all.extend(crate::creature_limbs::limb_records(&limbs, &records, fit.units));
        let all = crate::creature_skin::order_records(all);
        let (_, mesh) = crate::creature_skin::build(&all, 0.042).expect("skin");
        for l in &limbs {
            let mid = crate::creature_skin_view::to_creature(l.joints[1]);
            let near = mesh.positions.iter().filter(|p| p.distance(mid) < 0.15).count();
            println!("  limb tentacle {} joint1 {mid:.3}: {near} skin vertices within 0.15", l.tentacle);
            assert!(near > 0, "the limb is skinned");
        }
    }

    /// Installed data: a sample of the Maxis PNGs of each type converts; the
    /// rigid blocks round-trip to the file's transforms; creatures keep every
    /// part and limb.
    #[test]
    #[ignore = "reads the installed packages"]
    fn installed_maxis_creations_convert() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let catalog = PartCatalog::from_index(&index);
        for group in [0x4062_6200u32, 0x4063_6200, 0x4064_6200, 0x4065_6200] {
            let mut keys: Vec<ResourceKey> =
                index.records.keys().filter(|k| k.type_id == crate::front_end::maxis::PNG_TYPE && k.group_id == group).copied().collect();
            keys.sort_by_key(|k| k.instance_id);
            let step = (keys.len() / 12).max(1);
            let (mut ok, mut blocks, mut unresolved) = (0, 0, 0);
            for key in keys.iter().step_by(step).take(12) {
                let d = crate::front_end::png_creation::decode_installed(&index, *key).unwrap_or_else(|| panic!("{key:?}"));
                let m = parse(&d.model).unwrap_or_else(|| panic!("{key:?} parses"));
                blocks += m.blocks.len();
                if d.header.type_id == CRT {
                    let fit = SkinFit::direct();
                    let import = |def: &PartDefinition| import_scale(&index, def);
                    let slots = |body: &BodyProfile| -> [Vec3; 8] { std::array::from_fn(|b| body.slot(b as u32)) };
                    let (run, report) = creature_record(&m, &d.header.name, &catalog, &fit, &import, &slots).unwrap_or_else(|| panic!("{} converts", d.header.name));
                    assert!(run.body.problem().is_none(), "{}", d.header.name);
                    let limb_blocks = m.blocks.iter().filter(|b| b.limb_type.is_some()).count();
                    let rigid_limb = m.blocks.iter().filter(|b| b.limb_type == Some(2)).count();
                    // Every block is a vertebra, a limb segment or a part.
                    let all_vertebrae = m.blocks.iter().filter(|b| b.instance == VERTEBRA).count() as i64;
                    let expected = m.blocks.len() as i64 - all_vertebrae - limb_blocks as i64 + report.limbs as i64 + rigid_limb as i64
                        - report.unresolved.len() as i64;
                    assert!(run.parts.len() as i64 >= expected, "{}: {} parts, expected {expected} ({report:?})", d.header.name, run.parts.len());
                    if !report.unresolved.is_empty() {
                        println!("  unresolved {:08X?}", report.unresolved);
                    }
                    unresolved += report.unresolved.len();
                    println!(
                        "crt {:<24} blocks {:>3} vertebrae {:>2} limbs {:>2} parts {:>3} unresolved {} clamped {} paint {:?}",
                        d.header.name,
                        report.blocks,
                        report.vertebrae,
                        report.limbs,
                        report.parts,
                        report.unresolved.len(),
                        report.clamped,
                        run.creature_creation.paint.brushes
                    );
                } else {
                    let kind = rigid_kind(d.header.type_id, m.model_type).expect("class");
                    let (design, report) = rigid_design(&m, kind, &catalog, &d.header.name);
                    assert!(!design.parts.is_empty(), "{}", d.header.name);
                    let resolved: Vec<&Block> = m.blocks.iter().filter(|b| !report.unresolved.contains(&b.key())).collect();
                    assert_eq!(resolved.len(), design.parts.len());
                    for (b, p) in resolved.iter().zip(&design.parts) {
                        let def = catalog.by_definition_in(b.group, b.instance);
                        let (pos, rows) = rigid_block_back(p.block.as_ref().unwrap(), basis_of(def));
                        assert!(pos.distance(b.position) < 1e-4, "{}", d.header.name);
                        for k in 0..3 {
                            assert!(rows[k].distance(b.rows[k]) < 2e-3, "{} block {:08X}: {:?} vs {:?}", d.header.name, b.instance, rows[k], b.rows[k]);
                        }
                    }
                    unresolved += report.unresolved.len();
                    println!(
                        "{:08X} {:<24} -> {:<22} blocks {:>3} unresolved {}",
                        d.header.type_id,
                        d.header.name,
                        kind.setup_name(),
                        report.blocks,
                        report.unresolved.len()
                    );
                }
                ok += 1;
            }
            println!("group {group:08X}: {ok} creations, {blocks} blocks, {unresolved} unresolved");
            assert!(ok >= 8, "{group:08X}: {ok}");
            assert!(unresolved * 50 <= blocks, "{group:08X}: {unresolved} of {blocks} blocks unresolved");
        }
    }

    /// Installed data (audit, prints): the orientation rows and positions of
    /// the surface parts (eyes, mouths, details...) of a sample of Maxis
    /// creatures, to read the retail placement rule (`editors-audit`).
    #[test]
    #[ignore = "reads the installed packages"]
    fn installed_surface_part_orientations() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let catalog = PartCatalog::from_index(&index);
        let mut keys: Vec<ResourceKey> =
            index.records.keys().filter(|k| k.type_id == crate::front_end::maxis::PNG_TYPE && k.group_id == 0x4062_6200).copied().collect();
        keys.sort_by_key(|k| k.instance_id);
        let step = (keys.len() / 40).max(1);
        for key in keys.iter().step_by(step).take(40) {
            let Some(d) = crate::front_end::png_creation::decode_installed(&index, *key) else { continue };
            let Some(m) = parse(&d.model) else { continue };
            println!("CREATURE {}", d.header.name);
            for (i, b) in m.blocks.iter().enumerate() {
                let name = catalog.by_definition_in(b.group, b.instance).map_or("?".to_string(), |d| d.name.clone());
                if name.contains("vertebra") {
                    println!("  v{i} y {:.3} z {:.3} scale {:.2}", b.position.y, b.position.z, b.scale);
                    continue;
                }
                println!(
                    "  b{i} {name:<34} pos ({:.3},{:.3},{:.3}) x' ({:.2},{:.2},{:.2}) y' ({:.2},{:.2},{:.2}) z' ({:.2},{:.2},{:.2}) sym {:?}",
                    b.position.x, b.position.y, b.position.z,
                    b.rows[0].x, b.rows[0].y, b.rows[0].z, b.rows[1].x, b.rows[1].y, b.rows[1].z, b.rows[2].x, b.rows[2].y, b.rows[2].z,
                    b.symmetric
                );
            }
        }
    }
}
