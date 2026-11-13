//! The retail planet sky, fog and water state of the planet stages
//! (`notes/planet-look-notes.md`): the in-scatter cube, the TerrainAtmosphere
//! shell, the per-frame TerrainState values (sun, sphere, fog) shared by the
//! land, water and atmosphere shaders, the TerrainWater PCA animation, and the
//! stage clocks of Tribal / Civ.
//!
//! Original data / code (VERIFIED, addresses = ModAPI march2017):
//! - in-scatter cube = `BakeInScatterTextureShader` (shader pack 0x40212004,
//!   0x70000011) run by 0xF9D960 per face with customParams 0x206: c0 phases,
//!   c1 = 10 x density x brightness, c2..c4 lobe colours, c5 = sun (0, 0, 1);
//!   colour = sat(sum HG(g_i, sat(dir . sun)) c1_i c_i), alpha = 0.5 HG_2 c1_2.
//! - lobes 0xFBC740(p): colours from the atmosphere-type sky table
//!   (0x05359C50 + type), sky_table_density, sky_table_dust at column
//!   T * 63, row (1 - p) * 63; sunset = sky_table_sunset (B) at column
//!   (1 - A) * 63, row p * 63; phases / brightness lerped over the three
//!   PlanetMaterial keys by 2p; density = kPiScaleDensity x sunset;
//!   fogParms.w = sunset. p = sat(camera altitude / limit) x 0.6 + 0.39 (0xFBF540).
//! - TerrainState (shader data 0x305, 0xF98910 / 0xFBF540): vSunDir.w =
//!   cos(60 deg); sphereParms (R, altRange, kAtmThicknessByAtmosphereScore(A) x
//!   altRange, (2 - sat(alt / thickness)) / 2); illumParms (kAtmLuminenceRange);
//!   fogParms (-density, scale, min distance, sunset); waterColor = the
//!   0x54633685 table at column T * 63 (game modes).
//! - shader maths from the HLSL fragment pack (0x40212002): TerrainAtmosphere
//!   VS/PS, GetTerrainFogStrength, GetInScatterTerminator, TerrainWaterVS/PS.
//! - PCA water: pcawater.pcaw (0x1FA9AB84) = 30 frames of the 17-register
//!   pcaTexture struct (4 x float4x4 + mean), PCAwater2_0/1, kWaterPCAAnimationFPS 20.
//!
//! INFERRED (see the notes): T / A = terrainScriptWTALevels y / z; the cube
//! frame (the sun lobe faces the view ray); the stage start hour of Tribal /
//! Civ (startHour 8.5). VERIFIED since planet-light: the 0x1F8 altitude range
//! = kParticleMaxDistance 500 (loader 0xFBE7A0).
use crate::{asset_catalog::RW4, dbpf::ResourceKey, stages::GameStage};
use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, Face, TextureDimension, TextureFormat, TextureViewDescriptor,
        TextureViewDimension,
    },
    shader::ShaderRef,
};

/// Slots of the shared TerrainState uniform (`array<vec4<f32>, 24>`):
/// 0 sun (xyz world direction toward the sun, w cutoff), 1 water colour,
/// 2 planet centre (xyz world) + water radius, 3 sphereParms, 4 fogParms,
/// 5 illumParms, 6..8 the scatter-cube frame rows, 9 time (x seconds,
/// y depth-cube scale), 10..13 PCA basis rows c0 c2 c4 c6, 14 PCA mean,
/// 15 x = 1 when the retail path is active, w = 1 with the retail land
/// lighting; 16..23 the terrain lighting state (`planet_light::TerrainLighting::slots`).
pub const SKY_SLOTS: usize = 24;
pub type SkyState = [Vec4; SKY_SLOTS];

/// A 1 x 1 cube bound until a planet's scatter cube exists (cube bindings
/// cannot take the default 2D image).
pub const NEUTRAL_CUBE: Handle<Image> = bevy::asset::uuid_handle!("6c1f3a5e-2b7d-4e0a-9c41-8d5f20a7b9e3");

// PlanetMaterial 0x0243AD2B!0xD985994C (VERIFIED dump values).
const P_SCALE_DENSITY: [f32; 3] = [1.2, 1.0, 1.0]; // kP1..3ScaleDensity
const P_PHASE: [[f32; 3]; 3] = [[0.0, 0.0, -0.2], [0.35, 0.65, 0.8], [-0.8, -0.6, -0.6]];
const P_BRIGHTNESS: [[f32; 3]; 3] = [[1.5, 1.05, 0.4], [0.2, 0.08, 0.06], [3.0, 1.0, 0.6]];
/// cTerrainStateMgr +0x1F8 = kParticleMaxDistance 0x0257035A (500), read by
/// 0xFBE7A0 (VERIFIED, planet-light): the camera-altitude range of p.
pub(crate) const SKY_ALTITUDE_RANGE: f32 = 500.0;
pub(crate) const ATM_LUMINANCE_RANGE: (f32, f32) = (-1.0, 1.0); // kAtmLuminenceRange
const FOG_MIN_DENSITY: f32 = 0.001;
const FOG_MAX_DENSITY: f32 = 0.009;
const FOG_MIN_DISTANCE: f32 = 70.0;
const ATM_THICKNESS: [(f32, f32); 14] = [
    (0.0, 0.0), (0.1, 0.0), (0.11, 1.15), (0.25, 1.15), (0.26, 1.35), (0.39, 1.35), (0.4, 1.5),
    (0.59, 1.5), (0.6, 1.6), (0.74, 1.6), (0.75, 1.65), (0.89, 1.65), (0.9, 1.75), (1.0, 1.75),
];
const FOG_DIST_BIAS_BY_ATM: [(f32, f32, f32); 14] = [
    (0.0, 1000.0, 0.0), (0.1, 1000.0, 0.0), (0.11, 0.0, 0.5), (0.25, 0.0, 0.5), (0.26, 0.0, 0.7),
    (0.39, 0.0, 0.7), (0.4, 0.0, 1.0), (0.59, 0.0, 1.0), (0.6, -50.0, 1.25), (0.74, -50.0, 1.25),
    (0.75, -100.0, 1.5), (0.89, -100.0, 1.5), (0.9, -100.0, 2.0), (1.0, -100.0, 2.0),
];
const FOG_BIAS_BY_ELEV: [(f32, f32); 4] = [(0.0, 1.0), (40.0, 0.5), (50.0, 0.25), (120.0, 0.0)];
const FOG_DIST_BIAS_BY_TOD: [(f32, f32); 6] = [(4.0, 0.0), (7.0, -30.0), (8.0, -30.0), (9.0, -10.0), (10.0, 0.0), (12.0, 20.0)];
const FOG_BY_ELEV_CREATURE: [(f32, f32, f32); 4] = [(0.0, 1.0, 1.25), (50.0, 1.5, 1.0), (120.0, 3.0, 0.6), (300.0, 1.75, 0.9)];
const FOG_BY_ELEV_TRIBE: [(f32, f32, f32); 4] = [(0.0, 1.25, 1.0), (50.0, 1.5, 1.0), (120.0, 3.0, 0.6), (300.0, 1.75, 0.9)];
const FOG_BY_ELEV_CIV: [(f32, f32, f32); 4] = [(0.0, 1.5, 1.0), (50.0, 2.0, 1.0), (120.0, 3.0, 0.6), (300.0, 1.75, 0.9)];
/// kFogByElevSpace 0xA219CC45 (same PlanetMaterial, VERIFIED dump; used by the
/// Space planet view, `space_planets::sky`).
const FOG_BY_ELEV_SPACE: [(f32, f32, f32); 5] =
    [(0.0, 1.5, 1.0), (50.0, 1.5, 1.0), (80.0, 2.0, 1.0), (120.0, 3.0, 0.6), (300.0, 1.75, 0.9)];
