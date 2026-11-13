//! Spherical-harmonics lighting of the Cell Creator (build and paint).
//!
//! Retail lights every editor model with the 25 SH coefficients of its
//! `lighting_properties~` PROP (`CellEditor`, `CellEditorPaintMode`,
//! `shCoeffs` 0x0100EAC5) and an exposure (0x0100EAC4); the editor shader is
//! the pixel fragment `shLightingSpec_CellEditorsNMap` (Spore_Graphics
//! 0x40212002 instance 3, PS 25; the setup fragment `setupSHCellEditors`).
//!
//! VERIFIED (code image, `FUN_0070AF40` the light loader):
//! * The coefficient array is read as RGB, widened to RGBA and run through
//!   `0x7838E0`, which divides coefficient i by the real-SH normalisation
//!   constant K_i (the globals 0x1634xxx are initialised by `fld pi; fmul 4 / 16;
//!   fdivr C; fsqrt` chains: K = sqrt(C / (4 pi)) ... = 0.282095, 0.488603,
//!   1.092548, 0.315392, 0.546274, 0.590044, 2.890611, ...). The summed
//!   components are put back through `0x783640` (multiply by the same K_i), so
//!   for a light made of the coefficient array alone the shader coefficient is
//!   the PROP value itself.
//! * Every coefficient is then multiplied by `exposure` (loader tail, 0x100EAC4).
//! * `sun` / `sky` / `fill` slots only fill direction, colour and strength;
//!   the editor fragment reads `dirLightsWorld[0]` for the specular highlight
//!   only (the sun adds no diffuse light), so Paint (no `lightSun*`, exposure
//!   0.28) differs from Build only by the missing highlight.
//! * The fragment (ported to `shaders/cell_sh.wesl`): diffuse = c0 + A1 * band 1
//!   + A2 * band 2 at the (normal-mapped) normal with A1 = 2/3, A2 = 1/4 (A3 =
//!   0: band 3 is not in the diffuse); glossy = bands 0..3 unconvolved at the
//!   reflected normal; colour = base * (diffuse * (1 - gloss) + glossy * gloss)
//!   + pow(saturate(R . sunDir), specE) * specStrength * sunColour.
//!
//! `materialParams` (shader datum 0x210, five words) is decoded since cell-light2:
//! word 0 is the uniform slot header (0x26445Cxx in every installed material),
//! words 1..4 are the float4 the fragment setups read (`setupSHCellEditors`,
//! `setupSHBakedCreaturesNMap`): x specE, y specStrength, z the bump scale
//! (`nmapNormal.z *= z`), w gloss; specStrength and gloss are multiplied by the
//! alpha of the spec / bump map (`specNMap.a`). See [`MaterialParams`].
//!
//! The Cell *stage* is lit the same way: the `CellGame` light (the same 25
//! coefficients, exposure 0.30, sun strength 2.0) drives the baked creature
//! fragment `shLightingSpec_BakedCreaturesNormalMap` (the editor fragment plus
//! the cel rim and the shadow adjust of the specular).
//!
//! ADAPTED: materials without a `materialParams` datum (the editor's part and
//! pedestal models, the stage's RW4 food / rock / flora states) keep the
//! constants `GLOSS` / `SPEC_EXPONENT` / `SPEC_STRENGTH` (see below); the sun colour is taken as colour
//! times strength; the reflection vector uses the camera's forward axis
//! (`worldCameraDirection`) and the vertex part of the glossy term is
//! evaluated per pixel; the Spore -> Bevy axis map (x, y, z) -> (-x, z, y) is the
//! proper rotation taking the model's front -Y to the scene's -Z and up Z to Y
//! (it puts the sun highlight on the upper-left of the body like the retail
//! frame; the earlier (x, z, -y) put it on the lower edge).

use crate::cell_editor_data::{CellEditorData, EditorLight};
use crate::cell_editor_scene::{CellEditorOwned, CAMERA_OFFSET};
use bevy::{
    asset::embedded_asset,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};
use std::collections::HashMap;

/// Coefficients the editor fragment uses (`shCoeffs[16]`, bands 0..3).
pub const COEFFS: usize = 16;
/// Retail fragment constants (`kSH_A1`, `kSH_A2`; `kSH_A3` = 0).
pub const SH_A1: f32 = 2.0 / 3.0;
pub const SH_A2: f32 = 0.25;
/// ADAPTED: the materials without a `materialParams` datum (the editor's part
/// and pedestal models, the stage's RW4 food / rock states; every cell GMDL
/// carries one, decoded by [`MaterialParams`]) take these. The values follow the installed skin-paint
/// material PROPs (`paints~` 0x406A0200, `paintMaterialSpecExponent` /
/// `Glossiness` / `SpecStrength`: specE 8, gloss 0.0..0.1, strength 0.2..0.8 on
/// the base coats) and the retail Build frame (soft sun highlight on the
/// upper-left edge of the body, none in Paint mode, body otherwise diffuse).
/// `OS_CELL_SH_GLOSS` / `_SPEC_E` / `_SPEC_STRENGTH` tune them for captures.
pub const GLOSS: f32 = 0.1;
pub const SPEC_EXPONENT: f32 = 8.0;
pub const SPEC_STRENGTH: f32 = 0.35;

