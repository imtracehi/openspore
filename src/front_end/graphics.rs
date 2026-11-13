//! Settings > Graphics: the seven Low / Medium / High quality rows and what
//! each one does in OpenSpore.
//!
//! VERIFIED (installed data, `notes\front-end-notes.md` section 7):
//! * The rows of `GameSettingsGraphics` are windows whose control ids are the
//!   option properties of `Data\Config\Properties.txt` (OptionShadows
//!   0461709E, OptionTextureDetail 0461709F, OptionEffects 046170A0,
//!   OptionBakeQuality 046170A3 "Player Model Quality", OptionLighting
//!   046170A6, OptionPlanetQuality 046170A7, OptionDOF 4C0DD839); each row's
//!   radio buttons (group = the row id) carry the commands 1 / 2 / 3 = the
//!   config levels `$Low` / `$Medium` / `$High`.
//! * What a level sets is the config script `Options.txt` (Spore_Game.package
//!   `00000000!F6D4A4B9.024A0E52`): Shadows Low = `ShadowWorlds false` (blob
//!   shadows), Medium = world shadows without `ShadowNestedMaps`, High =
//!   nested maps; Texture Detail Low = `TMMIPLOD 1` (drop a mip level), High =
//!   `TMAnisotropic`; Effects = `EffectsLevel` 0 / 1 / 2 (particle targets
//!   20000 / 50000 / 100000); Planet Quality = `terrainLODThreshold` 300 / 600
//!   / 1200 (plus water / weather switches); DOF Low = `DOFOn false`; Player
//!   Model Quality = skin-paint ambient occlusion and the bake templates;
//!   Lighting = shader path limit and lighting bands.
//! * Defaults: `ConfigManager.txt` starts at Shadows High, Texture Medium,
//!   Effects / Player Model / Lighting / Planet High, DOF Low and then rates
//!   the machine (`cpuLevel` from the CPU's clock and core count, `gpuLevel`
//!   from the `VideoCards.txt` table of 2008 adapters); a high CPU with a high
//!   GPU keeps them and raises DOF to High. [`rated_defaults`] is the script's
//!   rule (lines 592 .. 723) for any rating; the rating itself stays ADAPTED
//!   (see [`MACHINE_CPU`]).
//! * Lighting Quality: the material manager (0x71264E ..) reads `ShaderPath`
//!   (0x1A34E253) and `shaderPathLimit` (0xE1408308) and uses
//!   `min(ShaderPath, shaderPathLimit)` when the limit is not negative: Low
//!   limits to the 2_m path (0), Medium to 2_x (2), High sets none. The readers
//!   of `lightingManagerBands`, `DofBlurQuality`, `DOFOn` and the bake options
//!   are not in the image by name or hash (0 hits): the config script sets them
//!   through the property map and the reading code was not traced.
//!
//! OpenSpore honours every row (front-end-4, notes section 8a):
//! * Shadows, Planet Quality: live.
//! * Texture Detail: live; the default sampler is rebuilt, the GPU images
//!   that used the old one get the new one and the standard materials are
//!   prepared again so their bind groups pick it up.
//! * Graphics Effects: the effect tier, read when the effect library loads
//!   (next start; the effect renderer caches looks per directory entry).
//! * Player Model Quality: ADAPTED, the creature skin resolution (Low = the
//!   creator's realtime resolution `editorSkinResolutionRealtime` 0.042,
//!   Medium / High = `editorSkinResolutionHighQuality` 0.032; the bake
//!   templates `@creature!Low..High` are not installed as named props).
//! * Lighting Quality: Low = `ExcludeSecondaryTextures` (the standard
//!   materials draw without their normal maps; live); Medium = High
//!   (ADAPTED: OpenSpore has no shader path limit / lighting bands).
//! * Depth of Field: Low = `DOFOn false` = the Cell background blur off
//!   (INFERRED: the only depth-of-field pass OpenSpore runs is the Cell
//!   chain); Medium / High = on (ADAPTED: one blur quality). Applied outside
//!   the Cell stage and when it starts.

use std::sync::atomic::{AtomicU8, Ordering};

use bevy::{
    image::ImageSamplerDescriptor,
    light::{CascadeShadowConfig, CascadeShadowConfigBuilder},
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_resource::{DefaultImageSampler, DefaultImageSamplerDescriptor},
        texture::GpuImage,
    },
};