/// kDuskDawnStartEnd.z = 60 deg: vSunDir.w = cos(60 deg) (0xF98910).
pub(crate) const CUTOFF: f32 = 0.5;
/// pcaTexture: 17 registers per frame (4 float4x4 + mean).
const PCA_FLOATS: usize = 68;
pub(crate) const PCA_FPS: f32 = 20.0; // kWaterPCAAnimationFPS 0x023227BD
const SCATTER_RES: u32 = 64;
/// The initial startHour global 0x1582E74 (Creature overrides it with 7.0).
pub const STAGE_START_HOUR: f32 = 8.5;

const IMAGE32: u32 = 0x03E421ED;
const fn key(instance_id: u32, type_id: u32) -> ResourceKey {
    ResourceKey { group_id: 0, instance_id, type_id }
}
const SKY_TABLE_BASE: u32 = 0x05359C50;
const SKY_DENSITY: ResourceKey = key(0xD1830E14, IMAGE32);
const SKY_DUST: ResourceKey = key(0xC74BC5B0, IMAGE32);
const SKY_SUNSET: ResourceKey = key(0x9C751F5E, IMAGE32);
const WATER_COLORS: ResourceKey = key(0x54633685, IMAGE32);
const WATER_TYPE_BASE: u32 = 0x0536DD50;
pub(crate) const PACKED_CURVES: ResourceKey = key(0xDEF55713, RW4);
pub(crate) const PCA0: ResourceKey = key(0x11B5EE6F, RW4);
pub(crate) const PCA1: ResourceKey = key(0x11B5EE6E, RW4);
pub(crate) const PCAW: ResourceKey = key(0x1FA9AB84, 0x02CB4F2F);
/// seabed_grand_shallow, the TerrainTextures default Below raster.
pub(crate) const SEABED_DEFAULT: u32 = 0xC66C3FCD;
/// Theme properties terrainAtmosphereType / terrainWaterType.
const PROP_ATMOSPHERE_TYPE: u32 = 0x0536250C;
const PROP_WATER_TYPE: u32 = 0x0536250D;

/// A 32bitImage: 20-byte header (BE width / height), BGRA texels.
#[derive(Clone, Debug)]
pub struct Table {
    pub width: usize,
    pub height: usize,
    bgra: Vec<u8>,
}

impl Table {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let width = u32::from_be_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
        let height = u32::from_be_bytes(bytes.get(8..12)?.try_into().ok()?) as usize;
        let bgra = bytes.get(20..20 + width * height * 4)?.to_vec();
        (width > 0 && height > 0).then_some(Self { width, height, bgra })
    }
    /// The sea colour of a water type's table at the temperature score
    /// (0xFBF540: column floor((w - 1) T), the table is one row high).
    pub fn water_rgb(&self, temperature: f32) -> [f32; 3] {
        let c = self.at(temperature, 0.0);
        [c[0], c[1], c[2]]
    }

    /// The texel the code reads: column = floor(u * (w - 1)), row = floor(v * (h - 1)).
    fn at(&self, u: f32, v: f32) -> [f32; 4] {
        let col = ((self.width - 1) as f32 * u.clamp(0.0, 1.0)).floor() as usize;
        let row = ((self.height - 1) as f32 * v.clamp(0.0, 1.0)).floor() as usize;
        let i = (row * self.width + col) * 4;
        let b = &self.bgra[i..i + 4];
        [b[2], b[1], b[0], b[3]].map(|c| c as f32 / 255.0)
    }
}

/// Planet inputs of the sky (terrain script WTA levels and theme types).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyInputs {
    pub temperature: f32,
    pub atmosphere: f32,
    pub atmosphere_type: u32,
    pub water_type: u32,
}

impl SkyInputs {
    pub fn from_descriptor(d: &crate::planet_data::PlanetDescriptor) -> Self {
        Self::from_descriptor_or(d, 0)
    }

    /// `default_type`: the atmosphere / water type of a script without the
    /// properties. The stage terrain setup (0xFAA2E0) leaves the reset value 0
    /// in place; the planet-record path of the Space stage (0xF35450, called
    /// through 0xB7D960) starts both at 11 (`mov eax, 0xb`, VERIFIED) - the
    /// Space pool scripts (`T0A0_*`, `AD_*`, `mars`, ...) carry neither.
    pub fn from_descriptor_or(d: &crate::planet_data::PlanetDescriptor, default_type: u32) -> Self {
        let u32_prop = |id| {
            d.properties
                .0
                .get(&id)
                .and_then(|p| p.values.first())
                .and_then(|v| Some(u32::from_be_bytes(v.get(..4)?.try_into().ok()?)))
        };
        Self {
            temperature: d.wta[1].clamp(0.0, 1.0),
            atmosphere: d.wta[2].clamp(0.0, 1.0),
            atmosphere_type: u32_prop(PROP_ATMOSPHERE_TYPE).unwrap_or(default_type).min(15),
            water_type: u32_prop(PROP_WATER_TYPE).unwrap_or(default_type),
        }
    }
}

/// The water colour table of a water type: SetWaterType 0xFBAF20 picks entry
/// `type` of the 16 tables 0x0536DD50 + i that 0xFBA8C0 loads (VERIFIED).
pub fn water_table(index: &crate::asset_catalog::ResourceIndex, water_type: u32) -> Option<Table> {
    index
        .read(key(WATER_TYPE_BASE + water_type.min(15), IMAGE32))
        .ok()
        .and_then(|b| Table::decode(&b))
}

/// The three scattering lobes at a camera altitude (0xFBC740).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lobes {
    pub phase: [f32; 3],
    /// 10 x density x brightness (bake c1).
    pub weight: [f32; 3],
    pub color: [[f32; 3]; 3],
    /// The sunset table value: lobe density factor and fogParms.w.
    pub sunset: f32,
}

pub struct SkyTables {
    pub atmosphere: Table,
    pub density: Table,
    pub dust: Table,
    pub sunset: Table,
    pub water: Option<Table>,
}

impl SkyTables {
    pub fn load(index: &crate::asset_catalog::ResourceIndex, atmosphere_type: u32, water_type: u32) -> Option<Self> {
        let table = |k: ResourceKey| index.read(k).ok().and_then(|b| Table::decode(&b));
        Some(Self {
            atmosphere: table(key(SKY_TABLE_BASE + atmosphere_type.min(15), IMAGE32))?,
            density: table(SKY_DENSITY)?,
            dust: table(SKY_DUST)?,
            sunset: table(SKY_SUNSET)?,
            // SetWaterType 0xFBAF20: mgr+0x1F4 = the water colour table
            // 0x0536DD50 + type (the 0x54633685 table is the initial one).
            water: table(key(WATER_TYPE_BASE + water_type.min(15), IMAGE32)).or_else(|| table(WATER_COLORS)),
        })
    }

    /// `user`: the editor / scenario branch of 0xFBC740 (user atmosphere colour,
    /// lobe 1 = mix(user, user at half saturation, 1 - p)) instead of the
    /// atmosphere-type table.
    pub fn lobes(&self, inputs: &SkyInputs, p: f32, user: Option<[f32; 3]>) -> Lobes {
        let p = p.clamp(0.0, 1.0);
        // Three keys: index floor(2p), fraction 2p - index.
        let x = 2.0 * p;
        let k = (x.floor() as usize).min(1);
        let f = x - k as f32;
        let lerp = |keys: [f32; 3]| keys[k] + (keys[k + 1] - keys[k]) * f;
        let sunset = self.sunset.at(1.0 - inputs.atmosphere, p)[2];
        let mut colors = [&self.atmosphere, &self.density, &self.dust]
            .map(|t| {
                let c = t.at(inputs.temperature, 1.0 - p);
                [c[0], c[1], c[2]]
            });
        if let Some(u) = user {
            let (h, s, v) = rgb_to_hsv(u);
            let d = hsv_to_rgb(h, s * 0.5, v);
            colors[0] = [0, 1, 2].map(|i| u[i] + (d[i] - u[i]) * (1.0 - p));
        }
        Lobes {
            phase: [0, 1, 2].map(|i| lerp(P_PHASE[i])),
            weight: [0, 1, 2].map(|i| 10.0 * P_SCALE_DENSITY[i] * sunset * lerp(P_BRIGHTNESS[i])),
            color: colors,
            sunset,
        }
    }

