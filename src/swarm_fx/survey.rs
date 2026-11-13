//! Test-only survey: print the whole component tree of exported effects
//! (`SWARM_DUMP=name1,name2 cargo test swarm_fx::survey::dump_effects -- --ignored --nocapture`).
//!
//! Every block is listed with its kind, flags, app flags, transform and a one-line summary of
//! the component it addresses (particle / decal / ribbon / model / game / ... fields that decide
//! whether the Swarm player draws it); nested effects, sequences and meta-particle carriers are
//! followed.

use std::collections::HashSet;

use super::format::{kind, IMPORT_MASK};
use super::{EffectRef, SwarmLibrary};

fn names() -> std::collections::HashMap<u32, String> {
    let reg = std::fs::read_to_string(
        "D:/SteamLibrary/steamapps/common/Spore/OpenSpore/target/format-reference/reg_file.txt",
    )
    .unwrap_or_default();
    reg.lines()
        .filter_map(|l| l.split('\t').next())
        .map(|n| (crate::world_objects::name_id(n.trim()), n.trim().to_string()))
        .collect()
}

fn block_line(lib: &SwarmLibrary, e: EffectRef, b: &super::format::EffectBlock, names: &std::collections::HashMap<u32, String>) -> String {
    let dir = lib.dir(e.dir);
    let n = |id: u32| names.get(&id).cloned().unwrap_or(format!("{id:08x}"));
    let rid = |r: super::format::ResourceId| format!("{:08x}!{}", r.group, n(r.instance));
    let local = b.index >= 0 && b.index & IMPORT_MASK == 0;
    let mut s = format!(
        "{} #{}{} flags {:#x} app {:#x}/{:#x} sel {}/{} lod {}..{} xf {:#x} scale {} off {:?}",
        super::census::name_of(b.kind as u16),
        b.index & !IMPORT_MASK,
        if local { "" } else { " (import)" },
        b.flags,
        b.app_flags,
        b.app_flags_mask,
        b.selection_group,
        b.selection_chance,
        b.lod_begin,
        b.lod_end,
        b.transform.flags,
        b.transform.scale,
        b.transform.offset,
    );
    if !local {
        return s;
    }
    let i = b.index as usize;
    match b.kind {
        kind::PARTICLES => {
            let p = &dir.particles[i];
            let flags: Vec<u32> = (0..64).filter(|bit| p.has(*bit)).collect();
            s += &format!(
                "\n        tex {} fmt {} draw {:#x}/{:#x} buf {} layer {} | life {:?} rate {:?} size {:?} alpha {:?} color {:?} | flagbits {:?} | emit vol {:?} dir {:?} speed {:?} | grav {} stretch {} physics {} align {} tiles {:?} frames {}/{}",
                rid(p.texture.resource), p.texture.format, p.texture.draw_mode, p.texture.draw_flags, p.texture.buffer, p.texture.layer,
                p.life, p.rate, p.size, p.alpha, p.color, flags, p.emit_volume, p.emit_direction, p.emit_speed,
                p.gravity_strength, p.velocity_stretch, p.physics_type, p.align_mode, p.tile_count, p.frame_start, p.frame_count
            );
        }
        kind::META_PARTICLES => {
            let m = &dir.metas[i];
            s += &format!("\n        life {:?} rate {:?} size {:?} component {:?} delay {:?}", m.life, m.rate, m.size, m.component, m.emit_delay);
        }
        kind::RIBBON => {
            let r = &dir.ribbons[i];
            s += &format!("\n        {:#?}", r);
        }
        kind::DECAL => {
            let d = &dir.decals[i];
            s += &format!(
                "\n        type {} life {} life_type {} flags {:#x} tex {} draw {:#x}/{:#x} layer {} size {:?} alpha {:?} color {:?} aspect {:?} rot {:?}",
                d.kind, d.life, d.life_type, d.flags, rid(d.texture.resource), d.texture.draw_mode, d.texture.draw_flags, d.texture.layer,
                d.size, d.alpha, d.color, d.aspect, d.rotation
            );
        }
        kind::MODEL => {
            let m = &dir.models[i];
            s += &format!("\n        model {} size {} material {} flags {:#x}", rid(m.resource), m.size, rid(m.material), m.flags);
        }
        kind::GAME_MODEL => {
            let g = &dir.game_models[i];
            s += &format!("\n        gameModel {} world {:08x} size {} flags {:#x} options {:#x} anims {}", rid(g.model), g.world, g.size, g.flags, g.options, g.animations.len());
        }
        kind::SEQUENCE => {
            s += &format!("\n        {:?}", dir.sequences[i]);
        }
        kind::SOUND => {
            let d = &dir.sounds[i];
            s += &format!("\n        sound {} flags {:#x}", rid(d.sound), d.flags);
        }
        kind::SCREEN => {
            let d = &dir.screens[i];
            s += &format!("\n        screen mode {} flags {:#x} life {} tex {}", d.mode, d.flags, d.life, rid(d.texture));
        }
        _ => {}
    }
    s
}