/// The rows in layout order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Shadows,
    Texture,
    Effects,
    Model,
    Lighting,
    Planet,
    Dof,
}

pub const ROWS: [Row; 7] = [Row::Shadows, Row::Texture, Row::Effects, Row::Model, Row::Lighting, Row::Planet, Row::Dof];

/// Row window ids = the option property ids (Properties.txt, VERIFIED).
pub const ROW_IDS: [u32; 7] = [0x0461_709E, 0x0461_709F, 0x0461_70A0, 0x0461_70A3, 0x0461_70A6, 0x0461_70A7, 0x4C0D_D839];

/// Hardware levels of `ConfigManager.txt` (`cpuLevelLow` .. `gpuLevelHigh`).
#[allow(dead_code)]
pub const LEVEL_LOW: u8 = 1;
#[allow(dead_code)]
pub const LEVEL_MEDIUM: u8 = 2;
pub const LEVEL_HIGH: u8 = 3;

/// The machine's rating. ADAPTED: the retail rating is the CPU clock against
/// per-vendor cut-offs (2400 MHz and up is High on a multi-core Intel, 1900 on
/// AMD, `ConfigManager.txt` 237 .. 241) and the adapter's name in the 2008
/// `VideoCards.txt` table (an adapter missing from it is Medium); every machine
/// that runs OpenSpore's renderer is far beyond both, so OpenSpore takes the
/// top rating for both.
pub const MACHINE_CPU: u8 = LEVEL_HIGH;
pub const MACHINE_GPU: u8 = LEVEL_HIGH;

/// The option levels `ConfigManager.txt` sets for a machine rated `cpu` and
/// `gpu` (1 low .. 3 high): the rows of [`ROWS`] as level indices (0 Low .. 2
/// High) and the Sound Quality level. VERIFIED, from the script after its
/// initial defaults (Shadows High, Texture Medium, the rest High, DOF Low,
/// audio Medium):
/// * Effects, Player Model, Planet Quality and Audio Performance = `cpu`
///   (592 .. 596); Shadows = `min(cpu, gpu)` (654 .. 657); Lighting = `gpu`, Low on a
///   low CPU (678 .. 683); Texture Detail Low on a low GPU (639); DOF High when
///   both are high (647);
/// * a medium GPU with a high CPU lowers Planet Quality and Effects to Medium
///   (702 .. 703); a low GPU sets Effects, Player Model and Planet to Low (711 ..).
pub const fn rated_defaults(cpu: u8, gpu: u8) -> ([u8; 7], u8) {
    let c = if cpu < 1 {
        0
    } else if cpu > 3 {
        2
    } else {
        cpu - 1
    };
    let g = if gpu < 1 {
        0
    } else if gpu > 3 {
        2
    } else {
        gpu - 1
    };
    let mut effects = c;
    let mut model = c;
    let mut planet = c;
    if g == 1 && c == 2 {
        planet = 1;
        effects = 1;
    } else if g == 0 {
        effects = 0;
        model = 0;
        planet = 0;
    }
    let shadows = if c < g { c } else { g };
    let texture = if g == 0 { 0 } else { 1 };
    let lighting = if c == 0 { 0 } else { g };
    let dof = if g == 2 && c == 2 { 2 } else { 0 };
    ([shadows, texture, effects, model, lighting, planet, dof], c)
}

/// Level index (0 Low, 1 Medium, 2 High) of each row after
/// `ConfigManager.txt` on the machine rating above.
pub const DEFAULTS: [u8; 7] = rated_defaults(MACHINE_CPU, MACHINE_GPU).0;

/// Sound Quality after `ConfigManager.txt` (`OptionAudioPerformance` = `cpuLevel`).
pub const AUDIO_PERFORMANCE_DEFAULT: u8 = rated_defaults(MACHINE_CPU, MACHINE_GPU).1;

/// Rows OpenSpore can apply (all of them since front-end-4).
pub const HONOURED: [bool; 7] = [true; 7];

impl Row {
    pub fn index(self) -> usize {
        ROWS.iter().position(|r| *r == self).unwrap_or(0)
    }
}

