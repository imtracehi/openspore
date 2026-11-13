//! Swarm visual effects runtime (generic; any stage can use it).
//!
//! Retail Spore draws most of its atmosphere with Swarm effects: particle
//! emitters, meta-particles (particles that carry whole effects), models,
//! screen effects and ribbons, grouped into visual effects that live in effect
//! directories. This module loads the installed directories ([`format`]),
//! instantiates effects by their exported name id, simulates them ([`sim`]) and
//! draws them with Bevy ([`render`]).
//!
//! Provenance: the effect data, component layouts, flag meanings, curves and
//! texture/material references are original. The simulation semantics (how a
//! curve is sampled, what `vary` scales, stretch, wiggle, loop boxes...) follow
//! the SporeModder-FX field names and documented behaviour and are labelled
//! `ADAPTED` where the native Swarm code has not been traced.
//!
//! Screen filter chains (`filters.rs`, `filters_gpu.rs`), particle buffers,
//! ribbons (`render.rs`, `sim.rs`) and camera shakes (`shake.rs`) are described
//! in `notes\swarm-filters-notes.md`; volume components (fires, `volume.rs`)
//! in `notes\swarm-volume-notes.md`; decal components (ground marks, `decal.rs`)
//! in `notes\swarm-decal-notes.md`.
//!
//! Use: write a [`SwarmPlay`] message (or spawn a [`SwarmEffect`] component),
//! optionally attached to an entity; stop it with [`SwarmStop`] or by setting
//! [`SwarmEffect::stop`]. Stage code supplies effect maps (water currents...)
//! through [`SwarmMaps`] and app flags through [`SwarmAppFlags`].

pub mod decal;
pub mod game_anim;
pub mod game_model;
pub mod filters;
mod filters_gpu;
pub mod format;
pub mod galactic;
pub mod galactic_draw;
#[cfg(test)]
mod galactic_golden;
pub mod models;
pub mod render;
pub mod shake;
pub mod sim;
pub mod sound;
pub mod volume;
#[cfg(test)]
mod census;
#[cfg(test)]
mod survey;

use std::collections::HashMap;

use bevy::prelude::*;

use crate::{asset_catalog::ResourceIndex, dbpf::ResourceKey};
use format::{EffectDirectory, IMPORT_MASK, TYPE_EFFDIR};

/// Reference to one visual effect inside the loaded library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EffectRef {
    pub dir: u16,
    pub index: u32,
}

/// Every installed effect directory of the preferred quality tier.
#[derive(Resource, Default)]
pub struct SwarmLibrary {
    pub dirs: Vec<EffectDirectory>,
    /// Export name id -> effect (first directory wins, imports resolved).
    pub exports: HashMap<u32, EffectRef>,
    pub group: u32,
    pub loaded: bool,
    pub errors: Vec<String>,
    /// Directory holding [`SwarmLibrary::model_stand_in`] effects.
    pub synthetic: Option<u16>,
    /// The blend of each effect material by its compiled render states
    /// (`render::material_blends`).
    pub material_blends: HashMap<u32, render::Blend>,
}

impl SwarmLibrary {
    /// Decode the effect directories of the highest installed quality tier.
    pub fn load(index: &ResourceIndex) -> SwarmLibrary {
        let mut library = SwarmLibrary {
            loaded: true,
            ..Default::default()
        };
        // The tier of Settings > Graphics > Graphics Effects first.
        let Some(group) = crate::front_end::graphics::effect_groups().into_iter().find(|g| {
            index
                .records
                .keys()
                .any(|k| k.type_id == TYPE_EFFDIR && k.group_id == *g)
        }) else {
            library.errors.push("no installed effect directories".into());
            return library;
        };
        library.group = group;
        let mut keys: Vec<ResourceKey> = index
            .records
            .keys()
            .copied()
            .filter(|k| k.type_id == TYPE_EFFDIR && k.group_id == group)
            .collect();
        // Deterministic order; `games` holds most effects, so it goes first
        // when an export name appears twice.
        let games = crate::world_objects::name_id("games");
        keys.sort_by_key(|k| (k.instance_id != games, k.instance_id));
        for key in keys {
            match index
                .read(key)
                .and_then(|bytes| EffectDirectory::parse(&bytes))
            {
                Ok(mut dir) => {
                    dir.key_group = key.group_id;
                    dir.key_instance = key.instance_id;
                    library.dirs.push(dir);
                }
                Err(error) => library
                    .errors
                    .push(format!("effdir {:08x}!{:08x}: {error}", key.group_id, key.instance_id)),
            }
        }
        for (d, dir) in library.dirs.iter().enumerate() {
            for (&name, &index) in &dir.exports {
                if index < 0 || index & IMPORT_MASK != 0 {
                    continue;
                }
                library.exports.entry(name).or_insert(EffectRef {
                    dir: d as u16,
                    index: index as u32,
                });
            }
        }
        // Exported imports (an effect re-exported under another name).
        for dir in &library.dirs {
            for (&name, &index) in &dir.exports {
                if index >= 0 && index & IMPORT_MASK != 0 {
                    let import = dir.imports.get((index & !IMPORT_MASK) as usize).copied();
                    if let Some(target) = import.and_then(|n| library.exports.get(&n).copied()) {
                        library.exports.entry(name).or_insert(target);
                    }
                }
            }
        }
        library.material_blends = render::material_blends(index);
        for error in &library.errors {
            warn!("Swarm: {error}");
        }
        info!(
            "Swarm effects: {} directories, {} exported effects (group {:08x})",
            library.dirs.len(),
            library.exports.len(),
            group
        );
        library
    }

