//! Swarm `gameModel` animations: the model's own skeletal clips, driven by the
//! component's `animate` entries.
//!
//! NATIVE (installed data, notes `notes\swarm-anim-notes.md`):
//! * A model RW4 carries a skeleton + skin (`SkinsInK`), `KeyAnim` clips and the
//!   `0xFF0001` animations table (clip id -> `KeyAnim`); the engine's
//!   `IModelWorld::AnimAction / MoveToTime / GetAnimationRange` address a clip by
//!   that id. A `gameModel` `animate` entry names the clip by its `channel` id.
//! * Skinning: vertex bone indices are D3D palette offsets (index x 3), the bone
//!   pose is `world(bone) * inverse bind`; clip channels are matched to bones by
//!   bone name hash (the cell part rigs, `cell_anim::PartRig`, do the same).
//!
//! ADAPTED (the native model instance / effect component code was not traced):
//! * Time: see [`phase`].
//! * Skinning runs on the CPU into a per-instance mesh copy (a few dozen to a
//!   few thousand vertices per model), in the model's own (Spore Z-up) space
//!   before the importer's `Rx(-90)` basis.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::{
    math::{Affine3A, Vec3},
    mesh::Mesh,
};

use super::format::GameModelAnimationDef;
use crate::asset_catalog::ResourceIndex;
use crate::cell_anim::{Bone3, PartRig, Track};
use crate::dbpf::ResourceKey;
use crate::rw4;

/// Skin of one drawn surface in the model's own space.
#[derive(Clone, Debug)]
pub struct SurfaceSkin {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

impl SurfaceSkin {
    /// Bone indices are D3D constant-register offsets (3 registers per bone);
    /// the blend-pool form carries two indices and weights (same /3 rule as
    /// the cell part rigs). `None` when the mesh carries no usable skin.
    fn from_mesh(mesh: &rw4::ExtractedMesh, bones: usize) -> Option<Self> {
        let n = mesh.positions.len();
        let (joints, weights): (Vec<[u16; 4]>, Vec<[f32; 4]>) =
            if mesh.bone_indices.len() == n && mesh.bone_weights.len() == n {
                mesh.bone_indices
                    .iter()
                    .zip(&mesh.bone_weights)
                    .map(|(b, w)| {
                        let total: f32 = w.iter().sum();
                        let w = if total > 0.0 { w.map(|x| x / total) } else { [1.0, 0.0, 0.0, 0.0] };
                        (b.map(|x| x as u16 / 3), w)
                    })
                    .unzip()
            } else if mesh.skin_idx.len() == n && mesh.skin_w.len() == n {
                mesh.skin_idx
                    .iter()
                    .zip(&mesh.skin_w)
                    .map(|(b, w)| {
                        let total = w[0] + w[1];
                        let w = if total > 0.0 { [w[0] / total, w[1] / total, 0.0, 0.0] } else { [1.0, 0.0, 0.0, 0.0] };
                        ([b[0] / 3, b[1] / 3, 0, 0], w)
                    })
                    .unzip()
            } else {
                return None;
            };
        if joints.iter().zip(&weights).any(|(j, w)| j.iter().zip(w).any(|(&j, &w)| w > 0.0 && j as usize >= bones)) {
            return None;
        }
        if mesh.normals.len() != n {
            return None;
        }
        Some(Self {
            positions: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            joints,
            weights,
        })
    }