/// Decoded `materialParams` (shader datum 0x210) of a model material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialParams {
    pub spec_e: f32,
    pub spec_strength: f32,
    /// `materialParams.z`: multiplies the z of the normal map's normal (larger =
    /// flatter bump). 1.0 in every installed cell material.
    pub bump: f32,
    pub gloss: f32,
}

impl MaterialParams {
    /// The constants of the materials that carry no datum (ADAPTED, see `GLOSS`).
    pub fn fallback() -> Self {
        Self {
            spec_e: crate::world_presentation::tune("CELL_SH_SPEC_E", SPEC_EXPONENT),
            spec_strength: crate::world_presentation::tune("CELL_SH_SPEC_STRENGTH", SPEC_STRENGTH),
            bump: 1.0,
            gloss: crate::world_presentation::tune("CELL_SH_GLOSS", GLOSS),
        }
    }

    /// The five words of the datum: word 0 is the slot header, words 1..4 the
    /// float4 (specE, specStrength, bump, gloss). `None` when they are not
    /// finite numbers.
    pub fn from_words(words: [u32; 5]) -> Option<Self> {
        let f = [1, 2, 3, 4].map(|i| f32::from_bits(words[i]));
        f.iter().all(|v| v.is_finite()).then(|| Self { spec_e: f[0], spec_strength: f[1], bump: f[2], gloss: f[3] })
    }
}

static PARAMS: std::sync::Mutex<Option<HashMap<AssetId<StandardMaterial>, MaterialParams>>> = std::sync::Mutex::new(None);

/// Remember the `materialParams` of a freshly imported model material (called
/// by `baked_model::import`; the SH materials look them up by source material).
pub fn register_params(id: AssetId<StandardMaterial>, words: Option<[u32; 5]>) {
    if let (Some(words), Ok(mut map)) = (words, PARAMS.lock()) {
        if let Some(p) = MaterialParams::from_words(words) {
            map.get_or_insert_with(HashMap::new).insert(id, p);
        }
    }
}

/// Replace the `materialParams` of a material (the painted Cell body: its
/// settings come from the brush script, `cell_skinpaint`).
pub fn set_params(id: AssetId<StandardMaterial>, params: MaterialParams) {
    if let Ok(mut map) = PARAMS.lock() {
        map.get_or_insert_with(HashMap::new).insert(id, params);
    }
}

/// A copy of a registered material (the editor draws gamma-space copies).
pub fn copy_params(from: AssetId<StandardMaterial>, to: AssetId<StandardMaterial>) {
    if let Ok(mut map) = PARAMS.lock() {
        if let Some(p) = map.as_ref().and_then(|m| m.get(&from)).copied() {
            map.get_or_insert_with(HashMap::new).insert(to, p);
        }
    }
}

pub fn params_of(id: AssetId<StandardMaterial>) -> Option<MaterialParams> {
    PARAMS.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(&id).copied()))
}

/// One editor light in the shader's terms (Spore space, Z up).
#[derive(Clone, Debug, PartialEq)]
pub struct ShLight {
    /// `shCoeffs[i]`: the PROP coefficient times the exposure.
    pub coeffs: [[f32; 3]; COEFFS],
    /// Direction towards the sun, normalised.
    pub sun_dir: [f32; 3],
    /// Sun colour times strength (zero when the PROP has no `lightSun*`).
    pub sun_color: [f32; 3],
}