    pub fn find(&self, id: u32) -> Option<EffectRef> {
        self.exports.get(&id).copied()
    }

    /// ADAPTED stand-in: register a one-model effect drawing the installed
    /// model `id` (RW4 or `.prop`) under that id. Used when stage data names a
    /// model where an effect id is expected (several Cell rock structures do;
    /// retail would substitute the empty `cell_bad_effect`).
    pub fn model_stand_in(&mut self, id: u32) -> EffectRef {
        if let Some(found) = self.find(id) {
            return found;
        }
        if self.synthetic.is_none() {
            self.dirs.push(EffectDirectory::default());
            self.synthetic = Some(self.dirs.len() as u16 - 1);
        }
        let dir_index = self.synthetic.unwrap();
        let dir = &mut self.dirs[dir_index as usize];
        dir.models.push(format::ModelDef {
            resource: format::ResourceId { group: 0, instance: id },
            size: 1.0,
            color: [1.0; 3],
            alpha: 1.0,
            ..Default::default()
        });
        dir.effects.push(format::VisualEffectDef {
            flags: 0,
            app_flags_mask: 0,
            notify: 0,
            screen_size_range: [0.0; 2],
            cursor_active_distance: 0.0,
            cursor_button: 0,
            lod_distances: vec![],
            extended_lod_weights: [0.0; 3],
            seed: 0,
            blocks: vec![format::EffectBlock {
                kind: format::kind::MODEL,
                flags: 0,
                transform: format::BlockTransform {
                    flags: 0,
                    scale: 1.0,
                    rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                    offset: [0.0; 3],
                },
                lod_begin: 1,
                lod_end: 255,
                lod_scales: vec![],
                emit_scale: [1.0; 2],
                size_scale: [1.0; 2],
                alpha_scale: [1.0; 2],
                app_flags: 0,
                app_flags_mask: 0,
                selection_group: 0,
                selection_chance: 0,
                time_scale: 1.0,
                index: dir.models.len() as i32 - 1,
            }],
        });
        let found = EffectRef {
            dir: dir_index,
            index: dir.effects.len() as u32 - 1,
        };
        self.exports.insert(id, found);
        found
    }

    pub fn dir(&self, dir: u16) -> &EffectDirectory {
        &self.dirs[dir as usize]
    }

    /// Resolve a block index of kind EFFECT inside `dir` (local or imported).
    pub fn resolve_effect(&self, dir: u16, index: i32) -> Option<EffectRef> {
        if index < 0 {
            return None;
        }
        if index & IMPORT_MASK != 0 {
            let name = *self.dir(dir).imports.get((index & !IMPORT_MASK) as usize)?;
            return self.find(name);
        }
        ((index as usize) < self.dir(dir).effects.len()).then_some(EffectRef {
            dir,
            index: index as u32,
        })
    }
}

/// Bits of the app flags the effects test (`VisualEffectBlock.APP_FLAGS`).
pub mod app_flag {
    pub const CELL_GAME_MODE: u32 = 1 << 7;
    pub const CREATURE_GAME_MODE: u32 = 1 << 8;
    pub const TRIBE_GAME_MODE: u32 = 1 << 9;
    pub const CIV_GAME_MODE: u32 = 1 << 11;
    pub const SPACE_GAME_MODE: u32 = 1 << 12;
    pub const EDITOR_MODE: u32 = 1 << 16;
}

/// Current application flags blocks are filtered against.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SwarmAppFlags(pub u32);

/// Game-provided effect maps (retail `IEffectMap`): velocity and height per
/// map id, sampled at world positions (Bevy coordinates).
#[derive(Resource, Default)]
pub struct SwarmMaps {
    pub maps: HashMap<u32, SwarmMap>,
}