fn walk(
    lib: &SwarmLibrary,
    e: EffectRef,
    depth: usize,
    names: &std::collections::HashMap<u32, String>,
    seen: &mut HashSet<EffectRef>,
) {
    let pad = "  ".repeat(depth);
    let dir = lib.dir(e.dir);
    let def = &dir.effects[e.index as usize];
    println!(
        "{pad}effect #{} (dir {:08x}) flags {:#x} app {:#x} blocks {}",
        e.index, dir.key_instance, def.flags, def.app_flags_mask, def.blocks.len()
    );
    if !seen.insert(e) {
        println!("{pad}  (already shown)");
        return;
    }
    for b in &def.blocks {
        println!("{pad}  {}", block_line(lib, e, b, names));
        let local = b.index >= 0 && b.index & IMPORT_MASK == 0;
        match b.kind {
            kind::EFFECT => match lib.resolve_effect(e.dir, b.index) {
                Some(c) => walk(lib, c, depth + 2, names, seen),
                None => println!("{pad}    unresolved import {:?}", dir.imports.get((b.index & !IMPORT_MASK) as usize).map(|n| names.get(n).cloned().unwrap_or(format!("{n:08x}")))),
            },
            kind::SEQUENCE if local => {
                for (t, c) in &dir.sequences[b.index as usize].instances {
                    println!("{pad}    seq entry at {t:?}:");
                    if let Some(c) = lib.resolve_effect(e.dir, *c) {
                        walk(lib, c, depth + 3, names, seen);
                    }
                }
            }
            kind::META_PARTICLES if local => {
                let (index, type_code) = dir.metas[b.index as usize].component;
                if type_code == 0 {
                    if let Some(c) = lib.resolve_effect(e.dir, index) {
                        walk(lib, c, depth + 2, names, seen);
                    }
                }
            }
            _ => {}
        }
    }
}

#[test]
#[ignore = "requires the installed game"]
fn dump_effects() {
    let Ok(target) = std::env::var("SWARM_DUMP") else { return };
    let install = crate::spore_install::discover().unwrap();
    let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
    let lib = SwarmLibrary::load(&index);
    let names = names();
    for name in target.split(',') {
        println!("==== {name}");
        match lib.exports.get(&crate::world_objects::name_id(name)) {
            Some(root) => walk(&lib, *root, 0, &names, &mut HashSet::new()),
            None => println!("not exported"),
        }
    }
}

/// Every exported effect whose name contains `SWARM_FIND` (case-insensitive).
#[test]
#[ignore = "requires the installed game"]
fn find_effects() {
    let Ok(pattern) = std::env::var("SWARM_FIND") else { return };
    let install = crate::spore_install::discover().unwrap();
    let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
    let lib = SwarmLibrary::load(&index);
    let names = names();
    let pattern = pattern.to_lowercase();
    let mut found: Vec<&String> = names
        .iter()
        .filter(|(id, n)| lib.exports.contains_key(id) && n.to_lowercase().contains(&pattern))
        .map(|(_, n)| n)
        .collect();
    found.sort();
    for n in found {
        println!("{n}");
    }
}