impl ShLight {
    /// The light of a decoded editor light PROP (needs the 16 coefficients the
    /// editor fragment reads).
    pub fn from_light(light: &EditorLight) -> Option<Self> {
        if light.sh.len() < COEFFS {
            return None;
        }
        let mut coeffs = [[0.0; 3]; COEFFS];
        for (out, c) in coeffs.iter_mut().zip(&light.sh) {
            *out = c.map(|v| v * light.exposure);
        }
        let d = light.sun_dir;
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2] + 1e-8).sqrt();
        Some(Self {
            coeffs,
            sun_dir: d.map(|v| v / len),
            sun_color: light.sun_color.map(|v| v * light.sun_strength),
        })
    }

    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        let mut out = self.clone();
        for i in 0..COEFFS {
            for k in 0..3 {
                out.coeffs[i][k] = mix(self.coeffs[i][k], other.coeffs[i][k]);
            }
        }
        for k in 0..3 {
            out.sun_dir[k] = mix(self.sun_dir[k], other.sun_dir[k]);
            out.sun_color[k] = mix(self.sun_color[k], other.sun_color[k]);
        }
        out
    }

    fn band_0_1(&self, x: [f32; 3]) -> [f32; 3] {
        let c = &self.coeffs;
        std::array::from_fn(|k| c[1][k] * x[1] + c[2][k] * x[2] + c[3][k] * x[0])
    }

    fn band_2(&self, x: [f32; 3]) -> [f32; 3] {
        let c = &self.coeffs;
        let (x1, x2) = ([x[0] * x[0], x[1] * x[1], x[2] * x[2]], [x[0] * x[1], x[1] * x[2], x[2] * x[0]]);
        std::array::from_fn(|k| {
            c[4][k] * x2[0] + c[5][k] * x2[1] + c[6][k] * (3.0 * x1[2] - 1.0) + c[7][k] * x2[2] + c[8][k] * (x1[0] - x1[1])
        })
    }

    /// Coefficients 9..15 with the retail fragment's polynomials (index 10 is
    /// `x^2 z` in the fragment source).
    fn band_3(&self, x: [f32; 3]) -> [f32; 3] {
        let c = &self.coeffs;
        let x1 = [x[0] * x[0], x[1] * x[1], x[2] * x[2]];
        std::array::from_fn(|k| {
            c[9][k] * (3.0 * x1[0] - x1[1]) * x[1]
                + c[10][k] * x1[0] * x[2]
                + c[11][k] * (5.0 * x1[2] - 1.0) * x[1]
                + c[12][k] * (5.0 * x1[2] - 3.0) * x[2]
                + c[13][k] * (5.0 * x1[2] - 1.0) * x[0]
                + c[14][k] * (x1[0] - x1[1]) * x[2]
                + c[15][k] * (x1[0] - 3.0 * x1[1]) * x[0]
        })
    }

    /// Diffuse term at a unit normal (`shColorDiff`).
    pub fn diffuse(&self, n: [f32; 3]) -> [f32; 3] {
        let (b1, b2) = (self.band_0_1(n), self.band_2(n));
        std::array::from_fn(|k| self.coeffs[0][k] + b1[k] * SH_A1 + b2[k] * SH_A2)
    }

    /// Glossy term at a unit reflected direction (`shColorSpec`).
    pub fn glossy(&self, r: [f32; 3]) -> [f32; 3] {
        let (b1, b2, b3) = (self.band_0_1(r), self.band_2(r), self.band_3(r));
        std::array::from_fn(|k| self.coeffs[0][k] + b1[k] + b2[k] + b3[k])
    }

    /// Display-space colour of a surface of `base` (display values) with
    /// unit normal `n` seen along `forward` (`SHLightingNMap`).
    pub fn shade(&self, base: [f32; 3], n: [f32; 3], forward: [f32; 3], params: [f32; 3]) -> [f32; 3] {
        let d = forward[0] * n[0] + forward[1] * n[1] + forward[2] * n[2];
        let r: [f32; 3] = std::array::from_fn(|k| forward[k] - 2.0 * n[k] * d);
        let (diffuse, glossy) = (self.diffuse(n), self.glossy(r));
        let [gloss, spec_e, spec_strength] = params;
        let rl = (r[0] * self.sun_dir[0] + r[1] * self.sun_dir[1] + r[2] * self.sun_dir[2]).clamp(0.0, 1.0);
        let highlight = rl.powf(spec_e) * spec_strength;
        std::array::from_fn(|k| (base[k] * (diffuse[k] * (1.0 - gloss) + glossy[k] * gloss) + highlight * self.sun_color[k]).clamp(0.0, 1.0))
    }
}

/// Build and Paint lights of the open editor; `blend` 0 = Build, 1 = Paint
/// (the camera glide between the two modes drives it, like the old Bevy light).
#[derive(Resource, Default, Clone, Debug)]
pub struct ShRig {
    pub build: Option<ShLight>,
    pub paint: Option<ShLight>,
    pub blend: f32,
}

impl ShRig {
    pub fn from_data(data: &CellEditorData) -> Self {
        let build = data.light.as_ref().and_then(ShLight::from_light);
        let paint = data.paint_light.as_ref().and_then(ShLight::from_light).or_else(|| {
            // The Paint light is the Build light without its sun.
            build.clone().map(|mut l| {
                l.sun_color = [0.0; 3];
                l
            })
        });
        Self { build, paint, blend: 0.0 }
    }

    pub fn current(&self) -> Option<ShLight> {
        match (&self.build, &self.paint) {
            (Some(b), Some(p)) => Some(b.lerp(p, self.blend)),
            (Some(b), None) => Some(b.clone()),
            _ => None,
        }
    }
}

pub type ShMaterial = ExtendedMaterial<StandardMaterial, CellSh>;

/// Bits of `CellSh.params.w`.
/// The base colour already holds display (gamma) values: the stage's model
/// textures are uploaded as linear `Rgba8Unorm` (the bytes are the display
/// values), the editor's copies decode as sRGB.
const MODE_DISPLAY_BASE: f32 = 1.0;
/// The material belongs to the Cell stage (world axes of its camera, not the
/// Creator's).
const MODE_STAGE_AXES: f32 = 2.0;

