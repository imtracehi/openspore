//! The player creature's animation blocks (`notes/skin-re-notes.md` section
//! 5) and the Test Drive limb motion.
//!
//! Blocks (VERIFIED rules, FUN_009CBE10 / FUN_009B2E60 / FUN_009CBCE0): one
//! block per rigblock (here: the skin's vertebra and limb segment records,
//! then the mounted parts), the root first; capabilities from the parts'
//! `modelCapability<X>` properties through the code table 0x150C050 (name ->
//! PCTP capability -> 4-character tag; duplicates keep the highest level);
//! cap flags 'root' 1, 'limb' 2, 'foot' 4, 'spin' 8, 'psft' 0x10, 'mout'
//! 0x20, 'grsp' 0x40, 'nstr' 0x80; a creature without a foot gets
//! pseudo-feet at its lowest limb-chain ends. The list becomes a
//! `retarget::Rig` (Spore coordinates), which until now had no caller.
//!
//! The skinned body (retail bone weights, section 1.4; bone = record index)
//! and its clips in Test Drive and the Creature stage are `creature_clips`,
//! which uses `two_bone` / `ccd` below as its ADAPTED limb IK (the retail anim
//! IK is not traced). The pseudo-foot height fraction (constant 0x155123C,
//! not read) is 0.1.

use std::collections::HashMap;
use std::sync::OnceLock;

use bevy::prelude::*;

use crate::{
    asset_catalog::{ResourceIndex, CREATURE_PARTS},
    cell_editor_data::{number, prop_key},
    creature_editor::spore_to_bevy,
    creature_skin::{BlockKind, SkinRecord},
    game::RunProgress,
    Originals,
};

/// Table 0x150C050 (code immediates; `notes/skin-re/capability_property_table.txt`):
/// part property id -> capability name.
pub const CAPABILITY_PROPERTIES: &[(u32, &str)] = &[
    (0x0551BF16, "perch"), (0x055D370E, "tribeAttack"), (0x055D3747, "tribeSocial"), (0x055D374C, "tribeArmor"),
    (0x055D3750, "tribeGather"), (0x055D3754, "tribeFishing"), (0x100F0F5A, "spine"), (0xB00F0FDF, "limb"),
    (0x06F079DF, "nostretch"), (0xB00F0FE2, "grasper"), (0xB00F0FE5, "ear"), (0xB00F0FE9, "eye"), (0xB00F0FEC, "mouth"),
    (0xB00F0FEF, "foot"), (0xB00F0FF2, "slash"), (0xB00F0FF4, "poke"), (0xB00F0FF7, "bash"), (0xD00F0FFB, "fruit"),
    (0xD00F0FFE, "fin"), (0x02294DE1, "liquid"), (0x022E7977, "dayvision"), (0x022E7777, "nightvision"),
    (0x022E7847, "carnivorous"), (0x022E785C, "herbivorous"), (0x025C6878, "detail"), (0x91A570BA, "social"),
    (0x31C3E5B2, "health"), (0x51C3E5B4, "stealth"), (0xB1C3E5B5, "cuteness"), (0x330C117A, "jump"), (0xF354A879, "block"),
    (0xF354A87A, "call"), (0xB354A87C, "creaturespeed"), (0x3386C531, "sprint"), (0x04F4D188, "wing"), (0xB3E30313, "sense"),
    (0x04F4E1B4, "glide"), (0xB1C3E5B6, "mean"), (0xB1C3E5B7, "bite"), (0xB1C3E5B8, "charge"), (0xB1C3E5B9, "spit"),
    (0xB1C3E5C0, "strike"), (0xB1C3E5C1, "dance"), (0xB1C3E5C2, "sing"), (0xB1C3E5C3, "charm"), (0xB1C3E5C4, "pose"),
    (0x11B78A70, "speed"), (0x11B78A71, "defense"), (0x11B78A72, "power"),
];

/// `pctp` resources: capability name -> 4-character tag.
pub const PCTP_TYPE: u32 = 0x7C19_AA7A;