/// Property list dump of one PROP (`SWARM_PROP=group!instance`, hex, e.g. `41842000!a5aab70a`): every property
/// with its registry name and a decoded value.
#[test]
#[ignore = "requires the installed game"]
fn dump_prop() {
    let Ok(spec) = std::env::var("SWARM_PROP") else { return };
    let (g, i) = spec.split_once('!').unwrap();
    let key = crate::dbpf::ResourceKey {
        group_id: u32::from_str_radix(g, 16).unwrap(),
        instance_id: u32::from_str_radix(i, 16).unwrap(),
        type_id: crate::asset_catalog::PROP,
    };
    let install = crate::spore_install::discover().unwrap();
    let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
    let properties = crate::prop::parse(&index.read(key).unwrap()).unwrap();
    let reg = std::fs::read_to_string(
        "D:/SteamLibrary/steamapps/common/Spore/OpenSpore/target/format-reference/sporemodder-fx/reg_property.txt",
    )
    .unwrap_or_default();
    let prop_names: std::collections::HashMap<u32, String> = reg
        .lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let name = it.next()?;
            let id = u32::from_str_radix(it.next()?.trim().trim_start_matches("0x"), 16).ok()?;
            Some((id, name.to_string()))
        })
        .collect();
    let names = names();
    for (id, p) in &properties.0 {
        let name = prop_names.get(id).cloned().unwrap_or(format!("{id:08x}"));
        let value = if let Some(n) = p.number() {
            format!("{n}")
        } else if let Some(k) = p.key() {
            format!("key {:08x}!{}/{:08x}", k.group_id, names.get(&k.instance_id).cloned().unwrap_or(format!("{:08x}", k.instance_id)), k.type_id)
        } else if let Some(v) = p.vector3() {
            format!("{v:?}")
        } else if let Some((_, _, t)) = p.text() {
            t
        } else {
            format!("kind {:#x} x{} {:02x?}", p.kind, p.values.len(), p.values.first())
        };
        println!("{name} [{:#x}{}] = {value}", p.kind, if p.array { " array" } else { "" });
    }
}

/// Decode statistics of rasters by name (`SWARM_TEX=a,b`): mip chain, size, alpha / colour of the base level.
#[test]
#[ignore = "requires the installed game"]
fn probe_rasters() {
    let Ok(list) = std::env::var("SWARM_TEX") else { return };
    let install = crate::spore_install::discover().unwrap();
    let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
    for name in list.split(',') {
        let key = crate::dbpf::ResourceKey {
            group_id: 0,
            instance_id: crate::world_objects::name_id(name),
            type_id: crate::asset_catalog::RW4,
        };
        let Ok(bytes) = index.read(key) else {
            println!("{name}: not installed");
            continue;
        };
        let Some(mips) = crate::texture::decode_rw4_mips(&bytes) else {
            println!("{name}: decode failed ({} bytes)", bytes.len());
            continue;
        };
        let base = &mips[0];
        let n = (base.width * base.height) as f32;
        let alpha: Vec<u8> = base.rgba.chunks_exact(4).map(|p| p[3]).collect();
        let opaque = alpha.iter().filter(|a| **a > 16).count() as f32 / n;
        let mean_a = alpha.iter().map(|a| *a as f32).sum::<f32>() / n;
        let mean_rgb: Vec<f32> = (0..3).map(|c| base.rgba.chunks_exact(4).map(|p| p[c] as f32).sum::<f32>() / n).collect();
        println!(
            "{name}: {} mips {:?}, {}x{}, alpha mean {mean_a:.1} (>16: {:.1}%), rgb mean {mean_rgb:.1?}",
            mips.len(),
            mips.iter().map(|m| (m.width, m.height)).collect::<Vec<_>>(),
            base.width,
            base.height,
            opaque * 100.0
        );
    }
}