/// The row and level of a quality radio button: its parent row window's
/// control id and its command (1..3).
pub fn button_level(parent_control: u32, command: u32) -> Option<(usize, u8)> {
    let row = ROW_IDS.iter().position(|id| *id == parent_control)?;
    (1..=3).contains(&command).then(|| (row, (command - 1) as u8))
}

static TEXTURE: AtomicU8 = AtomicU8::new(DEFAULTS[1]);
static EFFECTS: AtomicU8 = AtomicU8::new(DEFAULTS[2]);
static MODEL: AtomicU8 = AtomicU8::new(DEFAULTS[3]);
static LIGHTING: AtomicU8 = AtomicU8::new(DEFAULTS[4]);
static PLANET: AtomicU8 = AtomicU8::new(DEFAULTS[5]);
static DOF: AtomicU8 = AtomicU8::new(DEFAULTS[6]);

/// Make the levels other modules read live (`UserSettings::publish`).
pub fn publish(levels: [u8; 7]) {
    TEXTURE.store(levels[Row::Texture.index()].min(2), Ordering::Relaxed);
    EFFECTS.store(levels[Row::Effects.index()].min(2), Ordering::Relaxed);
    MODEL.store(levels[Row::Model.index()].min(2), Ordering::Relaxed);
    LIGHTING.store(levels[Row::Lighting.index()].min(2), Ordering::Relaxed);
    PLANET.store(levels[Row::Planet.index()].min(2), Ordering::Relaxed);
    DOF.store(levels[Row::Dof.index()].min(2), Ordering::Relaxed);
}

/// Player Model Quality as the creature skin resolution (smaller = finer):
/// Low builds stage creatures at the creator's realtime resolution.
/// ADAPTED (see the module notes).
pub fn skin_resolution() -> f32 {
    skin_resolution_for(MODEL.load(Ordering::Relaxed))
}

pub fn skin_resolution_for(level: u8) -> f32 {
    if level == 0 {
        crate::creature_skin::RES_REALTIME
    } else {
        crate::creature_skin::RES_HIGH_QUALITY
    }
}

/// Lighting Quality: false at Low (`ExcludeSecondaryTextures`).
pub fn secondary_textures() -> bool {
    LIGHTING.load(Ordering::Relaxed) > 0
}

/// Depth of Field: false at Low (`DOFOn false`).
pub fn dof_on() -> bool {
    DOF.load(Ordering::Relaxed) > 0
}

/// Main-world side of the live rows: Lighting Quality strips / restores the
/// standard materials' normal maps; after a Texture Detail change the
/// standard materials are prepared again (their bind groups hold each
/// texture's sampler, which the render world swapped).
#[derive(Default)]
pub struct LiveRows {
    texture: Option<u8>,
    secondary: Option<bool>,
    stash: std::collections::HashMap<AssetId<StandardMaterial>, Handle<Image>>,
}

pub fn apply_live(
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    events: Option<Res<bevy::ecs::message::Messages<AssetEvent<StandardMaterial>>>>,
    mut cursor: Local<bevy::ecs::message::MessageCursor<AssetEvent<StandardMaterial>>>,
    mut live: Local<LiveRows>,
) {
    // Headless apps may run without the PBR assets.
    let (Some(mut materials), Some(events)) = (materials, events) else { return };
    let added: Vec<AssetId<StandardMaterial>> = cursor
        .read(&events)
        .filter_map(|e| match e {
            AssetEvent::Added { id } => Some(*id),
            _ => None,
        })
        .collect();
    let secondary = secondary_textures();
    if live.secondary != Some(secondary) {
        let first = live.secondary.is_none();
        live.secondary = Some(secondary);
        if !secondary {
            let ids: Vec<_> = materials.ids().collect();
            for id in ids {
                strip(&mut materials, &mut live.stash, id);
            }
        } else if !first {
            for (id, normal) in std::mem::take(&mut live.stash) {
                if let Some(mut m) = materials.get_mut(id) {
                    m.normal_map_texture = Some(normal);
                }
            }
        }
    } else if !secondary {
        for id in added {
            strip(&mut materials, &mut live.stash, id);
        }
    }
    let texture = TEXTURE.load(Ordering::Relaxed);
    match live.texture {
        Some(old) if old != texture => {
            let ids: Vec<_> = materials.ids().collect();
            for id in ids {
                if let Some(m) = materials.get_mut(id) {
                    let _ = m.into_inner();
                }
            }
            live.texture = Some(texture);
        }
        None => live.texture = Some(texture),
        _ => {}
    }
}