    /// Skinned positions and normals for rig-space bone matrices (`world *
    /// inverse_bind`).
    pub fn skin(&self, matrices: &[Affine3A]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
        let mut positions = Vec::with_capacity(self.positions.len());
        let mut normals = Vec::with_capacity(self.positions.len());
        for v in 0..self.positions.len() {
            let p = Vec3::from_array(self.positions[v]);
            let n = Vec3::from_array(self.normals[v]);
            let (mut sp, mut sn, mut total) = (Vec3::ZERO, Vec3::ZERO, 0.0);
            for k in 0..4 {
                let w = self.weights[v][k];
                if w <= 0.0 {
                    continue;
                }
                let m = &matrices[self.joints[v][k] as usize];
                sp += m.transform_point3(p) * w;
                sn += m.transform_vector3(n) * w;
                total += w;
            }
            if total <= 0.0 {
                positions.push(self.positions[v]);
                normals.push(self.normals[v]);
            } else {
                positions.push((sp / total).to_array());
                let sn = sn.normalize_or_zero();
                normals.push(if sn == Vec3::ZERO { self.normals[v] } else { sn.to_array() });
            }
        }
        (positions, normals)
    }
}

/// A model with a skeleton, clips and per-surface skins.
#[derive(Debug)]
pub struct AnimatedModel {
    pub rig: PartRig,
    /// Skins by the source section of the surface they belong to.
    pub skins: HashMap<usize, SurfaceSkin>,
}

impl AnimatedModel {
    /// Load the rig, the clip table and the surface skins of an RW4 model.
    /// `None` when the model has no single linked skin, no clips or no
    /// skinned surface.
    pub fn load(index: &ResourceIndex, key: ResourceKey) -> Option<Self> {
        let blob = index.read(key).ok()?;
        let rig = PartRig::from_rw4(&blob).ok()?;
        if rig.clips.is_empty() {
            return None;
        }
        let mut stats = rw4::MeshStats::default();
        let skins: HashMap<usize, SurfaceSkin> = rw4::extract_meshes(&blob, &mut stats)
            .iter()
            .filter_map(|m| SurfaceSkin::from_mesh(m, rig.bone_count()).map(|s| (m.source_section, s)))
            .collect();
        if skins.is_empty() {
            return None;
        }
        Some(Self { rig, skins })
    }

    /// Length of a clip in seconds.
    pub fn clip_length(&self, id: u32) -> Option<f32> {
        self.rig.clips.iter().find(|(c, _)| *c == id).map(|(_, c)| c.anim.length)
    }

