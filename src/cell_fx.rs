//! Cell-stage effects: plays installed Swarm effects for [`CellEffectRequest`]
//! messages and drives the Cell environment from the original effect map
//! (`CellEffects`) and background map (`CellBackgrounds`).
//!
//! NATIVE (installed data + `notes/cell-re-notes.md` sections 9, 4A, 1E):
//! the effect map entries and their four fade keys, the fade function
//! (0xE50200), entry types and which ones follow the camera look point, the
//! world-scale key (`cCellGame+0x514C`: 0.5, 1.5, 5, 15 ... = 0.5 * level
//! size of the scale-info table), `SetParameter(4, fade)` on every map effect,
//! the background colour = `CellBackgrounds` evaluated at the world scale and
//! sent to `cell_background_color` as parameter 5.
//! The effect-map transform scale (`cCellGFX+0x4C`) is `1 / (2 w)` = 1 / level
//! size (set in 0xE7FD00, rescaled with the world in 0xE6DB70); `FX_VIEW_UNIT`
//! converts retail world units to Bevy units (the engine's `UNIT`).
//! The avatar's `cell_dof_distort` / `cell_dof_distort_blind` screen filter
//! chain (0xE750C0) runs through the Swarm filter machinery
//! (`swarm_fx/filters.rs`); `cell_propulsion*` effects follow every cell
//! (0xE639D0) and the electric beams are ribbons along a 21-point path
//! (0xE6B9B0).
//! The background colour is interpolated between the two keys around the
//! world scale (0xE54D30 via 0xE55080, VERIFIED, [`background_color`]).
//! TileBackground entries (type 5, 0xE5E2C0) put their model effect
//! (`cell_ground_L1`: the ice floor, scale `CG_ground_L1`) on every loaded
//! background tile of their scale, the tile width as effect scale and the fade
//! as transparency ([`tile_backgrounds`]).
//! Effect worlds (GFX_Initialize 0xE5DBA0, VERIFIED): skybox 0x1010000 ->
//! renderer 2, background 0x1010011 (+ model world 0x1010010) -> renderer 1,
//! foreground 0x1010021 (+ 0x1010020) -> renderer 0, 0x1010031 -> renderer 4.
//! The two render callbacks 0xE54AB0 (skybox renderer) and
//! 0xE54B80 (background models, animation world and background renderer)
//! draw with the SECOND viewer (0x16B42B8, 0xE549B0), i.e. everything in the
//! skybox and background worlds (TracerBackground map effects, tile effects,
//! background cells and their effects) is seen from the unzoomed
//! `backgroundDistance` viewer. Here the effect-map TracerBackground entries
//! sit on the same plane as the tiles ([`background_plane_y`]).
//! ADAPTED: the second viewer is not a second transform; the background world
//! draws on [`BACKGROUND_LAYER`] through a second camera that shares the
//! main camera's transform, the viewer offset is reproduced by pushing the
//! background objects down by the height the camera came down, and the DoF
//! chain acts on that pass (retail's own pass structure is not ported, see
//! filters.rs); the water current map 0x01010003 is the engine's SPH water
//! (`cell_fluid`, retail's own effect map) and falls back to a uniform drift
//! while no stage runs.

use std::collections::HashSet;

use bevy::{
    camera::{visibility::RenderLayers, CameraOutputMode, ClearColorConfig, RenderTarget},
    core_pipeline::tonemapping::Tonemapping,
    post_process::dof::{DepthOfField, DepthOfFieldMode},
    prelude::*,
};

use crate::cell_contract::{CellEffectRequest, CellWorldState};
use crate::swarm_fx::{app_flag, SwarmAppFlags, SwarmEffect, SwarmLibrary, SwarmMap, SwarmMaps};
use crate::{
    stages::{GameStage, StageOwned},
    Originals,
};

const CELL_GLOBALS: u32 = 0x2A3C_E5B7;
const CELL_EFFECT_MAP: u32 = 0x433F_B70C;
const CELL_BACKGROUND_MAP: u32 = 0x612B_3191;
/// `cell_background_color` (screen effect, background mode).
pub const BACKGROUND_COLOR_EFFECT: u32 = 0x05C7_A4D0;
/// The water-current effect map the tracers advect with.
pub const FLOW_MAP: u32 = 0x0101_0003;
/// Tag base for effect-map instances (tag = base + entry index).
const MAP_TAG: u64 = 0xCE11_0000;
const BACKGROUND_TAG: u64 = 0xCE11_FFFF;
/// Bevy units per retail world unit for effect-map effects: the engine's
/// world -> Bevy factor, now that the camera frames the retail view
/// (`cell::CAMERA_HEIGHT_BASE`). Was 24 / 20 = 1.2 while the camera was too
/// low (the effects were kept retail-sized relative to the view, the cells
/// were not).
pub const FX_VIEW_UNIT: f32 = crate::cell_engine::UNIT;
/// Mean of the `cell_background_l0_screen` texture the water colour is
/// multiplied by (measured on the installed raster: 0.884; range 0.71-1).
const BACKGROUND_TEXTURE_MEAN: f32 = 0.884;
/// Uniform current used while no Cell water exists (no stage running). ADAPTED.
const FALLBACK_FLOW: f32 = 0.6;
/// `Height` the water map reports at least: none. Retail's `Height` is the
/// four-cell speed sum (`FluidGrid`) and the tracers' `heightRange` 3.5..10000
/// emits only where it is large. The undisturbed current is slow (level 1: map
/// |v| <= 0.52 x strength 1.5, a sum stays below 3), the sums that pass the gate
/// come from moving cells whose outline pins stir the water (`cell_softbody`:
/// measured, 0..19 % of the area passes, 1-8 % while the avatar swims among
/// cells). Until the soft bodies existed this was floored at the old constant 10
/// (ADAPTED); `OPENSPORE_WATER_HEIGHT_FLOOR=10` brings the old tracer density back.
const WATER_HEIGHT_FLOOR: f32 = 0.0;

/// `WATER_HEIGHT_FLOOR`, or `OPENSPORE_WATER_HEIGHT_FLOOR` (diagnostics, read once).
fn height_floor() -> f32 {
    static FLOOR: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *FLOOR.get_or_init(|| {
        std::env::var("OPENSPORE_WATER_HEIGHT_FLOOR")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(WATER_HEIGHT_FLOOR)
    })
}

/// The water as Swarm effect map 0x01010003: retail's `FlowAt` velocity and
/// the four-cell speed sum as `Height` (see [`WATER_HEIGHT_FLOOR`]).
#[derive(Debug)]
struct WaterMap(crate::cell_fluid::FluidGrid);

impl crate::swarm_fx::EffectMapSampler for WaterMap {
    fn velocity(&self, at: Vec3) -> Vec3 {
        crate::swarm_fx::EffectMapSampler::velocity(&self.0, at)
    }
    fn height(&self, at: Vec3) -> f32 {
        crate::swarm_fx::EffectMapSampler::height(&self.0, at).max(height_floor())
    }
}

/// `cCellScaleInfo` (RE 4A, .rdata 0x1483B70): (+0x0C size, +0x10 level size)
/// for entries 0..=20; entry i covers food >= 50 * i.
const SCALE_INFO: [(f32, f32); 21] = [
    (1.0, 1.0),
    (1.73, 1.0),
    (3.0, 3.0),
    (5.2, 3.0),
    (10.0, 10.0),
    (17.3, 10.0),
    (30.0, 30.0),
    (52.0, 30.0),
    (100.0, 100.0),
    (173.0, 100.0),
    (300.0, 300.0),
    (520.0, 300.0),
    (1000.0, 1000.0),
    (1730.0, 1000.0),
    (3000.0, 3000.0),
    (5200.0, 3000.0),
    (10000.0, 10000.0),
    (17300.0, 10000.0),
    (30000.0, 30000.0),
    (52000.0, 30000.0),
    (52000.0, 30000.0),
];

fn scale_info(food: i32) -> (f32, f32) {
    let i = if food >= 1000 {
        20
    } else {
        (food.max(0) / 50).min(19) as usize
    };
    SCALE_INFO[i]
}

/// The retail world scale the effect and background maps are keyed by:
/// `cCellGame+0x514C`, smoothed toward `worldScaleFor(food)` = 0.5 * the level
/// size of the scale-info table (0xE4EE80: 0.5, 1.5, 5, 15 ... 15000). The
/// engine publishes exactly that value in `CellWorldState::world_scale`
/// (`cell_engine::world_scale_for`); before an engine runs it is derived from
/// the food progression. (An earlier version rescaled the engine value as if
/// it followed the entry size, which put every level at about a third of its
/// key: pale water, the wrong tracer sets and three times too large effects.)
pub fn fx_world_scale(state: &CellWorldState) -> f32 {
    if state.world_scale > 0.0 {
        state.world_scale
    } else {
        0.5 * scale_info(state.food_progression).1
    }
}