fn strip(
    materials: &mut Assets<StandardMaterial>,
    stash: &mut std::collections::HashMap<AssetId<StandardMaterial>, Handle<Image>>,
    id: AssetId<StandardMaterial>,
) {
    if materials.get(id).is_some_and(|m| m.normal_map_texture.is_some()) {
        if let Some(normal) = materials.get_mut(id).and_then(|mut m| m.normal_map_texture.take()) {
            stash.insert(id, normal);
        }
    }
}

/// Depth of Field: the Cell background blur follows the row outside the Cell
/// stage and before its background pass exists (objects already moved to the
/// background pass would vanish if the pass stopped under them).
pub fn apply_dof(
    stage: Res<State<crate::stages::GameStage>>,
    blur: Option<ResMut<crate::cell_fx::CellBackgroundBlur>>,
    background: Query<(), With<crate::cell_fx::CellBackgroundCamera>>,
) {
    let Some(mut blur) = blur else { return };
    let on = dof_on() && crate::world_presentation::tune("CELL_BLUR", 1.0) > 0.5;
    let free = *stage.get() != crate::stages::GameStage::Cell || background.is_empty();
    if free && blur.enabled != on {
        blur.enabled = on;
    }
}

/// Planet Quality: `terrainLODThreshold` 300 / 600 / 1200 relative to High
/// (`planet_render::select_nodes` splits nodes within that many times its
/// split distance).
pub fn planet_lod_scale() -> f32 {
    [0.25, 0.5, 1.0][PLANET.load(Ordering::Relaxed).min(2) as usize]
}

/// The effect quality tiers `gameEffects_1~` / `_2~` / `_3~` to try, the
/// chosen Graphics Effects level first. INFERRED: `EffectsLevel` 0 / 1 / 2
/// selects tier 1 / 2 / 3 (the tier names; the loader that reads
/// EffectsLevel is not traced). Read when the effect library loads (start).
pub fn effect_groups() -> [u32; 3] {
    use crate::swarm_fx::format::EFFECT_GROUPS as G; // [_3, _2, _1]
    match EFFECTS.load(Ordering::Relaxed) {
        0 => [G[2], G[1], G[0]],
        1 => [G[1], G[0], G[2]],
        _ => [G[0], G[1], G[2]],
    }
}

/// Texture Detail as a sampler: Low drops the top mip level (`TMMIPLOD 1`),
/// Medium is the plain trilinear sampler, High adds anisotropic filtering.
pub fn texture_sampler(base: &ImageSamplerDescriptor, level: u8) -> ImageSamplerDescriptor {
    let mut d = base.clone();
    match level {
        0 => d.lod_min_clamp = d.lod_min_clamp.max(1.0),
        2 => {
            d.set_anisotropic_filter(16);
        }
        _ => {}
    }
    d
}

/// Render world: the level the default sampler was built for and the
/// original descriptor.
#[derive(Resource, Default)]
pub struct RenderTextureDetail {
    applied: Option<u8>,
    base: Option<ImageSamplerDescriptor>,
}

/// Rebuild the default image sampler when Texture Detail changes. Runs
/// before the render world prepares new images, so textures loaded from now
/// on use it; the images already on the GPU that used the old default
/// sampler are given the new one (live).
pub fn apply_texture_detail(world: &mut World) {
    let level = TEXTURE.load(Ordering::Relaxed);
    let Some(current) = world.get_resource::<DefaultImageSamplerDescriptor>().map(|d| d.0.clone()) else { return };
    let mut state = world.remove_resource::<RenderTextureDetail>().unwrap_or_default();
    if state.applied != Some(level) {
        let base = state.base.get_or_insert(current).clone();
        let old = world.get_resource::<DefaultImageSampler>().map(|s| (**s).clone());
        world.insert_resource(DefaultImageSamplerDescriptor(texture_sampler(&base, level)));
        let sampler = DefaultImageSampler::from_world(world);
        // Live: the images already on the GPU that use the default sampler
        // get the new one (`apply_live` prepares the materials again).
        let new = (*sampler).clone();
        if let (Some(old), Some(mut gpu)) = (old, world.get_resource_mut::<RenderAssets<GpuImage>>()) {
            for (_, image) in gpu.iter_mut() {
                if image.sampler.id() == old.id() {
                    image.sampler = new.clone();
                }
            }
        }
        world.insert_resource(sampler);
        state.applied = Some(level);
    }
    world.insert_resource(state);
}