    pub fn water_color(&self, inputs: &SkyInputs) -> [f32; 3] {
        self.water.as_ref().map_or([0.06, 0.21, 0.30], |t| t.water_rgb(inputs.temperature))
    }
}

fn rgb_to_hsv(c: [f32; 3]) -> (f32, f32, f32) {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let d = max - min;
    let h = if d <= 1e-6 {
        0.0
    } else if max == c[0] {
        ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if max == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    (h / 6.0, if max > 0.0 { d / max } else { 0.0 }, max)
}
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h6 = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let (r, g, b) = match h6 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

/// BakeInScatterTextureShader for one direction of the cube frame (sun = +Z).
pub fn scatter(lobes: &Lobes, dir: Vec3) -> [f32; 4] {
    let cos = dir.normalize_or_zero().z.clamp(0.0, 1.0);
    let hg = |g: f32| {
        let g2 = g * g;
        (1.0 - g2) / (4.0 * std::f32::consts::PI * (1.0 + g2 - 2.0 * g * cos).max(1e-6).powf(1.5))
    };
    let r: [f32; 3] = [0, 1, 2].map(|i| hg(lobes.phase[i]) * lobes.weight[i]);
    let mut c = [0.0f32; 4];
    for ch in 0..3 {
        c[ch] = (r[0] * lobes.color[0][ch] + r[1] * lobes.color[1][ch] + r[2] * lobes.color[2][ch]).clamp(0.0, 1.0);
    }
    c[3] = (r[1] * 0.5).clamp(0.0, 1.0);
    c
}

/// Direction of a cube texel (wgpu / D3D face order, as `depth_cube_image`).
fn cube_dir(face: usize, x: usize, y: usize, n: usize) -> Vec3 {
    let s = 2.0 * (x as f32 + 0.5) / n as f32 - 1.0;
    let t = 2.0 * (y as f32 + 0.5) / n as f32 - 1.0;
    match face {
        0 => Vec3::new(1.0, -t, -s),
        1 => Vec3::new(-1.0, -t, s),
        2 => Vec3::new(s, 1.0, t),
        3 => Vec3::new(s, -1.0, -t),
        4 => Vec3::new(s, -t, 1.0),
        _ => Vec3::new(-s, -t, -1.0),
    }
    .normalize()
}

pub fn rgba_cube(size: u32, data: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

pub fn scatter_cube(lobes: &Lobes) -> Image {
    let n = SCATTER_RES as usize;
    let mut data = Vec::with_capacity(6 * n * n * 4);
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let c = scatter(lobes, cube_dir(face, x, y, n));
                data.extend(c.map(|v| (v * 255.0).round() as u8));
            }
        }
    }
    rgba_cube(SCATTER_RES, data)
}