    /// Rig-space bone matrices for weighted clip phases.
    pub fn matrices(&self, tracks: &[Track]) -> Vec<Affine3A> {
        let mut locals: Vec<Bone3> = vec![];
        self.rig.locals(tracks, &mut locals);
        let mut out = vec![];
        self.rig.skin(&locals, &mut out);
        out
    }
}

/// Shared animated models by key (None = not animatable).
#[derive(Default)]
pub struct AnimatedModels {
    models: HashMap<ResourceKey, Option<Arc<AnimatedModel>>>,
}

impl AnimatedModels {
    pub fn get(&mut self, index: &ResourceIndex, key: ResourceKey) -> Option<Arc<AnimatedModel>> {
        self.models
            .entry(key)
            .or_insert_with(|| AnimatedModel::load(index, key).map(Arc::new))
            .clone()
    }
}

/// What a drawn gameModel wants played: its `animate` entries and age.
pub struct AnimRequest {
    pub defs: Vec<GameModelAnimationDef>,
    pub age: f32,
    pub uid: u64,
}

/// The clip tracks (clip id, phase in clip lengths, cyclic for `loop`
/// entries) for a request; entries naming a clip the model does not have are
/// skipped.
pub fn tracks(model: &AnimatedModel, request: &AnimRequest) -> Vec<Track> {
    request
        .defs
        .iter()
        .enumerate()
        .filter_map(|(i, def)| {
            model.clip_length(def.channel)?;
            Some(Track {
                id: def.channel,
                value: phase(def, request.age, roll(request.uid, i as u64 * 2), roll(request.uid, i as u64 * 2 + 1)),
                weight: 1.0,
                cyclic: def.mode == mode::LOOP,
            })
        })
        .collect()
}

/// `GameModelAnimation.mode` values (SporeModder-FX `MODE_FLAG_*`).
pub mod mode {
    pub const LOOP: u8 = 0;
    pub const SUSTAIN: u8 = 1;
    pub const SINGLE: u8 = 2;
}

/// A repeatable value in [0, 1) for an instance (`uid`) and a salt: the
/// per-instance rolls of the `length` range and the `vary` scale.
fn roll(uid: u64, salt: u64) -> f32 {
    let mut x = uid.wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 31;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 29;
    ((x >> 40) as f32) / (1u64 << 24) as f32
}

/// Clip position (in clip lengths) of one `animate` entry at a model age.
///
/// VERIFIED against the installed data (183 animated models, notes
/// `notes\swarm-anim-notes.md`): the `curve` values are the clip position in
/// clip lengths over the entry's `length` seconds (`unpack`: `0 1` over 5 s
/// with a 4.5 s clip; `cr_brain` keeps `0 0 1 1` over 4 s; single-key curves
/// such as the egg's `0.005` are fixed poses; the windmill's `0 60` over about
/// 300 s is 60 turns of a 2.5 s clip). ADAPTED: the keys are evenly spaced
/// over the entry's own time (as every Swarm curve here); `length` is drawn
/// uniformly from its range per instance; loop entries repeat the curve every
/// `length`, sustain / single entries hold its last value; `speedScale`
/// (0 = unset = 1) scales time and `vary` scales the curve values by
/// `1 + vary * (2r - 1)` per instance (no installed entry sets `speedScale`;
/// `vary` appears on two ambient loops).
pub fn phase(def: &GameModelAnimationDef, age: f32, length_roll: f32, vary_roll: f32) -> f32 {
    let [lo, hi] = def.length_range;
    let length = lo + (hi - lo) * length_roll;
    let speed = if def.speed_scale > 0.0 { def.speed_scale } else { 1.0 };
    let u = if length > 1e-4 { age.max(0.0) * speed / length } else { 0.0 };
    let u = if def.mode == mode::LOOP { u.fract() } else { u.min(1.0) };
    let value = super::sim::curve(&def.curve, u, 0.0);
    value * (1.0 + def.curve_vary * (2.0 * vary_roll - 1.0))
}

/// Write the skinned vertices of `skin` into `mesh` (positions and normals in
/// the importer's basis: the model's Z-up space turned by `Rx(-90)`, i.e.
/// `(x, y, z) -> (x, z, -y)`). A mesh whose vertex count differs is left alone.
pub fn write_skinned(mesh: &mut Mesh, skin: &SurfaceSkin, matrices: &[Affine3A]) {
    use bevy::mesh::VertexAttributeValues;
    let count = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(v)) => v.len(),
        _ => return,
    };
    if count != skin.positions.len() {
        return;
    }
    let (positions, normals) = skin.skin(matrices);
    let turn = |v: [f32; 3]| [v[0], v[2], -v[1]];
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.into_iter().map(turn).collect::<Vec<_>>());
    if matches!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL), Some(VertexAttributeValues::Float32x3(v)) if v.len() == count) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.into_iter().map(turn).collect::<Vec<_>>());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::swarm_fx::SwarmLibrary;

    fn entry(mode: u8, length: [f32; 2], curve: &[f32]) -> GameModelAnimationDef {
        GameModelAnimationDef {
            length_range: length,
            curve: curve.to_vec(),
            mode,
            ..Default::default()
        }
    }

    /// `unpack`: 0 -> 1 over 5 s, then the last value is held (sustain).
    #[test]
    fn sustain_ramps_then_holds_the_last_value() {
        let d = entry(mode::SUSTAIN, [5.0, 5.0], &[0.0, 1.0]);
        assert_eq!(phase(&d, 0.0, 0.0, 0.0), 0.0);
        assert!((phase(&d, 2.5, 0.0, 0.0) - 0.5).abs() < 1e-6);
        assert_eq!(phase(&d, 5.0, 0.0, 0.0), 1.0);
        assert_eq!(phase(&d, 50.0, 0.0, 0.0), 1.0);
    }

    /// A loop repeats the curve every `length`; values above 1 are whole
    /// extra clip turns (the windmill's `0 60` over about 300 s).
    #[test]
    fn loop_repeats_the_curve_and_curves_may_exceed_one() {
        let d = entry(mode::LOOP, [2.0, 2.0], &[0.0, 1.0]);
        assert!((phase(&d, 3.0, 0.0, 0.0) - 0.5).abs() < 1e-6);
        let turns = entry(mode::LOOP, [300.0, 300.0], &[0.0, 60.0]);
        assert!((phase(&turns, 150.0, 0.0, 0.0) - 30.0).abs() < 1e-4);
    }

    /// Single-key curves are fixed poses (the egg's crack stages), also with
    /// no length; the `length` range is rolled per instance.
    #[test]
    fn single_key_curves_are_fixed_poses_and_length_is_rolled() {
        let d = entry(mode::LOOP, [0.0, 0.0], &[0.019]);
        assert_eq!(phase(&d, 7.0, 0.3, 0.3), 0.019);
        let r = entry(mode::SINGLE, [1.0, 3.0], &[0.0, 1.0]);
        assert!((phase(&r, 1.0, 0.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((phase(&r, 1.0, 1.0, 0.0) - 1.0 / 3.0).abs() < 1e-6);
        let a = roll(7, 0);
        assert_eq!(a, roll(7, 0));
        assert!((0.0..1.0).contains(&a));
        assert_ne!(a, roll(7, 1));
    }

    /// A one-bone skin follows its matrix; zero weights keep the bind vertex.
    #[test]
    fn skinning_follows_the_bone_matrices() {
        let skin = SurfaceSkin {
            positions: vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 2],
            joints: vec![[0, 0, 0, 0], [1, 0, 0, 0]],
            weights: vec![[1.0, 0.0, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0]],
        };
        let up = Affine3A::from_translation(Vec3::new(0.0, 0.0, 2.0));
        let turn = Affine3A::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let (p, n) = skin.skin(&[up, turn]);
        assert!((Vec3::from_array(p[0]) - Vec3::new(1.0, 0.0, 2.0)).length() < 1e-6);
        // Vertex 1: half the translated point, half the turned one.
        let expect = (Vec3::new(0.0, 1.0, 2.0) + Vec3::new(-1.0, 0.0, 0.0)) * 0.5;
        assert!((Vec3::from_array(p[1]) - expect).length() < 1e-6);
        assert!((Vec3::from_array(n[0]) - Vec3::Z).length() < 1e-6);
    }

    /// Every installed animated model skins back onto its own vertices in the
    /// bind pose (the bone index mapping and inverse binds agree) and stays
    /// finite and bounded at the clip ends:
    /// `cargo test swarm_fx::game_anim::tests::installed_models_skin_to_their_bind_pose -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires the installed game"]
    fn installed_models_skin_to_their_bind_pose() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        let mut seen = std::collections::HashSet::new();
        let (mut models, mut worst_bind, mut worst_pose) = (0, 0f32, 0f32);
        for dir in &lib.dirs {
            for g in dir.game_models.iter().filter(|g| !g.animations.is_empty()) {
                let Some(key) = crate::swarm_fx::models::resolve_model_key_in(&index, g.model.group, g.model.instance)
                else {
                    continue;
                };
                if !seen.insert(key) {
                    continue;
                }
                let Some(model) = AnimatedModel::load(&index, key) else { continue };
                models += 1;
                let bind = model.matrices(&[]);
                for (section, skin) in &model.skins {
                    let (lo, hi) = skin.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(l, h), p| {
                        (l.min(Vec3::from_array(*p)), h.max(Vec3::from_array(*p)))
                    });
                    let extent = (hi - lo).max_element().max(1e-3);
                    let (p, _) = skin.skin(&bind);
                    let bind_err = p
                        .iter()
                        .zip(&skin.positions)
                        .map(|(a, b)| (Vec3::from_array(*a) - Vec3::from_array(*b)).length())
                        .fold(0f32, f32::max)
                        / extent;
                    worst_bind = worst_bind.max(bind_err);
                    // Every clip at 0, 0.5 and 1: finite and not flung away.
                    for (id, clip) in &model.rig.clips {
                        for phase in [0.0, 0.5, 1.0] {
                            let tracks = [Track { id: *id, value: phase, weight: 1.0, cyclic: false }];
                            let (q, _) = skin.skin(&model.matrices(&tracks));
                            let reach = q
                                .iter()
                                .map(|a| {
                                    let v = Vec3::from_array(*a);
                                    assert!(v.is_finite(), "{key:?} clip {id:08x}");
                                    (v - (lo + hi) * 0.5).length()
                                })
                                .fold(0f32, f32::max)
                                / extent;
                            worst_pose = worst_pose.max(reach);
                            if reach > 8.0 {
                                println!("far: {key:?} section {section} clip {id:08x} (len {}) phase {phase}: reach {reach:.1} extents", clip.anim.length);
                            }
                        }
                    }
                    if bind_err > 0.02 {
                        println!("bind mismatch {key:?} section {section}: {bind_err:.3}");
                    }
                }
            }
        }
        println!("{models} animated models: worst bind error {worst_bind:.4} of the extent, worst pose reach {worst_pose:.2} extents");
        assert!(models > 50);
        assert!(worst_bind < 0.05, "bind pose must reproduce the vertices");
    }

    /// Exported effects whose tree reaches a gameModel of a non-default model
    /// world: `cargo test swarm_fx::game_anim::tests::survey_world_effects -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_world_effects() {
        use crate::swarm_fx::format::{kind, IMPORT_MASK};
        use crate::swarm_fx::EffectRef;
        use std::collections::{BTreeMap, HashSet};
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        let reg = std::fs::read_to_string(
            "D:/SteamLibrary/steamapps/common/Spore/OpenSpore/target/format-reference/reg_file.txt",
        )
        .unwrap_or_default();
        let names: HashMap<u32, String> = reg
            .lines()
            .filter_map(|l| l.split('\t').next())
            .map(|n| (crate::world_objects::name_id(n.trim()), n.trim().to_string()))
            .collect();
        let name = |id: u32| names.get(&id).cloned().unwrap_or(format!("{id:08x}"));
        fn reach(
            lib: &SwarmLibrary,
            effect: EffectRef,
            memo: &mut HashMap<EffectRef, HashSet<(u32, u32)>>,
        ) -> HashSet<(u32, u32)> {
            if let Some(found) = memo.get(&effect) {
                return found.clone();
            }
            memo.insert(effect, HashSet::new());
            let dir = lib.dir(effect.dir);
            let mut out = HashSet::new();
            let mut follow = |index: i32, out: &mut HashSet<(u32, u32)>, memo: &mut HashMap<_, _>| {
                if let Some(e) = lib.resolve_effect(effect.dir, index) {
                    out.extend(reach(lib, e, memo));
                }
            };
            for b in &dir.effects[effect.index as usize].blocks {
                let local = b.index >= 0 && b.index & IMPORT_MASK == 0;
                match b.kind {
                    kind::EFFECT => follow(b.index, &mut out, memo),
                    kind::GAME_MODEL if local => {
                        let g = &dir.game_models[b.index as usize];
                        if g.world != 0 {
                            out.insert((g.world, g.model.instance));
                        }
                    }
                    kind::SEQUENCE if local => {
                        for (_, e) in &dir.sequences[b.index as usize].instances {
                            follow(*e, &mut out, memo);
                        }
                    }
                    kind::META_PARTICLES if local => {
                        let (index, type_code) = dir.metas[b.index as usize].component;
                        if type_code == 0 {
                            follow(index, &mut out, memo);
                        }
                    }
                    _ => {}
                }
            }
            memo.insert(effect, out.clone());
            out
        }
        let mut memo = HashMap::new();
        let mut by_world: BTreeMap<(u32, u32), Vec<String>> = BTreeMap::new();
        for (n, e) in &lib.exports {
            for key in reach(&lib, *e, &mut memo) {
                by_world.entry(key).or_default().push(name(*n));
            }
        }
        for ((world, model), mut effects) in by_world {
            effects.sort();
            println!("world {world:08x} model {}: {} exported effects {:?}", name(model), effects.len(), &effects[..effects.len().min(12)]);
        }
    }

    /// Paths from an exported effect to its gameModel blocks (`SWARM_DUMP=name`):
    /// `SWARM_DUMP=g2c_comet cargo test swarm_fx::game_anim::tests::dump_effect_tree -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires the installed game"]
    fn dump_effect_tree() {
        use crate::swarm_fx::format::{kind, IMPORT_MASK};
        use std::collections::HashSet;
        let Ok(target) = std::env::var("SWARM_DUMP") else { return };
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        let id = crate::world_objects::name_id(&target);
        let root = *lib.exports.get(&id).expect("exported effect");
        fn walk(
            lib: &SwarmLibrary,
            e: crate::swarm_fx::EffectRef,
            path: &mut Vec<String>,
            seen: &mut HashSet<crate::swarm_fx::EffectRef>,
            time: f32,
        ) {
            if !seen.insert(e) {
                return;
            }
            let dir = lib.dir(e.dir);
            for b in &dir.effects[e.index as usize].blocks {
                let local = b.index >= 0 && b.index & IMPORT_MASK == 0;
                if b.kind == kind::GAME_MODEL && local {
                    let g = &dir.game_models[b.index as usize];
                    println!(
                        "t={time:5.2}  {}  -> gameModel world {:08x} model {:08x} (block flags {:#x}, app {:#x}/{:#x}, sel {}, xform {:#x} scale {}, offset {:?})",
                        path.join(" > "), g.world, g.model.instance, b.flags, b.app_flags, b.app_flags_mask,
                        b.selection_group, b.transform.flags, b.transform.scale, b.transform.offset
                    );
                }
                if b.kind == kind::EFFECT {
                    if let Some(c) = lib.resolve_effect(e.dir, b.index) {
                        path.push(format!("{}{}", if local { "" } else { "import " }, c.index));
                        walk(lib, c, path, seen, time);
                        path.pop();
                    } else {
                        println!("t={time:5.2}  {}  unresolved effect block {}", path.join(" > "), b.index);
                    }
                }
                if b.kind == kind::SEQUENCE && local {
                    for (t, c) in &dir.sequences[b.index as usize].instances {
                        if let Some(c) = lib.resolve_effect(e.dir, *c) {
                            path.push(format!("seq@{t:?} {}", c.index));
                            walk(lib, c, path, seen, time + t[0].max(0.0));
                            path.pop();
                        } else {
                            println!("t={time:5.2}  {}  seq entry {c} at {t:?} unresolved", path.join(" > "));
                        }
                    }
                }
                if b.kind == kind::META_PARTICLES && local {
                    let m = &dir.metas[b.index as usize];
                    let (index, type_code) = m.component;
                    if type_code == 0 {
                        if let Some(c) = lib.resolve_effect(e.dir, index) {
                            path.push(format!("meta {} (delay {:?} life {:?}) -> {}", b.index, m.emit_delay, m.life, c.index));
                            walk(lib, c, path, seen, time);
                            path.pop();
                        }
                    }
                }
            }
        }
        walk(&lib, root, &mut vec![format!("root {}", root.index)], &mut HashSet::new(), 0.0);
    }

    /// Survey of the installed animated gameModels:
    /// `cargo test swarm_fx::game_anim::tests::survey_animated_models -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_animated_models() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        let reg = std::fs::read_to_string(
            "D:/SteamLibrary/steamapps/common/Spore/OpenSpore/target/format-reference/reg_file.txt",
        )
        .unwrap_or_default();
        let names: HashMap<u32, String> = reg
            .lines()
            .filter_map(|l| l.split('\t').next())
            .map(|n| (crate::world_objects::name_id(n.trim()), n.trim().to_string()))
            .collect();
        let name = |id: u32| names.get(&id).cloned().unwrap_or(format!("{id:08x}"));
        let mut seen = std::collections::HashSet::new();
        let (mut total, mut loaded, mut channel_hits, mut channel_all) = (0, 0, 0, 0);
        for dir in &lib.dirs {
            let by_index: HashMap<i32, u32> = dir.exports.iter().map(|(n, i)| (*i, *n)).collect();
            let mut owners: HashMap<usize, Vec<String>> = HashMap::new();
            for (e, effect) in dir.effects.iter().enumerate() {
                for b in &effect.blocks {
                    if b.kind == crate::swarm_fx::format::kind::GAME_MODEL && b.index >= 0 {
                        let n = by_index.get(&(e as i32)).map(|n| name(*n)).unwrap_or(format!("#{e}"));
                        owners.entry(b.index as usize).or_default().push(n);
                    }
                }
            }
            for (i, g) in dir.game_models.iter().enumerate() {
                if g.animations.is_empty() || !seen.insert((g.model.group, g.model.instance, i, dir.key_instance)) {
                    continue;
                }
                total += 1;
                let Some(key) = crate::swarm_fx::models::resolve_model_key_in(&index, g.model.group, g.model.instance)
                else {
                    println!("gm {i} {:08x}!{}: model not installed", g.model.group, name(g.model.instance));
                    continue;
                };
                let blob = index.read(key).unwrap();
                let sections = rw4::sections(&blob).unwrap();
                let skins = sections.iter().filter(|s| s.0 == rw4::T_SKINS_IN_K).count();
                let keyanims: Vec<f32> = {
                    let mut st = rw4::MeshStats::default();
                    rw4::extract_anims(&blob, &mut st).iter().map(|a| a.length).collect()
                };
                let table = crate::cell_anim::animation_table(&blob);
                let model = AnimatedModel::load(&index, key);
                loaded += model.is_some() as u32;
                let ids: Vec<u32> = table.as_ref().map(|t| t.iter().map(|x| x.0).collect()).unwrap_or_default();
                let mut line = format!(
                    "gm {i} {:08x}!{} -> {:08x}!{:08x}/{:08x} skins {skins} keyanims {:?} table {:?} loaded {} owners {:?}\n",
                    g.model.group,
                    name(g.model.instance),
                    key.group_id,
                    key.instance_id,
                    key.type_id,
                    keyanims,
                    table.as_ref().map(|t| t.iter().map(|x| name(x.0)).collect::<Vec<_>>()).map_err(|e| e.clone()),
                    model.is_some(),
                    owners.get(&i).map(|v| v.iter().take(3).collect::<Vec<_>>()),
                );
                for a in &g.animations {
                    channel_all += 1;
                    let hit = ids.contains(&a.channel);
                    channel_hits += hit as u32;
                    line += &format!(
                        "    anim channel {} (in table {hit}, clip len {:?}) mode {} len {:?} speed {} vary {} curve {:?}\n",
                        name(a.channel),
                        model.as_ref().and_then(|m| m.clip_length(a.channel)),
                        a.mode,
                        a.length_range,
                        a.speed_scale,
                        a.curve_vary,
                        a.curve
                    );
                }
                print!("{line}");
            }
        }
        println!("animated gameModels {total}, loaded rigs {loaded}, channels {channel_hits}/{channel_all} found in tables");
    }
}