/// The stage's cascades kept while Medium shadows use one cascade.
#[derive(Component)]
pub struct CascadesBackup(pub CascadeShadowConfig);

/// One cascade over the same range (`ShadowNestedMaps false`).
pub fn single_cascade(config: &CascadeShadowConfig) -> CascadeShadowConfig {
    let far = config.bounds.last().copied().unwrap_or(100.0);
    CascadeShadowConfigBuilder {
        num_cascades: 1,
        minimum_distance: config.minimum_distance,
        maximum_distance: far,
        first_cascade_far_bound: far,
        overlap_proportion: config.overlap_proportion,
    }
    .build()
}

/// Shadows: Low switches the sun's shadow maps off, Medium draws them with a
/// single cascade, High with the stage's nested cascades.
pub fn apply_shadows(
    commands: &mut Commands,
    level: u8,
    changed: bool,
    suns: &mut Query<(Entity, &mut DirectionalLight, Option<&mut CascadeShadowConfig>, Option<&CascadesBackup>), With<crate::GameSun>>,
) {
    for (entity, mut sun, config, backup) in suns.iter_mut() {
        if level == 0 && sun.shadow_maps_enabled {
            sun.shadow_maps_enabled = false;
        } else if level > 0 && changed && !sun.shadow_maps_enabled {
            sun.shadow_maps_enabled = true;
        }
        let Some(mut config) = config else { continue };
        match (level, backup) {
            (1, _) if config.bounds.len() > 1 => {
                commands.entity(entity).insert(CascadesBackup(config.clone()));
                *config = single_cascade(&config);
            }
            (2, Some(backup)) => {
                // The stage may have replaced the cascades meanwhile.
                if config.bounds.len() == 1 {
                    *config = backup.0.clone();
                }
                commands.entity(entity).remove::<CascadesBackup>();
            }
            _ => {}
        }
    }
}

/// Screen sizes the Screen Size button cycles through when no monitor reports
/// its modes (offscreen runs); otherwise [`screen_sizes`] of the monitors'
/// video modes. ADAPTED: retail lists the adapter's display modes in a
/// drop-down (`OptionScreenSize`, "populated in code: screen resolution and
/// refresh rate"; the default target is 1024x768 or 800x600, `ConfigManager.txt`
/// 627 .. 631).
pub const SCREEN_SIZES: [[u32; 2]; 8] = [
    [1024, 768],
    [1280, 720],
    [1280, 800],
    [1280, 900],
    [1366, 768],
    [1600, 900],
    [1920, 1080],
    [2560, 1440],
];

/// The smallest mode the game offers (the `resolutionTarget` of a low GPU,
/// `ConfigManager.txt` 629: 800 x 600).
pub const MIN_SCREEN_SIZE: [u32; 2] = [800, 600];

/// The distinct sizes of a monitor's video modes, smallest first, from the
/// retail minimum up (the refresh rates collapse into one entry).
pub fn screen_sizes(modes: impl IntoIterator<Item = [u32; 2]>) -> Vec<[u32; 2]> {
    let mut sizes: Vec<[u32; 2]> = modes.into_iter().filter(|s| s[0] >= MIN_SCREEN_SIZE[0] && s[1] >= MIN_SCREEN_SIZE[1]).collect();
    sizes.sort_by_key(|s| (s[0] * s[1], s[0]));
    sizes.dedup();
    sizes
}

/// The size after `current` in `sizes` ([`SCREEN_SIZES`] when empty).
pub fn next_screen_size_in(sizes: &[[u32; 2]], current: [u32; 2]) -> [u32; 2] {
    let sizes = if sizes.is_empty() { &SCREEN_SIZES[..] } else { sizes };
    match sizes.iter().position(|s| *s == current) {
        Some(i) => sizes[(i + 1) % sizes.len()],
        None => sizes.iter().copied().find(|s| s[0] * s[1] > current[0] * current[1]).unwrap_or(sizes[0]),
    }
}

