//! Installed-data checks and the census of effects that carry sounds
//! (`cargo test swarm_fx::sound::installed -- --ignored --nocapture`).

use std::collections::{BTreeMap, HashMap};

use bevy::prelude::*;

use super::{LiveSound, SwarmSounds};
use crate::swarm_fx::{
    format::sound_flag,
    sim::{EffectInstance, Frame, LodView, Mods, SimContext},
    EffectRef, SwarmLibrary, SwarmMaps,
};
use crate::world_objects::name_id;

/// Run `effect` for `seconds` and collect the distinct sound components it
/// reports (the first sight of each voice).
fn run(lib: &SwarmLibrary, effect: EffectRef, seconds: f32, seed: u64) -> Vec<LiveSound> {
    let maps = SwarmMaps::default();
    let mut ids = 0u64;
    let mut shake = Vec3::ZERO;
    let mut sounds = SwarmSounds::default();
    let mut instance = EffectInstance::new(effect, seed, false);
    let mut seen: Vec<LiveSound> = vec![];
    let mut t = 0.0;
    while t < seconds {
        sounds.live.clear();
        let mut ctx = SimContext {
            lib,
            maps: &maps,
            app_flags: 0,
            ids: &mut ids,
            camera: None,
            camera_rot: None,
            params: [None; 16],
            path: &[],
            shake: &mut shake,
            entity: Entity::PLACEHOLDER,
            sounds: &mut sounds,
            lod: LodView::OFF,
        };
        let alive = instance.update(&mut ctx, 0.05, Frame::new(Vec3::ZERO, Quat::IDENTITY, 1.0), Mods::default());
        for l in &sounds.live {
            if !seen.iter().any(|s| s.voice == l.voice) {
                seen.push(*l);
            }
        }
        if !alive {
            break;
        }
        t += 0.05;
    }
    seen
}

/// The stage an effect-name prefix belongs to (ADAPTED: by the naming of the
/// installed effects, `cell_*` Cell, `crg_*` / `cr_*` Creature, `trg_*`
/// Tribal, `civ_*` / `city_*` Civ, `sg_*` / `spg_*` Space ...).
fn stage_of(prefix: &str) -> &'static str {
    match prefix {
        "cell" => "Cell",
        "crg" | "cr" => "Creature",
        "trg" | "tribe" | "npc" => "Tribal",
        "civ" | "city" | "cg" | "planner" | "object" => "Civ",
        "sg" | "space" | "spg" | "planet" | "ufo" => "Space",
        "be" | "edt" | "editor" | "ui" | "gge" | "attention" => "Editors/UI",
        _ => "other",
    }
}

fn library() -> Option<SwarmLibrary> {
    let install = crate::spore_install::discover().ok()?;
    let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
    Some(SwarmLibrary::load(&index))
}

/// Effects report their sounds as they start (a sound-only effect ends in the
/// same step and still reports), a loop carries its flags, silent effects stay
/// silent.
#[test]
#[ignore = "needs the installed Spore effect packages"]
fn installed_effects_report_their_sounds() {
    let Some(lib) = library() else { return };
    let of = |name: &str, seconds: f32| {
        let root = lib.find(name_id(name)).unwrap_or_else(|| panic!("{name} missing"));
        run(&lib, root, seconds, 1)
    };
    let ids = |v: &[LiveSound]| v.iter().map(|s| s.sound).collect::<Vec<_>>();
    let die = of("cell_death_eaten", 3.0);
    assert!(ids(&die).contains(&name_id("sfx_cell_avatar_die_pop")), "{die:?}");
    let zap = of("cell_attack_electrical_charged_multiple", 1.0);
    let lp = zap
        .iter()
        .find(|s| s.sound == name_id("sfx_cell_electricity_lp"))
        .expect("the electricity loop");
    assert!(lp.flags & sound_flag::LOOP != 0);
    assert!(of("cell_core_Burst_md", 3.0).is_empty(), "silent effects stay silent");
}

#[derive(Default)]
struct Row {
    effects: usize,
    with_sound: usize,
    components: usize,
    flags: [usize; 5],
    loops_without_stop: usize,
    examples: Vec<String>,
}

fn add(row: &mut Row, started: &[LiveSound], label: &str, bits: &[u32; 5]) {
    row.effects += 1;
    if started.is_empty() {
        return;
    }
    row.with_sound += 1;
    row.components += started.len();
    for s in started {
        for (i, bit) in bits.iter().enumerate() {
            row.flags[i] += (s.flags & bit != 0) as usize;
        }
        if s.flags & sound_flag::LOOP != 0 && s.flags & sound_flag::STOP_WITH_EFFECT == 0 {
            row.loops_without_stop += 1;
        }
    }
    if row.examples.len() < 3 {
        row.examples.push(label.to_string());
    }
}