/// Parse a PCTP (SporeModder `PCTPUnit`): (name, tag) pairs.
pub fn parse_pctp(b: &[u8]) -> Option<Vec<(String, [u8; 4])>> {
    let be = |o: usize| b.get(o..o + 4).map(|s| u32::from_be_bytes(s.try_into().unwrap()));
    if be(0)? != 0x7063_7470 {
        return None;
    }
    let version = be(4)?;
    let mut o = if version > 3 { 12 } else { 8 };
    let count = be(o)? as usize;
    o += 4;
    let mut out = Vec::with_capacity(count.min(4096));
    for _ in 0..count.min(4096) {
        let n = be(o)? as usize;
        let name = String::from_utf8_lossy(b.get(o + 4..o + 4 + n)?).to_string();
        o += 4 + n;
        // Little-endian identifier = the tag's characters in reading order.
        let id = b.get(o..o + 4)?;
        out.push((name, [id[3], id[2], id[1], id[0]].map(|c| if c == 0 { b' ' } else { c })));
        o += 4;
    }
    Some(out)
}

/// Capability name (lower case) -> tag, from every installed PCTP.
pub fn capability_tags(index: &ResourceIndex) -> &'static HashMap<String, [u8; 4]> {
    static TAGS: OnceLock<HashMap<String, [u8; 4]>> = OnceLock::new();
    TAGS.get_or_init(|| {
        let mut out = HashMap::new();
        for key in index.records.keys().filter(|k| k.type_id == PCTP_TYPE) {
            if let Some(list) = index.read(*key).ok().and_then(|b| parse_pctp(&b)) {
                for (name, tag) in list {
                    out.entry(name.to_ascii_lowercase()).or_insert(tag);
                }
            }
        }
        out
    })
}

/// Capabilities (tag, level) of a part PROP; duplicates keep the max level.
pub fn part_capabilities(index: &ResourceIndex, group: u32, instance: u32) -> Vec<([u8; 4], i32)> {
    let Ok(p) = index.properties(prop_key(group, instance)) else { return Vec::new() };
    let tags = capability_tags(index);
    let mut out: Vec<([u8; 4], i32)> = Vec::new();
    for (id, name) in CAPABILITY_PROPERTIES {
        let level = number(&p, *id).map_or(0, |v| v.round() as i32);
        if level <= 0 {
            continue;
        }
        let Some(tag) = tags.get(&name.to_ascii_lowercase()) else { continue };
        match out.iter_mut().find(|(t, _)| t == tag) {
            Some(e) => e.1 = e.1.max(level),
            None => out.push((*tag, level)),
        }
    }
    out
}

/// FUN_009B2E60: capability tag -> animation flag.
pub fn cap_flag(tag: &[u8; 4]) -> u32 {
    match tag {
        b"root" => 1,
        b"limb" => 2,
        b"foot" => 4,
        b"spin" => 8,
        b"psft" => 0x10,
        b"mout" => 0x20,
        b"grsp" => 0x40,
        b"nstr" => 0x80,
        _ => 0,
    }
}

/// One animation block (player frame).
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    /// Skin record index (vertebrae, limb segments) or mounted part index.
    pub record: Option<usize>,
    pub part: Option<usize>,
    pub parent: Option<usize>,
    pub position: Vec3,
    pub caps: Vec<[u8; 4]>,
    pub flags: u32,
}

/// Mark pseudo-feet (FUN_009CBCE0): with no foot block, the limb-chain ends
/// within a fraction of the creature's height of its lowest point.
pub const PSEUDO_FOOT_FRACTION: f32 = 0.1;

pub fn mark_pseudo_feet(blocks: &mut [Block]) {
    if blocks.iter().any(|b| b.flags & 4 != 0) || blocks.is_empty() {
        return;
    }
    let (lo, hi) = blocks.iter().fold((f32::MAX, f32::MIN), |(a, c), b| (a.min(b.position.y), c.max(b.position.y)));
    let ends: Vec<usize> = (0..blocks.len())
        .filter(|&i| blocks[i].kind == BlockKind::Limb && !blocks.iter().any(|c| c.parent == Some(i) && c.kind == BlockKind::Limb))
        .collect();
    for i in ends {
        if blocks[i].position.y <= lo + (hi - lo) * PSEUDO_FOOT_FRACTION {
            blocks[i].caps.push(*b"psft");
            blocks[i].flags |= 0x10;
        }
    }
}