#[derive(Clone, Debug)]
pub enum SwarmMap {
    /// A uniform flow (world units per second) plus a height value.
    Uniform { velocity: Vec3, height: f32 },
    /// A tiling 2D vector field over the X/Z plane: `size` world units per
    /// tile, `cells` x `cells` samples (row-major, x fastest), times `scale`.
    Field {
        cells: usize,
        size: f32,
        origin: Vec2,
        vectors: std::sync::Arc<Vec<Vec2>>,
        scale: f32,
    },
    /// A map implemented by the game (retail `IEffectMap`): the Cell stage's
    /// water (`cell_fluid::FluidGrid`) answers with its velocity grid.
    Custom(std::sync::Arc<dyn EffectMapSampler>),
}

/// The velocity / height a game-implemented effect map reports at a world
/// position (Bevy coordinates).
pub trait EffectMapSampler: Send + Sync + std::fmt::Debug {
    fn velocity(&self, at: Vec3) -> Vec3;
    fn height(&self, at: Vec3) -> f32;
}

impl SwarmMaps {
    pub fn velocity(&self, id: u32, at: Vec3) -> Option<Vec3> {
        match self.maps.get(&id)? {
            SwarmMap::Uniform { velocity, .. } => Some(*velocity),
            SwarmMap::Custom(map) => Some(map.velocity(at)),
            SwarmMap::Field {
                cells,
                size,
                origin,
                vectors,
                scale,
            } => {
                if *cells == 0 || vectors.len() < cells * cells || *size <= 0.0 {
                    return None;
                }
                let n = *cells as f32;
                let p = (Vec2::new(at.x, at.z) - *origin) / *size * n;
                let (x0, y0) = (p.x.floor(), p.y.floor());
                let (fx, fy) = (p.x - x0, p.y - y0);
                let sample = |x: f32, y: f32| {
                    let xi = (x as i64).rem_euclid(*cells as i64) as usize;
                    let yi = (y as i64).rem_euclid(*cells as i64) as usize;
                    vectors[yi * cells + xi]
                };
                let v = sample(x0, y0).lerp(sample(x0 + 1.0, y0), fx).lerp(
                    sample(x0, y0 + 1.0).lerp(sample(x0 + 1.0, y0 + 1.0), fx),
                    fy,
                ) * *scale;
                Some(Vec3::new(v.x, 0.0, v.y))
            }
        }
    }
    pub fn height(&self, id: u32, at: Vec3) -> Option<f32> {
        match self.maps.get(&id)? {
            SwarmMap::Uniform { height, .. } => Some(*height),
            SwarmMap::Custom(map) => Some(map.height(at)),
            SwarmMap::Field { .. } => self.velocity(id, at).map(|v| v.length()),
        }
    }
}

/// Play an installed effect. `position` is the world position, or the local
/// offset (in the attached entity's frame) when `attach_to` is set.
#[derive(Message, Clone, Debug)]
pub struct SwarmPlay {
    pub effect: u32,
    pub position: Vec3,
    pub rotation: Quat,
    pub attach_to: Option<Entity>,
    pub scale: f32,
    /// Advance the effect by each emitter's preroll time before showing it
    /// (continuous ambience that should already be running).
    pub preroll: bool,
    /// Tag for later [`SwarmStop`] / queries; 0 = none.
    pub tag: u64,
}

impl SwarmPlay {
    pub fn at(effect: u32, position: Vec3) -> Self {
        Self {
            effect,
            position,
            rotation: Quat::IDENTITY,
            attach_to: None,
            scale: 1.0,
            preroll: false,
            tag: 0,
        }
    }
}

/// Stop effects (soft stop: emitters end, live particles finish).
#[derive(Message, Clone, Debug)]
pub struct SwarmStop {
    /// 0 matches any effect id.
    pub effect: u32,
    pub attach_to: Option<Entity>,
    /// 0 matches any tag.
    pub tag: u64,
    pub hard: bool,
}