/// Uniforms of `shaders/cell_sh.wesl`.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct CellSh {
    #[uniform(100)]
    pub coeffs: [Vec4; COEFFS],
    #[uniform(101)]
    pub sun: [Vec4; 2],
    /// x gloss, y specE, z specStrength, w mode bits.
    #[uniform(102)]
    pub params: Vec4,
    /// x bump scale (`materialParams.z`), y cel strength, z specular shadow
    /// adjust (`shadowAdjust`, 0.1 + 0.8 when lit).
    #[uniform(103)]
    pub extra: Vec4,
    /// The spec / bump map (its alpha scales specStrength and gloss).
    #[texture(104)]
    #[sampler(105)]
    pub spec: Option<Handle<Image>>,
    /// Drawn in the Cell stage (not driven by the editor's Build / Paint light).
    pub stage: bool,
}

/// Cel rim of the baked creature fragment (`sunDirAndCelStrength.w`).
// ADAPTED: the stage light PROP carries no cel strength that was found, so the
// rim is off; `OS_CELL_SH_CEL` turns it on for a comparison.
const STAGE_CEL: f32 = 0.0;
/// `shadowAdjust = 0.1 + 0.80 * (shadow > 0.80)` of the baked creature
/// fragment, with `shadow` 1 (no shadow map in the Cell stage; ADAPTED).
const STAGE_SHADOW_ADJUST: f32 = 0.9;

impl CellSh {
    pub fn new(light: &ShLight, params: MaterialParams, display_base: bool, stage: bool, spec: Option<Handle<Image>>) -> Self {
        let mut out = Self {
            coeffs: [Vec4::ZERO; COEFFS],
            sun: [Vec4::ZERO; 2],
            params: Vec4::ZERO,
            extra: Vec4::ZERO,
            spec,
            stage,
        };
        out.configure(params, display_base);
        out.set(light);
        out
    }

    /// The per-material part: `materialParams` and the colour space of the base.
    pub fn configure(&mut self, p: MaterialParams, display_base: bool) {
        let mode = if display_base { MODE_DISPLAY_BASE } else { 0.0 } + if self.stage { MODE_STAGE_AXES } else { 0.0 };
        self.params = Vec4::new(p.gloss, p.spec_e, p.spec_strength, mode);
        self.extra = if self.stage {
            Vec4::new(
                p.bump,
                crate::world_presentation::tune("CELL_SH_CEL", STAGE_CEL),
                STAGE_SHADOW_ADJUST,
                0.0,
            )
        } else {
            Vec4::new(p.bump, 0.0, 1.0, 0.0)
        };
    }

    pub fn set(&mut self, light: &ShLight) {
        for (u, c) in self.coeffs.iter_mut().zip(&light.coeffs) {
            *u = Vec4::new(c[0], c[1], c[2], 1.0);
        }
        self.sun[0] = Vec3::from(light.sun_dir).extend(0.0);
        self.sun[1] = Vec3::from(light.sun_color).extend(0.0);
    }
}