/// The size after `current` in [`SCREEN_SIZES`].
#[allow(dead_code)]
pub fn next_screen_size(current: [u32; 2]) -> [u32; 2] {
    next_screen_size_in(&SCREEN_SIZES, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_levels_come_from_the_option_ids_and_commands() {
        assert_eq!(button_level(0x0461_709E, 1), Some((0, 0)));
        assert_eq!(button_level(0x0461_70A7, 3), Some((5, 2)));
        assert_eq!(button_level(0x4C0D_D839, 2), Some((6, 1)));
        assert_eq!(button_level(0x0461_70A1, 1), None, "the screen-size button is not a row");
        assert_eq!(button_level(0x0461_709F, 0), None);
        assert_eq!(ROWS.len(), ROW_IDS.len());
    }

    #[test]
    fn medium_shadows_keep_the_range_with_one_cascade() {
        let three = CascadeShadowConfigBuilder {
            num_cascades: 3,
            minimum_distance: 0.1,
            first_cascade_far_bound: 18.0,
            maximum_distance: 140.0,
            overlap_proportion: 0.2,
        }
        .build();
        let one = single_cascade(&three);
        assert_eq!(one.bounds.len(), 1);
        assert_eq!(one.bounds[0], 140.0);
    }

    #[test]
    fn texture_detail_levels_change_the_sampler() {
        let base = ImageSamplerDescriptor::linear();
        assert_eq!(texture_sampler(&base, 0).lod_min_clamp, 1.0);
        assert_eq!(texture_sampler(&base, 1), base);
        assert_eq!(texture_sampler(&base, 2).anisotropy_clamp, 16);
    }

    #[test]
    fn every_row_is_honoured_with_its_mapping() {
        assert!(HONOURED.iter().all(|h| *h));
        assert!(skin_resolution_for(0) > skin_resolution_for(1), "Low builds a coarser skin");
        assert_eq!(skin_resolution_for(1), skin_resolution_for(2));
    }

    #[test]
    fn config_manager_rule_gives_the_documented_defaults() {
        // The top machine keeps the initial defaults and raises DOF; audio = cpuLevel.
        assert_eq!(DEFAULTS, [2, 1, 2, 2, 2, 2, 2]);
        assert_eq!(AUDIO_PERFORMANCE_DEFAULT, 2);
        // A medium GPU with a high CPU: shadows = min, Lighting = gpu, Effects /
        // Planet Medium, no DOF.
        assert_eq!(rated_defaults(LEVEL_HIGH, LEVEL_MEDIUM), ([1, 1, 1, 2, 1, 1, 0], 2));
        // A low GPU: Texture Low and everything the GPU drives Low.
        assert_eq!(rated_defaults(LEVEL_HIGH, LEVEL_LOW), ([0, 0, 0, 0, 0, 0, 0], 2));
        // A low CPU: Lighting Low (the 2_m path), audio Low.
        assert_eq!(rated_defaults(LEVEL_LOW, LEVEL_HIGH), ([0, 1, 0, 0, 0, 0, 0], 0));
        assert_eq!(rated_defaults(LEVEL_MEDIUM, LEVEL_HIGH), ([1, 1, 1, 1, 2, 1, 0], 1));
    }

    #[test]
    fn monitor_modes_make_the_screen_size_list() {
        let modes = [[1920, 1080], [640, 480], [1280, 720], [1920, 1080], [1024, 768], [800, 600], [2560, 1440]];
        let sizes = screen_sizes(modes);
        assert_eq!(sizes, vec![[800, 600], [1024, 768], [1280, 720], [1920, 1080], [2560, 1440]]);
        assert_eq!(next_screen_size_in(&sizes, [1280, 720]), [1920, 1080]);
        assert_eq!(next_screen_size_in(&sizes, [2560, 1440]), [800, 600]);
        assert_eq!(next_screen_size_in(&sizes, [1100, 800]), [1280, 720]);
        // No monitor: the built-in list.
        assert_eq!(next_screen_size_in(&[], [1280, 900]), [1366, 768]);
    }

    #[test]
    fn screen_sizes_cycle() {
        assert_eq!(next_screen_size([1280, 900]), [1366, 768]);
        assert_eq!(next_screen_size([2560, 1440]), [1024, 768]);
        assert_eq!(next_screen_size([1100, 800]), [1280, 720]);
    }
}
