//! Census of the installed effect component kinds (test only):
//! `cargo test swarm_fx::census -- --ignored --nocapture`.
//!
//! For every component kind: the number of components in the installed
//! directories (highest quality tier) and the number of exported effects whose
//! tree reaches it (nested effect blocks, sequences and the effects that
//! meta-particles carry), with a few example names, and whether the OpenSpore
//! runtime draws / simulates it. Screen components are split by mode.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::format::{kind, IMPORT_MASK};
use super::{EffectRef, SwarmLibrary};

/// OpenSpore support of a component kind.
pub fn support(kind_code: u16) -> &'static str {
    match kind_code as u8 {
        _ if kind_code == kind::EFFECT as u16 => "nested effect reference (followed)",
        kind::PARTICLES | kind::META_PARTICLES | kind::SEQUENCE | kind::SHAKE | kind::RIBBON => "simulated + drawn",
        kind::MODEL => "drawn (model animations not played)",
        kind::SCREEN => "modes 0-3, 5, 10 drawn; 4 skybox, 6-9 user not",
        kind::SOUND => "decoded; played by the audio systems",
        0x28 => "drawn (volume.rs)",
        0x25 => "drawn (game_model.rs; worlds 0 / Cell / Space, no groups; clips played, game_anim.rs)",
        kind::DECAL => "drawn (decal.rs; types 0-3 and 9)",
        _ => "not drawn",
    }
}

pub fn name_of(kind_code: u16) -> &'static str {
    match kind_code {
        0x00 => "effect",
        0x01 => "particles",
        0x02 => "metaParticles",
        0x03 => "decal",
        0x04 => "sequence",
        0x05 => "sound",
        0x06 => "shake",
        0x07 => "camera",
        0x08 => "model",
        0x09 => "screen",
        0x0A => "light",
        0x0B => "game",
        0x0C => "fastParticles",
        0x0D => "distribute",
        0x0E => "ribbon",
        0x20 => "brush",
        0x21 => "terrainScript",
        0x22 => "skinpaintSettings",
        0x23 => "skinpaintDistribute",
        0x25 => "gameModel",
        0x26 => "skinpaintParticle",
        0x27 => "skinpaintFlood",
        0x28 => "volume",
        0x29 => "splitController",
        0x2A => "terrainDistribute",
        0x2B => "cloud",
        0x2C => "groundCover",
        0x2D => "mixEvent",
        0x2F => "text",
        _ => "?",
    }
}

/// Kinds (and screen modes as 0x100 + mode) reached from one effect.
fn reach(lib: &SwarmLibrary, effect: EffectRef, memo: &mut HashMap<EffectRef, HashSet<u16>>) -> HashSet<u16> {
    if let Some(found) = memo.get(&effect) {
        return found.clone();
    }
    memo.insert(effect, HashSet::new());
    let dir = lib.dir(effect.dir);
    let mut out = HashSet::new();
    let mut follow = |lib: &SwarmLibrary, index: i32, out: &mut HashSet<u16>, memo: &mut HashMap<_, _>| {
        if let Some(e) = lib.resolve_effect(effect.dir, index) {
            out.extend(reach(lib, e, memo));
        }
    };
    for b in &dir.effects[effect.index as usize].blocks {
        out.insert(b.kind as u16);
        let local = b.index >= 0 && b.index & IMPORT_MASK == 0;
        match b.kind {
            kind::EFFECT => follow(lib, b.index, &mut out, memo),
            kind::SEQUENCE if local => {
                for (_, e) in &dir.sequences[b.index as usize].instances {
                    follow(lib, *e, &mut out, memo);
                }
            }
            kind::META_PARTICLES if local => {
                let (index, type_code) = dir.metas[b.index as usize].component;
                if type_code == 0 {
                    follow(lib, index, &mut out, memo);
                } else {
                    out.insert(type_code as u16);
                }
            }
            kind::SCREEN if local => {
                out.insert(0x100 + dir.screens[b.index as usize].mode as u16);
            }
            _ => {}
        }
    }
    memo.insert(effect, out.clone());
    out
}

#[test]
#[ignore = "requires the installed game"]
fn survey_component_kinds() {
    let install = crate::spore_install::discover().unwrap();
    let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
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
    let mut components: BTreeMap<u16, u32> = BTreeMap::new();
    for dir in &lib.dirs {
        for (&k, &n) in &dir.section_counts {
            *components.entry(k).or_default() += n;
        }
    }
    let mut memo = HashMap::new();
    let mut users: BTreeMap<u16, Vec<u32>> = BTreeMap::new();
    let mut exports: Vec<(u32, EffectRef)> = lib.exports.iter().map(|(k, v)| (*k, *v)).collect();
    exports.sort_by_key(|e| e.0);
    for (name, effect) in &exports {
        for k in reach(&lib, *effect, &mut memo) {
            users.entry(k).or_default().push(*name);
        }
    }
    println!("{} exported effects", exports.len());
    println!("kind  name                 components  exported-effects  support  (examples)");
    let mut rows: Vec<(u16, u32, usize)> = components
        .keys()
        .chain(users.keys().filter(|k| **k < 0x100))
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|k| (k, components.get(&k).copied().unwrap_or(0), users.get(&k).map_or(0, |u| u.len())))
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.2));
    for (k, n, u) in rows {
        let examples: Vec<String> = users
            .get(&k)
            .map(|v| {
                v.iter()
                    .filter_map(|id| names.get(id).cloned())
                    .take(4)
                    .collect()
            })
            .unwrap_or_default();
        println!("0x{k:02x}  {:<20} {n:>10}  {u:>16}  {}  {examples:?}", name_of(k), support(k));
    }
    for (k, v) in users.range(0x100..) {
        let examples: Vec<String> = v.iter().filter_map(|id| names.get(id).cloned()).take(4).collect();
        println!("screen mode {:>2}: {:>5} exported effects  {examples:?}", k - 0x100, v.len());
    }
}