/// Blocks of skin `records` (creature space, `units` player units per unit)
/// and the mounted parts (`part_blocks`: position, parent part, caps).
pub fn blocks_of(records: &[SkinRecord], units: f32, limb_ends: &[(usize, Vec3)], parts: &[(usize, Vec3, Vec<([u8; 4], i32)>)]) -> Vec<Block> {
    let to_p = |v: Vec3| spore_to_bevy(v) * units;
    let mut out: Vec<Block> = records
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let caps: Vec<[u8; 4]> = match r.kind {
                BlockKind::Vertebra => vec![*b"spin"],
                BlockKind::Limb => vec![*b"limb"],
                _ => vec![],
            };
            let flags = caps.iter().map(cap_flag).fold(0, |a, f| a | f);
            // A limb segment's position is its end joint (where the next
            // block, or the foot, attaches).
            let position = if r.kind == BlockKind::Limb { to_p(r.segment_end()) } else { to_p(r.position) };
            Block { kind: r.kind, record: Some(i), part: None, parent: (r.parent >= 0).then_some(r.parent as usize), position, caps, flags }
        })
        .collect();
    if let Some(root) = out.first_mut() {
        root.caps.push(*b"root");
        root.flags |= 1;
    }
    let n = out.len();
    for (part, position, caps) in parts {
        // The block a part hangs from: the limb segment it ends, else the
        // nearest vertebra.
        let parent = limb_ends
            .iter()
            .find(|(p, _)| p == part)
            .and_then(|(_, end)| (0..n).filter(|&i| out[i].kind == BlockKind::Limb).min_by(|&a, &b| out[a].position.distance(*end).total_cmp(&out[b].position.distance(*end))))
            .or_else(|| (0..n).filter(|&i| out[i].kind == BlockKind::Vertebra).min_by(|&a, &b| out[a].position.distance(*position).total_cmp(&out[b].position.distance(*position))));
        let tags: Vec<[u8; 4]> = caps.iter().map(|c| c.0).collect();
        let flags = tags.iter().map(cap_flag).fold(0, |a, f| a | f);
        out.push(Block { kind: BlockKind::Standard, record: None, part: Some(*part), parent, position: *position, caps: tags, flags });
    }
    mark_pseudo_feet(&mut out);
    out
}

/// The `retarget::Rig` of a block list (Spore coordinates, `units` player
/// units per creature unit).
pub fn rig(blocks: &[Block], units: f32) -> crate::retarget::Rig {
    let to_s = |p: Vec3| spore_to_bevy(p) / units;
    let bodies: Vec<crate::retarget::Body> = blocks
        .iter()
        .map(|b| crate::retarget::Body {
            capabilities: b.caps.iter().map(|c| String::from_utf8_lossy(c).trim().to_string()).collect(),
            parent: b.parent,
            position: to_s(b.position),
            rotation: Quat::IDENTITY,
        })
        .collect();
    let (min, max) = bodies.iter().fold((Vec3::MAX, Vec3::MIN), |(a, c), b| (a.min(b.position), c.max(b.position)));
    crate::retarget::Rig { bodies, root: 0, min: if min.is_finite() { min } else { Vec3::ZERO }, max: if max.is_finite() { max } else { Vec3::ZERO } }
}

/// The creature's blocks now (player frame) and the skin they index.
pub fn creature_blocks(originals: &Originals, run: &RunProgress) -> Option<(Vec<Block>, std::sync::Arc<crate::creature_skin_view::Built>)> {
    let b = crate::creature_skin_view::build(originals.baked_body.as_ref(), &run.body, crate::creature_skin::RES_HIGH_QUALITY)?;
    let limb_ends: Vec<(usize, Vec3)> = run
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.edits.on_limb != 0 && p.edits.segment == 0)
        .filter_map(|(i, p)| {
            let l = run.parts.iter().find(|q| q.edits.limb.as_ref().is_some_and(|l| l.id == p.edits.on_limb))?;
            Some((i, crate::creature_limbs::root_of(originals, &run.body, l) + l.edits.limb.as_ref()?.end()))
        })
        .collect();
    let parts: Vec<(usize, Vec3, Vec<([u8; 4], i32)>)> = run
        .parts
        .iter()
        .enumerate()
        // Rigid segment parts ride their limb (not separate animated blocks).
        .filter(|(_, p)| p.edits.limb.is_none() && p.edits.segment == 0)
        .filter_map(|(i, p)| {
            let def = originals.part_catalog.by_part_in(CREATURE_PARTS, p)?;
            Some((i, crate::creature_limbs::root_of(originals, &run.body, p), part_capabilities(&originals.resource_index, CREATURE_PARTS, def.key.instance_id)))
        })
        .collect();
    Some((blocks_of(&b.records, b.unit, &limb_ends, &parts), b))
}

