//! Swarm `gameModel` components (type 0x25): models of the game's model world
//! placed by an effect - tribal tool and held props (fishing nets, gathering
//! baskets, maracas, staffs, food piles), Civ unfolding city walls, balloons
//! and vignette props, Space terraform tool pods, Creature eggs / cocoons /
//! brains, editor dais and rigblock detail models.
//!
//! NATIVE (effect data `format::GameModelDef`, SporeModder-FX
//! `GameModelEffect`, versions 7 / 8, decoded byte-exact for all 296
//! installed components; notes `notes\swarm-model-notes.md`):
//! * Layout: flags (4 message, 8 persist, 0x10 fixed size, 0x20 no
//!   attachments), size, colour (LE rgb), alpha, the model key (instance
//!   FIRST, then group - the reverse of a ResourceID), a model world name
//!   hash, animations (length range, time curve, vary, speed scale, channel
//!   hash, mode 0 loop / 1 sustain / 2 single), splitters and splits (model
//!   splitting, only on four effects), model groups, options (bit set, see
//!   `format::game_model_option`), pick level, override set, message id.
//! * Placement: the block frame of the effect (offset / rotation / scale) like
//!   a `model` component; the model is the RW4 of that key, or the
//!   `modelMeshLOD0` of the model property list (`.prop`) of that key.
//!
//! * Worlds (see [`world`], notes `notes\swarm-anim-notes.md`): the `world`
//!   hash names the retail model world (`IModelManager::GetWorld`) the model
//!   lives in; the games' own code creates them (ModAPI constants and the
//!   decompiled creators). 0 is the default world.
//!
//! ADAPTED (the native model instance was not traced):
//! * Only models of a world OpenSpore has a scene for in the current stage are
//!   drawn (see [`drawn`]): world 0, the Cell model world in the Cell stage and
//!   the Space model world in Space, and the editor backdrop world
//!   (`cEditor::mpBackgroundModelWorld`) while a creator session shows a Test
//!   Drive backdrop ([`set_editor_backdrop`]; its models are the
//!   `background`-group dioramas of `editor_doom_mode_entry` /
//!   `editor_sci_fi_mode_entry`). The other worlds are separate renderer
//!   scenes (planet terrain / weather, galaxy background, hazard volumes);
//!   groups (`background`, `overdraw`, `testEnv`,
//!   `partsPaintEnv`, `rigblockEffect`, `gameBackground`) route a model into a
//!   pass of those scenes, so grouped models are not drawn. OpenSpore's
//!   editors and Space views build their own environments.
//! * Size: `size x block scale x parent size modifier` is a uniform scale of
//!   the model; the `fixed` flag drops the effect's own scales.
//! * Colour / alpha: `colour x alpha` tint the model's material (the
//!   `applyColor*` options, `highlight`, `applyNightLights` are not applied);
//!   models are lit by the scene lights; `aboveGround` is not applied (the
//!   caller's frame is already on the ground).
//! * Life: see `sim::GameModelState`. Animations: game_anim.rs (the model's own
//!   clips by channel id, the curve as clip position over the entry's length).

use bevy::prelude::*;

use super::format::GameModelDef;
use super::sim::Frame;

/// `OPENSPORE_GAME_MODELS=0` turns gameModel drawing off (before / after
/// captures, debugging).
fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OPENSPORE_GAME_MODELS").map_or(true, |v| v != "0"))
}

/// Retail model worlds named by `gameModel` components (`world` hashes; the
/// registries do not know their names).
pub mod world {
    /// The default world.
    pub const DEFAULT: u32 = 0;
    /// `Simulator::Cell::cCellGFX::kCellModelWorldID` (ModAPI): the Cell
    /// stage's main model world, the one the cells themselves are drawn in
    /// (`CG_cometHatch`).
    pub const CELL: u32 = 0x0101_0020;
    /// A Space subsystem's model world: the function at 0xEB24F0 (march2017
    /// image) creates the model world and a lighting world of this id on
    /// start (`sg_comet`). INFERRED to be the Space stage's main scene.
    pub const SPACE: u32 = 0x0487_B104;
    /// `Simulator::cGameViewManager::GraphicsWorldID` (ModAPI): the simulator
    /// model world; its only users are the `gameBackground` galaxy models.
    pub const SIMULATOR: u32 = 0x00EB_9968;
    /// `Terrain::ITerrain::WORLD_ID`: the terrain sphere's model world (hazard
    /// volumes in model group 0x06086E65).
    pub const TERRAIN: u32 = 0x03FB_AE24;
    /// `Terrain::cWeatherManager::WORLD_ID`.
    pub const WEATHER: u32 = 0x0611_C912;
    /// `cEditor::mpBackgroundModelWorld` (creator editors' backdrop world).
    pub const EDITOR_BACKGROUND: u32 = 0x021B_37D6;
}