/// Piecewise-linear curve lookup (keys sorted by x, clamped at the ends).
fn curve2(keys: &[(f32, f32)], x: f32) -> f32 {
    let first = keys[0];
    if x <= first.0 {
        return first.1;
    }
    for w in keys.windows(2) {
        if x <= w[1].0 {
            let t = (x - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
            return w[0].1 + (w[1].1 - w[0].1) * t;
        }
    }
    keys[keys.len() - 1].1
}
fn curve3(keys: &[(f32, f32, f32)], x: f32) -> (f32, f32) {
    let a: Vec<(f32, f32)> = keys.iter().map(|k| (k.0, k.1)).collect();
    let b: Vec<(f32, f32)> = keys.iter().map(|k| (k.0, k.2)).collect();
    (curve2(&a, x), curve2(&b, x))
}

pub fn atmosphere_thickness(atmosphere: f32) -> f32 {
    curve2(&ATM_THICKNESS, atmosphere)
}

/// kFogDistBiasByTimeOfDay: authored (7, 8, 9, 10, 12, 4); read as a daily
/// cycle, 12:00 -> 04:00 next day interpolates (INFERRED).
fn tod_bias(hour: f32) -> f32 {
    let h = hour.rem_euclid(24.0);
    if h >= 12.0 {
        let t = (h - 12.0) / 16.0;
        20.0 + (0.0 - 20.0) * t
    } else if h < 4.0 {
        let t = (h + 12.0) / 16.0;
        20.0 + (0.0 - 20.0) * t
    } else {
        curve2(&FOG_DIST_BIAS_BY_TOD, h)
    }
}

/// fogParms (x -density, y scale, z min distance, w sunset) for a camera
/// altitude above R (0xFBF540).
pub fn fog_parms(stage: GameStage, inputs: &SkyInputs, lobes: &Lobes, altitude: f32, hour: f32, sphere_w: f32) -> Vec4 {
    let by_elev: &[(f32, f32, f32)] = match stage {
        GameStage::Tribal => &FOG_BY_ELEV_TRIBE,
        GameStage::Civilization => &FOG_BY_ELEV_CIV,
        GameStage::Space => &FOG_BY_ELEV_SPACE,
        _ => &FOG_BY_ELEV_CREATURE,
    };
    let (dist_factor, density_factor) = curve3(by_elev, altitude);
    let (atm_bias, atm_density) = curve3(&FOG_DIST_BIAS_BY_ATM, inputs.atmosphere);
    let bias = curve2(&FOG_BIAS_BY_ELEV, altitude);
    let base = FOG_MIN_DENSITY + (FOG_MAX_DENSITY - FOG_MIN_DENSITY) * inputs.atmosphere;
    let density = base * density_factor * sphere_w * atm_density;
    let min_distance = (FOG_MIN_DISTANCE * dist_factor + bias * tod_bias(hour) + atm_bias * bias).max(0.0);
    Vec4::new(-density, 1.0, min_distance, lobes.sunset)
}

/// Rows of the world -> scatter-cube rotation: the cube's +Z (the baked sun
/// lobe) is the direction AWAY from the sun, so a lookup with the retail
/// "point -> camera" vector peaks when the view ray faces the sun
/// (INFERRED: the matrix at cTerrainStateMgr+0x40 is not traced).
pub fn scatter_frame(sun: Vec3) -> [Vec4; 3] {
    let z = -sun.normalize_or(Vec3::Y);
    let helper = if z.y.abs() > 0.95 { Vec3::X } else { Vec3::Y };
    let x = helper.cross(z).normalize();
    let y = z.cross(x);
    [x.extend(0.0), y.extend(0.0), z.extend(0.0)]
}

/// Water PCA animation: frames of the pcaTexture struct.
pub fn decode_pcaw(bytes: &[u8]) -> Option<Vec<[f32; PCA_FLOATS]>> {
    let word = |i: usize| -> Option<u32> { Some(u32::from_le_bytes(bytes.get(i * 4..i * 4 + 4)?.try_into().ok()?)) };
    if word(0)? != 0xFFEE_FFEE {
        return None;
    }
    let frames = word(3)? as usize;
    if frames == 0 || bytes.len() < 24 + frames * PCA_FLOATS * 4 {
        return None;
    }
    Some(
        (0..frames)
            .map(|f| {
                let mut v = [0.0f32; PCA_FLOATS];
                for (i, x) in v.iter_mut().enumerate() {
                    let o = 24 + (f * PCA_FLOATS + i) * 4;
                    *x = f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
                }
                v
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// The TerrainAtmosphere shell.

pub type RetailAtmosphereMaterial = ExtendedMaterial<StandardMaterial, RetailAtmosphere>;

/// TerrainAtmosphere (TerrainAtmosphereShader 0x70000015): back faces of the
/// shell at R + thickness, colour = scatter x (illum + scatter.a), alpha =
/// view-angle curve x illum x sunset x 2.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct RetailAtmosphere {
    #[uniform(100)]
    pub sky: SkyState,
    #[texture(101, dimension = "cube")]
    #[sampler(102)]
    pub scatter: Handle<Image>,
    /// atmosphere_packed_curves (64 x 1): R illum curve, G view-angle curve.
    #[texture(103)]
    #[sampler(104)]
    pub curves: Handle<Image>,
}

impl MaterialExtension for RetailAtmosphere {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/planet_sky.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

#[derive(Component)]
pub struct RetailAtmosphereShell;

/// Live sky of the stage planet.
#[derive(Resource)]
pub struct PlanetSky {
    pub inputs: SkyInputs,
    pub tables: SkyTables,
    pub pca_frames: Vec<[f32; PCA_FLOATS]>,
    pub pca: [Handle<Image>; 2],
    /// terrainThemeBelow of the planet's theme (seabed raster).
    pub below: Handle<Image>,
    pub curves: Handle<Image>,
    pub scatter: Handle<Image>,
    /// The TerrainAtmosphere shell (only with `OPENSPORE_PLANET_SKY=1`).
    pub material: Option<Handle<RetailAtmosphereMaterial>>,
    /// Lobe 1 from this user colour (the palette AirColor) instead of the
    /// atmosphere-type table: the default while the table sky is gated.
    pub user_air: Option<[f32; 3]>,
    /// p of the last bake (camera-altitude row of the tables).
    pub baked_p: f32,
    pub state: SkyState,
    pub water_color: [f32; 3],
    pub thickness: f32,
    /// Stage values of the stand-in sky dome (zenith, horizon, sun halo), the
    /// camera fog colour and the adapted rim shell strength before the night
    /// dimming (`dim_preview_sky`).
    pub preview_base: Option<(Vec4, Vec4, f32, Color)>,
    pub shell_base: Option<f32>,
    /// The dome's AirColor / cloud opacity before the night dimming.
    pub air_base: Option<Vec4>,
}

/// Tribal / Civ clock (hours); Creature keeps `SpeciesWorld::time_of_day`.
#[derive(Resource, Clone, Debug)]
pub struct StageClock {
    pub stage: GameStage,
    pub hour: f32,
}

pub(crate) fn image_rgba8(width: u32, height: u32, data: Vec<u8>, repeat: bool) -> Image {
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    let mut image = Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    let mode = if repeat { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode,
        address_mode_v: mode,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

pub(crate) fn rw4_image(index: &crate::asset_catalog::ResourceIndex, k: ResourceKey, repeat: bool, srgb: bool) -> Option<Image> {
    let mips = crate::texture::decode_rw4_mips(&index.read(k).ok()?)?;
    let first = mips.first()?;
    let mut image = image_rgba8(first.width, first.height, first.rgba.clone(), repeat);
    if srgb {
        image.texture_descriptor.format = TextureFormat::Rgba8UnormSrgb;
    }
    image.texture_descriptor.mip_level_count = mips.len() as u32;
    image.data = Some(mips.into_iter().flat_map(|p| p.rgba).collect());
    Some(image)
}

/// Spawn the retail atmosphere for a stage planet; `false` when the installed
/// data is missing (the caller then keeps the adapted shell).
pub fn spawn(world: &mut World, stage: &crate::planet_query::StagePlanet, air: [f32; 3]) -> bool {
    if std::env::var("OPENSPORE_RETAIL_SKY").is_ok_and(|v| v == "0") {
        return false;
    }
    // Round 2: the table-coloured TerrainAtmosphere shell is opt-in; by
    // default the preview sky dome (clouds) and the adapted shell stay, and
    // the fog / water cube takes lobe 1 from the palette AirColor.
    let table_sky = std::env::var("OPENSPORE_PLANET_SKY").is_ok_and(|v| v == "1");
    let Some(script) = world
        .get_resource::<crate::terrain::PlanetCache>()
        .map(|c| c.script.clone())
    else {
        return false;
    };
    if !world.contains_resource::<Assets<RetailAtmosphereMaterial>>() {
        return false;
    }
    let Some(originals) = world.get_resource::<crate::Originals>() else {
        return false;
    };
    let index = &originals.resource_index;
    let descriptor = if script.first_group {
        crate::planet_data::load_descriptor(index, script.terrain_script)
    } else {
        crate::planet_data::load_descriptor_key(index, script.key)
    };
    let Ok(descriptor) = descriptor else {
        return false;
    };
    let mut inputs = SkyInputs::from_descriptor(&descriptor);
    // Development / capture override of the atmosphere type (sky checks).
    if let Some(t) = std::env::var("OPENSPORE_ATMOSPHERE_TYPE").ok().and_then(|v| v.parse::<u32>().ok()) {
        inputs.atmosphere_type = t.min(15);
    }
    let flora = theme_flora(index, &descriptor);
    info!("planet flora: {} of {} theme slots installed", flora.iter().flatten().count(), flora.len());
    set_theme_flora(flora);
    let Some(tables) = SkyTables::load(index, inputs.atmosphere_type, inputs.water_type) else {
        return false;
    };
    let curves = index
        .read(PACKED_CURVES)
        .ok()
        .and_then(|b| crate::texture::decode_rw4_mips(&b))
        .and_then(|m| m.into_iter().next())
        .map(|p| image_rgba8(p.width, p.height, p.rgba, false));
    let Some(curves) = curves else {
        return false;
    };
    let pca = [rw4_image(index, PCA0, true, false), rw4_image(index, PCA1, true, false)];
    let below_instance = descriptor
        .properties
        .0
        .get(&crate::world_catalog::BELOW_TEXTURE)
        .and_then(|p| p.key())
        .map_or(SEABED_DEFAULT, |k| k.instance_id);
    let below = rw4_image(index, key(below_instance, RW4), true, true)
        .or_else(|| rw4_image(index, key(SEABED_DEFAULT, RW4), true, true));
    let pca_frames = index.read(PCAW).ok().and_then(|b| decode_pcaw(&b)).unwrap_or_default();
    let water_color = tables.water_color(&inputs);
    let thickness = atmosphere_thickness(inputs.atmosphere) * stage.planet.altitude_range;
    let user_air = (!table_sky).then_some(air);
    let lobes = tables.lobes(&inputs, 0.39, user_air);
    let cube = scatter_cube(&lobes);
    let (curves, scatter, pca, below) = {
        let mut images = world.resource_mut::<Assets<Image>>();
        let pca = pca.map(|i| i.map(|i| images.add(i)).unwrap_or_default());
        let below = below.map(|i| images.add(i)).unwrap_or_default();
        (images.add(curves), images.add(cube), pca, below)
    };
    let mut state = [Vec4::ZERO; SKY_SLOTS];
    state[15].x = 1.0;
    if !table_sky {
        world.insert_resource(PlanetSky {
            inputs,
            tables,
            pca_frames,
            pca,
            below,
            curves,
            scatter,
            material: None,
            user_air,
            baked_p: 0.39,
            state,
            water_color,
            thickness,
            preview_base: None,
            shell_base: None,
            air_base: None,
        });
        return false;
    }
    let material = world.resource_mut::<Assets<RetailAtmosphereMaterial>>().add(RetailAtmosphereMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            fog_enabled: false,
            // Back faces only: the shell surrounds the camera on the ground
            // and only its far side shows around the limb from space.
            cull_mode: Some(Face::Front),
            alpha_mode: AlphaMode::Blend,
            ..default()
        },
        extension: RetailAtmosphere { sky: state, scatter: scatter.clone(), curves: curves.clone() },
    });
    let radius = stage.planet.radius + thickness;
    let mesh = {
        let mut sphere = Sphere::new(radius).mesh().ico(5).unwrap();
        sphere.asset_usage = RenderAssetUsages::default();
        world.resource_mut::<Assets<Mesh>>().add(sphere)
    };
    world.spawn((
        crate::stages::StageOwned,
        RetailAtmosphereShell,
        NotShadowCaster,
        Name::new("planet atmosphere (TerrainAtmosphere)"),
        Mesh3d(mesh),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(stage.frame.centre),
    ));
    info!(
        "planet sky: T {:.2} A {:.2} atmosphere type {} water type {} thickness {:.0}",
        inputs.temperature, inputs.atmosphere, inputs.atmosphere_type, inputs.water_type, thickness
    );
    world.insert_resource(PlanetSky {
        inputs,
        tables,
        pca_frames,
        pca,
        below,
        curves,
        scatter,
        material: Some(material),
        user_air,
        baked_p: 0.39,
        state,
        water_color,
        thickness,
        preview_base: None,
        shell_base: None,
        air_base: None,
    });
    true
}

/// terrainThemeFlora 0x04E13CB7: the theme's nine flora definitions
/// (0x40666200!name, in terrainDistribute type order: LargePlant1..3,
/// MediumPlant1..3, SmallPlant1..3). Each definition's modelMeshLOD0 is the
/// same instance in the flora model group 0x40667100 (VERIFIED for the
/// FirstGroup themes), so the slots resolve to installed gmdl keys.
pub const PROP_THEME_FLORA: u32 = 0x04E13CB7;
/// GameplayThemeNN (0x4284A100) -> GPTGameplayThemeNN_* instance.
const GPT_THEME_TWINS: [(u32, u32); 15] = [
    (0x296AA146, 0xD90EA036), (0x296AA140, 0xE4611F2C), (0x296AA141, 0xE3005439), (0x296AA143, 0xCC6D4ED2),
    (0x2A6AA337, 0x86801EA5), (0x2A6AA336, 0x73CC30A0), (0x2A6AA335, 0x50B7A035), (0x2A6AA333, 0x2D4A8B90),
    (0x2A6AA330, 0xBB1CC237), (0x2A6AA33F, 0xACD1852D), (0x2A6AA33E, 0xCEDFE578), (0x2B6AA4AA, 0x1E511F19),
    (0x2B6AA4AB, 0xAAEF9C31), (0x2B6AA4A8, 0x7AB1C9A7), (0x2B6AA4A9, 0x608A9CDF),
];
static THEME_FLORA: std::sync::Mutex<Vec<Option<ResourceKey>>> = std::sync::Mutex::new(Vec::new());

/// The FirstGroup scripts inherit GameplayThemeNN, which carries no flora (nor
/// weatherCloudType); the patch-era twin GPTGameplayThemeNN_* (same atmosphere /
/// water types) holds them (INFERRED pairing by theme number).
pub fn theme_twin(
    index: &crate::asset_catalog::ResourceIndex,
    d: &crate::planet_data::PlanetDescriptor,
) -> Option<crate::prop::PropertyList> {
    let script = crate::prop::parse(&index.read(d.script).ok()?).ok()?;
    let parent = script.0.get(&0x00B2CCCB)?.key()?;
    let gpt = GPT_THEME_TWINS.iter().find(|(a, _)| *a == parent.instance_id)?.1;
    crate::prop::parse(&index.read(ResourceKey { group_id: 0x4284A100, instance_id: gpt, type_id: 0x00B1B104 }).ok()?).ok()
}

pub fn theme_flora(
    index: &crate::asset_catalog::ResourceIndex,
    d: &crate::planet_data::PlanetDescriptor,
) -> Vec<Option<ResourceKey>> {
    let twin_props = if d.properties.0.contains_key(&PROP_THEME_FLORA) { None } else { theme_twin(index, d) };
    let Some(p) = d
        .properties
        .0
        .get(&PROP_THEME_FLORA)
        .or_else(|| twin_props.as_ref().and_then(|t| t.0.get(&PROP_THEME_FLORA)))
    else {
        return Vec::new();
    };
    p.values
        .iter()
        .map(|v| {
            let instance_id = u32::from_le_bytes(v.get(..4)?.try_into().ok()?);
            let k = ResourceKey {
                group_id: crate::baked_model::FLORA_GROUP,
                instance_id,
                type_id: crate::decode::TYPE_GMDL,
            };
            index.records.contains_key(&k).then_some(k)
        })
        .collect()
}

/// The stage planet's theme flora slots (set when the planet sky spawns;
/// read by `baked_model::scenery_where`).
pub fn current_theme_flora() -> Vec<Option<ResourceKey>> {
    // The old themed scatter (opt-in). The planet's flora is placed by
    // `planet_flora` (terrainDistribute groves, on by default); this only
    // applies with `OPENSPORE_PLANET_FLORA=0`. The frame spikes once seen
    // with it did not reproduce (planet-flora notes F5).
    if !std::env::var("OPENSPORE_THEME_FLORA").is_ok_and(|v| v == "1") {
        return Vec::new();
    }
    THEME_FLORA.lock().map(|v| v.clone()).unwrap_or_default()
}
fn set_theme_flora(slots: Vec<Option<ResourceKey>>) {
    if let Ok(mut v) = THEME_FLORA.lock() {
        *v = slots;
    }
}

/// The local hour of the stage: Creature's ecology clock, else the Tribal /
/// Civ clock (`StageClock`, started at `STAGE_START_HOUR`).
pub fn stage_hour(world: &World) -> Option<f32> {
    // Development / capture override of the shown hour.
    if let Some(h) = std::env::var("OPENSPORE_PLANET_HOUR").ok().and_then(|v| v.parse::<f32>().ok()) {
        return Some(h.rem_euclid(24.0));
    }
    let stage = world.get_resource::<State<GameStage>>()?.get().clone();
    match stage {
        GameStage::Creature => world.get_resource::<crate::game::RunProgress>().map(|r| r.species_world.time_of_day),
        GameStage::Tribal | GameStage::Civilization => world
            .get_resource::<StageClock>()
            .filter(|c| c.stage == stage)
            .map(|c| c.hour),
        _ => None,
    }
}

/// Advance the Tribal / Civ clock with its stage's hourly multiplier table
/// (Tribe = the Creature table, CityCiv its own; `notes/planet-re-notes.md` 1F).
pub fn advance_stage_clock(world: &mut World) {
    let Some(stage) = world.get_resource::<State<GameStage>>().map(|s| s.get().clone()) else {
        return;
    };
    if !matches!(stage, GameStage::Tribal | GameStage::Civilization) || !world.contains_resource::<crate::planet_render::PlanetRender>() {
        return;
    }
    let dt = world.get_resource::<Time>().map_or(0.0, |t| t.delta_secs());
    let mut clock = world
        .get_resource::<StageClock>()
        .cloned()
        .filter(|c| c.stage == stage)
        .unwrap_or(StageClock { stage: stage.clone(), hour: STAGE_START_HOUR });
    clock.hour = if stage == GameStage::Tribal {
        crate::creature_ecology::advance_clock(clock.hour, dt)
    } else {
        advance_table_clock(clock.hour, dt, &crate::planet_time::CIV_SPACE_HOURLY)
    };
    world.insert_resource(clock);
}

fn table_rate(table: &[f32; 25], hour: f32) -> f32 {
    let h = hour.rem_euclid(24.0);
    let i = (h.floor() as usize).min(23);
    let t = h - i as f32;
    table[i] * (1.0 - t) + table[i + 1] * t
}

/// The clock runs at the hourly multiplier, normalised so a full day lasts
/// defaultDayLength (as `creature_ecology::advance_clock`).
pub fn advance_table_clock(hour: f32, dt: f32, table: &[f32; 25]) -> f32 {
    if !hour.is_finite() {
        return STAGE_START_HOUR;
    }
    let steps = 24 * 32;
    let integral: f32 = (0..steps)
        .map(|i| 1.0 / table_rate(table, (i as f32 + 0.5) * 24.0 / steps as f32).max(1e-3) * 24.0 / steps as f32)
        .sum();
    let norm = integral / crate::planet_time::DAY_LENGTH;
    let mut h = hour.rem_euclid(24.0);
    let mut left = dt.max(0.0);
    while left > 0.0 {
        let step = left.min(1.0);
        h = (h + table_rate(table, h) * norm * step).rem_euclid(24.0);
        left -= step;
    }
    h
}

/// Per frame (after `planet_render::update`): fill the TerrainState slots
/// from the sun, camera and planet, re-bake the scatter cube when the camera
/// altitude row changes, and hand the state to the land, water and
/// atmosphere materials.
pub fn update(world: &mut World) {
    frame_stats(world);
    let Some(render) = world.get_resource::<crate::planet_render::PlanetRender>() else {
        // Stage left: drop the planet's sky and give the camera back.
        if world.remove_resource::<PlanetSky>().is_some() {
            restore_presentation(world);
        }
        return;
    };
    let stage_planet = render.stage.clone();
    let land = render.land_material.clone();
    capture_view(world, &stage_planet);
    if !world.contains_resource::<PlanetSky>() {
        return;
    }
    let Some(camera) = world
        .query_filtered::<&Transform, With<crate::GameCamera>>()
        .iter(world)
        .next()
        .map(|t| t.translation)
    else {
        return;
    };
    let stage = world.get_resource::<State<GameStage>>().map_or(GameStage::Creature, |s| s.get().clone());
    let hour = stage_hour(world).unwrap_or(crate::planet_time::CREATURE_START_HOUR);
    let azimuth = crate::world_presentation::tune("SUN_AZ", -55.).to_radians();
    let sun = crate::planet_time::sun_direction(hour, azimuth);
    let time = world.get_resource::<Time>().map_or(0.0, |t| t.elapsed_secs());
    let planet = &stage_planet.planet;
    let centre = stage_planet.frame.centre;
    let altitude = (camera - centre).length() - planet.radius;
    let mut sky = world.remove_resource::<PlanetSky>().unwrap();
    let p = (altitude / SKY_ALTITUDE_RANGE).clamp(0.0, 1.0) * 0.6 + 0.39;
    let lobes = sky.tables.lobes(&sky.inputs, p, sky.user_air);
    if (p - sky.baked_p).abs() > 1.0 / 128.0 {
        let cube = scatter_cube(&lobes);
        if let Some(mut slot) = world.resource_mut::<Assets<Image>>().get_mut(&sky.scatter) {
            *slot = cube;
        }
        sky.baked_p = p;
    }
    let sphere_w = ((1.0 - (altitude / (sky.thickness + 1e-6)).clamp(0.0, 1.0)) + 1.0) * 0.5;
    let mut s = [Vec4::ZERO; SKY_SLOTS];
    s[0] = sun.extend(CUTOFF);
    let w = sky.water_color;
    s[1] = Vec4::new(w[0], w[1], w[2], 1.0);
    s[2] = centre.extend(planet.water_radius());
    s[3] = Vec4::new(planet.radius, planet.altitude_range, sky.thickness, sphere_w);
    s[4] = fog_parms(stage, &sky.inputs, &lobes, altitude, hour, sphere_w);
    s[5] = Vec4::new(ATM_LUMINANCE_RANGE.0, ATM_LUMINANCE_RANGE.1, 0.0, 0.0);
    let frame = scatter_frame(sun);
    s[6] = frame[0];
    s[7] = frame[1];
    s[8] = frame[2];
    // z: the land's diffuse scale of the retail lighting (planet_light.rs:
    // the palette ramp stand-in is full scale, the retail colour map half).
    s[9] = Vec4::new(time, crate::planet_render::DEPTH_SCALE, crate::planet_light::land_diffuse_scale(), 0.0);
    if !sky.pca_frames.is_empty() {
        let f = &sky.pca_frames[((time * PCA_FPS) as usize) % sky.pca_frames.len()];
        let reg = |r: usize| Vec4::new(f[r * 4], f[r * 4 + 1], f[r * 4 + 2], f[r * 4 + 3]);
        s[10] = reg(0);
        s[11] = reg(2);
        s[12] = reg(4);
        s[13] = reg(6);
        s[14] = reg(16);
    }
    let debug = std::env::var("OPENSPORE_WATER_DEBUG").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
    // Retail land lighting (planet_light.rs) unless OPENSPORE_RETAIL_LIGHT=0.
    let retail_light = !std::env::var("OPENSPORE_RETAIL_LIGHT").is_ok_and(|v| v == "0");
    s[15] = Vec4::new(1.0, if sky.pca_frames.is_empty() { 0.0 } else { 1.0 }, debug, if retail_light { 1.0 } else { 0.0 });
    if !world.contains_resource::<crate::planet_light::TerrainLighting>() {
        let lighting = world
            .get_resource::<crate::Originals>()
            .map(|o| crate::planet_light::TerrainLighting::load(&o.resource_index))
            .unwrap_or_default();
        world.insert_resource(lighting);
    }
    s[16..24].copy_from_slice(&world.resource::<crate::planet_light::TerrainLighting>().slots());
    sky.state = s;
    if let Some(handle) = sky.material.clone() {
        if let Some(mut m) = world.resource_mut::<Assets<RetailAtmosphereMaterial>>().get_mut(&handle) {
            m.extension.sky = s;
        }
    }
    if let Some(mut m) = world.resource_mut::<Assets<crate::terrain::TerrainMaterial>>().get_mut(&land) {
        m.extension.sky = s;
        m.extension.scatter = sky.scatter.clone();
        m.extension.below_texture = sky.below.clone();
        m.base.fog_enabled = false;
    }
    let seas: Vec<Handle<crate::water::WaterMaterial>> = world
        .query_filtered::<&MeshMaterial3d<crate::water::WaterMaterial>, With<crate::terrain::WorldGround>>()
        .iter(world)
        .map(|m| m.0.clone())
        .collect();
    for sea in seas {
        if let Some(mut m) = world.resource_mut::<Assets<crate::water::WaterMaterial>>().get_mut(&sea) {
            m.extension.sky = s;
            m.extension.scatter = sky.scatter.clone();
            m.extension.pca0 = sky.pca[0].clone();
            m.extension.pca1 = sky.pca[1].clone();
            if m.base.alpha_mode != AlphaMode::Premultiplied {
                m.base.alpha_mode = AlphaMode::Premultiplied;
                m.base.fog_enabled = false;
            }
        }
    }
    // The sky is the shell: the preview sky dome and its clear colour step
    // aside (space shows where the atmosphere is thin or on the night side).
    if sky.material.is_none() {
        if !std::env::var("OPENSPORE_RETAIL_LIGHT").is_ok_and(|v| v == "0") {
            dim_preview_sky(world, &mut sky, sun, (camera - centre).normalize_or(Vec3::Y));
        }
        world.insert_resource(sky);
        return;
    }
    for mut v in world
        .query_filtered::<&mut Visibility, With<crate::world_presentation::PreviewAtmosphere>>()
        .iter_mut(world)
    {
        *v = Visibility::Hidden;
    }
    for mut c in world.query_filtered::<&mut Camera, With<crate::GameCamera>>().iter_mut(world) {
        if !matches!(c.clear_color, ClearColorConfig::Custom(_)) {
            c.clear_color = ClearColorConfig::Custom(Color::BLACK);
        }
    }
    world.insert_resource(sky);
}

/// GetInScatterTerminator: 1 while N.L is above the cutoff (cos 60), fading
/// to 0 when the sun is the cutoff below the horizon.
pub fn terminator(n: Vec3, sun: Vec3) -> f32 {
    let dp = n.dot(sun);
    if dp > CUTOFF {
        1.0
    } else {
        (dp + CUTOFF).max(0.0) / (2.0 * CUTOFF)
    }
}

/// ADAPTED night of the stand-in sky (the table-coloured shell is opt-in):
/// the preview dome's zenith / horizon and the models' fog colour follow the
/// retail in-scatter terminator at the camera (the term that takes the land
/// fog away on the night side, where retail shows space through the shell);
/// the dome's sun halo follows the sun.
fn dim_preview_sky(world: &mut World, sky: &mut PlanetSky, sun: Vec3, up: Vec3) {
    let term = terminator(up, sun);
    let factor = term.max(0.03);
    let handles: Vec<(Entity, Handle<crate::world_presentation::PreviewSkyMaterial>)> = world
        .query_filtered::<(Entity, &MeshMaterial3d<crate::world_presentation::PreviewSkyMaterial>), With<crate::world_presentation::PreviewAtmosphere>>()
        .iter(world)
        .map(|(e, m)| (e, m.0.clone()))
        .collect();
    let camera = world.query_filtered::<Entity, With<crate::GameCamera>>().iter(world).next();
    let fog = camera.and_then(|c| world.get::<DistanceFog>(c).map(|f| f.color)).unwrap_or(Color::WHITE);
    for (_, handle) in handles {
        let mut materials = world.resource_mut::<Assets<crate::world_presentation::PreviewSkyMaterial>>();
        let Some(mut m) = materials.get_mut(&handle) else {
            continue;
        };
        let base = *sky.preview_base.get_or_insert((m.extension.zenith, m.extension.horizon, m.extension.sun.w, fog));
        let scale = |v: Vec4| Vec4::new(v.x * factor, v.y * factor, v.z * factor, v.w);
        m.extension.zenith = scale(base.0);
        m.extension.horizon = scale(base.1);
        m.extension.sun = sun.normalize_or(Vec3::Y).extend(base.2 * term);
        // The haze AirColor and the cloud sprites (opacity) fade as well.
        let air = *sky.air_base.get_or_insert(m.extension.atmosphere);
        m.extension.atmosphere = air * factor;
    }
    if let (Some(c), Some(base)) = (camera, sky.preview_base) {
        if let Some(mut f) = world.get_mut::<DistanceFog>(c) {
            let l = base.3.to_linear();
            f.color = Color::linear_rgba(l.red * factor, l.green * factor, l.blue * factor, l.alpha);
        }
    }
    // The adapted rim shell's AirColor glow (planet_render) fades the same way.
    let shells: Vec<Handle<crate::planet_render::AtmosphereMaterial>> = world
        .query_filtered::<&MeshMaterial3d<crate::planet_render::AtmosphereMaterial>, With<crate::planet_render::PlanetAtmosphereShell>>()
        .iter(world)
        .map(|m| m.0.clone())
        .collect();
    for handle in shells {
        let mut materials = world.resource_mut::<Assets<crate::planet_render::AtmosphereMaterial>>();
        if let Some(mut m) = materials.get_mut(&handle) {
            let base = *sky.shell_base.get_or_insert(m.extension.color.w);
            m.extension.color.w = base * factor;
        };
    }
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        info!("planet sky: stand-in sky night factor {factor:.2} (terminator {term:.2})");
    }
}

fn restore_presentation(world: &mut World) {
    for mut v in world
        .query_filtered::<&mut Visibility, With<crate::world_presentation::PreviewAtmosphere>>()
        .iter_mut(world)
    {
        *v = Visibility::Inherited;
    }
    for mut c in world.query_filtered::<&mut Camera, With<crate::GameCamera>>().iter_mut(world) {
        if matches!(c.clear_color, ClearColorConfig::Custom(col) if col == Color::BLACK) {
            c.clear_color = ClearColorConfig::Default;
        }
    }
}

/// `OPENSPORE_FRAME_STATS=1`: mean / max frame time of each 60 planet frames
/// (development measurement for the frame-time report).
fn frame_stats(world: &mut World) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("OPENSPORE_FRAME_STATS").is_some()) {
        return;
    }
    if !world.contains_resource::<crate::planet_render::PlanetRender>() {
        return;
    }
    static SAMPLES: std::sync::Mutex<Vec<f32>> = std::sync::Mutex::new(Vec::new());
    // Wall clock: quick captures run a fixed virtual timestep.
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let now = std::time::Instant::now();
    let dt = LAST.lock().unwrap().replace(now).map_or(0.0, |t| (now - t).as_secs_f32() * 1000.0);
    let mut s = SAMPLES.lock().unwrap();
    s.push(dt);
    if s.len() == 60 {
        let mean = s.iter().sum::<f32>() / 60.0;
        let max = s.iter().cloned().fold(0.0, f32::max);
        eprintln!("frame stats: mean {mean:.2} ms, max {max:.2} ms (60 frames)");
        s.clear();
    }
}

/// `OPENSPORE_CAPTURE_VIEW=water[,height,distance]` (captures): put the game
/// camera near the closest sea to the stage origin, looking across it toward
/// the horizon, so water, shore and sky share the frame.
fn capture_view(world: &mut World, stage: &crate::planet_query::StagePlanet) {
    let Ok(spec) = std::env::var("OPENSPORE_CAPTURE_VIEW") else {
        return;
    };
    let mut parts = spec.split(',');
    if parts.next() != Some("water") {
        return;
    }
    let height: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(12.0);
    let back: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(40.0);
    static TARGET: std::sync::OnceLock<Option<(Vec3, Vec3)>> = std::sync::OnceLock::new();
    let planet = &stage.planet;
    let frame = &stage.frame;
    let found = *TARGET.get_or_init(|| {
        let origin = frame.dir_to_local(Vec3::Y);
        let helper = if origin.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
        let a = origin.cross(helper).normalize();
        let b = origin.cross(a);
        for ring in 1..400 {
            let ang = ring as f32 * 2.0 / planet.radius;
            for k in 0..64 {
                let t = k as f32 / 64.0 * std::f32::consts::TAU;
                let d = (origin * ang.cos() + (a * t.cos() + b * t.sin()) * ang.sin()).normalize();
                if planet.height(d) < planet.water_radius() - 2.0 {
                    return Some((d, (d - origin).normalize_or(a)));
                }
            }
        }
        None
    });
    let Some((water_dir, heading)) = found else {
        return;
    };
    let water_world = frame.dir_to_world(water_dir);
    let heading_world = frame.dir_to_world(heading);
    let up = water_world.normalize();
    let shore = frame.centre + up * planet.water_radius();
    let eye_base = shore - (heading_world - up * heading_world.dot(up)).normalize_or(Vec3::X) * back;
    let eye_up = (eye_base - frame.centre).normalize();
    let ground = planet.height(frame.dir_to_local(eye_up)).max(planet.water_radius());
    let eye = frame.centre + eye_up * (ground + height);
    let target = shore + (heading_world - up * heading_world.dot(up)).normalize_or(Vec3::X) * 120.0;
    let cameras: Vec<Entity> = world.query_filtered::<Entity, With<crate::GameCamera>>().iter(world).collect();
    for c in cameras {
        if let Some(mut t) = world.get_mut::<Transform>(c) {
            *t = Transform::from_translation(eye).looking_at(target, eye_up);
        }
    }
}

fn insert_neutral_cube(images: Option<ResMut<Assets<Image>>>) {
    if let Some(mut images) = images {
        let _ = images.insert(&NEUTRAL_CUBE, rgba_cube(1, vec![0; 24]));
    }
}

/// Register the sky (called from `planet_render::build`).
pub fn build(app: &mut App) {
    bevy::asset::embedded_asset!(app, "shaders/planet_sky.wesl");
    crate::world_presentation::dev_shader_override(app, "planet_sky.wesl");
    app.add_plugins(MaterialPlugin::<RetailAtmosphereMaterial>::default());
    app.add_systems(Startup, insert_neutral_cube);
    app.add_systems(Update, advance_stage_clock);
    app.add_systems(
        PostUpdate,
        update
            .after(crate::planet_render::update)
            .before(bevy::transform::TransformSystems::Propagate),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_table(rgba: [u8; 4]) -> Table {
        Table { width: 64, height: 64, bgra: [rgba[2], rgba[1], rgba[0], rgba[3]].repeat(64 * 64) }
    }
    fn tables() -> SkyTables {
        SkyTables {
            atmosphere: flat_table([100, 150, 250, 255]),
            density: flat_table([200, 200, 200, 255]),
            dust: flat_table([210, 60, 10, 255]),
            sunset: flat_table([150, 60, 230, 255]),
            water: None,
        }
    }

    #[test]
    fn water_colour_is_the_table_column_of_the_temperature_score() {
        // 64 x 1 table whose column c is (c, 0, 255 - c): 0xFBF540 reads
        // floor((w - 1) T).
        let mut bgra = Vec::new();
        for c in 0..64u8 {
            bgra.extend_from_slice(&[255 - c, 0, c, 255]);
        }
        let table = Table { width: 64, height: 1, bgra };
        let at = |t: f32| table.water_rgb(t);
        assert!((at(0.5)[0] - 31.0 / 255.0).abs() < 1e-6, "{:?}", at(0.5));
        assert!((at(0.0)[0]).abs() < 1e-6 && (at(1.0)[0] - 63.0 / 255.0).abs() < 1e-6);
        assert!((at(0.99)[2] - (255.0 - 62.0) / 255.0).abs() < 1e-6);
    }

    /// Water type 2 (Thorny, Volcanoes) is a red sea in the installed data:
    /// column 31 (T 0.5) of 0x0536DD52 is (161, 53, 57). Type 11, the
    /// default of the Space planet-record path, is a blue-violet one.
    #[test]
    #[ignore = "requires the installed game"]
    fn installed_water_types_are_hues_of_one_wheel() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let red = water_table(&index, 2).unwrap().water_rgb(0.5);
        assert!(red[0] > 0.55 && red[1] < 0.3 && red[2] < 0.3, "{red:?}");
        let blue = water_table(&index, 11).unwrap().water_rgb(0.5);
        assert!(blue[2] > 0.7 && blue[0] < 0.3, "{blue:?}");
        // Every one of the 16 tables exists.
        for t in 0..16 {
            assert!(water_table(&index, t).is_some(), "water type {t}");
        }
    }

    #[test]
    fn lobes_follow_the_three_keys_and_the_sunset_table() {
        let inputs = SkyInputs { temperature: 0.5, atmosphere: 0.5, atmosphere_type: 0, water_type: 0 };
        let l = tables().lobes(&inputs, 0.39, None);
        // 2p = 0.78: key 0 -> 1 at 0.78.
        assert!((l.phase[1] - (0.35 + 0.3 * 0.78)).abs() < 1e-5);
        assert!((l.phase[2] - (-0.8 + 0.2 * 0.78)).abs() < 1e-5);
        let sunset = 230.0 / 255.0;
        assert!((l.sunset - sunset).abs() < 1e-5);
        assert!((l.weight[0] - 10.0 * 1.2 * sunset * (1.5 - 0.45 * 0.78)).abs() < 1e-4);
        assert!((l.color[0][2] - 250.0 / 255.0).abs() < 1e-5);
        // High camera: keys 1 -> 2.
        let h = tables().lobes(&inputs, 0.99, None);
        // User colour branch: lobe 1 moves toward half saturation by 1 - p.
        let u = tables().lobes(&inputs, 0.39, Some([0.2, 0.4, 1.0]));
        assert!((u.color[0][2] - 1.0).abs() < 1e-5 && u.color[0][0] > 0.2 && u.color[0][0] < 0.6, "{:?}", u.color[0]);
        assert!((h.phase[1] - (0.65 + 0.15 * 0.98)).abs() < 1e-4);
    }

    #[test]
    fn scatter_peaks_on_the_sun_lobe_and_is_flat_on_the_far_side() {
        let inputs = SkyInputs { temperature: 0.5, atmosphere: 0.5, atmosphere_type: 0, water_type: 0 };
        let l = tables().lobes(&inputs, 0.39, None);
        let toward = scatter(&l, Vec3::Z);
        let side = scatter(&l, Vec3::X);
        let away = scatter(&l, -Vec3::Z);
        assert!(toward[3] > side[3] * 5.0, "{toward:?} {side:?}");
        // sat(dir . sun) makes the whole far hemisphere equal to the side.
        assert_eq!(side, away);
        assert!(side[2] > 0.5, "the sky table colour dominates: {side:?}");
    }

    #[test]
    fn scatter_frame_maps_the_anti_sun_direction_to_plus_z() {
        let sun = Vec3::new(0.3, 0.8, -0.2).normalize();
        let f = scatter_frame(sun);
        let z = Vec3::new(f[0].truncate().dot(-sun), f[1].truncate().dot(-sun), f[2].truncate().dot(-sun));
        assert!((z - Vec3::Z).length() < 1e-5);
    }

    #[test]
    fn fog_parameters_follow_the_tables() {
        let inputs = SkyInputs { temperature: 0.5, atmosphere: 0.5, atmosphere_type: 0, water_type: 0 };
        let l = tables().lobes(&inputs, 0.39, None);
        let f = fog_parms(GameStage::Creature, &inputs, &l, 0.0, 10.0, 1.0);
        // density = lerp(0.001, 0.009, 0.5) x 1.25 x 1 x 1.0; min distance 70.
        assert!((f.x + 0.005 * 1.25).abs() < 1e-6);
        assert!((f.z - 70.0).abs() < 1e-4);
        // No atmosphere: +1000 bias pushes the fog out of sight.
        let thin = SkyInputs { atmosphere: 0.0, ..inputs };
        assert!(fog_parms(GameStage::Creature, &thin, &l, 0.0, 10.0, 1.0).z > 900.0);
        assert!((atmosphere_thickness(0.5) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn civ_clock_lasts_one_day_length() {
        // Time until the clock wraps once, from midnight: one defaultDayLength.
        let (mut h, mut t) = (0.0f32, 0.0f32);
        loop {
            let n = advance_table_clock(h, 0.25, &crate::planet_time::CIV_SPACE_HOURLY);
            t += 0.25;
            if n < h || t > 2.0 * crate::planet_time::DAY_LENGTH {
                break;
            }
            h = n;
        }
        let ratio = t / crate::planet_time::DAY_LENGTH;
        assert!((ratio - 1.0).abs() < 0.03, "{t} s");
    }

    #[test]
    fn pcaw_frames_decode() {
        let mut b = Vec::new();
        for w in [0xFFEE_FFEEu32, 2, 3, 2, 16, 1] {
            b.extend(w.to_le_bytes());
        }
        for i in 0..2 * PCA_FLOATS {
            b.extend((i as f32).to_le_bytes());
        }
        let f = decode_pcaw(&b).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[1][0], PCA_FLOATS as f32);
    }

    #[test]
    #[ignore = "requires the installed game"]
    fn installed_sky_tables_and_pca_decode() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let tables = SkyTables::load(&index, 0, 11).expect("sky tables");
        assert_eq!((tables.atmosphere.width, tables.atmosphere.height), (64, 64));
        assert!(tables.water.is_some());
        let frames = decode_pcaw(&index.read(PCAW).unwrap()).unwrap();
        assert_eq!(frames.len(), 30);
        // The mean normal is ~(0.5, 1, 0.5) x 255.
        assert!((frames[0][64] - 127.5).abs() < 2.0 && frames[0][65] > 240.0);
        let curves = crate::texture::decode_rw4_mips(&index.read(PACKED_CURVES).unwrap()).unwrap();
        assert_eq!((curves[0].width, curves[0].height), (1, 64));
    }
}