/// Retail fade (0xE50200) of an effect-map entry at world scale `w`.
pub fn map_fade(w: f32, [a, b, c, d]: [f32; 4]) -> f32 {
    let ramp = |x: f32, lo: f32, hi: f32| {
        if hi > lo {
            ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
        } else if x >= hi {
            1.0
        } else {
            0.0
        }
    };
    match (a == -1.0, d == -1.0) {
        (true, true) => 1.0,
        (true, false) => 1.0 - ramp(w, c, d),
        (false, true) => ramp(w, a, b),
        (false, false) => {
            if w < a || w >= d {
                0.0
            } else if w < b {
                ramp(w, a, b)
            } else if w <= c {
                1.0
            } else {
                1.0 - ramp(w, c, d)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectMapEntry {
    pub effect: u32,
    /// 0 Foreground, 1 Background, 2 Tracer, 3 TracerBackground, 4 Skybox,
    /// 5 TileBackground.
    pub kind: u32,
    pub fade: [f32; 4],
    /// TileBackground: CellStageScale of the tiles that carry it.
    pub tile_scale: i32,
}

/// The decoded Cell environment maps.
#[derive(Resource, Clone, Debug, Default)]
pub struct CellFxMaps {
    pub effect_map: Vec<EffectMapEntry>,
    /// (colour, world-scale key), keys ascending.
    pub backgrounds: Vec<([f32; 3], f32)>,
    pub loaded: bool,
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn f32le(b: &[u8], at: usize) -> Option<f32> {
    u32le(b, at).map(f32::from_bits)
}

/// `cCellEffectMapResource`: count, offset 8, 0x1C-byte entries.
pub fn parse_effect_map(b: &[u8]) -> Option<Vec<EffectMapEntry>> {
    let count = u32le(b, 0)? as usize;
    let offset = u32le(b, 4)? as usize;
    if count > 256 || (count > 0 && offset != 8) || b.len() < offset + count * 28 {
        return None;
    }
    (0..count)
        .map(|i| {
            let at = offset + i * 28;
            Some(EffectMapEntry {
                effect: u32le(b, at)?,
                kind: u32le(b, at + 4)?,
                fade: [f32le(b, at + 8)?, f32le(b, at + 12)?, f32le(b, at + 16)?, f32le(b, at + 20)?],
                tile_scale: u32le(b, at + 24)? as i32,
            })
        })
        .collect()
}

/// `cCellBackgroundMapResource`: count, offset 8, (rgb, key) entries.
pub fn parse_background_map(b: &[u8]) -> Option<Vec<([f32; 3], f32)>> {
    let count = u32le(b, 0)? as usize;
    let offset = u32le(b, 4)? as usize;
    if count > 256 || (count > 0 && offset != 8) || b.len() < offset + count * 16 {
        return None;
    }
    let mut out: Vec<([f32; 3], f32)> = (0..count)
        .map(|i| {
            let at = offset + i * 16;
            Some(([f32le(b, at)?, f32le(b, at + 4)?, f32le(b, at + 8)?], f32le(b, at + 12)?))
        })
        .collect::<Option<_>>()?;
    out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    Some(out)
}

/// Background colour at world scale `w` (VERIFIED, 0xE54D30 called by 0xE55080
/// with `cCellGame+0x514C`): every key is scanned (the order does not matter),
/// the colour with the largest key below `w` and the one with the smallest key
/// from `w` on bracket it, and the result is `lo + (hi - lo) * t` with
/// `t = (w - lo.key) / (hi.key - lo.key + 2^-16)` (0x1041CE0, not clamped).
/// Without both neighbours (`w` at or below the lowest key, or above the
/// highest) the routine returns magenta (1, 0, 1), the constants at
/// 0x15A7BEC..0x15A7BF4; the installed map spans 0 .. 100000, the stage keys
/// 0.5 .. 15000, so that never shows. `None` only for an empty map.
pub fn background_color(map: &[([f32; 3], f32)], w: f32) -> Option<Vec3> {
    if map.is_empty() {
        return None;
    }
    let mut lo: Option<&([f32; 3], f32)> = None;
    let mut hi: Option<&([f32; 3], f32)> = None;
    for e in map {
        if e.1 < w && lo.is_none_or(|l| e.1 > l.1) {
            lo = Some(e);
        }
        if e.1 >= w && hi.is_none_or(|h| e.1 < h.1) {
            hi = Some(e);
        }
    }
    let (Some(lo), Some(hi)) = (lo, hi) else {
        return Some(Vec3::new(1.0, 0.0, 1.0));
    };
    let t = (w - lo.1) / (hi.1 - lo.1 + 1.525_878_9e-5);
    Some(Vec3::from_array(lo.0).lerp(Vec3::from_array(hi.0), t))
}

/// Height (Bevy y) of the plane the background effect world's z = 0 maps to:
/// the tile effects (0xE5E2C0: centre (x, y, 0), identity rotation, the global
/// matrix 0x16B3DAC is the identity written by the static initialiser
/// 0x137EB40) and the TracerBackground / Background / Skybox map effects
/// (0xE63560: origin or look point, z = 0) are all drawn by the background
/// viewer `backgroundDistance` above that plane. It is the depth of an
/// elevation-0 background cell (`depth = backgroundDistance - elevation`).
pub fn background_plane_y(background_distance: f32, zoom_factor: f32) -> f32 {
    -crate::cell_engine::background_draw_depth(
        background_distance * crate::cell_engine::UNIT * crate::cell_population::BG_DEPTH_SCALE,
        zoom_factor,
    )
}

fn load_maps(originals: &Originals) -> CellFxMaps {
    let mut out = CellFxMaps {
        loaded: true,
        ..default()
    };
    let index = &originals.resource_index;
    let Some(globals) = index
        .records
        .keys()
        .copied()
        .filter(|k| k.type_id == CELL_GLOBALS)
        .min_by_key(|k| (k.group_id, k.instance_id))
    else {
        return out;
    };
    let Ok(bytes) = index.read(globals) else { return out };
    let read = |type_id: u32, instance_id: u32| {
        [globals.group_id, 0].into_iter().find_map(|group_id| {
            index
                .read(crate::dbpf::ResourceKey {
                    type_id,
                    group_id,
                    instance_id,
                })
                .ok()
        })
    };
    if let Some(map) = u32le(&bytes, 0x3C).and_then(|id| read(CELL_EFFECT_MAP, id)) {
        out.effect_map = parse_effect_map(&map).unwrap_or_default();
    }
    if let Some(map) = u32le(&bytes, 0x40).and_then(|id| read(CELL_BACKGROUND_MAP, id)) {
        out.backgrounds = parse_background_map(&map).unwrap_or_default();
    }
    info!(
        "Cell effect map: {} entries, background map: {} colours",
        out.effect_map.len(),
        out.backgrounds.len()
    );
    out
}

/// Debug/capture override of the world scale (`--fx-world-scale`).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CellFxOverride {
    pub world_scale: Option<f32>,
}

/// Marks the per-stage environment effects spawned from the effect map.
#[derive(Component)]
pub struct CellMapEffect {
    pub entry: usize,
}

pub struct CellFxPlugin;

impl Plugin for CellFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CellEffectRequest>()
            .add_message::<CellBeamRequest>()
            .init_resource::<CellWorldState>()
            .init_resource::<CellFxMaps>()
            .init_resource::<CellFxOverride>()
            .init_resource::<CellAnimFx>()
            .init_resource::<CellBackgroundBlur>()
            .add_systems(
                PostUpdate,
                (
                    app_flags,
                    play_requests,
                    play_beams,
                    propulsion_effects,
                    (
                        fallback_world_state,
                        environment,
                        tile_backgrounds,
                        dof_chain,
                        retire_fake_layers,
                        ldr_camera,
                        hide_fallbacks,
                        anim_event_effects,
                        mating_call_rings,
                        charged_glow,
                        background_layers,
                        background_lights,
                        background_layer_audit,
                    )
                        .chain()
                        .run_if(in_state(GameStage::Cell)),
                )
                    .chain()
                    .before(crate::swarm_fx::SwarmSet::Simulate)
                    .after(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                background_camera
                    .run_if(in_state(GameStage::Cell))
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta),
            )
            .add_systems(OnExit(GameStage::Cell), restore_game_camera);
    }
}

fn app_flags(
    state: Option<Res<State<GameStage>>>,
    space_view: Option<Res<crate::space_view::SpaceView>>,
    mut flags: ResMut<SwarmAppFlags>,
) {
    let mode = match state.as_deref().map(State::get) {
        Some(GameStage::Cell) => app_flag::CELL_GAME_MODE,
        Some(GameStage::Creature) => app_flag::CREATURE_GAME_MODE,
        Some(GameStage::Tribal) => app_flag::TRIBE_GAME_MODE,
        Some(GameStage::Civilization) => app_flag::CIV_GAME_MODE,
        // The galaxy map also sets kAppFlagSpaceGameGalaxyMode (bit 18; the
        // gate of the star hover effect 0x28115AB8, `space_galaxy::hover`).
        Some(GameStage::Space) => {
            app_flag::SPACE_GAME_MODE
                | space_view.as_deref().map_or(0, |v| if v.galaxy { crate::space_galaxy::hover::APP_FLAG_GALAXY_MODE } else { 0 })
        }
        _ => 0,
    };
    if flags.0 != mode {
        flags.0 = mode;
    }
}

/// Every [`CellEffectRequest`] becomes a running Swarm effect. The request
/// scale multiplies the attached entity's own transform scale.
fn play_requests(
    mut commands: Commands,
    mut requests: MessageReader<CellEffectRequest>,
    mut library: ResMut<SwarmLibrary>,
    originals: Option<Res<Originals>>,
    mut running: Query<&mut SwarmEffect>,
) {
    for request in requests.read() {
        // The avatar's decoration effects replace each other (retail keeps one
        // instance, RE 1E / 9): a new level or eye count stops the old one.
        if let Some(target) = request.attach_to {
            let hero = |id: u32| HERO_EFFECTS.iter().any(|n| crate::world_objects::name_id(n) == id);
            if hero(request.effect) {
                for mut other in &mut running {
                    if other.attach_to == Some(target) && other.effect != request.effect && hero(other.effect) {
                        other.stop = true;
                    }
                }
            }
        }
        if library.loaded && library.find(request.effect).is_none() {
            // ADAPTED: a structure naming an installed model instead of an
            // effect draws that model (see SwarmLibrary::model_stand_in).
            // Retail CreateEffect (0x628480, VERIFIED) answers an unknown id by
            // creating `cell_bad_effect` (0xE18D6423), an installed effect with
            // no blocks: nothing is drawn by that path. The model is shown so
            // the rock structures that name models are not invisible.
            let installed = originals.as_ref().is_some_and(|o| {
                [crate::asset_catalog::RW4, 0x00B1_B104].into_iter().any(|type_id| {
                    o.resource_index.records.contains_key(&crate::dbpf::ResourceKey {
                        type_id,
                        group_id: 0,
                        instance_id: request.effect,
                    })
                })
            });
            if installed {
                library.model_stand_in(request.effect);
            }
        }
        let mut effect = SwarmEffect::new(request.effect);
        effect.position = request.position;
        effect.attach_to = request.attach_to;
        effect.scale = if request.scale > 0.0 { request.scale } else { 1.0 };
        commands.spawn((effect, StageOwned));
    }
}

/// Fill the presentation fields when no Cell engine publishes them (the old
/// OpenSpore Cell loop): camera height and player position from the scene.
fn fallback_world_state(
    mut state: ResMut<CellWorldState>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    player: Query<&GlobalTransform, With<crate::game::Player>>,
    overrides: Res<CellFxOverride>,
) {
    if state.world_scale > 0.0 && state.camera_height > 0.0 && overrides.world_scale.is_none() {
        return; // an engine owns the state
    }
    let player = player.iter().next().map(|g| g.translation());
    if let Some(p) = player {
        state.player = Vec2::new(p.x, p.z);
    }
    if let Some(camera) = camera.iter().next() {
        state.camera_height = camera.translation().y - player.map_or(0.0, |p| p.y);
    }
    if let Some(w) = overrides.world_scale {
        // Preview a given retail world scale: pick the food of that level.
        let level = (2.0 * w).max(1.0);
        let i = SCALE_INFO.iter().position(|(_, l)| *l >= level - 1e-3).unwrap_or(20);
        state.food_progression = (i as i32 * 50).min(1000);
        state.world_scale = w;
    } else if state.world_scale <= 0.0 {
        state.world_scale = 1.0;
    }
}

/// `OS_CELL_FX_ONLY=3,5,16` (debug): run only these effect-map entries.
fn debug_entry_allowed(index: usize) -> bool {
    static ONLY: std::sync::OnceLock<Option<Vec<usize>>> = std::sync::OnceLock::new();
    ONLY.get_or_init(|| {
        std::env::var("OS_CELL_FX_ONLY")
            .ok()
            .map(|v| v.split(',').filter_map(|s| s.trim().parse().ok()).collect())
    })
    .as_ref()
    .is_none_or(|only| only.contains(&index))
}

/// Keep the effect-map effects and the background in step with the world
/// scale (retail `LoadEffectMap`, run every frame from the GFX post-step).
#[allow(clippy::too_many_arguments)]
fn environment(
    mut commands: Commands,
    library: Res<SwarmLibrary>,
    originals: Option<Res<Originals>>,
    mut maps: ResMut<CellFxMaps>,
    mut swarm_maps: ResMut<SwarmMaps>,
    state: Res<CellWorldState>,
    blur: Res<CellBackgroundBlur>,
    editor: Option<Res<crate::editor::EditorState>>,
    game: Option<Res<crate::cell_engine::CellGame>>,
    mut clear: ResMut<ClearColor>,
    mut override_color: ResMut<crate::swarm_fx::filters::SwarmOverrideColor>,
    mut backdrop: ResMut<crate::swarm_fx::filters::SwarmBlendBackdrop>,
    db: Option<Res<crate::cell_data::CellDatabase>>,
    mut effects: Query<(Entity, &mut SwarmEffect, Option<&CellMapEffect>)>,
) {
    if !library.loaded {
        return;
    }
    if !maps.loaded {
        let Some(originals) = originals else { return };
        if originals.resource_index.records.is_empty() {
            return;
        }
        *maps = load_maps(&originals);
    }
    let w = fx_world_scale(&state).max(1e-6);
    // cCellGFX+0x4C = 1 / (2 w) (RE: set in 0xE7FD00 as 1 / (field_514C * 2),
    // rescaled with the world in 0xE6DB70), i.e. 1 / level size; the retail
    // effects are authored for it. FX_VIEW_UNIT converts retail units to ours.
    let scale = FX_VIEW_UNIT / (2.0 * w) * crate::world_presentation::tune("CELL_FX_SCALE", 1.0);
    let look = Vec3::new(state.player.x, 0.0, state.player.y);
    // The background / skybox effect worlds are drawn by the background viewer
    // (0xE54AB0 / 0xE54B80): their z = 0 plane is the elevation-0 plane of the
    // background cells and tiles. Without the engine (old Cell loop) there is
    // no background pass geometry: the plane stays at the play plane.
    let background_y = game.as_ref().map_or(0.0, |game| {
        let base = db.as_ref().and_then(|db| db.globals.as_ref()).map_or(35.0, |g| g.background_distance);
        background_plane_y(base, game.view_factor())
    });
    // Water currents for advected particles: retail registers the water as the
    // effect map 0x01010003 (vtable 0x14852B8: Velocity = FlowAt, Height = the
    // four-cell speed sum), so the tracers follow the SPH fluid (the old height
    // floor is gone, see `WATER_HEIGHT_FLOOR`). Until the engine's water
    // exists (no stage running) the old uniform drift stays, scaled with the
    // view so it reads the same at every level (ADAPTED).
    if let Some(game) = game.as_deref().filter(|g| g.fluid.ready()) {
        swarm_maps
            .maps
            .insert(FLOW_MAP, SwarmMap::Custom(std::sync::Arc::new(WaterMap(game.fluid.grid().clone()))));
    } else {
        let angle = state.time * 0.05;
        swarm_maps.maps.insert(
            FLOW_MAP,
            SwarmMap::Uniform {
                velocity: Vec3::new(angle.cos(), 0.0, angle.sin()) * FALLBACK_FLOW * (1.0 + state.flow_strength.max(0.0) * 0.1),
                height: 10.0,
            },
        );
    }
    let mut existing: std::collections::HashMap<usize, Entity> = Default::default();
    let mut background = None;
    for (entity, effect, map) in &effects {
        if let Some(map) = map {
            if effect.alive && !effect.stop {
                existing.insert(map.entry, entity);
            }
        } else if effect.tag == BACKGROUND_TAG && !effect.stop {
            background = Some(entity);
        }
    }
    for (i, entry) in maps.effect_map.iter().enumerate() {
        if entry.kind == 5 {
            continue; // TileBackground: placed on engine background tiles
        }
        let fade = if debug_entry_allowed(i) { map_fade(w, entry.fade) } else { 0.0 };
        // Background 1 / TracerBackground 3 (background effect world) and
        // Skybox 4 draw in the background pass.
        let layer = blur.layer(matches!(entry.kind, 1 | 3 | 4));
        // Tracer (2) / TracerBackground (3) follow the look point (0xE63560),
        // the others sit at the origin; the background-world kinds lie on the
        // background plane.
        let position = Vec3::new(
            if matches!(entry.kind, 2 | 3) { look.x } else { 0.0 },
            if matches!(entry.kind, 1 | 3 | 4) { background_y } else { 0.0 },
            if matches!(entry.kind, 2 | 3) { look.z } else { 0.0 },
        );
        match (fade > 0.0, existing.get(&i).copied()) {
            (true, Some(entity)) => {
                if let Ok((_, mut effect, _)) = effects.get_mut(entity) {
                    effect.params[crate::swarm_fx::sim::PARAM_INTENSITY] = Some(Vec4::splat(fade));
                    effect.scale = scale;
                    effect.position = position;
                    effect.layer = layer;
                }
            }
            (true, None) => {
                if library.find(entry.effect).is_none() {
                    continue;
                }
                let mut effect = SwarmEffect::new(entry.effect);
                effect.preroll = true;
                effect.scale = scale;
                effect.tag = MAP_TAG + i as u64;
                effect.position = position;
                effect.params[crate::swarm_fx::sim::PARAM_INTENSITY] = Some(Vec4::splat(fade));
                effect.layer = layer;
                commands.spawn((effect, CellMapEffect { entry: i }, StageOwned));
            }
            (false, Some(entity)) => {
                if let Ok((_, mut effect, _)) = effects.get_mut(entity) {
                    effect.stop = true;
                }
            }
            (false, None) => {}
        }
    }
    if let Some(color) = background_color(&maps.backgrounds, w) {
        // 0xE55080 also hands the colour to the effects renderer, where the
        // filter chains' `overrideColor` reads it (see SwarmOverrideColor).
        if override_color.0 != Some(color) {
            override_color.0 = Some(color);
        }
        // Alpha-blended sprites are blended as Spore does (gamma space) over
        // the water as it shows: the colour times the mean of the screen
        // texture (`cell_background_l0_screen`, 0.884, measured on the
        // installed raster). ADAPTED: the gamma-space blend fit itself
        // (`swarm_fx/render.rs`), a presentation stand-in for the retail
        // UNORM blending; the colour and texture are data.
        if crate::world_presentation::tune("CELL_BLEND_FIT", 1.0) > 0.5 {
            let water = color * BACKGROUND_TEXTURE_MEAN;
            if backdrop.0 != Some(water) {
                backdrop.0 = Some(water);
            }
        }
        let param = Vec4::new(color.x, color.y, color.z, 1.0);
        match background {
            Some(entity) => {
                if let Ok((_, mut effect, _)) = effects.get_mut(entity) {
                    effect.params[crate::swarm_fx::sim::PARAM_COLOR] = Some(param);
                }
            }
            None if library.find(BACKGROUND_COLOR_EFFECT).is_some() => {
                let mut effect = SwarmEffect::new(BACKGROUND_COLOR_EFFECT);
                effect.tag = BACKGROUND_TAG;
                effect.params[crate::swarm_fx::sim::PARAM_COLOR] = Some(param);
                effect.layer = blur.layer(true);
                commands.spawn((effect, StageOwned));
            }
            None => {}
        }
        // The Cell creator sets its own backdrop colour while open.
        if !editor.is_some_and(|e| e.open) {
            clear.0 = Color::srgb(color.x, color.y, color.z);
        }
    }
}

/// A TileBackground effect on one background tile.
#[derive(Component)]
pub struct CellTileEffect {
    pub entry: usize,
    pub key: crate::cell_population::TileKey,
}

/// TileBackground entries (effect-map type 5, `LoadEffectMap` -> 0xE5E2C0):
/// every background-layer tile whose scale is the entry's `int18` carries the
/// entry's effect while the entry's fade is above zero. The effect is created
/// in the background effect world with a source transform at the tile centre,
/// the tile width as scale (the ground models span one unit) and
/// `kParamTransparency` = the fade. VERIFIED (0xE5E2C0, read in full): which
/// tiles (layer 1, scale == `int18`), fade, centre (x, y, 0) =
/// (tile + 0.5) * `TileSize(s) = realSize[s] / w * TileFactor * 0.85`, scale =
/// that width, rotation = the identity matrix (global 0x16B3DAC, written by the
/// static initialiser 0x137EB40), effect world 0x1010011 (`sCellGFX+0x161BC`).
/// The z = 0 plane is the elevation-0 plane of the background cells
/// ([`background_plane_y`]); tiles and sizes go through the engine's world ->
/// Bevy conversion (`UNIT`), a unit change, not a rule.
#[allow(clippy::too_many_arguments)]
fn tile_backgrounds(
    mut commands: Commands,
    library: Res<SwarmLibrary>,
    maps: Res<CellFxMaps>,
    state: Res<CellWorldState>,
    blur: Res<CellBackgroundBlur>,
    tiles: Option<Res<crate::cell_population::CellTiles>>,
    game: Option<Res<crate::cell_engine::CellGame>>,
    db: Option<Res<crate::cell_data::CellDatabase>>,
    mut effects: Query<(Entity, &mut SwarmEffect, &CellTileEffect)>,
) {
    let (Some(tiles), Some(game)) = (tiles, game) else { return };
    if !library.loaded || !maps.loaded {
        return;
    }
    let w = fx_world_scale(&state).max(1e-6);
    let p = state.scale;
    let base = db.as_ref().and_then(|db| db.globals.as_ref()).map_or(35.0, |g| g.background_distance);
    // The background effect world is drawn by the unzoomed background viewer
    // (0xE549B0): the zoom-1 depth plus the height the camera came down.
    let y = background_plane_y(base, game.view_factor());
    let width_per_abs = crate::cell_engine::UNIT / game.world_scale.max(1e-6);
    let mut live: std::collections::HashMap<(usize, crate::cell_population::TileKey), Entity> = Default::default();
    for (entity, mut effect, tile) in &mut effects {
        let wanted = maps.effect_map.get(tile.entry).is_some_and(|e| {
            e.kind == 5 && map_fade(w, e.fade) > 0.0 && tiles.loaded.contains_key(&tile.key)
        });
        if !wanted || effect.stop {
            effect.stop = true;
            commands.entity(entity).despawn();
            continue;
        }
        let entry = &maps.effect_map[tile.entry];
        let ts = crate::cell_population::tile_size_abs(tile.key.scale, p);
        let centre = game.to_world(Vec2::new(tile.key.x as f32 + 0.5, tile.key.y as f32 + 0.5) * ts);
        effect.position = Vec3::new(centre.x, y, centre.y);
        effect.scale = ts * width_per_abs;
        effect.params[crate::swarm_fx::sim::PARAM_INTENSITY] = Some(Vec4::splat(map_fade(w, entry.fade)));
        effect.layer = blur.layer(true);
        live.insert((tile.entry, tile.key), entity);
    }
    for (i, entry) in maps.effect_map.iter().enumerate() {
        if entry.kind != 5 || !debug_entry_allowed(i) {
            continue;
        }
        let fade = map_fade(w, entry.fade);
        if fade <= 0.0 || library.find(entry.effect).is_none() {
            continue;
        }
        for key in tiles.loaded.keys().filter(|k| k.layer == 1 && k.scale == entry.tile_scale) {
            if live.contains_key(&(i, *key)) {
                continue;
            }
            let ts = crate::cell_population::tile_size_abs(key.scale, p);
            let centre = game.to_world(Vec2::new(key.x as f32 + 0.5, key.y as f32 + 0.5) * ts);
            let mut effect = SwarmEffect::new(entry.effect);
            effect.position = Vec3::new(centre.x, y, centre.y);
            effect.scale = ts * width_per_abs;
            effect.tag = MAP_TAG + 0x8000 + i as u64;
            effect.params[crate::swarm_fx::sim::PARAM_INTENSITY] = Some(Vec4::splat(fade));
            effect.layer = blur.layer(true);
            commands.spawn((effect, CellTileEffect { entry: i, key: *key }, StageOwned));
        }
    }
}

/// `cell_propulsionJet / Cilia / Flagella` per cell (retail `FUN_00e639d0`,
/// called from the per-cell GFX loop 0xE73F60 for every cell that has a model
/// (`+0xFC != 0`), each GFX update, RE notes section 9). VERIFIED: the effect
/// ids and slots (gfx record +0xA0 / +0xA4 / +0xA8); there is no distance or
/// view test: the cells that exist are the ones the population keeps loaded
/// (the 2 x 2 visible boxes, `cell_population`), so every one of them has its
/// wake. A slot's effect is created once the strength is >= 0
/// ([`crate::cell_motion::CellMotion::propulsion`]) and then lives with the
/// cell (a strength < 0 afterwards only sets the emit rate to 0, the effect is
/// not stopped); `kParamEmitScale` (3) = clamp(10 * strength, 0, 99) and
/// `kParamTransparency` (4) = 1, or 0.25 for cells within one size level of the
/// avatar's (`GetScaleDifferenceWithPlayer <= 1`); the effect follows the cell's
/// transform; background cells use the background world.
/// ADAPTED: the strength is divided by the frame time and multiplied by 1/30
/// so the wake does not depend on the frame rate (the retail value is the
/// per-step velocity change of its simulation steps; OpenSpore steps with the
/// frame time).
const PROPULSION_EFFECTS: [&str; 3] = ["cell_propulsionJet", "cell_propulsionCilia", "cell_propulsionFlagella"];

#[derive(Component, Default)]
pub struct CellPropulsionFx {
    effects: [Option<Entity>; 3],
}

/// `kParamEmitScale` of a wake (0xE639D0: `clamp(10 * strength, 0, 99)`, 0 for
/// a negative strength) with the frame-rate normalisation of the adaptation.
pub fn wake_emit(strength: f32, dt: f32) -> f32 {
    (10.0 * strength.max(0.0) / dt.max(1.0e-3) / 30.0).clamp(0.0, 99.0)
}

#[allow(clippy::type_complexity)]
fn propulsion_effects(
    mut commands: Commands,
    time: Res<Time>,
    library: Res<SwarmLibrary>,
    blur: Res<CellBackgroundBlur>,
    run: Option<Res<crate::game::RunProgress>>,
    cells: Query<(
        Entity,
        &crate::cell_motion::CellMotion,
        &crate::cell_engine::CellObject,
        Option<&CellPropulsionFx>,
    )>,
    mut effects: Query<&mut SwarmEffect>,
) {
    if !library.loaded {
        return;
    }
    let dt = time.delta_secs().clamp(1.0e-3, 0.1);
    let player = run.map_or(crate::cell_data::CellScale(1), |r| crate::cell_engine::player_scale(r.cell.food_progression));
    for (entity, motion, object, fx) in &cells {
        let mut slots = fx.map_or([None; 3], |f| f.effects);
        let mut changed = false;
        for (i, name) in PROPULSION_EFFECTS.iter().enumerate() {
            let strength = motion.propulsion[i];
            if object.dead {
                if let Some(mut e) = slots[i].and_then(|e| effects.get_mut(e).ok()) {
                    e.stop = true;
                }
                if slots[i].take().is_some() {
                    changed = true;
                }
                continue;
            }
            let layer = blur.layer(object.background);
            let emit = wake_emit(strength, dt);
            let alpha = if crate::cell_engine::scale_diff(object.scale, player) < 2 { 0.25 } else { 1.0 };
            match slots[i].and_then(|e| effects.get_mut(e).ok()) {
                Some(mut effect) => {
                    effect.emit = emit;
                    effect.alpha = alpha;
                    effect.layer = layer;
                }
                None => {
                    if strength < 0.0 {
                        continue;
                    }
                    let id = crate::world_objects::name_id(name);
                    if library.find(id).is_none() {
                        continue;
                    }
                    let mut effect = SwarmEffect::new(id);
                    effect.attach_to = Some(entity);
                    effect.emit = emit;
                    effect.alpha = alpha;
                    effect.layer = layer;
                    let spawned = commands.spawn((effect, StageOwned)).id();
                    slots[i] = Some(spawned);
                    changed = true;
                }
            }
        }
        if changed {
            commands.entity(entity).insert(CellPropulsionFx { effects: slots });
        }
    }
}

/// An electric beam between two cells (retail `FUN_00e6b9b0`: the beam effect
/// is created on the zapper, `cell_attack_electrical_primaryBeam` when its
/// model has an electric part, else `..._secondaryBeam`, and gets 21 points
/// from the target to the zapper's electric part as `kParamPath`).
#[derive(Message, Clone, Debug)]
pub struct CellBeamRequest {
    /// `true` for the primary (the zapper's own electric part) beam.
    pub primary: bool,
    /// The zapped cell (path start) and the zapper (path end), world positions.
    pub from: Vec3,
    pub to: Vec3,
    /// Effect scale (the zapper's size).
    pub scale: f32,
}

fn play_beams(mut commands: Commands, mut beams: MessageReader<CellBeamRequest>, library: Res<SwarmLibrary>) {
    for beam in beams.read() {
        let name = if beam.primary {
            "cell_attack_electrical_primaryBeam"
        } else {
            "cell_attack_electrical_secondaryBeam"
        };
        let id = crate::world_objects::name_id(name);
        if library.loaded && library.find(id).is_none() {
            continue;
        }
        let mut effect = SwarmEffect::new(id);
        effect.position = beam.to;
        effect.scale = beam.scale.max(1.0e-3);
        // 21 points from the zapped cell to the zapper (retail FUN_00e6b9b0).
        effect.path = (0..21).map(|i| beam.from.lerp(beam.to, i as f32 * 0.05)).collect();
        commands.spawn((effect, StageOwned));
    }
}

/// Names of the avatar decoration effects (`cCellGFX` 0xE6C330): the blur-mask
/// blob the DoF chain reads, one per size level, and the eye variants.
const HERO_EFFECTS: [&str; 12] = [
    "cell_hero_L1_decorations",
    "cell_hero_L2_decorations",
    "cell_hero_L4_decorations",
    "cell_hero_L10_decorations",
    "cell_hero_L20_decorations",
    "cell_hero_L40_decorations",
    "cell_hero_L100_decorations",
    "cell_hero_L200_decorations",
    "cell_hero_L400_decorations",
    "cell_hero_NoEyes",
    "cell_hero_OnePairEyes",
    "cell_hero_L0_decorations",
];

/// The avatar's screen filter chain (retail `PlaceStart` 0xE750C0 creates
/// `cell_dof_distort` for an avatar with eyes, `cell_dof_distort_blind`
/// without, in the main effect world, and starts it for the whole stage).
/// Here it runs on the background pass (render layer 1), see `swarm_fx/filters.rs`.
#[derive(Component)]
pub struct CellDofChain(pub u32);

fn dof_chain(
    mut commands: Commands,
    library: Res<SwarmLibrary>,
    blur: Res<CellBackgroundBlur>,
    flow: Option<Res<crate::cell_flow::CellFlow>>,
    mut chains: Query<(&CellDofChain, &mut SwarmEffect)>,
) {
    if !library.loaded {
        return;
    }
    if !(blur.enabled && blur.chain) {
        for (_, mut effect) in &mut chains {
            effect.stop = true;
        }
        return;
    }
    // `OS_CELL_BLIND=1` previews the eyeless variant.
    let blind = flow.as_ref().and_then(|f| f.decoration) == Some(crate::world_objects::name_id("cell_hero_NoEyes"))
        || crate::world_presentation::tune("CELL_BLIND", 0.0) > 0.5;
    let wanted = crate::world_objects::name_id(if blind {
        "cell_dof_distort_blind"
    } else {
        "cell_dof_distort"
    });
    let mut running = false;
    for (tag, mut effect) in &mut chains {
        if effect.stop {
            continue;
        }
        if tag.0 == wanted {
            running = true;
            effect.layer = BACKGROUND_LAYER;
        } else {
            effect.stop = true;
        }
    }
    if !running && library.find(wanted).is_some() {
        let mut effect = SwarmEffect::new(wanted);
        effect.layer = BACKGROUND_LAYER;
        commands.spawn((effect, CellDofChain(wanted), StageOwned));
    }
}

/// Spore draws the Cell stage as plain LDR without tone mapping; keep the
/// Cell camera's colours as authored (`world_presentation` applies its stage
/// defaults once on entry; this only overrides the tone mapping).
fn ldr_camera(
    mut commands: Commands,
    cameras: Query<(Entity, Option<&bevy::core_pipeline::tonemapping::Tonemapping>), With<crate::GameCamera>>,
) {
    for (camera, tonemapping) in &cameras {
        if tonemapping != Some(&Tonemapping::None) {
            commands.entity(camera).insert(Tonemapping::None);
        }
    }
}

/// Render layer of the Cell background pass: background-tile cells (with
/// their visuals and attached effects), the Background / TracerBackground /
/// Skybox effect-map entries and the background colour.
pub const BACKGROUND_LAYER: usize = 1;

/// Second Cell camera (order -1): draws [`BACKGROUND_LAYER`] into the shared
/// main texture and blurs it; the GameCamera then draws the foreground over
/// it without clearing. Only exists in the Cell stage (stage-owned).
/// ADAPTED: retail draws the background effect world (0x1010011), its models
/// and the background tiles in a pass of their own from the second viewer
/// (0xE54B80, see the module header) and the avatar's `cell_dof_distort` chain
/// softens it; here the pass is a second camera on the main camera's
/// transform and the softening is that chain ([`dof_chain`]). The fallback
/// (`OS_CELL_CHAIN=0`) is Bevy's Gaussian depth of field focused just in front
/// of the camera, which blurs the whole background pass by an almost constant
/// amount (translucent cell bodies write no depth, so a depth-dependent blur
/// would not hold).
#[derive(Component)]
pub struct CellBackgroundCamera;

/// Focus distance of the background blur: everything in the background pass
/// is 30+ units away, so the circle of confusion is ~constant.
const BLUR_FOCUS: f32 = 1.0;

/// The fallback constant blur of the background pass (`OS_CELL_CHAIN=0`).
fn background_dof(fov: f32, sensor_height: f32, coc: f32) -> DepthOfField {
    DepthOfField {
        mode: DepthOfFieldMode::Gaussian,
        focal_distance: BLUR_FOCUS,
        sensor_height,
        aperture_f_stops: blur_aperture(fov, sensor_height, coc),
        // Empty pixels (background colour) count as this far.
        max_depth: 1000.0,
        ..default()
    }
}

/// Aperture (f-stops) giving a blur circle of `coc` frame heights for
/// distant objects (Bevy's thin-lens circle of confusion).
fn blur_aperture(fov: f32, sensor_height: f32, coc: f32) -> f32 {
    let focal_length = 0.5 * sensor_height / (0.5 * fov).tan();
    focal_length * focal_length / (sensor_height * coc.max(1e-5) * (BLUR_FOCUS - focal_length))
}

/// A shadowless copy of a scene directional light on [`BACKGROUND_LAYER`]
/// (Bevy lights only reach views that share their render layers).
#[derive(Component)]
pub struct CellBackgroundLight(pub Entity);

/// Background pass settings: `OS_CELL_BLUR=0` keeps the single-camera path
/// (no background pass, no blur); `OS_CELL_BLUR_PX` is the blur circle in
/// pixels of a 900-pixel-high frame (scales with the frame height).
#[derive(Resource, Clone, Copy, Debug)]
pub struct CellBackgroundBlur {
    pub enabled: bool,
    /// Blur circle diameter as a fraction of the frame height. ADAPTED:
    /// matched by eye to the retail frames in `refs\cell\` (giants' pores).
    /// Only used by the fallback depth-of-field blur (`chain` off); the default
    /// path runs the retail `cell_dof_distort` filter chain, which has no such
    /// number.
    pub coc: f32,
    /// The background pass is filtered by the retail `cell_dof_distort(_blind)`
    /// screen filter chain (`OS_CELL_CHAIN=0` falls back to the constant blur).
    pub chain: bool,
}

impl Default for CellBackgroundBlur {
    fn default() -> Self {
        use crate::world_presentation::tune;
        Self {
            enabled: tune("CELL_BLUR", 1.0) > 0.5,
            coc: tune("CELL_BLUR_PX", 12.0).max(0.0) / 900.0,
            chain: tune("CELL_CHAIN", 1.0) > 0.5 && crate::swarm_fx::filters::filters_enabled(),
        }
    }
}

impl CellBackgroundBlur {
    /// Render layer for something that belongs to the background pass.
    pub fn layer(&self, background: bool) -> usize {
        if self.enabled && background {
            BACKGROUND_LAYER
        } else {
            0
        }
    }
}

/// Keep the background camera on the GameCamera's view (transform,
/// projection, sub view, render target, ambient light). The GameCamera stops
/// clearing while the pass runs and clears again while it is off (Cell
/// creator open, `OS_CELL_BLUR=0`).
#[allow(clippy::type_complexity)]
fn background_camera(
    mut commands: Commands,
    blur: Res<CellBackgroundBlur>,
    editor: Option<Res<crate::editor::EditorState>>,
    mut main: Query<
        (&mut Camera, &GlobalTransform, &Projection, &RenderTarget, Option<&AmbientLight>, Option<&Msaa>),
        (With<crate::GameCamera>, Without<CellBackgroundCamera>),
    >,
    mut background: Query<
        (
            Entity,
            &mut Camera,
            &mut Transform,
            &mut GlobalTransform,
            &mut Projection,
            &mut RenderTarget,
            Option<&mut DepthOfField>,
            Option<&mut AmbientLight>,
        ),
        (With<CellBackgroundCamera>, Without<crate::GameCamera>),
    >,
) {
    let Ok((mut camera, global, projection, target, ambient, msaa)) = main.single_mut() else {
        return;
    };
    let active = blur.enabled && camera.is_active && !editor.is_some_and(|e| e.open);
    match (active, camera.clear_color) {
        (true, ClearColorConfig::None) | (false, ClearColorConfig::Default) => {}
        (true, _) => camera.clear_color = ClearColorConfig::None,
        (false, _) => camera.clear_color = ClearColorConfig::Default,
    }
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };
    let Some((entity, mut bg, mut transform, mut bg_global, mut bg_projection, mut bg_target, dof, bg_ambient)) =
        background.iter_mut().next()
    else {
        if blur.enabled {
            let sensor_height = DepthOfField::default().sensor_height;
            let mut spawned = commands.spawn((
                CellBackgroundCamera,
                StageOwned,
                Name::new("cell-background-camera"),
                Camera3d::default(),
                Camera {
                    order: -1,
                    is_active: active,
                    // The GameCamera writes the composed frame.
                    output_mode: CameraOutputMode::Skip,
                    sub_camera_view: camera.sub_camera_view,
                    ..default()
                },
                projection.clone(),
                target.clone(),
                global.compute_transform(),
                *global,
                RenderLayers::layer(BACKGROUND_LAYER),
                Tonemapping::None,
                bevy::core_pipeline::tonemapping::DebandDither::Disabled,
                crate::swarm_fx::filters::SwarmFilterView(crate::swarm_fx::filters::VIEW_BACKGROUND),
            ));
            if !blur.chain {
                spawned.insert(background_dof(fov, sensor_height, blur.coc));
            }
            if let Some(ambient) = ambient {
                spawned.insert(ambient.clone());
            }
            if let Some(msaa) = msaa {
                spawned.insert(*msaa);
            }
        }
        return;
    };
    if bg.is_active != active {
        bg.is_active = active;
    }
    if bg.sub_camera_view != camera.sub_camera_view {
        bg.sub_camera_view = camera.sub_camera_view;
    }
    if *bg_global != *global {
        *bg_global = *global;
        *transform = global.compute_transform();
    }
    let same_projection = match (&*bg_projection, projection) {
        (Projection::Perspective(a), Projection::Perspective(b)) => a.fov == b.fov && a.near == b.near && a.far == b.far,
        _ => false,
    };
    if !same_projection {
        *bg_projection = projection.clone();
    }
    if bg_target.normalize(None) != target.normalize(None) {
        *bg_target = target.clone();
    }
    match (dof, blur.chain) {
        (Some(mut dof), false) => {
            let aperture = blur_aperture(fov, dof.sensor_height, blur.coc);
            if (dof.aperture_f_stops - aperture).abs() > 1e-4 * aperture {
                dof.aperture_f_stops = aperture;
            }
        }
        (Some(_), true) => {
            commands.entity(entity).remove::<DepthOfField>();
        }
        (None, false) => {
            let sensor_height = DepthOfField::default().sensor_height;
            commands.entity(entity).insert(background_dof(fov, sensor_height, blur.coc));
        }
        (None, true) => {}
    }
    match (ambient, bg_ambient) {
        (Some(a), Some(mut b)) => {
            if b.color != a.color || b.brightness != a.brightness {
                *b = a.clone();
            }
        }
        (Some(a), None) => {
            commands.entity(entity).insert(a.clone());
        }
        _ => {}
    }
}

/// Background cells and their visuals draw on [`BACKGROUND_LAYER`], and so do
/// the effects attached to them (retail: background world, 0xE66840).
fn background_layers(
    mut commands: Commands,
    blur: Res<CellBackgroundBlur>,
    objects: Query<(Entity, &crate::cell_engine::CellObject)>,
    children: Query<&Children>,
    layers: Query<Option<&RenderLayers>>,
    mut effects: Query<&mut SwarmEffect>,
) {
    if !blur.enabled {
        return;
    }
    let background = RenderLayers::layer(BACKGROUND_LAYER);
    let mut cells = HashSet::new();
    for (entity, object) in &objects {
        if !object.background {
            continue;
        }
        cells.insert(entity);
        for e in std::iter::once(entity).chain(children.iter_descendants(entity)) {
            if layers.get(e).ok().flatten() != Some(&background) {
                commands.entity(e).try_insert(background.clone());
            }
        }
    }
    for mut effect in &mut effects {
        let Some(target) = effect.attach_to else { continue };
        let layer = blur.layer(cells.contains(&target));
        if effect.layer != layer {
            effect.layer = layer;
        }
    }
}

/// `OPENSPORE_FX_DEBUG`: list meshes below the play plane that are not on
/// the background layer (every 100 frames).
#[allow(clippy::type_complexity)]
fn background_layer_audit(
    lib: Res<SwarmLibrary>,
    mut frame: Local<u32>,
    meshes: Query<(Entity, &GlobalTransform, Option<&Name>, Option<&ChildOf>, Option<&RenderLayers>), With<Mesh3d>>,
    names: Query<&Name>,
    effects: Query<(Entity, &SwarmEffect)>,
    objects: Query<(&crate::cell_engine::CellObject, &GlobalTransform)>,
    cells: Query<(Entity, &crate::cell_engine::CellObject, &GlobalTransform, Option<&Name>)>,
    children: Query<&Children>,
    drawn: Query<(Option<&RenderLayers>, Has<Mesh3d>, &GlobalTransform, &InheritedVisibility)>,
) {
    *frame += 1;
    if *frame % 100 != 0 || std::env::var_os("OPENSPORE_FX_DEBUG").is_none() {
        return;
    }
    for (entity, object, global, name) in &cells {
        let mut parts = vec![];
        for e in std::iter::once(entity).chain(children.iter_descendants(entity)) {
            if let Ok((layers, true, g, visible)) = drawn.get(e) {
                parts.push(format!("{e}:{layers:?}:y{:.1}:s{:.1}:{}", g.translation().y, g.scale().x, visible.get()));
            }
        }
        info!(
            "fx-cell: {entity} {name:?} bg {} y {:.1} radius {:.2} pos {:?} meshes {parts:?}",
            object.background,
            global.translation().y,
            object.radius,
            global.translation()
        );
    }
    for (_, effect) in &effects {
        // Map effects: particle counts and the share inside the view box.
        if effect.tag & 0xFFFF_0000 != MAP_TAG & 0xFFFF_0000 || effect.tag == BACKGROUND_TAG {
            continue;
        }
        let Some(instance) = &effect.instance else { continue };
        instance.for_each_particles(&lib, &mut |sys, def| {
            let near = sys.particles.iter().filter(|p| p.pos.x.abs() < 25.0 && p.pos.z.abs() < 25.0).count();
            info!(
                "fx-map: entry {} {:08x} sys {} alive {} near-origin {} size {:?} scale {:.3} rate {:?} origin {:?}",
                effect.tag & 0xFFFF,
                effect.effect,
                sys.index,
                sys.particles.len(),
                near,
                def.size.first(),
                effect.scale,
                def.rate.first(),
                sys.frame.map(|f| f.origin)
            );
        });
    }
    // Particles far above the play plane or very large (camera-covering).
    for (entity, effect) in &effects {
        let Some(instance) = &effect.instance else { continue };
        instance.for_each_particles(&lib, &mut |sys, def| {
            let top = sys.particles.iter().map(|p| p.pos.y).fold(f32::MIN, f32::max);
            let size = def.size.iter().copied().fold(0.0f32, f32::max) * sys.frame.map_or(0.0, |f| f.scale);
            if !sys.particles.is_empty() && (top > 3.0 || size > 15.0) {
                info!(
                    "fx-high: {entity} {:08x} sys {} alive {} top-y {top:.1} max-size {size:.1} origin {:?}",
                    effect.effect,
                    sys.index,
                    sys.particles.len(),
                    sys.frame.map(|f| f.origin)
                );
            }
        });
    }
    for (entity, effect) in &effects {
        let target = effect.attach_to.map(|t| (t, names.get(t).ok(), objects.get(t).ok().map(|(o, g)| (o.background, g.translation().y))));
        info!(
            "fx-effect: {entity} {:08x} tag {:x} layer {} pos {:?} attach {target:?}",
            effect.effect, effect.tag, effect.layer, effect.position
        );
    }
    let background = RenderLayers::layer(BACKGROUND_LAYER);
    for (entity, global, name, parent, layers) in &meshes {
        let y = global.translation().y;
        if y < -2.0 && layers != Some(&background) {
            let parent_name = parent.and_then(|p| names.get(p.parent()).ok());
            info!("fx-layer: {entity} {name:?} parent {:?} {parent_name:?} y {y:.1} layers {layers:?}", parent.map(|p| p.parent()));
        }
    }
}

/// Mirror the scene's directional lights onto the background layer without
/// shadows so background cells keep their lighting.
#[allow(clippy::type_complexity)]
fn background_lights(
    mut commands: Commands,
    blur: Res<CellBackgroundBlur>,
    lights: Query<(Entity, &DirectionalLight, &GlobalTransform, Option<&RenderLayers>), Without<CellBackgroundLight>>,
    mut copies: Query<(Entity, &CellBackgroundLight, &mut DirectionalLight, &mut Transform)>,
) {
    if !blur.enabled {
        return;
    }
    let shadowless = |light: &DirectionalLight| DirectionalLight {
        shadow_maps_enabled: false,
        contact_shadows_enabled: false,
        ..light.clone()
    };
    let mut mirrored = HashSet::new();
    for (entity, source, mut light, mut transform) in &mut copies {
        let Ok((_, original, global, _)) = lights.get(source.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        mirrored.insert(source.0);
        if light.color != original.color || light.illuminance != original.illuminance {
            *light = shadowless(original);
        }
        let wanted = global.compute_transform();
        if *transform != wanted {
            *transform = wanted;
        }
    }
    let main_layer = RenderLayers::layer(0);
    for (entity, light, global, layers) in &lights {
        if mirrored.contains(&entity) || layers.is_some_and(|l| !l.intersects(&main_layer)) {
            continue;
        }
        commands.spawn((
            CellBackgroundLight(entity),
            StageOwned,
            shadowless(light),
            global.compute_transform(),
            RenderLayers::layer(BACKGROUND_LAYER),
        ));
    }
}

/// Leaving the Cell stage: the GameCamera clears again (the background
/// camera and light copies are stage-owned and go with the stage).
fn restore_game_camera(
    mut cameras: Query<&mut Camera, With<crate::GameCamera>>,
    mut override_color: ResMut<crate::swarm_fx::filters::SwarmOverrideColor>,
    mut backdrop: ResMut<crate::swarm_fx::filters::SwarmBlendBackdrop>,
) {
    override_color.0 = None;
    backdrop.0 = None;
    for mut camera in &mut cameras {
        if matches!(camera.clear_color, ClearColorConfig::None) {
            camera.clear_color = ClearColorConfig::Default;
        }
    }
}

/// One effect event of an animation clip: clip seconds, effect id, whether it
/// plays at the Mouth channel (else Root).
#[derive(Clone, Copy, Debug)]
struct AnimFx {
    time: f32,
    effect: u32,
    mouth: bool,
}

/// Animation-event effects: per clip, the events whose id names an installed
/// effect (the event id is the FNV-1 hash of the event name), and per cell the
/// clip position seen last frame.
#[derive(Resource, Default)]
pub struct CellAnimFx {
    clips: std::collections::HashMap<u32, std::sync::Arc<Vec<AnimFx>>>,
    cursors: std::collections::HashMap<Entity, (u32, f32)>,
}

impl CellAnimFx {
    fn clip(&mut self, clip: u32, originals: &Originals, library: &SwarmLibrary) -> std::sync::Arc<Vec<AnimFx>> {
        self.clips
            .entry(clip)
            .or_insert_with(|| {
                let key = crate::dbpf::ResourceKey {
                    type_id: crate::decode::TYPE_ANIM,
                    group_id: 0,
                    instance_id: clip,
                };
                let mut out = vec![];
                if let Ok(anim) = originals.resource_index.read(key).and_then(|b| crate::animation::parse(&b)) {
                    for channel in &anim.channels {
                        let mouth = channel.name.eq_ignore_ascii_case("mouth");
                        for component in &channel.components {
                            let crate::animation::Keys::Info(keys) = &component.keys else { continue };
                            for k in keys {
                                let first = k.event_start as usize;
                                for event in anim.events.iter().skip(first).take(k.event_count as usize) {
                                    if library.find(event.id).is_some() {
                                        out.push(AnimFx {
                                            time: k.frame as f32 * anim.frame_length,
                                            effect: event.id,
                                            mouth,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                std::sync::Arc::new(out)
            })
            .clone()
    }
}

/// Mouth position in a cell's local frame. ADAPTED: the front of the body
/// (cells face local -Z, bodies are fitted to radius 0.9). Evidence
/// (`tests::dump_mouth_channels`): the `Mouth` channel of the Cell clips is one
/// info component (the events) plus four deform components (the mouth morphs),
/// never position keys, so the clip carries no position; the retail point comes
/// from the creature rig (the hashed name `mouth`, 0x3C2A2452, occurs once in
/// the code image, at 0x5AAE93, not decoded for Cell models).
const MOUTH_OFFSET: Vec3 = Vec3::new(0.0, 0.0, -0.8);

/// Fire the installed effects named by animation events when the playing clip
/// passes their frames (retail: the animation event system).
fn anim_event_effects(
    mut commands: Commands,
    library: Res<SwarmLibrary>,
    originals: Option<Res<Originals>>,
    mut fx: ResMut<CellAnimFx>,
    cells: Query<(Entity, &crate::cell_anim::CellAnimPlayback)>,
) {
    let Some(originals) = originals else { return };
    if !library.loaded {
        return;
    }
    let mut seen = std::collections::HashSet::new();
    for (entity, playback) in &cells {
        let Some(current) = &playback.current else { continue };
        seen.insert(entity);
        let now = current.sample_time();
        let previous = fx.cursors.insert(entity, (current.clip, now));
        let events = fx.clip(current.clip, &originals, &library);
        if events.is_empty() {
            continue;
        }
        let (from, wrapped) = match previous {
            Some((clip, t)) if clip == current.clip => (t, now < t),
            _ => (-1.0, false),
        };
        for e in events.iter() {
            let hit = if wrapped {
                e.time > from || e.time <= now
            } else {
                e.time > from && e.time <= now
            };
            if hit {
                let mut effect = SwarmEffect::new(e.effect);
                effect.attach_to = Some(entity);
                effect.position = if e.mouth { MOUTH_OFFSET } else { Vec3::ZERO };
                commands.spawn((effect, StageOwned));
            }
        }
    }
    fx.cursors.retain(|e, _| seen.contains(e));
}

/// Marker: this cell's call-effect slot is taken (retail cell GFX record +0x9C,
/// filled once and never cleared while the cell lives).
#[derive(Component)]
pub struct CellCallFx;

/// `cell_call_avatar` / `cell_call_mate`: the effect a cell plays for its call
/// flag (+0x18B), the avatar's or any other cell's.
pub fn call_effect(is_avatar: bool) -> u32 {
    if is_avatar {
        0xA2B9_1940
    } else {
        0xE2C8_BCE4
    }
}

/// Mating call rings (VERIFIED, 0xE63BF0 called per cell from the GFX loop
/// 0xE73F60 with the cell's call flag +0x18B and its transform): while the flag
/// is set and the cell's slot (+0x9C) is empty, `cell_call_avatar` (the avatar,
/// cell id == `sCellGame+0x411C`) or `cell_call_mate` (every other cell) is
/// created in the cell's effect world, started and placed with the cell's
/// transform; with the flag clear the slot's effect is stopped and released.
/// The flag is written only by 0xE53D30 (`MatingCall` mission step: `t < 8` flags
/// the mate, `t < 4` the avatar, `t` = seconds since the call; the first step
/// after the mate spawn 0xE760F0 runs it with t = 0, so both are flagged at once)
/// and by the cell constructor (0xE43A47, zero): nothing clears it. So each
/// call plays the rings of a new mate once, and the avatar's rings only on its
/// first call (its slot stays taken); the effects run their own length.
/// ADAPTED: OpenSpore has no mission object with the retail `t`, so the call
/// moment (t = 0) is the first frame a mate that has not played its rings yet
/// exists; a released cell's effects wind down in place like any attached
/// effect (retail releases them with the cell's GFX record).
fn mating_call_rings(
    mut commands: Commands,
    library: Res<SwarmLibrary>,
    players: Query<(Entity, Has<CellCallFx>), With<crate::game::Player>>,
    mates: Query<(Entity, Has<CellCallFx>), With<crate::cell_engine::CellMateObject>>,
) {
    if !library.loaded {
        return;
    }
    let mut called = false;
    for (mate, done) in &mates {
        if done {
            continue;
        }
        called = true;
        commands.entity(mate).insert(CellCallFx);
        spawn_call_effect(&mut commands, &library, call_effect(false), mate);
    }
    if called {
        for (player, done) in &players {
            if !done {
                commands.entity(player).insert(CellCallFx);
                spawn_call_effect(&mut commands, &library, call_effect(true), player);
            }
        }
    }
}

fn spawn_call_effect(commands: &mut Commands, library: &SwarmLibrary, id: u32, cell: Entity) {
    if library.find(id).is_none() {
        return;
    }
    let mut effect = SwarmEffect::new(id);
    effect.attach_to = Some(cell);
    commands.spawn((effect, StageOwned));
}

/// Marker on a player part entity whose part is electric (set by
/// `cell_anim::attach_part`): where the avatar's charged glow sits.
#[derive(Component)]
pub struct CellElectricPart;

/// The charged-glow effects of one cell (the retail per-cell effect list).
#[derive(Component, Default)]
pub struct CellChargedFx(pub Vec<Entity>);

/// `cell_attack_electrical_charged_multiple` / `cell_attack_electrical_charged`.
const CHARGED_MULTIPLE: u32 = 0x17AA_B61F;
const CHARGED_SINGLE: u32 = 0x5CA6_36DC;

/// Retail charged flag (+0x18A, 0xE71980): set while the electric cooldown is
/// over; in the cooldown's last second it flickers (0xE507B0(t, 0.1); the
/// 0.1 s on / off phase is INFERRED).
pub fn charged_flag(cooldown: f32) -> bool {
    cooldown <= 0.0 || (cooldown < 1.0 && ((cooldown / 0.1) as i32) % 2 == 0)
}

/// Charged-electric glow id by the electric part count n[6] (caps + 0x121C):
/// below 3 `cell_attack_electrical_charged_multiple`, else `..._charged`.
pub fn charged_effect(n6: u32) -> u32 {
    if n6 < 3 {
        CHARGED_MULTIPLE
    } else {
        CHARGED_SINGLE
    }
}

/// Charged-electric glow (retail 0xE6BC10, called per cell from 0xE73F60):
/// while the charged flag is set, one effect per electric part (type 6) of the
/// cell's model ([`charged_effect`]) at the part's transform with scale
/// max(2 * range (+0x160), 2^-16); not charged: every effect of the list is
/// stopped (Stop(1)) and released. VERIFIED (decompilation).
/// An NPC's part positions are the animated poses of its electric rigblocks
/// (`cell_anim::CellPartPoses`): retail's 0xE84150 (model, part id, out
/// transform) returns the part's CURRENT pose: the block's entry in the
/// animated model's pose array (+0x2E4, stride 0x2BC) rotated by the model's
/// orientation quaternion (+0x3C..+0x48) and moved by its position, so the
/// glow follows the part as the clip bends the body. Before the cell is rigged
/// (headless runs) the static part sphere centres stand in
/// (`CellArt::spheres`, RE 11.6). The avatar's are its electric part
/// entities; own-species cells (the player's design on an NPC) get none.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn charged_glow(
    mut commands: Commands,
    library: Res<SwarmLibrary>,
    blur: Res<CellBackgroundBlur>,
    db: Option<Res<crate::cell_data::CellDatabase>>,
    art: Option<Res<crate::cell_engine::CellArtCache>>,
    game: Option<Res<crate::cell_engine::CellGame>>,
    run: Option<Res<crate::game::RunProgress>>,
    originals: Res<crate::Originals>,
    cells: Query<(
        Entity,
        &GlobalTransform,
        &crate::cell_engine::CellObject,
        &crate::cell_rules::CellCombat,
        Option<&CellChargedFx>,
        Option<&crate::cell_anim::CellPartPoses>,
    )>,
    player_parts: Query<(&GlobalTransform, &ChildOf), With<CellElectricPart>>,
    parents: Query<&ChildOf>,
    mut effects: Query<&mut SwarmEffect>,
) {
    use crate::cell_engine::CellKind;
    let (Some(db), Some(art), Some(game), Some(run)) = (db, art, game, run) else {
        return;
    };
    if !library.loaded {
        return;
    }
    let Some(globals) = db.globals.as_ref() else {
        return;
    };
    let player_caps = crate::cell_motion::player_caps(&run, &originals, &db);
    let ancestor_of = |mut e: Entity, root: Entity| {
        for _ in 0..8 {
            if e == root {
                return true;
            }
            match parents.get(e) {
                Ok(p) => e = p.parent(),
                Err(_) => return false,
            }
        }
        false
    };
    for (entity, global, object, combat, fx, poses) in &cells {
        let caps = match object.kind {
            CellKind::Player => Some(player_caps),
            CellKind::Creature => art
                .art
                .get(&object.cell)
                .filter(|a| a.modelled)
                .map(|a| crate::cell_motion::CellCaps::from_counts(a.counts, &db.graphs, globals)),
            _ => None,
        };
        // Part positions (world) while charged.
        let mut points: Vec<Vec3> = Vec::new();
        let mut range = 0.0;
        if let Some(caps) = caps.filter(|c| c.n[6] > 0 && !object.dead && !object.background) {
            let electric = crate::cell_rules::electric_values(&db, object, &caps, false, game.difficulty, game.world_scale);
            if let Some((_, _, r, _)) = electric.filter(|_| charged_flag(combat.electric_cooldown)) {
                range = r;
                let at = global.translation();
                if object.kind == CellKind::Player {
                    for (part, parent) in &player_parts {
                        if ancestor_of(parent.parent(), entity) {
                            points.push(part.translation());
                        }
                    }
                } else if let Some(pose) = poses.filter(|p| p.of_kind(6).next().is_some()) {
                    let rot = Quat::from_rotation_y(object.heading);
                    for part in pose.of_kind(6) {
                        points.push(at + rot * Vec3::new(part.point.x, 0.0, part.point.y) * object.radius);
                    }
                } else if let Some(a) = art.art.get(&object.cell) {
                    let rot = Quat::from_rotation_y(object.heading);
                    for s in a.spheres.iter().filter(|s| s.kind == 6) {
                        points.push(at + rot * Vec3::new(s.center.x, 0.0, s.center.y) * object.radius);
                    }
                }
            }
        }
        if points.is_empty() && fx.is_none_or(|f| f.0.is_empty()) {
            continue;
        }
        let mut list = fx.map(|f| f.0.clone()).unwrap_or_default();
        let before = list.clone();
        let scale = (range * 2.0).max(1.0 / 65536.0);
        let id = caps.map_or(CHARGED_SINGLE, |c| charged_effect(c.n[6]));
        let rotation = Quat::from_rotation_y(object.heading);
        let layer = blur.layer(object.background);
        for (i, point) in points.iter().enumerate() {
            match list.get(i).and_then(|e| effects.get_mut(*e).ok()) {
                Some(mut effect) => {
                    effect.position = *point;
                    effect.rotation = rotation;
                    effect.scale = scale;
                }
                None => {
                    if library.find(id).is_none() {
                        break;
                    }
                    let mut effect = SwarmEffect::new(id);
                    effect.position = *point;
                    effect.rotation = rotation;
                    effect.scale = scale;
                    effect.layer = layer;
                    let spawned = commands.spawn((effect, StageOwned)).id();
                    if i < list.len() {
                        list[i] = spawned;
                    } else {
                        list.push(spawned);
                    }
                }
            }
        }
        // Surplus effects (fewer parts, or not charged): Stop(1) and release.
        while list.len() > points.len() {
            if let Some(e) = list.pop() {
                if let Ok(mut effect) = effects.get_mut(e) {
                    effect.stop = true;
                    effect.hard_stop = true;
                }
            }
        }
        if list != before {
            commands.entity(entity).insert(CellChargedFx(list));
        }
    }
}

/// Hide the engine's stand-in mesh of a cell (rocks: retail art is only an
/// effect attachment) while an attached Swarm effect draws its model; show it
/// again if the effect is missing or stops drawing.
fn hide_fallbacks(
    drawn: Option<Res<crate::swarm_fx::models::SwarmAttachedDraws>>,
    mut fallbacks: Query<(&ChildOf, &mut Visibility), With<crate::cell_contract::CellFallbackVisual>>,
) {
    let Some(drawn) = drawn else { return };
    for (parent, mut visibility) in &mut fallbacks {
        let want = if drawn.0.contains(&parent.parent()) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != want {
            *visibility = want;
        }
    }
}

/// Once the original environment runs, remove the adapted parallax layers and
/// backdrop that stood in for it (`cell_parallax.rs`, `cell_environment.rs`).
fn retire_fake_layers(
    mut commands: Commands,
    maps: Res<CellFxMaps>,
    library: Res<SwarmLibrary>,
    layers: Query<
        Entity,
        Or<(
            With<crate::cell_parallax::CellParallax>,
            With<crate::cell_parallax::CellBackdrop>,
            With<crate::cell_environment::CellEnvironmentLayerSource>,
        )>,
    >,
) {
    if !maps.loaded || maps.effect_map.is_empty() || library.exports.is_empty() {
        return;
    }
    for entity in &layers {
        commands.entity(entity).despawn();
    }
}

/// Effect id from a name (FNV-1 of the lowercase name) or `0x` hex id.
pub fn parse_id(name: &str) -> u32 {
    name.strip_prefix("0x")
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .unwrap_or_else(|| crate::world_objects::name_id(name))
}

/// Quick-capture support (`--quick-capture DIR cell ...`):
/// `--fx NAME|0xID` fires the effect on the player at frame `--fx-at N`
/// (default `frames - 40`), `--fx-scale S` scales it, `--fx-snaps a,b,c`
/// also writes `fx-NAME-<a>.png` a, b, c frames after firing, and
/// `--fx-world-scale W` previews the environment at retail world scale W.
pub fn capture_fx_step(
    app: &mut App,
    args: &[String],
    frame: usize,
    frames: usize,
    output: &std::path::Path,
    target: &Handle<Image>,
) {
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    if frame == 0 {
        if let Some(w) = flag("--fx-world-scale").and_then(|w| w.parse().ok()) {
            app.world_mut().resource_mut::<CellFxOverride>().world_scale = Some(w);
        }
    }
    // `--cell-npc NAME [--cell-npc-at N] [--cell-npc-offset X,Z] [--cell-npc-scale S]`
    // spawns one cell record (e.g. `cell_buzzy`, electric; `cell_poisoner`, poison)
    // next to the avatar at frame N, to look at its parts (retail world units).
    if let Some(name) = flag("--cell-npc") {
        let at = flag("--cell-npc-at").and_then(|v| v.parse().ok()).unwrap_or(30usize);
        if frame == at {
            let world = app.world_mut();
            let here = world
                .query_filtered::<&Transform, With<crate::game::Player>>()
                .iter(world)
                .next()
                .map(|t| Vec2::new(t.translation.x, t.translation.z))
                .unwrap_or_default();
            let offset = flag("--cell-npc-offset")
                .and_then(|v| {
                    let (x, z) = v.split_once(',')?;
                    Some(Vec2::new(x.parse().ok()?, z.parse().ok()?))
                })
                .unwrap_or(Vec2::new(3.0, -2.5));
            let scale = flag("--cell-npc-scale")
                .and_then(|v| v.parse().ok())
                .map(crate::cell_data::CellScale)
                .unwrap_or_else(|| crate::cell_engine::player_scale(world.resource::<crate::game::RunProgress>().cell.food_progression));
            world.resource_mut::<crate::cell_engine::CellSpawnQueue>().0.push(crate::cell_engine::SpawnRequest {
                cell: crate::world_objects::name_id(&name),
                scale,
                position: here + offset * crate::cell_engine::UNIT,
                size_roll: 0.5,
                invulnerable: 600.0,
                ..default()
            });
            info!("cell-fx capture: spawned {name} at frame {frame}");
        }
    }
    let Some(name) = flag("--fx") else { return };
    let at = flag("--fx-at")
        .and_then(|v| v.parse().ok())
        .unwrap_or(frames.saturating_sub(40));
    if frame == at {
        let world = app.world_mut();
        let player = world
            .query_filtered::<Entity, With<crate::game::Player>>()
            .iter(world)
            .next();
        let scale = flag("--fx-scale").and_then(|v| v.parse().ok()).unwrap_or(1.0);
        world.write_message(CellEffectRequest {
            effect: parse_id(&name),
            position: Vec3::ZERO,
            attach_to: player,
            scale,
        });
        info!("cell-fx capture: fired {name} at frame {frame}");
    }
    if let Some(snaps) = flag("--fx-snaps") {
        for snap in snaps.split(',').filter_map(|s| s.trim().parse::<usize>().ok()) {
            if frame == at + snap {
                let clean: String = name
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '-' })
                    .collect();
                let path = output.join(format!("fx-{clean}-{snap:03}.png"));
                crate::runtime::capture_render(app, target, &path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charged_glow_ids_flag_and_camera_framing() {
        // 0xE6BC10: n[6] < 3 -> "_multiple", else the plain glow.
        assert_eq!(charged_effect(2), crate::world_objects::name_id("cell_attack_electrical_charged_multiple"));
        assert_eq!(charged_effect(4), crate::world_objects::name_id("cell_attack_electrical_charged"));
        assert_eq!(0xE2C8_BCE4, crate::world_objects::name_id("cell_call_mate"));
        assert!(charged_flag(0.0));
        assert!(!charged_flag(3.0));
        assert!(charged_flag(0.55) != charged_flag(0.45));
        // The camera frames the retail view: 0x7C5400(50) -> vertical
        // half-tangent 0.75 tan 25 deg at the 20-unit offset (45 degree
        // vertical FOV in Bevy).
        let half = crate::cell::CAMERA_HEIGHT_BASE * std::f32::consts::FRAC_PI_8.tan() / crate::cell_engine::UNIT;
        let retail = 20.0 * 0.75 * 25f32.to_radians().tan();
        assert!((half - retail).abs() < 0.005, "{half} vs {retail}");
        assert!((FX_VIEW_UNIT - crate::cell_engine::UNIT).abs() < 1e-6);
    }

    /// Inventory of Cell animation events (run on demand).
    #[test]
    #[ignore = "requires the installed game"]
    fn dump_cell_animation_events() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut keys: Vec<_> = index
            .records
            .keys()
            .copied()
            .filter(|k| k.type_id == crate::decode::TYPE_ANIM)
            .collect();
        keys.sort_by_key(|k| k.instance_id);
        for key in keys {
            let Ok(anim) = index.read(key).and_then(|b| crate::animation::parse(&b)) else { continue };
            if !anim.source_path.to_lowercase().contains("cell") || anim.events.is_empty() {
                continue;
            }
            let mut frames = vec![];
            for c in &anim.channels {
                for comp in &c.components {
                    if let crate::animation::Keys::Info(keys) = &comp.keys {
                        for k in keys.iter().filter(|k| k.event_count > 0) {
                            frames.push((k.frame, k.event_start, k.event_count, c.name.clone()));
                        }
                    }
                }
            }
            println!("{:08x} {} len {:.2}s", key.instance_id, anim.source_path, anim.duration_seconds());
            for (i, e) in anim.events.iter().enumerate() {
                let caps: Vec<_> = e.selectors.iter().map(|s| s.capability.as_str()).filter(|s| !s.is_empty()).collect();
                println!("   ev{i} id {:08x} grp {:08x} arch {:08x} name {:?} p0 {:08x} p1 {:08x} caps {caps:?} flags {:x}", e.id, e.group, e.archetype, e.name, e.parameter0, e.parameter1, e.flags);
            }
            println!("   info {frames:?}");
        }
    }

    /// What the `mouth` channel of the Cell clips carries (run on demand).
    #[test]
    #[ignore = "requires the installed game"]
    fn dump_mouth_channels() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut keys: Vec<_> = index
            .records
            .keys()
            .copied()
            .filter(|k| k.type_id == crate::decode::TYPE_ANIM)
            .collect();
        keys.sort_by_key(|k| k.instance_id);
        let mut shown = 0;
        for key in keys {
            let Ok(anim) = index.read(key).and_then(|b| crate::animation::parse(&b)) else { continue };
            if !anim.source_path.to_lowercase().contains("cell") {
                continue;
            }
            for c in anim.channels.iter().filter(|c| c.name.to_lowercase().contains("mouth")) {
                let mut kinds = vec![];
                for comp in &c.components {
                    let (kind, n, first) = match &comp.keys {
                        crate::animation::Keys::Info(k) => ("info", k.len(), String::new()),
                        crate::animation::Keys::Position(k) => ("pos", k.len(), format!("{:?}", k.first().map(|p| p.position))),
                        crate::animation::Keys::Rotation(k) => ("rot", k.len(), String::new()),
                        crate::animation::Keys::Deform(k) => ("deform", k.len(), String::new()),
                        crate::animation::Keys::Unknown(k) => ("unknown", k.len(), String::new()),
                    };
                    kinds.push(format!("{kind}x{n} {first}"));
                }
                println!("{:08x} {} channel {:?} flags {:x} bind {:x} comps {kinds:?}", key.instance_id, anim.source_path, c.name, c.movement_flags, c.bind_flags);
                shown += 1;
            }
        }
        println!("mouth channels: {shown}");
    }

    /// `OPENSPORE_FX_DUMP=name,name cargo test dump_effects -- --ignored`
    #[test]
    #[ignore = "requires the installed game"]
    fn dump_effects() {
        use crate::swarm_fx::format::kind;
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        let names = std::env::var("OPENSPORE_FX_DUMP").unwrap_or_else(|_| "cell_dof_distort_blind,cell_dof_distort".into());
        fn r_dir_of(lib: &SwarmLibrary, dir: &crate::swarm_fx::format::EffectDirectory) -> usize {
            lib.dirs.iter().position(|d| std::ptr::eq(d, dir)).unwrap_or(usize::MAX)
        }
        fn walk(lib: &SwarmLibrary, r: crate::swarm_fx::EffectRef, depth: usize) {
            let dir = lib.dir(r.dir);
            let Some(effect) = dir.effects.get(r.index as usize) else { return };
            let pad = "  ".repeat(depth + 1);
            println!("{pad}effect flags {:x} app {:x} blocks {}", effect.flags, effect.app_flags_mask, effect.blocks.len());
            for b in &effect.blocks {
                println!("{pad}- kind {:#x} index {} flags {:x}", b.kind, b.index, b.flags);
                match b.kind {
                    kind::SCREEN => {
                        if let Some(s) = dir.screens.get(b.index as usize) {
                            println!("{pad}  screen {s:?}");
                            for r in &s.params_resource {
                                for (d, other) in lib.dirs.iter().enumerate() {
                                    if let Some(m) = other.materials.get(&r.instance) {
                                        println!("{pad}  material {:08x} in dir {d} (this {}): {m:?}", r.instance, r_dir_of(lib, dir));
                                    }
                                }
                            }
                        }
                    }
                    kind::PARTICLES => {
                        if let Some(p) = dir.particles.get(b.index as usize) {
                            println!("{pad}  particles flags {:x} tex {:?}", p.flags, p.texture);
                            if p.texture.draw_mode == crate::swarm_fx::format::draw::MATERIAL {
                                if let Some(m) = dir.materials.get(&p.texture.resource.instance) {
                                    println!("{pad}  material {:08x}: {m:?}", p.texture.resource.instance);
                                }
                            }
                            if std::env::var_os("OPENSPORE_FX_DUMP_FULL").is_some() {
                                println!("{pad}  {p:?}");
                            }
                        }
                    }
                    kind::RIBBON => {
                        if let Some(rb) = dir.ribbons.get(b.index as usize) {
                            println!("{pad}  ribbon {rb:?}");
                        }
                    }
                    kind::SHAKE => {
                        if let Some(sh) = dir.shakes.get(b.index as usize) {
                            println!("{pad}  shake {sh:?}");
                        }
                    }
                    kind::MODEL => {
                        if let Some(m) = dir.models.get(b.index as usize) {
                            println!("{pad}  model {m:?} block transform {:?}", b.transform);
                        }
                    }
                    kind::META_PARTICLES => {
                        if let Some(m) = dir.metas.get(b.index as usize) {
                            println!("{pad}  meta component {:?} flags {:x}", m.component, m.flags);
                            if std::env::var_os("OPENSPORE_FX_DUMP_FULL").is_some() {
                                println!("{pad}  {m:?}");
                            }
                            if depth < 3 {
                                if let Some(child) = lib.resolve_effect(r.dir, m.component.0) {
                                    println!("{pad}  meta child:");
                                    walk(lib, child, depth + 1);
                                }
                            }
                        }
                    }
                    kind::EFFECT => {
                        if let Some(child) = lib.resolve_effect(r.dir, b.index) {
                            walk(lib, child, depth + 1);
                        }
                    }
                    _ => {}
                }
            }
        }
        for name in names.split(',') {
            let id = parse_id(name.trim());
            println!("{name} {id:08x}");
            if let Some(r) = lib.find(id) {
                walk(&lib, r, 0);
            }
        }
    }

    /// Which `cell_*` effects carry an attractor (particle flag 0x17) and whether its
    /// location comes from `kParamAttractorLocation` (flag 0x18, set by the
    /// effect-map loop 0xE63560 as parameters 9 / 10): run on demand.
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_cell_attractors() {
        use crate::swarm_fx::format::{kind, pflag};
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        let reg = std::fs::read_to_string(
            "D:/SteamLibrary/steamapps/common/Spore/OpenSpore/target/format-reference/reg_file.txt",
        )
        .unwrap_or_default();
        let mut rows: Vec<(String, u32)> = reg
            .lines()
            .filter_map(|l| l.split('\t').next())
            .map(|n| n.trim())
            .filter(|n| n.starts_with("cell_"))
            .map(|n| (n.to_string(), crate::world_objects::name_id(n)))
            .collect();
        rows.sort();
        rows.dedup();
        fn walk(lib: &SwarmLibrary, r: crate::swarm_fx::EffectRef, depth: usize, out: &mut Vec<String>) {
            if depth > 6 {
                return;
            }
            let dir = lib.dir(r.dir);
            let Some(effect) = dir.effects.get(r.index as usize) else { return };
            for b in &effect.blocks {
                match b.kind {
                    kind::PARTICLES => {
                        if let Some(p) = dir.particles.get(b.index as usize) {
                            if p.has(pflag::ATTRACTOR) || p.has(pflag::NOT_PRESET_ATTRACTOR) {
                                out.push(format!(
                                    "particles #{} attractor {} not-preset {} origin {:?} range {} kill {} strength {:?}",
                                    b.index,
                                    p.has(pflag::ATTRACTOR),
                                    p.has(pflag::NOT_PRESET_ATTRACTOR),
                                    p.attractor_origin,
                                    p.attractor.range,
                                    p.attractor.kill_range,
                                    p.attractor.strength
                                ));
                            }
                        }
                    }
                    kind::EFFECT => {
                        if let Some(child) = lib.resolve_effect(r.dir, b.index) {
                            walk(lib, child, depth + 1, out);
                        }
                    }
                    _ => {}
                }
            }
        }
        for (name, id) in rows {
            let Some(r) = lib.find(id) else { continue };
            let mut out = vec![];
            walk(&lib, r, 0, &mut out);
            for line in out {
                println!("{name}: {line}");
            }
        }
    }

    /// `OPENSPORE_FX_SURVEY=path cargo test survey_effects -- --ignored`: which
    /// named effects use particle buffers 2/3, filter chains, ribbons, shakes.
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_effects() {
        use crate::swarm_fx::format::kind;
        use std::fmt::Write;
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        let reg = std::fs::read_to_string(
            "D:/SteamLibrary/steamapps/common/Spore/OpenSpore/target/format-reference/reg_file.txt",
        )
        .unwrap_or_default();
        let names: std::collections::HashMap<u32, String> = reg
            .lines()
            .filter_map(|l| l.split('\t').next())
            .map(|n| (crate::world_objects::name_id(n.trim()), n.trim().to_string()))
            .collect();
        #[derive(Default)]
        struct Found {
            buffers: std::collections::BTreeMap<u8, Vec<String>>,
            screens: Vec<String>,
            ribbons: usize,
            shakes: usize,
        }
        fn walk(lib: &SwarmLibrary, r: crate::swarm_fx::EffectRef, depth: usize, f: &mut Found, seen: &mut Vec<crate::swarm_fx::EffectRef>) {
            if depth > 8 || seen.contains(&r) {
                return;
            }
            seen.push(r);
            let dir = lib.dir(r.dir);
            let Some(effect) = dir.effects.get(r.index as usize) else { return };
            for b in &effect.blocks {
                match b.kind {
                    kind::PARTICLES => {
                        if let Some(p) = dir.particles.get(b.index as usize) {
                            if p.texture.buffer != 0 {
                                f.buffers.entry(p.texture.buffer).or_default().push(format!(
                                    "{:08x} draw {} flags {:x}",
                                    p.texture.resource.instance, p.texture.draw_mode, p.flags
                                ));
                            }
                        }
                    }
                    kind::SCREEN => {
                        if let Some(s) = dir.screens.get(b.index as usize) {
                            f.screens.push(format!("mode {} filters {} tex {:08x}", s.mode, s.filters.len(), s.texture.instance));
                        }
                    }
                    kind::RIBBON => f.ribbons += 1,
                    kind::SHAKE => f.shakes += 1,
                    kind::EFFECT => {
                        if let Some(child) = lib.resolve_effect(r.dir, b.index) {
                            walk(lib, child, depth + 1, f, seen);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut out = String::new();
        let mut rows: Vec<(String, u32)> = lib
            .exports
            .keys()
            .map(|&id| (names.get(&id).cloned().unwrap_or_else(|| format!("{id:08x}")), id))
            .collect();
        rows.sort();
        for (name, id) in rows {
            let mut f = Found::default();
            walk(&lib, lib.find(id).unwrap(), 0, &mut f, &mut vec![]);
            if f.buffers.is_empty() && f.screens.iter().all(|s| !s.starts_with("mode 10")) && f.ribbons == 0 && f.shakes == 0 {
                continue;
            }
            let _ = writeln!(out, "{name} ({id:08x}) ribbons {} shakes {} screens {:?}", f.ribbons, f.shakes, f.screens);
            for (b, list) in &f.buffers {
                let _ = writeln!(out, "    buffer {b}: {list:?}");
            }
        }
        let path = std::env::var("OPENSPORE_FX_SURVEY").unwrap_or_else(|_| "fx_survey.txt".into());
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn background_blur_is_nearly_constant_with_depth() {
        // Bevy's circle of confusion (dof.wgsl): scale * |d - F| / (d * (F - f)).
        let (fov, sensor, coc) = (std::f32::consts::FRAC_PI_4, 0.01866, 12.0 / 900.0);
        let n = blur_aperture(fov, sensor, coc);
        let f = 0.5 * sensor / (0.5 * fov).tan();
        let circle = |d: f32| f * f / (sensor * n) * (d - BLUR_FOCUS).abs() / (d * (BLUR_FOCUS - f));
        assert!((circle(1.0e6) - coc).abs() < 1e-6);
        // Background cells sit 30+ units from the camera: within 4 %.
        assert!(circle(30.0) > 0.96 * coc && circle(1000.0) <= coc);
    }

    #[test]
    fn fade_follows_the_retail_ramp() {
        // cell_backgrounds_L1_tracers
        let keys = [0.42, 0.7, 1.6, 1.95];
        assert_eq!(map_fade(0.3, keys), 0.0);
        assert!((map_fade(0.56, keys) - 0.5).abs() < 1e-5);
        assert_eq!(map_fade(1.5, keys), 1.0);
        assert!((map_fade(1.775, keys) - 0.5).abs() < 1e-5);
        assert_eq!(map_fade(2.0, keys), 0.0);
        // cell_backgrounds_L0_tracers: visible from the start.
        assert_eq!(map_fade(0.5, [-1.0, -1.0, 0.75, 0.9]), 1.0);
        assert_eq!(map_fade(0.95, [-1.0, -1.0, 0.75, 0.9]), 0.0);
        assert_eq!(map_fade(7.0, [-1.0, -1.0, -1.0, -1.0]), 1.0);
    }

    #[test]
    fn world_scale_key_is_half_the_level_size() {
        let mut state = CellWorldState::default();
        assert_eq!(fx_world_scale(&state), 0.5);
        state.food_progression = 250;
        assert_eq!(fx_world_scale(&state), 5.0);
        // The engine publishes the smoothed key itself (0xE4EE80 / 0x514C).
        state.world_scale = 4.2;
        assert_eq!(fx_world_scale(&state), 4.2);
        state.food_progression = 1000;
        state.world_scale = 0.0;
        assert_eq!(fx_world_scale(&state), 15000.0);
        // The engine's own key for every food value is the retail map key.
        for food in [0, 49, 120, 250, 349, 999, 1000] {
            let state = CellWorldState {
                food_progression: food,
                world_scale: crate::cell_engine::world_scale_for(food),
                ..default()
            };
            assert_eq!(fx_world_scale(&state), 0.5 * scale_info(food).1, "food {food}");
        }
    }

    #[test]
    fn background_colour_is_the_retail_key_rule() {
        // The installed CellBackgrounds keys 0 .. 5 (notes cell-data-dump).
        let map = vec![
            ([0.753, 0.898, 0.922], 0.0f32),
            ([0.639, 0.822, 1.0], 0.5),
            ([0.472, 0.644, 1.0], 1.5),
            ([0.222, 0.338, 0.706], 5.0),
        ];
        // The stage key of food 200..290 (level 10, w = 5): the key itself is the
        // upper bracket, t = 1 - 2^-16 / 3.5.
        let c = background_color(&map, 5.0).unwrap();
        assert!(c.abs_diff_eq(Vec3::new(0.222, 0.338, 0.706), 1e-4), "{c}");
        assert!(c.x > 0.222, "t stays just below 1 (+2^-16 in the span)");
        // Halfway between the 1.5 and 5 keys.
        let mid = background_color(&map, 3.25).unwrap();
        assert!(mid.abs_diff_eq(Vec3::new(0.347, 0.491, 0.853), 1e-3), "{mid}");
        // The stage start (w = 0.5): lower bracket 0, upper the 0.5 key.
        let start = background_color(&map, 0.5).unwrap();
        assert!(start.abs_diff_eq(Vec3::new(0.639, 0.822, 1.0), 1e-4), "{start}");
        // At or below the lowest key, and above the highest: magenta; empty: none.
        assert_eq!(background_color(&map, 0.0), Some(Vec3::new(1.0, 0.0, 1.0)));
        assert_eq!(background_color(&map, 5.5), Some(Vec3::new(1.0, 0.0, 1.0)));
        assert_eq!(background_color(&[], 1.0), None);
        // The order of the keys does not matter (every entry is scanned).
        let mut shuffled = map.clone();
        shuffled.reverse();
        assert_eq!(background_color(&shuffled, 3.25), Some(mid));
    }

    #[test]
    fn background_plane_wakes_and_call_effects_follow_the_retail_rules() {
        // The background effect plane is the depth of an elevation-0 background cell.
        let y1 = background_plane_y(35.0, 1.0);
        assert!(y1 < 0.0);
        assert!((y1 + 35.0 * crate::cell_engine::UNIT * crate::cell_population::BG_DEPTH_SCALE).abs() < 1e-4);
        // Zooming in lowers the camera: the plane follows (the viewer is unzoomed).
        assert!(background_plane_y(35.0, 0.5) < y1);
        // 0xE639D0: emit = clamp(10 * strength, 0, 99); a negative strength gives 0.
        assert_eq!(wake_emit(-1.0, 1.0 / 30.0), 0.0);
        assert!((wake_emit(0.5, 1.0 / 30.0) - 5.0).abs() < 1e-4);
        assert_eq!(wake_emit(100.0, 1.0 / 30.0), 99.0);
        // 0xE63BF0: the avatar plays cell_call_avatar, every other cell cell_call_mate.
        assert_eq!(call_effect(true), crate::world_objects::name_id("cell_call_avatar"));
        assert_eq!(call_effect(false), crate::world_objects::name_id("cell_call_mate"));
    }

    #[test]
    fn maps_parse_and_background_interpolates() {
        let mut b = vec![];
        b.extend(2u32.to_le_bytes());
        b.extend(8u32.to_le_bytes());
        for (rgb, key) in [([0.0f32, 0.0, 1.0], 5.0f32), ([1.0, 1.0, 1.0], 0.0)] {
            for c in rgb {
                b.extend(c.to_le_bytes());
            }
            b.extend(key.to_le_bytes());
        }
        let map = parse_background_map(&b).unwrap();
        assert_eq!(map[0].1, 0.0, "sorted by key");
        let c = background_color(&map, 2.5).unwrap();
        assert!(c.abs_diff_eq(Vec3::new(0.5, 0.5, 1.0), 1e-4));
        // Outside the keys 0xE54D30 returns magenta.
        assert_eq!(background_color(&map, 99.0).unwrap(), Vec3::new(1.0, 0.0, 1.0));
        let mut e = vec![];
        e.extend(1u32.to_le_bytes());
        e.extend(8u32.to_le_bytes());
        e.extend(0x8133_A66Cu32.to_le_bytes());
        e.extend(2u32.to_le_bytes());
        for f in [0.42f32, 0.7, 1.6, 1.95] {
            e.extend(f.to_le_bytes());
        }
        e.extend(0u32.to_le_bytes());
        let entries = parse_effect_map(&e).unwrap();
        assert_eq!(entries[0].effect, 0x8133_A66C);
        assert_eq!(entries[0].kind, 2);
        assert_eq!(entries[0].fade, [0.42, 0.7, 1.6, 1.95]);
        assert!(parse_effect_map(&e[..20]).is_none());
    }
}