/// The census: per effect-name prefix, how many exported effects start a sound
/// within 4 s of simulation, how many components that is, the flag mix, and how
/// the component's 3D flag compares with its soundProp's `is3d`.
#[test]
#[ignore = "needs the installed Spore effect packages; prints the census"]
fn survey_effect_sounds() {
    let Some(lib) = library() else { return };
    let reg =
        std::fs::read_to_string("D:/SteamLibrary/steamapps/common/Spore/OpenSpore/target/format-reference/reg_file.txt")
            .unwrap_or_default();
    let names: HashMap<u32, String> = reg
        .lines()
        .filter_map(|l| l.split('\t').next())
        .map(|n| (name_id(n.trim()), n.trim().to_string()))
        .collect();
    let mut exports: Vec<(u32, EffectRef)> = lib.exports.iter().map(|(k, v)| (*k, *v)).collect();
    exports.sort_by_key(|e| e.0);
    let prefix = |name: &str| -> String {
        let lower = name.to_lowercase();
        lower.split(['_', '-']).next().unwrap_or("").chars().take(12).collect()
    };
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let mut stages: BTreeMap<&'static str, Row> = BTreeMap::new();
    let mut dirs: BTreeMap<String, Row> = BTreeMap::new();
    let mut totals = Row::default();
    let mut unnamed = 0;
    let install = crate::spore_install::discover().unwrap();
    let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
    let audio = crate::game_audio::AudioLibrary::from_index(&index);
    let mut flag_vs_prop: BTreeMap<(bool, bool), usize> = BTreeMap::new();
    let bits = [
        sound_flag::THREE_D,
        sound_flag::LOOP,
        sound_flag::VOLUME,
        sound_flag::STOP_WITH_EFFECT,
        sound_flag::TRACK,
    ];
    for (name, effect) in &exports {
        let label = match names.get(name) {
            Some(n) => n.clone(),
            None => {
                unnamed += 1;
                format!("#{name:08x}")
            }
        };
        let started = run(&lib, *effect, 4.0, 7);
        let key = prefix(&label);
        let stage = if label.starts_with('#') { "unnamed" } else { stage_of(&key) };
        add(stages.entry(stage).or_default(), &started, &label, &bits);
        let dir_key = lib.dir(effect.dir);
        let dir_name = names.get(&dir_key.key_instance).cloned().unwrap_or_else(|| format!("{:08x}", dir_key.key_instance));
        add(dirs.entry(dir_name).or_default(), &started, &label, &bits);
        add(rows.entry(key).or_default(), &started, &label, &bits);
        add(&mut totals, &started, &label, &bits);
        for s in &started {
            if let Ok(def) = audio.resolve(&format!("#{:08X}", s.sound)) {
                *flag_vs_prop.entry((s.flags & sound_flag::THREE_D != 0, def.is_3d)).or_default() += 1;
            }
        }
    }
    println!("{} exported effects ({unnamed} without a registered name)", exports.len());
    println!("prefix        effects  with-sound  components  3d  loop  volume  stop  track  loops-without-stop  examples");
    let mut list: Vec<_> = rows.iter().filter(|(_, r)| r.with_sound > 0).collect();
    list.sort_by_key(|(_, r)| std::cmp::Reverse(r.with_sound));
    for (name, r) in list.iter().take(70) {
        println!(
            "{name:<12} {:>8} {:>11} {:>11} {:>3} {:>5} {:>7} {:>5} {:>6} {:>19}  {:?}",
            r.effects,
            r.with_sound,
            r.components,
            r.flags[0],
            r.flags[1],
            r.flags[2],
            r.flags[3],
            r.flags[4],
            r.loops_without_stop,
            r.examples
        );
    }
    println!(
        "TOTAL        {:>8} {:>11} {:>11} {:>3} {:>5} {:>7} {:>5} {:>6} {:>19}",
        totals.effects,
        totals.with_sound,
        totals.components,
        totals.flags[0],
        totals.flags[1],
        totals.flags[2],
        totals.flags[3],
        totals.flags[4],
        totals.loops_without_stop
    );
    println!("by stage (named effects by name prefix; the rest are unnamed in reg_file): effects  with-sound  components");
    for (stage, r) in &stages {
        println!("  {stage:<12} {:>6} {:>6} {:>6}", r.effects, r.with_sound, r.components);
    }
    println!("by effect directory: effects  with-sound  components");
    for (dir, r) in &dirs {
        println!("  {dir:<24} {:>6} {:>6} {:>6}", r.effects, r.with_sound, r.components);
    }
    println!("component 3d flag vs soundProp is3d (flag, prop) -> components: {flag_vs_prop:?}");
}