/// A running effect. The instance tree is created on the first update.
#[derive(Component)]
pub struct SwarmEffect {
    pub effect: u32,
    pub attach_to: Option<Entity>,
    /// World position, or the offset in the attached entity's frame.
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
    /// Overall intensity (alpha multiplier); the Cell effect map drives it.
    pub alpha: f32,
    /// Emission multiplier.
    pub emit: f32,
    /// Colour multiplier of everything the effect draws (retail effect
    /// parameter 5, `kParamColor`: the Civ / Space spice geysers get the
    /// planet's spice colour, `cPlanet +0x1C4`, from the display strategy
    /// 0xE8FF80). White = no change.
    pub tint: Vec3,
    pub preroll: bool,
    pub stop: bool,
    pub hard_stop: bool,
    pub tag: u64,
    /// Effect parameters set by the game (retail `SetParameter`).
    pub params: [Option<Vec4>; 16],
    /// World-space points of the retail `kParamPath` parameter (the Cell electric
    /// beams set 21): static ribbons follow them.
    pub path: Vec<Vec3>,
    pub instance: Option<sim::EffectInstance>,
    /// Despawn the entity once the effect has finished (default true).
    pub despawn_when_done: bool,
    /// False when the id was not found or the instance ended.
    pub alive: bool,
    /// Render layer of everything this effect draws (0 = the main layer;
    /// stages may draw some worlds through other cameras).
    pub layer: usize,
}

impl SwarmEffect {
    pub fn new(effect: u32) -> Self {
        Self {
            effect,
            attach_to: None,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: 1.0,
            alpha: 1.0,
            emit: 1.0,
            tint: Vec3::ONE,
            preroll: false,
            stop: false,
            hard_stop: false,
            tag: 0,
            params: [None; 16],
            path: vec![],
            instance: None,
            despawn_when_done: true,
            alive: true,
            layer: 0,
        }
    }
}

/// Monotonic id source for particle systems and other render-side state.
#[derive(Resource, Default)]
pub struct SwarmIds(pub u64);

/// Effects whose id is not installed are reported once each.
#[derive(Resource, Default)]
pub struct SwarmDiagnostics {
    pub missing: std::collections::HashSet<u32>,
    pub started: u64,
}

/// Simulation side: library, messages, the effect update. Safe to add in
/// headless apps (no render resources needed).
pub struct SwarmFxPlugin;

impl Plugin for SwarmFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SwarmLibrary>()
            .init_resource::<SwarmMaps>()
            .init_resource::<SwarmAppFlags>()
            .init_resource::<SwarmIds>()
            .init_resource::<SwarmDiagnostics>()
            .init_resource::<shake::SwarmShake>()
            .init_resource::<sound::SwarmSounds>()
            .init_resource::<filters::SwarmOverrideColor>()
            .init_resource::<filters::SwarmBlendBackdrop>()
            .add_message::<SwarmPlay>()
            .add_message::<SwarmStop>()
            .add_systems(
                PostUpdate,
                (load_library, spawn_requests, stop_requests, sim::simulate)
                    .chain()
                    .after(bevy::transform::TransformSystems::Propagate)
                    .in_set(SwarmSet::Simulate),
            )
            .add_systems(First, shake::undo_shake)
            .add_systems(
                PostUpdate,
                shake::apply_shake.before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// Render side: material, textures, mesh building. Needs the render plugins.
pub struct SwarmFxRenderPlugin;

impl Plugin for SwarmFxRenderPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/swarm_particle.wesl");
        bevy::asset::embedded_asset!(app, "shaders/swarm_filter.wesl");
        bevy::asset::embedded_asset!(app, "shaders/swarm_volume.wesl");
        bevy::asset::embedded_asset!(app, "shaders/swarm_galactic.wesl");
        render::build(app);
        galactic_draw::build(app);
        volume::build(app);
        decal::build(app);
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SwarmSet {
    Simulate,
    Draw,
}

fn load_library(mut library: ResMut<SwarmLibrary>, originals: Option<Res<crate::Originals>>) {
    if library.loaded {
        return;
    }
    let Some(originals) = originals else { return };
    if originals.resource_index.records.is_empty() {
        return;
    }
    *library = SwarmLibrary::load(&originals.resource_index);
}

fn spawn_requests(mut commands: Commands, mut plays: MessageReader<SwarmPlay>) {
    for play in plays.read() {
        let mut effect = SwarmEffect::new(play.effect);
        effect.position = play.position;
        effect.rotation = play.rotation;
        effect.attach_to = play.attach_to;
        effect.scale = play.scale;
        effect.preroll = play.preroll;
        effect.tag = play.tag;
        commands.spawn((effect, crate::stages::StageOwned));
    }
}

fn stop_requests(mut stops: MessageReader<SwarmStop>, mut effects: Query<&mut SwarmEffect>) {
    for stop in stops.read() {
        for mut effect in &mut effects {
            if (stop.effect == 0 || stop.effect == effect.effect)
                && (stop.tag == 0 || stop.tag == effect.tag)
                && (stop.attach_to.is_none() || stop.attach_to == effect.attach_to)
            {
                effect.stop = true;
                effect.hard_stop |= stop.hard;
            }
        }
    }
}