// ---------------------------------------------------------------------------
// Test Drive limb motion (ADAPTED)
// ---------------------------------------------------------------------------

/// Two-bone IK: the middle joint for a chain `a` (root) -> `b` -> `c` reaching
/// `target`, bending toward the rest middle joint.
pub fn two_bone(a: Vec3, b: Vec3, c: Vec3, target: Vec3) -> (Vec3, Vec3) {
    let (l1, l2) = (a.distance(b), b.distance(c));
    let to = target - a;
    let d = to.length().clamp((l1 - l2).abs() + 1e-4, l1 + l2 - 1e-4);
    let dir = to.normalize_or(Vec3::NEG_Y);
    let pole = (b - a) - dir * (b - a).dot(dir);
    let pole = pole.try_normalize().unwrap_or_else(|| dir.any_orthonormal_vector());
    let x = (l1 * l1 - l2 * l2 + d * d) / (2.0 * d);
    let h = (l1 * l1 - x * x).max(0.0).sqrt();
    let knee = a + dir * x + pole * h;
    (knee, a + dir * d)
}

/// CCD for other chain lengths (joints[0] fixed).
pub fn ccd(joints: &mut [Vec3], target: Vec3, iterations: usize) {
    let n = joints.len();
    if n < 2 {
        return;
    }
    for _ in 0..iterations {
        for k in (0..n - 1).rev() {
            let (pivot, end) = (joints[k], joints[n - 1]);
            let q = Quat::from_rotation_arc((end - pivot).normalize_or(Vec3::Y), (target - pivot).normalize_or(Vec3::Y));
            for j in k + 1..n {
                joints[j] = pivot + q * (joints[j] - pivot);
            }
        }
    }
}

/// A leg's foot target in the gait (relative to the limb root): the rest end
/// stepping forward/back (-z is the creature's front) and lifting on the
/// forward swing.
pub fn gait_target(rest_end: Vec3, length: f32, phase: f32, run: bool) -> Vec3 {
    let (stride, lift) = if run { (0.35 * length, 0.25 * length) } else { (0.22 * length, 0.15 * length) };
    rest_end + Vec3::new(0.0, lift * phase.sin().max(0.0), -stride * phase.cos())
}

// The skinned body and its motion moved to `creature_clips` (the clip
// player); the names stay for the plugin's registration.
pub use crate::creature_clips::{drive_player as test_drive, DriveState};