/// A creator session shows a Test Drive backdrop whose effect holds models of
/// the editor background world ([`set_editor_backdrop`]).
static EDITOR_BACKDROP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The creator session (`creature_editor`) turns the editor background world on
/// while a Test Drive backdrop other than the default room is shown, and off
/// again otherwise (the default room is OpenSpore's own `editor_environment`
/// room; its backdrop effect would draw a second landscape over it).
pub fn set_editor_backdrop(on: bool) {
    EDITOR_BACKDROP.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The model groups of the Test Drive scenes' models (`editor_*_mode_entry`:
/// the ground planes in world 0 carry 0x0223E8E0, the backdrop dioramas 0x0223E8E0
/// or 0x35907808; the registries do not name them).
const TEST_DRIVE_GROUPS: [u32; 2] = [0x0223_E8E0, 0x3590_7808];

/// Whether OpenSpore draws this model in `stage` (see the module notes);
/// `stage` None (no stage machine, tests) accepts every world OpenSpore draws.
pub fn drawn(def: &GameModelDef, stage: Option<&crate::stages::GameStage>) -> bool {
    drawn_with(def, stage, EDITOR_BACKDROP.load(std::sync::atomic::Ordering::Relaxed))
}

/// Whether a creator session shows a Test Drive backdrop ([`set_editor_backdrop`]).
pub fn editor_backdrop() -> bool {
    EDITOR_BACKDROP.load(std::sync::atomic::Ordering::Relaxed)
}

/// [`drawn`] with the editor backdrop switch given.
pub fn drawn_with(def: &GameModelDef, stage: Option<&crate::stages::GameStage>, editor_backdrop: bool) -> bool {
    use crate::stages::GameStage;
    if !enabled() {
        return false;
    }
    // The editor background world's models (the backdrop dioramas) draw with
    // the backdrop; so do the Test Drive ground planes of world 0 (a
    // `ce_beach_ground_plane-<bg>` per background, in the groups below). Other
    // grouped models are separate renderer passes OpenSpore has no scene for.
    if def.world == world::EDITOR_BACKGROUND {
        return editor_backdrop;
    }
    if !def.groups.is_empty() {
        return editor_backdrop && def.world == world::DEFAULT && def.groups.iter().all(|g| TEST_DRIVE_GROUPS.contains(g));
    }
    match def.world {
        world::DEFAULT => true,
        world::CELL => stage.is_none_or(|s| *s == GameStage::Cell),
        world::SPACE => stage.is_none_or(|s| *s == GameStage::Space),
        _ => false,
    }
}

/// World transform of a game model at `frame` (`unbasis` = AXES transposed,
/// as for model components).
pub fn transform(def: &GameModelDef, frame: &Frame, size_mod: f32, unbasis: Mat3) -> Transform {
    let scale = if def.fixed_size() {
        def.size
    } else {
        frame.scale * def.size * size_mod
    };
    Transform {
        translation: frame.origin,
        rotation: Quat::from_mat3(&(frame.rot * unbasis)),
        scale: Vec3::splat(scale.max(0.0)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use super::super::format::tests::{block_bytes, W};
    use super::super::format::{kind, EffectDirectory, IMPORT_MASK};
    use super::super::sim::{GameModelState, Mods};
    use super::super::SwarmLibrary;
    use super::*;

    /// A directory with one gameModel component (version 8) and one effect
    /// using it at `offset`.
    fn directory(version: u16) -> Vec<u8> {
        let mut body = W::default();
        body.u32(0x10).f32(2.0).le(&[1.0, 0.5, 0.25]).f32(0.5);
        // The model key: instance first, then group.
        body.u32(0x1234_5678).u32(0x0BAD_F00D).u32(0);
        // One animation: length 1..2, a three key curve, vary, speed, channel, mode.
        body.i32(1).le(&[1.0, 2.0]).floats(&[0.0, 0.5, 1.0]).f32(0.1).f32(1.5).u32(0xAABB_CCDD).u8(2);
        // One splitter, one split (control, transform, two indices, origin).
        body.i32(1).i32(7).u8(1).u8(4);
        body.i32(1).u8(1).i32(3);
        body.u16(5).f32(2.0).le(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]).le(&[1.0, 2.0, 3.0]);
        body.i32(2).i32(4).i32(5).le(&[0.0, 0.0, 9.0]);
        // Groups, options, pick level, override set.
        body.i32(1).u32(0x0223_E8E0).u32(0x14).u8(1).u8(2);
        if version >= 8 {
            body.u32(0xCAFE_0001);
        }
        let mut w = W::default();
        w.u16(4);
        w.u16(kind::GAME_MODEL as u16).u16(version).u32(body.0.len() as u32 + 4).u32(1);
        w.0.extend(&body.0);
        w.u16(0xFFFF); // end of components
        w.u16(0xFFFF); // end of resources
        w.u16(1).u32(1); // one visual effect
        w.u32(0).u32(0).u32(0).le(&[0.0, 0.0]).f32(0.0).u8(0);
        w.floats(&[]).le(&[0.0; 3]).i32(0);
        w.i32(1);
        block_bytes(&mut w, kind::GAME_MODEL, 0, [0.0, 0.0, 1.0]);
        w.i32(0).u32(0).u32(0x42);
        w.i32(-1);
        w.i32(-1);
        w.u32(0);
        w.0
    }

    #[test]
    fn game_model_component_decodes_every_field() {
        let dir = EffectDirectory::parse(&directory(8)).unwrap();
        assert_eq!(dir.game_models.len(), 1);
        let g = &dir.game_models[0];
        assert_eq!(g.flags, 0x10);
        assert!(g.fixed_size());
        assert_eq!(g.size, 2.0);
        assert_eq!(g.color, [1.0, 0.5, 0.25]);
        assert_eq!(g.alpha, 0.5);
        assert_eq!((g.model.instance, g.model.group), (0x1234_5678, 0x0BAD_F00D));
        assert_eq!(g.world, 0);
        assert_eq!(g.animations.len(), 1);
        let a = &g.animations[0];
        assert_eq!(a.length_range, [1.0, 2.0]);
        assert_eq!(a.curve, vec![0.0, 0.5, 1.0]);
        assert_eq!((a.curve_vary, a.speed_scale, a.channel, a.mode), (0.1, 1.5, 0xAABB_CCDD, 2));
        assert_eq!(g.splitters, vec![(7, 1, 4)]);
        assert_eq!(g.splits.len(), 1);
        assert_eq!(g.splits[0].kind, 1);
        assert_eq!(g.splits[0].index, 3);
        assert_eq!(g.splits[0].transform.offset, [1.0, 2.0, 3.0]);
        assert_eq!(g.splits[0].indices, vec![4, 5]);
        assert_eq!(g.splits[0].origin, [0.0, 0.0, 9.0]);
        assert_eq!(g.groups, vec![0x0223_E8E0]);
        assert_eq!(g.options, 0x14);
        assert!(g.has_option(super::super::format::game_model_option::ABOVE_GROUND));
        assert_eq!((g.pick_level, g.override_set), (1, 2));
        assert_eq!(g.message_id, 0xCAFE_0001);
        assert_eq!(dir.effects[0].blocks[0].kind, kind::GAME_MODEL);
    }

    /// Version 7 has no message id and drops the message flag.
    #[test]
    fn version_seven_has_no_message_id() {
        let dir = EffectDirectory::parse(&directory(7)).unwrap();
        assert_eq!(dir.game_models[0].message_id, 0);
        assert_eq!(dir.game_models[0].flags, 0x10);
    }

    fn model(world: u32, groups: Vec<u32>) -> GameModelDef {
        GameModelDef {
            flags: 0,
            size: 2.0,
            color: [1.0; 3],
            alpha: 1.0,
            model: super::super::format::ResourceId { group: 0, instance: 1 },
            world,
            animations: vec![],
            splitters: vec![],
            splits: vec![],
            groups,
            options: 0,
            pick_level: 0,
            override_set: 0,
            message_id: 0,
        }
    }

    #[test]
    fn only_known_worlds_without_groups_draw() {
        use crate::stages::GameStage;
        assert!(drawn(&model(0, vec![]), None));
        assert!(drawn(&model(0, vec![]), Some(&GameStage::Tribal)));
        assert!(!drawn(&model(world::TERRAIN, vec![]), None));
        assert!(!drawn_with(&model(world::EDITOR_BACKGROUND, vec![]), None, false));
        assert!(!drawn_with(&model(0, vec![0x0223_E8E0]), None, false));
        // The Cell and Space worlds are those stages' own scenes.
        assert!(drawn(&model(world::CELL, vec![]), Some(&GameStage::Cell)));
        assert!(!drawn(&model(world::CELL, vec![]), Some(&GameStage::Space)));
        assert!(drawn(&model(world::SPACE, vec![]), Some(&GameStage::Space)));
        assert!(!drawn(&model(world::SPACE, vec![]), Some(&GameStage::Cell)));
        // The editor backdrop world draws only while a creator backdrop is shown.
        // ... and so do the world-0 ground planes of the Test Drive groups.
        let plane = model(0, vec![0x0223_E8E0]);
        assert!(!drawn_with(&plane, None, false));
        assert!(drawn_with(&plane, None, true));
        assert!(!drawn_with(&model(0, vec![0x0223_E8E1]), None, true));
        let backdrop = model(world::EDITOR_BACKGROUND, vec![0x3590_7808]);
        assert!(!drawn_with(&backdrop, None, false));
        assert!(drawn_with(&backdrop, None, true));
        assert!(!drawn_with(&model(world::TERRAIN, vec![0x3590_7808]), None, true));
        // The simulator world only carries grouped (background) models.
        assert!(!drawn(&model(world::SIMULATOR, vec![0x064A_C354]), None));
    }

    #[test]
    fn size_scales_with_the_frame_unless_fixed() {
        let frame = Frame::new(Vec3::new(1.0, 2.0, 3.0), Quat::IDENTITY, 3.0);
        let unbasis = super::super::sim::AXES.transpose();
        let t = transform(&model(0, vec![]), &frame, 0.5, unbasis);
        assert_eq!(t.translation, Vec3::new(1.0, 2.0, 3.0));
        assert!((t.scale - Vec3::splat(3.0)).length() < 1e-6);
        let mut fixed = model(0, vec![]);
        fixed.flags = 0x10;
        let t = transform(&fixed, &frame, 0.5, unbasis);
        assert!((t.scale - Vec3::splat(2.0)).length() < 1e-6);
        // The importer's basis cancels: the model's up (Bevy +Y) is Spore +Z.
        let up = (transform(&model(0, vec![]), &Frame::new(Vec3::ZERO, Quat::IDENTITY, 1.0), 1.0, unbasis).rotation)
            * Vec3::Y;
        let spore_up = Frame::new(Vec3::ZERO, Quat::IDENTITY, 1.0).rot * Vec3::Z;
        assert!((up - spore_up).length() < 1e-5);
    }

    fn state(age: f32, stopped: bool) -> GameModelState {
        GameModelState {
            dir: 0,
            index: 0,
            uid: 1,
            frame: None,
            mods: Mods::default(),
            age,
            stopped,
        }
    }

    /// Life rule: props run until stopped; only all-`single` animations end.
    #[test]
    fn life_follows_the_animation_modes() {
        use super::super::format::GameModelAnimationDef;
        let anim = |mode, length| GameModelAnimationDef {
            length_range: [length, length],
            mode,
            ..Default::default()
        };
        let mut def = model(0, vec![]);
        assert!(state(99.0, false).running(&def));
        assert!(!state(0.0, true).running(&def));
        def.animations = vec![anim(2, 3.0)];
        assert!(state(2.9, false).running(&def));
        assert!(!state(3.1, false).running(&def));
        def.animations = vec![anim(2, 3.0), anim(1, 0.0)];
        assert!(state(99.0, false).running(&def));
        def.animations = vec![anim(0, 1.0)];
        assert!(state(99.0, false).running(&def));
    }

    /// Survey of the installed `gameModel` components:
    /// `cargo test swarm_fx::game_model::tests::survey_game_models -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_game_models() {
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
        let name = |id: u32| names.get(&id).cloned().unwrap_or(format!("{id:08x}"));
        // Resource types present per instance id.
        let mut by_instance: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
        for k in index.records.keys() {
            by_instance.entry(k.instance_id).or_default().push((k.type_id, k.group_id));
        }
        let mut hist: BTreeMap<String, u32> = BTreeMap::new();
        for (d, dir) in lib.dirs.iter().enumerate() {
            println!(
                "dir {d} {:08x}!{:08x}: {} gameModels (section count {:?})",
                dir.key_group,
                dir.key_instance,
                dir.game_models.len(),
                dir.section_counts.get(&0x25)
            );
            assert_eq!(dir.section_counts.get(&0x25).copied().unwrap_or(0) as usize, dir.game_models.len());
            let mut owners: HashMap<usize, Vec<String>> = HashMap::new();
            let by_index: HashMap<i32, u32> = dir.exports.iter().map(|(n, i)| (*i, *n)).collect();
            for (e, effect) in dir.effects.iter().enumerate() {
                for b in &effect.blocks {
                    if b.kind == kind::GAME_MODEL && b.index >= 0 && b.index & IMPORT_MASK == 0 {
                        let n = by_index.get(&(e as i32)).map(|n| name(*n)).unwrap_or(format!("#{e}"));
                        owners.entry(b.index as usize).or_default().push(format!(
                            "{n}[bflags {:#x} xform {:#x} scale {} off {:?}]",
                            b.flags, b.transform.flags, b.transform.scale, b.transform.offset
                        ));
                    }
                }
            }
            for (i, g) in dir.game_models.iter().enumerate() {
                let types: Vec<String> = by_instance
                    .get(&g.model.instance)
                    .map(|v| v.iter().map(|(t, gr)| format!("{t:08x}/{gr:08x}")).collect())
                    .unwrap_or_default();
                let key = format!(
                    "world {} groups {:?} options {:#x} flags {:#x} pick {} override {} anims {} splits {}",
                    name(g.world),
                    g.groups.iter().map(|x| name(*x)).collect::<Vec<_>>(),
                    g.options,
                    g.flags,
                    g.pick_level,
                    g.override_set,
                    g.animations.len(),
                    g.splits.len()
                );
                *hist.entry(key).or_default() += 1;
                println!(
                    "  gm {i}: model {:08x}!{} types {:?} size {} color {:?} alpha {} world {} groups {:?} opts {:#x} flags {:#x} anims {:?} | {:?}",
                    g.model.group,
                    name(g.model.instance),
                    types,
                    g.size,
                    g.color,
                    g.alpha,
                    name(g.world),
                    g.groups.iter().map(|x| name(*x)).collect::<Vec<_>>(),
                    g.options,
                    g.flags,
                    g.animations.iter().map(|a| (a.channel, a.mode, a.curve.len(), a.length_range)).collect::<Vec<_>>(),
                    owners.get(&i).map(|v| v.iter().take(3).collect::<Vec<_>>())
                );
            }
        }
        println!("--- histogram");
        for (k, n) in hist {
            println!("{n:>5}  {k}");
        }
    }

    /// Property lists of the models gameModels name:
    /// `cargo test swarm_fx::game_model::tests::dump_model_props -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires the installed game"]
    fn dump_model_props() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        let base = "D:/SteamLibrary/steamapps/common/Spore/OpenSpore/target/format-reference/sporemodder-fx/";
        let reg = std::fs::read_to_string(format!("{base}reg_property.txt")).unwrap_or_default();
        let props: HashMap<u32, String> = reg
            .lines()
            .filter_map(|l| l.split('\t').next())
            .map(|n| (crate::world_objects::name_id(n.trim()), n.trim().to_string()))
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut hist: BTreeMap<String, u32> = BTreeMap::new();
        let mut shown = 0;
        for dir in &lib.dirs {
            for g in &dir.game_models {
                if !seen.insert((g.model.group, g.model.instance)) {
                    continue;
                }
                let key = crate::dbpf::ResourceKey { type_id: 0x00B1_B104, group_id: g.model.group, instance_id: g.model.instance };
                let Ok(bytes) = index.read(key) else { continue };
                let Ok(list) = crate::prop::parse(&bytes) else { continue };
                for id in list.0.keys() {
                    *hist.entry(props.get(id).cloned().unwrap_or(format!("{id:08x}"))).or_default() += 1;
                }
                if shown < 6 {
                    shown += 1;
                    println!("== {:08x}!{:08x}", g.model.group, g.model.instance);
                    for (id, p) in &list.0 {
                        println!("   {} = {:?}", props.get(id).cloned().unwrap_or(format!("{id:08x}")), p);
                    }
                }
            }
        }
        println!("--- property histogram");
        for (k, n) in hist {
            println!("{n:>5} {k}");
        }
    }
}