impl MaterialExtension for CellSh {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/cell_sh.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// The light of the Cell stage (`CellGame`, the same 25 coefficients as the
/// editor's, exposure 0.30, sun strength 2.0) and the switch of the stage swap
/// (`OS_CELL_STAGE_SH=0` keeps the Bevy light for a before / after comparison).
#[derive(Resource)]
pub struct StageSh {
    light: Option<ShLight>,
    loaded: bool,
    enabled: bool,
}

impl Default for StageSh {
    fn default() -> Self {
        Self { light: None, loaded: false, enabled: std::env::var("OS_CELL_STAGE_SH").map_or(true, |v| v != "0") }
    }
}

pub struct CellShPlugin;

impl Plugin for CellShPlugin {
    fn build(&self, app: &mut App) {
        // The headless unit-test harnesses add the Cell Creator without the
        // render stack (no `Assets<Shader>`, no material pipeline): nothing to
        // draw with there, and the editor keeps its `StandardMaterial`s.
        if !app.is_plugin_added::<bevy::pbr::PbrPlugin>() {
            return;
        }
        embedded_asset!(app, "shaders/cell_sh.wesl");
        crate::world_presentation::dev_shader_override(app, "cell_sh.wesl");
        app.add_plugins(MaterialPlugin::<ShMaterial>::default())
            .init_resource::<ShRig>()
            .init_resource::<StageSh>()
            .add_systems(
                PostUpdate,
                (swap_materials, drive_light)
                    .chain()
                    .run_if(crate::cell_editor::active)
                    .before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(Update, load_stage_light.run_if(|s: Res<StageSh>| s.enabled && !s.loaded))
            .add_systems(
                PostUpdate,
                swap_stage_materials
                    .run_if(stage_ready)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
        if std::env::var_os("OS_CELL_PERF").is_some() {
            app.add_systems(Last, frame_probe);
        }
    }
}

/// `OS_CELL_PERF=1`: print the wall time per frame (mean / median / worst of
/// every 120 frames; the capture loop renders every frame, so this includes
/// the GPU) and the SH material count.
fn frame_probe(
    lit: Res<Assets<ShMaterial>>,
    mut last: Local<Option<std::time::Instant>>,
    mut window: Local<Vec<f32>>,
    mut frames: Local<u32>,
) {
    let now = std::time::Instant::now();
    if let Some(before) = last.replace(now) {
        window.push(now.duration_since(before).as_secs_f32() * 1000.0);
    }
    *frames += 1;
    if window.len() >= 120 {
        let mean = window.iter().sum::<f32>() / window.len() as f32;
        let worst = window.iter().cloned().fold(0.0, f32::max);
        let mut sorted = window.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let median = sorted[sorted.len() / 2];
        eprintln!("CELLPERF frame {} mean {mean:.2} ms median {median:.2} ms worst {worst:.2} ms, {} SH materials", *frames, lit.len());
        window.clear();
    }
}

/// The swap runs in the Cell stage once the light is loaded (a system that
/// does not run keeps the changes of its query for the first run).
fn stage_ready(stage: Res<StageSh>, state: Option<Res<State<crate::stages::GameStage>>>) -> bool {
    stage.light.is_some() && state.is_some_and(|s| *s.get() == crate::stages::GameStage::Cell)
}

/// Read the `CellGame` light once the install index is there.
fn load_stage_light(originals: Res<crate::Originals>, mut stage: ResMut<StageSh>) {
    if originals.resource_index.records.is_empty() {
        return;
    }
    stage.loaded = true;
    stage.light = crate::cell_editor_data::light(&originals.resource_index, crate::world_objects::name_id("CellGame"))
        .as_ref()
        .and_then(ShLight::from_light);
}

/// Re-point every lit Cell-stage material at the SH shader (the stage light,
/// `CellGame`). The editor's own entities keep theirs ([`swap_materials`]).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn swap_stage_materials(
    mut commands: Commands,
    stage: Res<StageSh>,
    standard: Res<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    mut lit: ResMut<Assets<ShMaterial>>,
    changed: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        (With<crate::stages::StageOwned>, Without<CellEditorOwned>, Changed<MeshMaterial3d<StandardMaterial>>),
    >,
    mut cache: Local<HashMap<AssetId<StandardMaterial>, AssetId<ShMaterial>>>,
) {
    let Some(light) = stage.light.clone() else {
        return;
    };
    cache.retain(|source, target| standard.contains(*source) && lit.contains(*target));
    // Follow the sources (the paint code rewrites the body texture in place).
    for (source, target) in cache.iter() {
        let (Some(src), Some(dst)) = (standard.get(*source), lit.get(*target)) else {
            continue;
        };
        if src.base_color_texture != dst.base.base_color_texture
            || src.normal_map_texture != dst.base.normal_map_texture
            || src.base_color != dst.base.base_color
            || src.emissive != dst.base.emissive
        {
            let display = display_base(src, &images);
            if let Some(mut dst) = lit.get_mut(*target) {
                dst.base = src.clone();
                dst.extension.spec = src.normal_map_texture.clone();
                dst.extension.configure(params_of(*source).unwrap_or_else(MaterialParams::fallback), display);
            }
        }
    }
    for (entity, handle) in &changed {
        let Some(base) = standard.get(&handle.0) else {
            continue;
        };
        if base.unlit {
            continue;
        }
        let swapped = match cache.get(&handle.0.id()).and_then(|target| lit.get_strong_handle(*target)) {
            Some(shared) => shared,
            None => {
                let params = params_of(handle.0.id()).unwrap_or_else(MaterialParams::fallback);
                let strong = lit.add(ExtendedMaterial {
                    base: base.clone(),
                    extension: CellSh::new(&light, params, display_base(base, &images), true, base.normal_map_texture.clone()),
                });
                cache.insert(handle.0.id(), strong.id());
                strong
            }
        };
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(swapped));
    }
}

/// Whether the base colour of a material already holds display values: the
/// shared importer uploads the model rasters as linear `Rgba8Unorm` (the byte
/// is what Spore's gamma-space shading reads), the editor's copies as sRGB.
fn display_base(material: &StandardMaterial, images: &Assets<Image>) -> bool {
    use bevy::render::render_resource::TextureFormat;
    material
        .base_color_texture
        .as_ref()
        .and_then(|t| images.get(t))
        .is_some_and(|image| image.texture_descriptor.format == TextureFormat::Rgba8Unorm)
}

/// Re-point every lit editor material at the SH shader: the scene code keeps
/// spawning `StandardMaterial` copies, this swaps them on the editor's own
/// entities (unlit ones, like the backdrop, keep theirs). The SH material keeps
/// following its source: the paint code replaces the body's texture handle
/// when it repaints.
#[allow(clippy::type_complexity)]
fn swap_materials(
    mut commands: Commands,
    data: Res<CellEditorData>,
    mut rig: ResMut<ShRig>,
    standard: Res<Assets<StandardMaterial>>,
    mut lit: ResMut<Assets<ShMaterial>>,
    changed: Query<(Entity, &MeshMaterial3d<StandardMaterial>), (With<CellEditorOwned>, Changed<MeshMaterial3d<StandardMaterial>>)>,
    mut cache: Local<HashMap<AssetId<StandardMaterial>, AssetId<ShMaterial>>>,
) {
    if rig.build.is_none() && data.loaded {
        let loaded = ShRig::from_data(&data);
        if loaded.build.is_some() {
            *rig = loaded;
        }
    }
    let Some(light) = rig.current() else {
        return;
    };
    // The cache holds ids only: a material nobody draws any more goes.
    cache.retain(|source, target| standard.contains(*source) && lit.contains(*target));
    for (source, target) in cache.iter() {
        let (Some(src), Some(dst)) = (standard.get(*source), lit.get(*target)) else {
            continue;
        };
        if src.base_color_texture != dst.base.base_color_texture
            || src.normal_map_texture != dst.base.normal_map_texture
            || src.base_color != dst.base.base_color
            || src.emissive != dst.base.emissive
        {
            if let Some(mut dst) = lit.get_mut(*target) {
                dst.base = src.clone();
                dst.extension.spec = src.normal_map_texture.clone();
                // The painted body's settings follow its textures.
                dst.extension.configure(params_of(*source).unwrap_or_else(MaterialParams::fallback), false);
            }
        }
    }
    for (entity, handle) in &changed {
        let Some(base) = standard.get(&handle.0) else {
            continue;
        };
        if base.unlit {
            continue;
        }
        let swapped = match cache.get(&handle.0.id()).and_then(|target| lit.get_strong_handle(*target)) {
            Some(shared) => shared,
            None => {
                // The editor's copies decode their rasters as sRGB (the base
                // colour holds linear values); `materialParams` follow the
                // source material (the copies inherit them, `copy_params`).
                let params = params_of(handle.0.id()).unwrap_or_else(MaterialParams::fallback);
                let strong = lit.add(ExtendedMaterial {
                    base: base.clone(),
                    extension: CellSh::new(&light, params, false, false, base.normal_map_texture.clone()),
                });
                cache.insert(handle.0.id(), strong.id());
                strong
            }
        };
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(swapped));
    }
}