/// Parts whose model is not loaded yet (e.g. after CellToCreature or a limb
/// purchase) are imported once.
pub fn ensure_part_models(
    stage: Option<Res<State<crate::stages::GameStage>>>,
    run: Res<RunProgress>,
    mut originals: ResMut<Originals>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
    mut state: ResMut<crate::editor::EditorState>,
    mut tried: Local<Vec<u32>>,
) {
    if stage.is_none_or(|s| *s.get() != crate::stages::GameStage::Creature) || !run.is_changed() && tried.len() > 0 {
        return;
    }
    let missing: Vec<crate::asset_catalog::PartDefinition> = run
        .parts
        .iter()
        .filter(|p| p.edits.limb.is_none() && crate::editor::model_for_part(&originals, p).is_none())
        .filter_map(|p| originals.part_catalog.by_part_in(CREATURE_PARTS, p).cloned())
        .filter(|d| !tried.contains(&d.key.instance_id))
        .collect();
    if tried.is_empty() {
        tried.push(0);
    }
    for def in missing {
        tried.push(def.key.instance_id);
        match crate::editor::ensure_model(&mut originals, &def, &mut meshes, &mut images, &mut materials) {
            Ok(()) => state.visuals_dirty = true,
            Err(e) => warn!("part model {}: {e}", def.name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_bone_reaches_and_bends_toward_the_pole() {
        let (a, b, c) = (Vec3::ZERO, Vec3::new(0.0, -0.5, -0.1), Vec3::new(0.0, -1.0, 0.0));
        let target = Vec3::new(0.0, -0.8, 0.0);
        let (knee, foot) = two_bone(a, b, c, target);
        assert!(foot.distance(target) < 1e-3);
        assert!((knee.distance(a) - a.distance(b)).abs() < 1e-4);
        assert!((knee.distance(foot) - b.distance(c)).abs() < 1e-3);
        assert!(knee.z < 0.0, "bends forward like the rest knee");
        // Out of reach: straight toward the target.
        let (_, foot) = two_bone(a, b, c, Vec3::new(0.0, -5.0, 0.0));
        assert!(foot.distance(a) < a.distance(b) + b.distance(c) + 1e-3);
        let mut chain = vec![Vec3::ZERO, Vec3::new(0.0, -0.4, 0.0), Vec3::new(0.0, -0.8, 0.0), Vec3::new(0.0, -1.2, 0.0)];
        let t = Vec3::new(0.3, -0.9, 0.2);
        ccd(&mut chain, t, 16);
        assert!(chain[3].distance(t) < 0.02);
        // Gait: lifts on the forward swing only.
        let rest = Vec3::new(0.0, -1.0, 0.0);
        assert!(gait_target(rest, 1.0, std::f32::consts::FRAC_PI_2, false).y > rest.y);
        assert!((gait_target(rest, 1.0, -std::f32::consts::FRAC_PI_2, false).y - rest.y).abs() < 1e-6);
    }

    #[test]
    fn blocks_caps_flags_and_pseudo_feet() {
        assert_eq!(cap_flag(b"foot"), 4);
        assert_eq!(cap_flag(b"psft"), 0x10);
        assert_eq!(cap_flag(b"eye "), 0);
        // A spine of two vertebrae and one two-segment leg without a foot.
        let mut recs = vec![
            SkinRecord { kind: BlockKind::Vertebra, position: Vec3::new(0.0, 0.0, 1.0), ..Default::default() },
            SkinRecord { kind: BlockKind::Vertebra, position: Vec3::new(0.0, -0.5, 1.0), parent: 0, ..Default::default() },
        ];
        for k in 0..2 {
            recs.push(SkinRecord {
                kind: BlockKind::Limb,
                parent: if k == 0 { 1 } else { 2 },
                position: Vec3::new(0.3, -0.5, 1.0 - 0.45 * k as f32),
                orientation: [Vec3::X, Vec3::Z, Vec3::NEG_Y],
                socket_distance: 0.45,
                ..Default::default()
            });
        }
        let blocks = blocks_of(&recs, 1.0, &[], &[(0, Vec3::new(0.0, 1.0, -0.6), vec![(*b"eye ", 1)])]);
        assert_eq!(blocks.len(), 5);
        assert_eq!(blocks[0].flags & 1, 1, "root");
        assert_eq!(blocks[1].flags & 8, 8, "spine");
        assert_eq!(blocks[2].flags & 2, 2, "limb");
        assert!(blocks[3].flags & 0x10 != 0, "the lowest chain end is a pseudo-foot");
        assert!(blocks[2].flags & 0x10 == 0);
        assert_eq!(blocks[4].parent, Some(1), "the eye hangs from the nearest vertebra");
        let rig = rig(&blocks, 1.0);
        rig.validate().unwrap();
        assert_eq!(rig.select(0, "psft").unwrap(), vec![3]);
        assert_eq!(rig.select(0, "limb").unwrap(), vec![2, 3]);
        assert!(rig.limb_length(3).unwrap() > 0.4);
    }

    /// Installed data: the PCTP tags of the capability names match the tags
    /// stored in the `.creaturedata` test vector (`limb limb foot cute cspd`).
    #[test]
    fn installed_capability_tags() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let tags = capability_tags(&index);
        println!("{} capability tags", tags.len());
        let tag = |n: &str| tags.get(n).map(|t| String::from_utf8_lossy(t).to_string());
        assert_eq!(tag("limb").as_deref(), Some("limb"));
        assert_eq!(tag("foot").as_deref(), Some("foot"));
        assert_eq!(tag("creaturespeed").as_deref(), Some("cspd"));
        println!("cuteness {:?} spine {:?} mouth {:?} grasper {:?} eye {:?}", tag("cuteness"), tag("spine"), tag("mouth"), tag("grasper"), tag("eye"));
        // The foot of the x_leg_09 assembly carries 'foot'.
        let caps = part_capabilities(&index, CREATURE_PARTS, 0x7F81_5C91);
        println!("Pads: {:?}", caps.iter().map(|(t, l)| (String::from_utf8_lossy(t).to_string(), *l)).collect::<Vec<_>>());
        assert!(caps.iter().any(|(t, _)| t == b"foot"));
    }
}