/// Follow the Build / Paint glide of the camera with the light.
fn drive_light(
    editor: Res<crate::cell_editor::CellEditor>,
    mut rig: ResMut<ShRig>,
    mut lit: ResMut<Assets<ShMaterial>>,
    mut applied: Local<Option<(f32, usize)>>,
) {
    let blend = ((CAMERA_OFFSET - editor.camera_offset) / (2.0 * CAMERA_OFFSET)).clamp(0.0, 1.0);
    if (rig.blend - blend).abs() > 1e-6 {
        rig.blend = blend;
    }
    let Some(light) = rig.current() else {
        return;
    };
    // New materials are created with the current light; only a changed blend
    // (or a changed material count after a rebuild) needs the loop.
    let key = (blend, lit.len());
    if *applied == Some(key) {
        return;
    }
    *applied = Some(key);
    for (_, material) in lit.iter_mut().filter(|(_, m)| !m.extension.stage) {
        material.extension.set(&light);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(c0: f32) -> ShLight {
        let mut coeffs = [[0.0; 3]; COEFFS];
        coeffs[0] = [c0; 3];
        ShLight { coeffs, sun_dir: [0.0, 0.0, 1.0], sun_color: [0.0; 3] }
    }

    #[test]
    fn material_params_are_words_one_to_four() {
        // Word 0 is the uniform slot header (0x26445C04 in the installed data).
        let w = |f: f32| f.to_bits();
        let p = MaterialParams::from_words([0x2644_5C04, w(10.0), w(0.76), w(1.0), w(0.14)]).unwrap();
        assert_eq!((p.spec_e, p.spec_strength, p.bump, p.gloss), (10.0, 0.76, 1.0, 0.14));
        assert!(MaterialParams::from_words([0, f32::NAN.to_bits(), 0, 0, 0]).is_none());
    }

    #[test]
    fn registered_params_follow_material_copies() {
        let mut assets = Assets::<StandardMaterial>::default();
        let (a, b, c) = (
            assets.add(StandardMaterial::default()),
            assets.add(StandardMaterial::default()),
            assets.add(StandardMaterial::default()),
        );
        let w = |f: f32| f.to_bits();
        register_params(a.id(), Some([0, w(30.0), w(0.57), w(1.0), w(0.07)]));
        register_params(c.id(), None);
        copy_params(a.id(), b.id());
        assert_eq!(params_of(b.id()), params_of(a.id()));
        assert_eq!(params_of(b.id()).unwrap().spec_e, 30.0);
        assert!(params_of(c.id()).is_none(), "no datum, no entry (the constants apply)");
    }

    #[test]
    fn a_constant_light_shades_every_normal_alike() {
        let l = flat(0.6);
        for n in [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]] {
            assert_eq!(l.diffuse(n), [0.6; 3]);
            assert_eq!(l.glossy(n), [0.6; 3]);
        }
    }

    #[test]
    fn band_one_is_y_z_x_and_convolved_by_two_thirds() {
        // shCoeffs[1] * y, [2] * z, [3] * x (VS shLightingDiffSpec9).
        let mut l = flat(1.0);
        l.coeffs[2] = [0.3; 3];
        assert!((l.diffuse([0.0, 0.0, 1.0])[0] - (1.0 + 0.3 * SH_A1)).abs() < 1e-6);
        assert!((l.diffuse([0.0, 0.0, -1.0])[0] - (1.0 - 0.3 * SH_A1)).abs() < 1e-6);
        assert_eq!(l.diffuse([1.0, 0.0, 0.0])[0], 1.0);
        // The glossy term takes the band unconvolved.
        assert!((l.glossy([0.0, 0.0, 1.0])[0] - 1.3).abs() < 1e-6);
        l.coeffs[3] = [0.2; 3];
        assert!((l.diffuse([1.0, 0.0, 0.0])[0] - (1.0 + 0.2 * SH_A1)).abs() < 1e-6);
    }

    #[test]
    fn band_three_only_reaches_the_glossy_term() {
        let mut l = flat(0.0);
        l.coeffs[12] = [1.0; 3]; // (5 z^2 - 3) z
        assert_eq!(l.diffuse([0.0, 0.0, 1.0]), [0.0; 3], "kSH_A3 = 0");
        assert!((l.glossy([0.0, 0.0, 1.0])[0] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn shading_is_gamma_space_base_times_light_plus_the_highlight() {
        let mut l = flat(0.5);
        l.sun_color = [1.0, 1.0, 1.0];
        let up = [0.0, 0.0, 1.0];
        // Looking straight down: the reflection is straight up, at the sun.
        let c = l.shade([0.5; 3], up, [0.0, 0.0, -1.0], [0.0, 20.0, 0.0]);
        assert!((c[0] - 0.25).abs() < 1e-6);
        let c = l.shade([0.5; 3], up, [0.0, 0.0, -1.0], [0.0, 20.0, 0.4]);
        assert!((c[0] - 0.65).abs() < 1e-6, "{c:?}");
    }

    #[test]
    fn installed_cell_editor_light_is_the_retail_data() {
        // CellEditor / CellEditorPaintMode: 25 coefficients, exposure 0.28; the
        // build light adds a sun, the paint light has none.
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let build = crate::cell_editor_data::light(&index, crate::world_objects::name_id("CellEditor")).expect("CellEditor");
        let paint = crate::cell_editor_data::light(&index, crate::world_objects::name_id("CellEditorPaintMode")).expect("paint");
        assert_eq!((build.sh.len(), paint.sh.len()), (25, 25));
        assert!((build.exposure - 0.28).abs() < 1e-6 && (paint.exposure - 0.28).abs() < 1e-6);
        assert!(build.sun_strength > 0.9 && paint.sun_strength == 0.0, "{} {}", build.sun_strength, paint.sun_strength);
        let (b, p) = (ShLight::from_light(&build).unwrap(), ShLight::from_light(&paint).unwrap());
        assert_eq!(b.coeffs, p.coeffs, "the same coefficients");
        // An up-facing surface of base 1 is lit to about full brightness.
        let up = b.diffuse([0.0, 0.0, 1.0]);
        assert!(up.iter().all(|v| (0.7..1.5).contains(v)), "{up:?}");
    }

    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn census_cell_material_params() {
        use crate::{asset_catalog::RW4, dbpf::ResourceKey};
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let f = |bytes: &[u8]| -> Vec<f32> { bytes.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect() };
        // GMDL cell bodies.
        let mut ids: std::collections::BTreeMap<String, usize> = Default::default();
        let mut params: std::collections::BTreeMap<String, usize> = Default::default();
        let mut keys: Vec<_> = index.records.keys().copied().filter(|k| k.type_id == crate::decode::TYPE_GMDL && k.group_id == crate::cell_visuals::CELL_MODEL_GROUP).collect();
        keys.sort_by_key(|k| k.instance_id);
        for key in &keys {
            let Ok(model) = crate::baked_model::parse(&index.read(*key).unwrap()) else { continue };
            for m in &model.materials {
                let k = m.shader_data.keys().map(|k| format!("{k:03X}")).collect::<Vec<_>>().join(",");
                *ids.entry(k).or_default() += 1;
                if let Some(p) = m.shader_data.get(&0x210) {
                    *params.entry(format!("{:?}", f(p))).or_default() += 1;
                }
            }
        }
        println!("GMDL cell bodies: {}", keys.len());
        for (k, n) in &ids { println!("  shader data ids [{k}] x{n}"); }
        for (k, n) in &params { println!("  materialParams {k} x{n}"); }
        // RW4 static models.
        for name in ["CG_liquidFood_L1", "CG_solidFood_1_L1", "cell_flora_clover_L0", "CG_rock1_L1", "CG_ground_L1"] {
            let key = ResourceKey { type_id: RW4, group_id: 0, instance_id: crate::world_objects::name_id(name) };
            let Ok(bytes) = index.read(key) else { println!("{name}: not found"); continue };
            let sections = crate::rw4::sections(&bytes).unwrap();
            for (i, (t, payload)) in sections.iter().enumerate() {
                if *t != crate::rw4::T_COMPILED_STATE { continue; }
                let Ok(state) = crate::material_state::parse(payload) else { continue };
                let u: Vec<String> = state.shader_data.iter().map(|d| format!("{:03X}{}", d.index, if d.index == 0x210 { format!("{:?}", d.words.iter().map(|w| f32::from_bits(*w)).collect::<Vec<_>>()) } else { String::new() })).collect();
                println!("{name} sec {i}: renderer {:08X} flags {:?} uniforms {:?} slots {:?}", state.renderer_id, state.flags, u, state.texture_slots.iter().map(|s| s.sampler).collect::<Vec<_>>());
            }
        }
        // Cell part models (RW4): the material state of every part.
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        let mut seen = std::collections::BTreeMap::<String, usize>::new();
        for part in catalog.parts.iter().filter(|p| p.key.group_id == crate::asset_catalog::CELL_PARTS) {
            let Ok(bytes) = index.read(part.model) else { continue };
            let Ok(sections) = crate::rw4::sections(&bytes) else { continue };
            for (t, payload) in sections.iter() {
                if *t != crate::rw4::T_COMPILED_STATE { continue; }
                let Ok(state) = crate::material_state::parse(payload) else { continue };
                let u: Vec<String> = state.shader_data.iter().map(|d| format!("{:03X}{}", d.index, if d.index == 0x210 { format!("{:?}", d.words.iter().map(|w| f32::from_bits(*w)).collect::<Vec<_>>()) } else { String::new() })).collect();
                let k = format!("renderer {:08X} uniforms {:?} slots {:?}", state.renderer_id, u, state.texture_slots.iter().map(|s| s.sampler).collect::<Vec<_>>());
                *seen.entry(k).or_default() += 1;
            }
        }
        println!("cell part material states ({} parts):", catalog.parts.len());
        for (k, n) in &seen { println!("  {k} x{n}"); }
        // The stage light PROPs, every property id with its value kind.
        for name in ["CellGame", "CellEditor", "CellEditorPaintMode"] {
            let key = crate::cell_editor_data::prop_key(0x4020_0100, crate::world_objects::name_id(name));
            let p = index.properties(key).unwrap();
            let mut rows: Vec<String> = p.0.iter().map(|(id, v)| {
                let n = v.number().map(|n| n.to_string()).unwrap_or_default();
                format!("{id:08X} kind {:#x} n {} {}", v.kind, v.values.len(), if v.values.len() <= 2 { format!("{:?} {}", v.values.iter().map(|b| if b.len() % 4 == 0 { b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>() } else { vec![] }).collect::<Vec<_>>(), n) } else { n })
            }).collect();
            rows.sort();
            println!("LIGHT {name}:");
            for r in rows { println!("  {r}"); }
        }
        // Alpha of the spec / bump textures (sampler 1) of the cell bodies.
        let mut alphas = vec![];
        for key in &keys {
            let Ok(model) = crate::baked_model::parse(&index.read(*key).unwrap()) else { continue };
            for m in &model.materials {
                for b in m.textures.iter().filter(|b| b.sampler == 1) {
                    let Ok(mips) = crate::baked_model::texture_mips(&index, b.key) else { continue };
                    let Some(base) = mips.first() else { continue };
                    let a: Vec<f32> = base.rgba.chunks_exact(4).map(|p| p[3] as f32 / 255.0).collect();
                    let mean = a.iter().sum::<f32>() / a.len() as f32;
                    let lo = a.iter().cloned().fold(1.0, f32::min);
                    let hi = a.iter().cloned().fold(0.0, f32::max);
                    let bz = base.rgba.chunks_exact(4).map(|p| p[2] as f32 / 255.0).sum::<f32>() / a.len() as f32;
                    alphas.push((key.instance_id, mean, lo, hi, bz, base.width, base.height));
                }
            }
        }
        for (id, mean, lo, hi, bz, w, h) in alphas.iter().take(12) {
            println!("spec/bump {id:08X} {w}x{h}: alpha mean {mean:.2} min {lo:.2} max {hi:.2}; blue mean {bz:.2}");
        }
    }
}
