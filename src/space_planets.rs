//! Space stage planets from the planet engine (planet-re 4E step 7).
//!
//! * Orbit / system view: every world is drawn from its own terrain script,
//!   generated on a worker thread at `ORBIT_RES` texels per face and cached:
//!   the homeworld from the FirstGroup script of the save seed (the same
//!   script and seed as the Creature..Civ planet, `terrain::PlanetCache`),
//!   other worlds and moons from the T0-T3 pool script the galaxy recorded
//!   (planet-re 2A). The body becomes a cube-sphere displaced by the real
//!   height field (altRange / R) with a colour atlas baked from the planet's
//!   colour ramp, beaches, cliffs and water, plus the atmosphere shell.
//! * Planet view: the same planet at the retail resolution (512, generated on
//!   demand, the last few kept) drawn with the planet renderer's quadtree
//!   chunks and the planet-mode terrain material at `PlanetView::unit` scene
//!   units per retail unit, a water sphere, the atmosphere shell, a sky haze
//!   by altitude (planetAtmosphereStart / End) and the colonies' cities on
//!   flattened ground. The UFO and the camera follow the generated ground
//!   (`space_view::TerrainProbe`).
//! * Terraforming: the water level and the dead -> living colour ramp follow
//!   the world's ecology (`look_for`, ADAPTED where the retail rule was not
//!   traced).
//!
//! Only apps that render (the terrain material is registered) generate
//! planets; headless runs keep the plain spheres, so simulation tests do not
//! depend on a worker thread.

use crate::{
    asset_catalog::{ResourceIndex, PROP},
    dbpf::ResourceKey,
    planet_query::{face_point, Planet, PlanetFrame, BEACH_LEVEL},
    planet_render::{AtmosphereMaterial, AtmosphereShell, NodeKey},
    stages::{GameStage, StageOwned},
};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, Mutex};

mod clouds;
mod fluid;
pub mod impostor;
pub mod simdata;
mod sky;

/// Texels per cube face of the orbit-view planets (the retail cube is 512).
pub const ORBIT_RES: usize = 128;
/// Cells per cube face of the orbit mesh.
const ORBIT_CELLS: usize = 32;
/// Full-resolution planets kept for the planet view (least recently used go).
const FULL_KEEP: usize = 3;

// ---------------------------------------------------------------------------
// Terraform tuning (TerraformTuning 02AE0C7E!A6D1BC70, original data)
// ---------------------------------------------------------------------------

pub const MIN_WATER_WITH_PERFECT_SCORE: f32 = 0.4;
pub const MIN_WATER_RADIUS: f32 = 0.45;
pub const MAX_WATER: f32 = 0.7;
pub const MIN_WATER_VACUUM_THRESHOLD: f32 = 0.2;
pub const MIN_WATER_HEAT_THRESHOLD: (f32, f32) = (0.8, 0.9);
pub const ICE_THRESHOLD: f32 = 0.2;
pub const LAVA_THRESHOLD: f32 = 0.95;
pub const LAVA_MAX_COVERAGE: f32 = 0.4;
pub const MIN_WATER_ICE_THRESHOLD: f32 = 0.15;
pub const MIN_WATER_ICE_ATMOSPHERE: f32 = 0.0;
pub const MIN_WATER_ICE_VACUUM: f32 = 0.0;
pub const MIN_WATER_ATMOSPHERE: f32 = 0.8;
pub const MAX_WATER_THRESHOLD: f32 = 0.7;

/// kAtmThicknessByAtmosphereScore (PlanetMaterial 0x0243AD2B!0xD985994C).
pub const ATM_THICKNESS_BY_SCORE: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.1, 0.0),
    (0.11, 1.15),
    (0.25, 1.15),
    (0.26, 1.35),
    (0.39, 1.35),
    (0.4, 1.5),
    (0.59, 1.5),
    (0.6, 1.6),
    (0.74, 1.6),
    (0.75, 1.65),
    (0.89, 1.65),
    (0.9, 1.75),
    (1.0, 1.75),
];

/// Cloud colours by temperature score band <= 0.2 / 0.4 / 0.6 / 0.8 / 1.0
/// (weather 0x0243AD2B!0x12E95823: ice / cold / warm grey 0.95, hot, lava),
/// as cPlanetGfx_CreateCloudsEffect 0xE978C0 picks them (VERIFIED bands,
/// space-visuals: hot above 0.6, lava above 0.8).
pub fn cloud_color(temperature: f32) -> [f32; 3] {
    clouds::cloud_color(temperature)
}

/// Cloud cover seen from space: cPlanetGfx_CreateCloudsEffect picks the thin /
/// medium / thick effect by atmosphere score (<= 0.25, else, >= 0.75), but
/// SG_solar_planet_clouds_thin / medium / thickAtmo~ (0x6B306EEC / EB / EA)
/// are empty effects in every installed quality tier (VERIFIED, space-visuals):
/// retail draws no clouds on the orbiting worlds. The planet view has the
/// weather clouds (`clouds`).
pub fn cloud_alpha(_atmosphere: f32) -> f32 {
    0.0
}

fn curve(points: &[(f32, f32)], x: f32) -> f32 {
    crate::space_view::curve(points, x)
}

/// Atmosphere thickness (0..1.75) of an atmosphere score.
pub fn atmosphere_thickness(atmosphere: f32) -> f32 {
    curve(ATM_THICKNESS_BY_SCORE, atmosphere.clamp(0.0, 1.0))
}

/// The water the climate allows (0xFC20A0, `spaceTerraform*` tuning of
/// `TerraformTuning`, the loader at 0xFC24A0 stores the values at 0x16D6F0C..):
/// `(least, most)` water fraction at an atmosphere and temperature score.
///
/// * least: the larger of the bullseye water `MinWaterWithPerfectScore x
///   (1 - d / MinWaterRadius)` (d the distance of (T, A) from the bullseye
///   (0.5, 0.5), none beyond the radius), the ice water `lerp(IceVacuum,
///   IceAtmosphere, A) x (IceThreshold - T) / IceThreshold` (cold worlds) and
///   the atmosphere water `(A - VacuumThreshold) x MinWaterAtmosphere / (1 -
///   VacuumThreshold)` faded out by the heat, `1 - smoothstep(T)` between
///   `HeatThresholdLow` and `High`;
/// * most: the least, or `MaxWater x min(1, (MaxWaterThreshold - q) /
///   (MaxWaterThreshold - 0.5))` with `q = T^2 + (A - 1)^2`, the larger.
pub fn water_range(temperature: f32, atmosphere: f32) -> (f32, f32) {
    let (t, a) = (temperature, atmosphere);
    let d = Vec2::new(t - 0.5, a - 0.5).length().min(0.5);
    let perfect = (MIN_WATER_WITH_PERFECT_SCORE * (MIN_WATER_RADIUS - d) / MIN_WATER_RADIUS).max(0.0);
    let ice = ((MIN_WATER_ICE_VACUUM * (1.0 - a) + MIN_WATER_ICE_ATMOSPHERE * a)
        * (MIN_WATER_ICE_THRESHOLD - t)
        / MIN_WATER_ICE_THRESHOLD)
        .max(0.0);
    let atmosphere_water =
        (a - MIN_WATER_VACUUM_THRESHOLD) * MIN_WATER_ATMOSPHERE / (1.0 - MIN_WATER_VACUUM_THRESHOLD);
    let (lo, hi) = MIN_WATER_HEAT_THRESHOLD;
    let heat = ((t.clamp(lo, hi) - lo) / (hi - lo)).clamp(0.0, 1.0);
    let hot = 1.0 - heat * heat * (3.0 - 2.0 * heat);
    let least = ice.max(atmosphere_water * hot).max(perfect);
    let q = t * t + (a - 1.0) * (a - 1.0);
    let cap = ((MAX_WATER_THRESHOLD - q) / (MAX_WATER_THRESHOLD - 0.5)).min(1.0) * MAX_WATER;
    (least, least.max(cap))
}

/// The water fraction (WTA.x) a world shows in a climate (0xFC22C0: the
/// terrain script's own water held in `water_range`, and no less than the lava
/// coverage `(T - LavaThreshold) x LavaMaxCoverage / (1 - LavaThreshold)` of a
/// very hot world, which the world shows as a lava sea).
pub fn climate_water(base_water: f32, temperature: f32, atmosphere: f32) -> f32 {
    let (least, most) = water_range(temperature, atmosphere);
    let water = base_water.max(least).min(most);
    let lava = ((temperature - LAVA_THRESHOLD) * LAVA_MAX_COVERAGE / (1.0 - LAVA_THRESHOLD)).max(0.0);
    water.max(lava)
}

// ---------------------------------------------------------------------------
// Colour ramps (planet_color_ramps_{dead,living,ice,lava}, 64 x 64 32bitImage)
// ---------------------------------------------------------------------------

const RAMP_TYPE: u32 = 0x03E4_21ED;
const RAMP_DEAD: u32 = 0x362A_C39E;
const RAMP_LIVING: u32 = 0x2EC4_5F93;
const RAMP_ICE: u32 = 0x7C6B_9FD9;
const RAMP_LAVA: u32 = 0x8856_6056;
/// `0xF30D0A76.32bitImage`: the beach and cliff band colours (0xFBBBD0 stores its rows at mgr +0x50C).
const RAMP_BANDS: u32 = 0xF30D_0A76;
const RAMP_SIZE: usize = 64;

/// The installed planet colour images as the game's texture memory holds them (BGRA bytes,
/// row-major, 64 wide): the four ramps (64 rows) and the band colours of beaches and cliffs
/// (`0xF30D0A76.32bitImage`, 10 rows), the inputs of the life table (0xFB8760).
#[derive(Clone, Debug, Default)]
pub struct RawRamps {
    pub dead: Vec<u8>,
    pub living: Vec<u8>,
    pub ice: Vec<u8>,
    pub lava: Vec<u8>,
    pub bands: Vec<u8>,
}

/// The four installed planet colour ramps, sRGB 0..1, row-major 64 x 64, and the retail
/// life tables built from them (one per ramp column, cached).
#[derive(Clone, Debug)]
pub struct Ramps {
    pub dead: Vec<[f32; 3]>,
    pub living: Vec<[f32; 3]>,
    pub ice: Vec<[f32; 3]>,
    pub lava: Vec<[f32; 3]>,
    /// The images as stored; `None` when only the colours are known (the life table needs them).
    pub raw: Option<RawRamps>,
    tables: Arc<Mutex<HashMap<usize, Arc<simdata::LifeTable>>>>,
}

/// The retail land colours of a world (`simdata`): the land ramp as the Above pass shades the
/// map values 0 .. 1, the beach and cliff colours and the life table column.
struct RetailLook {
    ramp: Vec<[f32; 3]>,
    beach: [f32; 3],
    cliff: [f32; 3],
    column: f32,
}

impl Ramps {
    pub fn load(index: &ResourceIndex) -> Option<Self> {
        let read = |instance: u32, rows: usize| -> Option<Vec<u8>> {
            let key = ResourceKey {
                group_id: 0,
                instance_id: instance,
                type_id: RAMP_TYPE,
            };
            let bitmap = crate::bitmap::parse(RAMP_TYPE, &index.read(key).ok()?).ok()?;
            if bitmap.width as usize != RAMP_SIZE || bitmap.height as usize != rows {
                return None;
            }
            Some(bitmap.data)
        };
        Some(Self::from_raw(RawRamps {
            dead: read(RAMP_DEAD, RAMP_SIZE)?,
            living: read(RAMP_LIVING, RAMP_SIZE)?,
            ice: read(RAMP_ICE, RAMP_SIZE)?,
            lava: read(RAMP_LAVA, RAMP_SIZE)?,
            bands: read(RAMP_BANDS, simdata::BAND_IMAGE_ROWS)?,
        }))
    }

    /// The ramps from the stored images (BGRA bytes).
    pub fn from_raw(raw: RawRamps) -> Self {
        let colours = |bytes: &[u8]| -> Vec<[f32; 3]> {
            bytes
                .chunks_exact(4)
                .map(|p| [p[2] as f32 / 255.0, p[1] as f32 / 255.0, p[0] as f32 / 255.0])
                .collect()
        };
        Self {
            dead: colours(&raw.dead),
            living: colours(&raw.living),
            ice: colours(&raw.ice),
            lava: colours(&raw.lava),
            raw: Some(raw),
            tables: Arc::default(),
        }
    }

    /// Column `x` (terrainScriptPlanetInfo selector) as a ramp from low to
    /// high ground. INFERRED: a column is one ramp, rows run with elevation
    /// (the ice and dead columns brighten toward the last row: snow caps).
    /// The retail look no longer goes through this (`retail_look`).
    pub fn column(image: &[[f32; 3]], x: f32) -> Vec<[f32; 3]> {
        let x = (x.round() as usize).min(RAMP_SIZE - 1);
        (0..RAMP_SIZE).map(|row| image[row * RAMP_SIZE + x]).collect()
    }

    /// The life table of a ramp column (0xFB8760 with the dead / living / ice / lava images).
    fn life_table(&self, column: usize) -> Option<Arc<simdata::LifeTable>> {
        let raw = self.raw.as_ref()?;
        if raw.dead.len() != simdata::RAMP_IMAGE_BYTES
            || raw.living.len() != simdata::RAMP_IMAGE_BYTES
            || raw.ice.len() != simdata::RAMP_IMAGE_BYTES
            || raw.lava.len() != simdata::RAMP_IMAGE_BYTES
        {
            return None;
        }
        let mut tables = self.tables.lock().ok()?;
        Some(
            tables
                .entry(column)
                .or_insert_with(|| {
                    Arc::new(simdata::LifeTable::build(&raw.dead, &raw.living, &raw.ice, &raw.lava, column, column))
                })
                .clone(),
        )
    }

    /// The retail land colours of a world: the ramp column from `terrainScriptPlanetInfo.x`
    /// (`simdata::ramp_column`), the life table column from the temperature and atmosphere
    /// scores (`simdata::life_column`, 0xFB88A0) and the band colours by temperature
    /// (`simdata::band_colours`, 0xFB9AA0 / 0xFB9C70). The ramp is the Above pass at the 64
    /// map values `i / 63` (the Above pass is `simdata::above_pass`), brightened by
    /// `RAMP_GAIN` as the earlier ramps were.
    fn retail_look(&self, info_x: f32, temperature: f32, atmosphere: f32) -> Option<RetailLook> {
        let ramp_col = simdata::ramp_column(info_x);
        let table = self.life_table(ramp_col)?;
        let bands = self.raw.as_ref()?.bands.as_slice();
        if bands.len() < simdata::RAMP_IMAGE_SIZE * simdata::BAND_IMAGE_ROWS * 4 {
            return None;
        }
        let column = simdata::life_column(temperature, 1.0 - atmosphere);
        let gain = |c: [f32; 3]| c.map(|x| (x * RAMP_GAIN).clamp(0.0, 1.0));
        let ramp = (0..RAMP_SIZE).map(|i| gain(table.above(i as f32 / (RAMP_SIZE - 1) as f32, column).0)).collect();
        let (beach, cliff) = simdata::band_colours(bands, ramp_col, ramp_col, temperature);
        Some(RetailLook { ramp, beach: gain(beach), cliff: gain(cliff), column })
    }
}

/// What lies in a world's basins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fluid {
    Water,
    Ice,
    Lava,
}

/// Everything the drawing needs about a world's colours and climate.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// Land colour ramp, sRGB, low to high ground: for the other worlds the Above pass's
    /// colour (`simdata::above_pass` on the retail life table) at the map values 0 .. 1.
    pub ramp: Vec<[f32; 3]>,
    pub beach: [f32; 3],
    pub cliff: [f32; 3],
    pub water: [f32; 3],
    pub fluid: Fluid,
    /// mWaterLevel on the h scale (2 W - 1).
    pub water_level: f32,
    pub air: [f32; 3],
    /// kAtmThicknessByAtmosphereScore value (0 = airless).
    pub thickness: f32,
    /// The land theme's own ramp (the homeworld, as in the earlier stages).
    pub home: bool,
    /// The climate now (temperature, atmosphere scores): T / A of the planet
    /// view's TerrainState (`sky`).
    pub climate: (f32, f32),
    /// The life table column and fraction that `ramp` was shaded with (`simdata::life_column`;
    /// 0 when the look has no life table: the homeworld's theme ramp, no installed ramps).
    pub life_column: f32,
}

/// Re-bake key of a look: the visible state only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LookKey {
    t_score: u8,
    fluid: Fluid,
    water: i32,
    /// The sea colour (8-bit): it follows the temperature score through the
    /// water colour table, so a change re-bakes the orbit atlas.
    sea: [u8; 3],
    /// The life table column and its fraction (1/16 steps): the land colour follows them.
    life: i32,
}

/// ADAPTED: ramp brightness. The installed ramps are dark (mean ~0.3); the
/// terrain program multiplies them by kTerrainBrightness 1.1 and the day light
/// colour 1.75 where the preview lights with white.
const RAMP_GAIN: f32 = 1.6;

/// Inputs of `look_for` that come from the world's records.
#[derive(Clone, Copy, Debug)]
pub struct Climate {
    /// terrainScriptPlanetInfo x / y. x selects the ramp column of every ramp (VERIFIED,
    /// `simdata::ramp_column`: 0xFBD850 stores `(x - 0.5) / 64` and 0xFBBBD0 reads the column
    /// from it for the dead, living, ice and lava ramps alike); y only reaches the dead
    /// ramp in the scenario game mode, so the land colour does not use it.
    pub info: [f32; 2],
    /// The terrain script's water (WTA.x).
    pub base_water: f32,
    /// The climate now (ecology) and its T-score.
    pub now: (f32, f32),
    pub t_score: u8,
}

/// A world's look from its climate. The homeworld keeps the land theme's
/// palette (the ramp the earlier stages drew it with). Other worlds: the retail
/// land colour (`Ramps::retail_look`): the life table of the world's ramp column
/// (0xFB8760: ice ramp at column 0, living at 0x7F, lava at 0xFF, the dead ramp between,
/// each skewed into its neighbours) read at the column the temperature and atmosphere
/// select (0xFB88A0), and the band colours of beaches and cliffs by temperature
/// (0xFB9AA0 / 0xFB9C70). ADAPTED: the map value of the Above pass is the elevation
/// fraction (`Look::surface`); without the installed images the earlier stand-ins stay
/// (a grey ramp, beaches and cliffs half the theme's colours, half the ramp's).
pub fn look_for(
    home: bool,
    theme: Option<&crate::world_catalog::PlanetPalette>,
    ramps: Option<&Ramps>,
    c: Climate,
) -> Look {
    let (temperature, atmosphere) = c.now;
    let fluid = if home {
        Fluid::Water
    } else if temperature < ICE_THRESHOLD {
        Fluid::Ice
    } else if temperature > LAVA_THRESHOLD {
        Fluid::Lava
    } else {
        Fluid::Water
    };
    let water = if home { c.base_water } else { climate_water(c.base_water, temperature, atmosphere) };
    let theme_beach = theme.map_or([0.76, 0.7, 0.52], |p| p.beach);
    let theme_cliff = theme.map_or([0.42, 0.38, 0.34], |p| p.cliff);
    let theme_water = theme.map_or([0.06, 0.18, 0.32], |p| p.water);
    let theme_air = theme.map_or([0.55, 0.72, 1.0], |p| p.atmosphere);
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t);
    let retail = if home { None } else { ramps.and_then(|r| r.retail_look(c.info[0], temperature, atmosphere)) };
    let ramp: Vec<[f32; 3]> = match (home, theme, retail.as_ref()) {
        (true, Some(p), _) => (0..64).map(|i| p.sample(i as f32 / 63.0)).collect(),
        (false, _, Some(retail)) => retail.ramp.clone(),
        _ => (0..64)
            .map(|i| mix([0.32, 0.3, 0.24], [0.62, 0.58, 0.52], i as f32 / 63.0))
            .collect(),
    };
    let (beach, cliff) = if home {
        (theme_beach, theme_cliff)
    } else if let Some(retail) = retail.as_ref() {
        (retail.beach, retail.cliff)
    } else {
        (mix(theme_beach, ramp[2], 0.5), mix(theme_cliff, ramp[40], 0.5))
    };
    let water_color = match fluid {
        Fluid::Water => theme_water,
        Fluid::Ice => [0.80, 0.86, 0.92],
        Fluid::Lava => [1.0, 0.33, 0.04],
    };
    let air = if home {
        theme_air
    } else if temperature < ICE_THRESHOLD {
        mix(theme_air, [0.78, 0.86, 1.0], 0.5)
    } else if temperature > 0.6 {
        mix(theme_air, cloud_color(temperature), 0.6)
    } else {
        theme_air
    };
    Look {
        ramp,
        beach,
        cliff,
        water: water_color,
        fluid,
        water_level: 2.0 * water - 1.0,
        air,
        thickness: atmosphere_thickness(atmosphere),
        home,
        climate: c.now,
        life_column: retail.as_ref().map_or(0.0, |r| r.column),
    }
}

impl Look {
    fn key(&self, t_score: u8) -> LookKey {
        LookKey {
            t_score: if self.home { 3 } else { t_score },
            fluid: self.fluid,
            water: (self.water_level * 200.0).round() as i32,
            sea: self.water.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
            life: (self.life_column * 16.0).round() as i32,
        }
    }

    fn ramp_at(&self, t: f32) -> [f32; 3] {
        let n = self.ramp.len() - 1;
        let x = t.clamp(0.0, 1.0) * n as f32;
        let (a, b) = (self.ramp[x.floor() as usize], self.ramp[(x.ceil() as usize).min(n)]);
        let f = x.fract();
        [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * f)
    }

    /// The colour of the ground (or the sea) at a direction, as the planet's
    /// terrain program shades it without detail textures: the ramp over 0.6 x
    /// the altitude range above the water (ADAPTED: the elevation fraction stands in for the
    /// per-texel map value of the Above pass, whose producer is not decoded), the beach band
    /// (mBeachLevel), the cliff colour between the 32 and 45 degree gradients, water
    /// darkening with depth (kWaterFogMaxDepth 15).
    pub fn surface(&self, planet: &Planet, dir: Vec3) -> ([f32; 3], f32) {
        let h = planet.h_at(dir);
        let wl = self.water_level;
        let alt = planet.altitude_range;
        if h < wl {
            let depth = (wl - h) * alt;
            let shallow = [0, 1, 2].map(|k| self.water[k] + (self.beach[k] - self.water[k]) * 0.35);
            let deep = self.water.map(|v| v * 0.55);
            let t = (depth / 15.0).clamp(0.0, 1.0);
            let c = [0, 1, 2].map(|k| shallow[k] + (deep[k] - shallow[k]) * t);
            let glow = if self.fluid == Fluid::Lava { 1.0 } else { 0.0 };
            return (c, glow);
        }
        let above = (h - wl) * alt;
        let span = alt * (1.0 - wl).max(0.05) * 0.6;
        let mut c = self.ramp_at(above / span);
        // grading: DESAT 0.2, ALBEDO 0.9 (terrain_layers).
        let lum = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
        c = c.map(|v| (v + (lum - v) * 0.2) * 0.9);
        // Beach pass (TerrainBrushTextureBeachShader 0x7000002D, `simdata::beach_alpha`):
        // the beach colour is full up to (mBeachLevel + mWaterLevel) x altitude range
        // and fades over one altitude unit; the cliff pass (0x7000002E) is drawn over it
        // as a second over blend. ADAPTED: the port's cliff alpha below (the normal
        // map's w channel the retail program reads is not modelled).
        let beach = simdata::beach_alpha(h, simdata::beach_top(BEACH_LEVEL, wl, alt));
        let up = dir.normalize();
        let slope = planet.ground_normal(dir).dot(up);
        let cliff = 1.0
            - smoothstep(
                crate::planet_query::MAX_CLIFF_GRADIENT_DEG.to_radians().cos(),
                crate::planet_query::MIN_CLIFF_GRADIENT_DEG.to_radians().cos(),
                slope,
            );
        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t);
        c = simdata::blend_over(c, self.beach, beach);
        c = lerp(c, self.cliff, cliff);
        (c, 0.0)
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ---------------------------------------------------------------------------
// Generation (worker thread)
// ---------------------------------------------------------------------------

/// Where a world's terrain comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerrainSource {
    /// The homeworld of a save seed (`terrain::generate_planet_cache`).
    Home(u64),
    /// A terrain script of the Space pools (group, instance).
    Script(u32, u32),
}

/// A generated world.
pub struct Generated {
    pub planet: Arc<Planet>,
    pub info: [f32; 2],
    pub wta: [f32; 3],
    /// The homeworld's stage origin (planet-local), at full resolution.
    pub origin: Option<Vec3>,
    pub name: String,
    /// terrainAtmosphereType / terrainWaterType (and the script's T / A).
    pub sky: crate::planet_sky::SkyInputs,
    /// terrainThemeBelow raster instance (the sea floor).
    pub below: Option<u32>,
    /// weatherCloudType 1..6 (`sky::PROP_WEATHER_CLOUD_TYPE`).
    pub cloud_type: u8,
}

/// Orbit visual cache key: (source, look, display radius bits).
type VisualKey = (TerrainSource, LookKey, u32);

enum Job {
    Generate { source: TerrainSource, res: usize },
    /// Bake an orbit mesh and its colour atlas (CPU work off the frame).
    Bake { key: VisualKey, planet: Arc<Planet>, look: Box<Look>, radius: f32 },
}

enum Done {
    Generated {
        source: TerrainSource,
        res: usize,
        result: Result<Generated, String>,
        millis: u128,
    },
    Baked {
        key: VisualKey,
        mesh: Box<Mesh>,
        image: Box<Image>,
        emissive: Option<Box<Image>>,
        millis: u128,
    },
}

fn bake(planet: &Planet, look: &Look, radius: f32) -> (Mesh, Image, Option<Image>) {
    let mesh = orbit_mesh(planet, radius, look.water_level);
    let (rgba, glow) = orbit_atlas(planet, look);
    let (w, h) = (ORBIT_RES * 3, ORBIT_RES * 2);
    (mesh, atlas_image(rgba, w, h), glow.map(|g| atlas_image(g, w, h)))
}

fn generate(
    index: &ResourceIndex,
    library: &mut Option<crate::planet_data::BrushLibrary>,
    source: TerrainSource,
    res: usize,
) -> Result<Generated, String> {
    use crate::planet_data as pd;
    if let (TerrainSource::Home(seed), true) = (source, res >= crate::planet_query::FACE_RES) {
        // Exactly the earlier stages' homeworld (script, cube and origin).
        let cache = crate::terrain::generate_planet_cache(index, seed)
            .ok_or("homeworld not generated")?;
        let descriptor = pd::load_descriptor(index, cache.script.terrain_script)?;
        let (sky, below, cloud_type) = sky::script_inputs(index, &descriptor);
        return Ok(Generated {
            planet: cache.base.clone(),
            info: [descriptor.planet_info[0], descriptor.planet_info[1]],
            wta: descriptor.wta,
            origin: Some(cache.origin),
            name: cache.script.description.clone(),
            sky,
            below,
            cloud_type,
        });
    }
    let (script, descriptor) = match source {
        TerrainSource::Home(seed) => {
            let scripts = pd::first_group(index);
            let script = pd::choose_homeworld(&scripts, seed)
                .cloned()
                .ok_or("no FirstGroup planet scripts")?;
            let descriptor = pd::load_descriptor(index, script.terrain_script)?;
            (script, descriptor)
        }
        TerrainSource::Script(group, instance) => {
            let key = ResourceKey {
                group_id: group,
                instance_id: instance,
                type_id: PROP,
            };
            let descriptor = pd::load_descriptor_key(index, key)?;
            let script = pd::PlanetScript {
                key,
                description: format!("{group:08x}!{instance:08x}"),
                terrain_script: instance,
                first_group: false,
                exclude_from_gge: false,
            };
            (script, descriptor)
        }
    };
    let lib = match library.take() {
        Some(lib) => lib,
        None => pd::BrushLibrary::load(index)?,
    };
    let source_data = pd::PlanetSource::from_parts(index, script, descriptor, lib);
    let (cube, _) = crate::planet_gen::generate_res(&source_data, res);
    let d = &source_data.descriptor;
    let planet = Planet::new(cube, d.radius(), d.water_level());
    let (sky, below, cloud_type) = sky::script_inputs(index, d);
    let out = Generated {
        planet: Arc::new(planet),
        info: [d.planet_info[0], d.planet_info[1]],
        wta: d.wta,
        origin: None,
        name: source_data.script.description.clone(),
        sky,
        below,
        cloud_type,
    };
    *library = Some(source_data.library);
    Ok(out)
}

/// The generation cache and its worker.
#[derive(Resource, Default)]
pub struct SpacePlanets {
    jobs: Option<mpsc::Sender<Job>>,
    results: Option<Mutex<mpsc::Receiver<Done>>>,
    pending: HashSet<(TerrainSource, usize)>,
    failed: HashSet<(TerrainSource, usize)>,
    orbit: HashMap<TerrainSource, Arc<Generated>>,
    /// Full-resolution worlds, most recently used last.
    full: Vec<(TerrainSource, Arc<Generated>)>,
    ramps: Option<Option<Arc<Ramps>>>,
    /// The land theme's palette (the homeworld's colours), loaded once.
    theme: Option<Option<crate::world_catalog::PlanetPalette>>,
    /// Orbit visuals by (source, look, radius bits), and those being baked.
    visuals: HashMap<VisualKey, OrbitVisual>,
    baking: HashSet<VisualKey>,
    /// Finished bakes waiting for their assets (added on the main thread).
    baked: Vec<(VisualKey, Mesh, Image, Option<Image>)>,
    /// Colony sites (planet-local directions) per world id.
    sites: HashMap<u32, (usize, Vec<Vec3>)>,
    /// Generation times (source, res, ms) for the log and the report.
    pub log: Vec<(TerrainSource, usize, u128)>,
    /// Sky tables by (atmosphere type, water type) and the baked impostor
    /// scatter cubes (with "black") by their inputs (`impostor`).
    sky_tables: HashMap<(u32, u32), Option<crate::planet_sky::SkyTables>>,
    /// Water colour tables by water type (`sea_color`).
    water_tables: HashMap<u32, Option<crate::planet_sky::Table>>,
    scatter_cubes: HashMap<(u32, u32, u32, u32, [u32; 3]), Option<(Handle<Image>, bool)>>,
    /// "planetAtmoImpostor" spheres by display radius bits.
    atmosphere_meshes: HashMap<u32, Handle<Mesh>>,
}

#[derive(Clone)]
struct OrbitVisual {
    mesh: Handle<Mesh>,
    image: Handle<Image>,
    emissive: Option<Handle<Image>>,
}

impl SpacePlanets {
    /// The sea colour the retail terrain state gives a world: the colour table
    /// of its terrainWaterType at its temperature score (0xFBF540 reads
    /// column floor((w - 1) T) of the table SetWaterType 0xFBAF20 selected).
    /// From orbit the impostor is baked from that terrain, so the sea there
    /// has the planet view's colour (INFERRED for the bake, VERIFIED for the
    /// table): space-visuals3 replaced the theme palette's editor water
    /// colour (0x968C496A, only used with the user-colour branch of the
    /// terrain state), which showed the homeworld's blue sea from the
    /// solar-system view over a red one in the planet view.
    fn sea_color(&mut self, index: &ResourceIndex, water_type: u32, temperature: f32) -> Option<[f32; 3]> {
        let table = self
            .water_tables
            .entry(water_type.min(15))
            .or_insert_with(|| crate::planet_sky::water_table(index, water_type));
        table.as_ref().map(|t| t.water_rgb(temperature.clamp(0.0, 1.0)))
    }

    /// The world's impostor scatter cube (0xF9E490 step 3: the in-scatter
    /// bake at p 0.99 with its sky tables and current climate) and whether it
    /// is black (no atmosphere).
    fn scatter_cube(
        &mut self,
        world: &mut World,
        index: &ResourceIndex,
        sky: &crate::planet_sky::SkyInputs,
        look: &Look,
    ) -> Option<(Handle<Image>, bool)> {
        let inputs = crate::planet_sky::SkyInputs {
            temperature: look.climate.0.clamp(0.0, 1.0),
            atmosphere: look.climate.1.clamp(0.0, 1.0),
            ..*sky
        };
        let key = (
            inputs.atmosphere_type,
            inputs.water_type,
            inputs.temperature.to_bits(),
            inputs.atmosphere.to_bits(),
            look.air.map(f32::to_bits),
        );
        if let Some(cube) = self.scatter_cubes.get(&key) {
            return cube.clone();
        }
        let tables = self
            .sky_tables
            .entry((inputs.atmosphere_type, inputs.water_type))
            .or_insert_with(|| crate::planet_sky::SkyTables::load(index, inputs.atmosphere_type, inputs.water_type));
        let cube = tables.as_ref().map(|t| {
            let image = impostor::bake_scatter(t, &inputs, look.air);
            let black = impostor::cube_is_black(&image);
            (world.resource_mut::<Assets<Image>>().add(image), black)
        });
        self.scatter_cubes.insert(key, cube.clone());
        cube
    }

    fn atmosphere_mesh(&mut self, world: &mut World, radius: f32) -> Option<Handle<Mesh>> {
        if let Some(h) = self.atmosphere_meshes.get(&radius.to_bits()) {
            return Some(h.clone());
        }
        let mesh = Sphere::new(radius * impostor::ATMOSPHERE_SCALE).mesh().ico(4).ok()?;
        let handle = world.resource_mut::<Assets<Mesh>>().add(mesh);
        self.atmosphere_meshes.insert(radius.to_bits(), handle.clone());
        Some(handle)
    }

    fn start(&mut self, index: &ResourceIndex) {
        if self.jobs.is_some() {
            return;
        }
        let (jobs, queue) = mpsc::channel::<Job>();
        let (done, results) = mpsc::channel::<Done>();
        let index = index.clone();
        let spawned = std::thread::Builder::new()
            .name("space-planets".into())
            .spawn(move || {
                let mut library = None;
                while let Ok(job) = queue.recv() {
                    let start = std::time::Instant::now();
                    let message = match job {
                        Job::Generate { source, res } => {
                            let result = generate(&index, &mut library, source, res);
                            Done::Generated {
                                source,
                                res,
                                result,
                                millis: start.elapsed().as_millis(),
                            }
                        }
                        Job::Bake { key, planet, look, radius } => {
                            let (mesh, image, emissive) = bake(&planet, &look, radius);
                            Done::Baked {
                                key,
                                mesh: Box::new(mesh),
                                image: Box::new(image),
                                emissive: emissive.map(Box::new),
                                millis: start.elapsed().as_millis(),
                            }
                        }
                    };
                    if done.send(message).is_err() {
                        break;
                    }
                }
            });
        if spawned.is_ok() {
            self.jobs = Some(jobs);
            self.results = Some(Mutex::new(results));
        }
    }

    fn request(&mut self, source: TerrainSource, res: usize) {
        let key = (source, res);
        if self.pending.contains(&key) || self.failed.contains(&key) || self.get(source, res).is_some() {
            return;
        }
        if let Some(jobs) = &self.jobs {
            if jobs.send(Job::Generate { source, res }).is_ok() {
                self.pending.insert(key);
            }
        }
    }

    fn request_bake(&mut self, key: VisualKey, planet: Arc<Planet>, look: &Look, radius: f32) {
        if self.baking.contains(&key) || self.visuals.contains_key(&key) {
            return;
        }
        if let Some(jobs) = &self.jobs {
            let job = Job::Bake {
                key,
                planet,
                look: Box::new(look.clone()),
                radius,
            };
            if jobs.send(job).is_ok() {
                self.baking.insert(key);
            }
        }
    }

    fn get(&self, source: TerrainSource, res: usize) -> Option<Arc<Generated>> {
        if res >= crate::planet_query::FACE_RES {
            self.full.iter().find(|(s, _)| *s == source).map(|(_, g)| g.clone())
        } else {
            self.orbit.get(&source).cloned()
        }
    }

    fn touch_full(&mut self, source: TerrainSource) {
        if let Some(i) = self.full.iter().position(|(s, _)| *s == source) {
            let entry = self.full.remove(i);
            self.full.push(entry);
        }
    }

    fn insert(&mut self, source: TerrainSource, res: usize, generated: Generated) {
        let g = Arc::new(generated);
        if res >= crate::planet_query::FACE_RES {
            self.full.retain(|(s, _)| *s != source);
            self.full.push((source, g));
            while self.full.len() > FULL_KEEP {
                self.full.remove(0);
            }
        } else {
            self.orbit.insert(source, g);
        }
    }

    fn poll(&mut self) {
        let Some(results) = &self.results else {
            return;
        };
        let done: Vec<Done> = match results.lock() {
            Ok(r) => r.try_iter().collect(),
            Err(_) => return,
        };
        for d in done {
            match d {
                Done::Generated { source, res, result, millis } => {
                    self.pending.remove(&(source, res));
                    match result {
                        Ok(g) => {
                            info!("space planets: {:?} {} at {}^2 in {} ms", source, g.name, res, millis);
                            self.log.push((source, res, millis));
                            self.insert(source, res, g);
                        }
                        Err(e) => {
                            warn!("space planets: {:?} at {}^2 failed: {e}", source, res);
                            self.failed.insert((source, res));
                        }
                    }
                }
                Done::Baked { key, mesh, image, emissive, millis } => {
                    debug!("space planets: orbit visual {:?} baked in {} ms", key.0, millis);
                    self.baking.remove(&key);
                    self.baked.push((key, *mesh, *image, emissive.map(|e| *e)));
                }
            }
        }
    }

    fn ramps(&mut self, index: &ResourceIndex) -> Option<Arc<Ramps>> {
        self.ramps
            .get_or_insert_with(|| Ramps::load(index).map(Arc::new))
            .clone()
    }
}

// ---------------------------------------------------------------------------
// Orbit visuals
// ---------------------------------------------------------------------------

/// The child that draws an orbit body with the impostor program.
#[derive(Component)]
pub struct OrbitImpostorBody;

/// A body drawn from its generated terrain.
#[derive(Component, Clone, Debug)]
pub struct OrbitTerrain {
    pub source: TerrainSource,
    key: LookKey,
    pub look: Look,
    /// Display radius of the body (scene units).
    pub radius: f32,
}

/// Atlas: 3 x 2 tiles of `ORBIT_RES` texels, one per cube face.
fn atlas_uv(face: usize, u: f32, v: f32) -> [f32; 2] {
    let tile = ORBIT_RES as f32;
    let (tx, ty) = ((face % 3) as f32 * tile, (face / 3) as f32 * tile);
    // Texel centres span the face edge to edge (no bleeding across tiles).
    [
        (tx + 0.5 + u * (tile - 1.0)) / (tile * 3.0),
        (ty + 0.5 + v * (tile - 1.0)) / (tile * 2.0),
    ]
}

/// The orbit mesh: a cube-sphere of `radius` displaced by the height field
/// (true proportion: altRange / R), the sea at the water level.
pub fn orbit_mesh(planet: &Planet, radius: f32, water_level: f32) -> Mesh {
    let n = ORBIT_CELLS;
    let verts = 6 * (n + 1) * (n + 1);
    let mut positions = Vec::with_capacity(verts);
    let mut normals = Vec::with_capacity(verts);
    let mut uvs = Vec::with_capacity(verts);
    // The sphere normal for the impostor program (its In.texcoord<t3>).
    let mut spheres: Vec<[f32; 4]> = Vec::with_capacity(verts);
    let mut indices: Vec<u32> = Vec::with_capacity(6 * n * n * 6);
    let k = radius / planet.radius;
    for face in 0..6 {
        let base = positions.len() as u32;
        for j in 0..=n {
            for i in 0..=n {
                let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                let dir = face_point(face, u, v).normalize();
                let h = planet.h_at(dir);
                let r = planet.radius + planet.altitude_range * h.max(water_level);
                positions.push((dir * r * k).to_array());
                let normal = if h <= water_level { dir } else { planet.ground_normal(dir) };
                normals.push(normal.to_array());
                spheres.push(dir.extend(1.0).to_array());
                uvs.push(atlas_uv(face, u, v));
            }
        }
        for j in 0..n {
            for i in 0..n {
                let a = base + (j * (n + 1) + i) as u32;
                let b = a + 1;
                let c = a + (n + 1) as u32;
                let d = c + 1;
                indices.extend([a, b, c, b, d, c]);
            }
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, spheres);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Bake the colour atlas (and the lava glow atlas) of a planet.
pub fn orbit_atlas(planet: &Planet, look: &Look) -> (Vec<u8>, Option<Vec<u8>>) {
    let tile = ORBIT_RES;
    let (w, h) = (tile * 3, tile * 2);
    let mut rgba = vec![0u8; w * h * 4];
    let mut glow = (look.fluid == Fluid::Lava).then(|| vec![0u8; w * h * 4]);
    for face in 0..6 {
        let (tx, ty) = ((face % 3) * tile, (face / 3) * tile);
        for row in 0..tile {
            for col in 0..tile {
                let u = col as f32 / (tile - 1) as f32;
                let v = row as f32 / (tile - 1) as f32;
                let dir = face_point(face, u, v).normalize();
                let (c, g) = look.surface(planet, dir);
                let o = ((ty + row) * w + tx + col) * 4;
                for k in 0..3 {
                    rgba[o + k] = (c[k].clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                // Alpha: the water mask (the impostor's specular channel).
                let water = look.fluid == Fluid::Water && planet.h_at(dir) <= look.water_level;
                rgba[o + 3] = if water { 255 } else { 0 };
                if let Some(glow) = glow.as_mut() {
                    for k in 0..3 {
                        glow[o + k] = (c[k] * g).clamp(0.0, 1.0).mul_add(255.0, 0.0).round() as u8;
                    }
                    glow[o + 3] = 255;
                }
            }
        }
    }
    (rgba, glow)
}

/// An sRGB image with a box-filtered mip chain (the orbit planets shrink to a
/// few pixels in the solar view).
fn atlas_image(rgba: Vec<u8>, width: usize, height: usize) -> Image {
    let mut mips = vec![rgba];
    let (mut w, mut h) = (width, height);
    while w > 1 && h > 1 {
        let (nw, nh) = (w / 2, h / 2);
        let last = mips.last().unwrap();
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let s: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)]
                        .iter()
                        .map(|(dx, dy)| last[((y * 2 + dy) * w + x * 2 + dx) * 4 + c] as u32)
                        .sum();
                    next[(y * nw + x) * 4 + c] = ((s + 2) / 4) as u8;
                }
            }
        }
        mips.push(next);
        w = nw;
        h = nh;
    }
    let count = mips.len() as u32;
    let mut image = Image::new(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        mips[0].clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = count;
    image.data = Some(mips.into_iter().flatten().collect());
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// The atmosphere shell material (planet_render's ADAPTED rim glow) for a
/// body: `centre`, ground radius and shell radius in world units.
fn shell_material(look: &Look, centre: Vec3, ground: f32, shell: f32) -> AtmosphereMaterial {
    let air = Color::srgb(look.air[0], look.air[1], look.air[2]).to_linear();
    AtmosphereMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            fog_enabled: false,
            cull_mode: None,
            alpha_mode: AlphaMode::Add,
            ..default()
        },
        extension: AtmosphereShell {
            sphere: centre.extend(ground),
            // ADAPTED: the stages' shell strength 0.5 at thickness 1.5.
            color: Vec4::new(air.red, air.green, air.blue, 0.5 * look.thickness / 1.5),
            params: Vec4::new(2.0, shell, 0.0, 0.0),
        },
    }
}

/// Shell radius over the planet radius (planet_render: R + 0.45 altRange).
fn shell_ratio(planet: &Planet) -> f32 {
    (planet.radius + planet.altitude_range * 0.45) / planet.radius
}

fn render_app(world: &World) -> bool {
    // `OPENSPORE_SPACE_PLANETS=0` keeps the plain spheres (A/B checks).
    !std::env::var("OPENSPORE_SPACE_PLANETS").is_ok_and(|v| v == "0")
        && world.contains_resource::<Assets<crate::terrain::TerrainMaterial>>()
        && world.contains_resource::<Assets<AtmosphereMaterial>>()
        && world.contains_resource::<Assets<Mesh>>()
        && world.contains_resource::<Assets<Image>>()
        && world.contains_resource::<Assets<StandardMaterial>>()
}

fn in_space(world: &World) -> bool {
    world
        .get_resource::<State<GameStage>>()
        .is_some_and(|s| *s.get() == GameStage::Space)
}

/// The pool entry (water, temperature, atmosphere) of a script, if listed.
fn pool_entry(group: u32, instance: u32) -> Option<(f32, f32, f32)> {
    crate::space_worldgen_data::POOLS
        .iter()
        .flat_map(|pool| pool.iter())
        .find(|e| e.0 == group && e.1 == instance)
        .map(|e| (e.2, e.3, e.4))
}

/// A moon without a recorded script: a T0 pool script drawn from its orbit
/// (ADAPTED: barren moons get no script in the system record).
fn moon_fallback(moon: &crate::space::Moon) -> (u32, u32) {
    let pool = crate::space_worldgen_data::POOL_T0;
    let h = (moon.distance.to_bits() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (moon.tilt.to_bits() as u64).rotate_left(17);
    let e = pool[(h % pool.len() as u64) as usize];
    (e.0, e.1)
}

/// Per frame in Space: collect finished generations, give every world and
/// moon of the system its generated look (re-baked when terraforming changes
/// it) and keep the atmosphere shells on their orbiting bodies.
pub fn orbit_planets(world: &mut World) {
    if !in_space(world) || !render_app(world) {
        return;
    }
    let started = std::time::Instant::now();
    orbit_planets_inner(world);
    if std::env::var_os("OPENSPORE_FRAME_TRACE").is_some() {
        eprintln!("space planets: orbit update {:?}", started.elapsed());
    }
}

fn orbit_planets_inner(world: &mut World) {
    if world
        .get_resource::<crate::Originals>()
        .is_none_or(|o| o.resource_index.records.is_empty())
    {
        return;
    }
    world.resource_scope(|world, originals: Mut<crate::Originals>| {
        orbit_update(world, &originals.resource_index);
    });
    // Decode the city models and the planet view's land inputs once the
    // system is drawn, not on the first descent (a few hundred ms of GMDL and
    // raster decoding in the dev profile).
    if !world.contains_resource::<CityArt>() && world.query::<&OrbitTerrain>().iter(world).next().is_some() {
        for key in city_keys() {
            city_model(world, key);
        }
        crate::terrain::warm_space_planet_material(world);
    }
}

fn orbit_update(world: &mut World, index: &ResourceIndex) {
    let mut cache = world.remove_resource::<SpacePlanets>().unwrap_or_default();
    cache.start(index);
    cache.poll();
    // The earlier stages' homeworld, when this process played them.
    let seed = world
        .get_resource::<crate::game::RunProgress>()
        .map_or(0, |r| r.species_world.seed);
    if cache.get(TerrainSource::Home(seed), crate::planet_query::FACE_RES).is_none() {
        if let Some(pc) = world
            .get_resource::<crate::terrain::PlanetCache>()
            .filter(|c| c.seed == seed)
        {
            let descriptor = crate::planet_data::load_descriptor(index, pc.script.terrain_script).ok();
            let (sky, below, cloud_type) = descriptor.as_ref().map_or(
                (
                    crate::planet_sky::SkyInputs {
                        temperature: 0.5,
                        atmosphere: 0.5,
                        atmosphere_type: sky::SPACE_DEFAULT_TYPE,
                        water_type: sky::SPACE_DEFAULT_TYPE,
                    },
                    None,
                    1,
                ),
                |d| sky::script_inputs(index, d),
            );
            let generated = Generated {
                planet: pc.base.clone(),
                info: descriptor.as_ref().map_or([32.0; 2], |d| [d.planet_info[0], d.planet_info[1]]),
                wta: descriptor.as_ref().map_or([0.5; 3], |d| d.wta),
                origin: Some(pc.origin),
                name: pc.script.description.clone(),
                sky,
                below,
                cloud_type,
            };
            cache.insert(TerrainSource::Home(seed), crate::planet_query::FACE_RES, generated);
        }
    }
    let ramps = cache.ramps(index);
    if cache.theme.is_none() {
        cache.theme = Some(
            world
                .get_resource::<crate::world_presentation::LandTheme>()
                .map(|t| t.palette.clone())
                .or_else(|| crate::world_presentation::LandTheme::load(index).ok().map(|t| t.palette)),
        );
    }
    let theme = cache.theme.clone().flatten();
    // Finished bakes become assets.
    for (key, mesh, image, emissive) in std::mem::take(&mut cache.baked) {
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let mut images = world.resource_mut::<Assets<Image>>();
        let image = images.add(image);
        let emissive = emissive.map(|e| images.add(e));
        cache.visuals.insert(key, OrbitVisual { mesh, image, emissive });
    }
    // (world, script, temperature, atmosphere, T-score) of the system's bodies.
    let bodies: Vec<(Option<u32>, Option<(u32, u32)>, f32, f32, u8)> = world
        .get_resource::<crate::space::CurrentSystem>()
        .map(|s| {
            s.0.bodies
                .iter()
                .map(|b| (b.world, b.script, b.temperature, b.atmosphere, b.t_score))
                .collect()
        })
        .unwrap_or_default();
    // Worlds.
    let planets: Vec<(Entity, u32, f32, Option<LookKey>, Option<Handle<StandardMaterial>>)> = world
        .query_filtered::<(
            Entity,
            &crate::persistence::WorldId,
            &crate::game::Planet,
            Option<&OrbitTerrain>,
            Option<&MeshMaterial3d<StandardMaterial>>,
        ), With<crate::space_visuals::PlanetSurface>>()
        .iter(world)
        .map(|(e, id, p, t, m)| (e, id.0, p.radius, t.map(|t| t.key), m.map(|m| m.0.clone())))
        .collect();
    let ecos: HashMap<u32, crate::space::PlanetEcology> = match world.get_resource::<crate::game::RunProgress>() {
        Some(run) => planets
            .iter()
            .map(|p| (p.1, run.space_campaign.ecology_for(p.1)))
            .collect(),
        None => HashMap::new(),
    };
    let mut jobs: Vec<(Entity, TerrainSource, Look, LookKey, f32, Option<Handle<StandardMaterial>>, bool)> = Vec::new();
    for (entity, id, radius, applied, material) in planets {
        let body = bodies.iter().find(|b| b.0 == Some(id));
        let source = if id == 0 {
            Some(TerrainSource::Home(seed))
        } else {
            body.and_then(|b| b.1).map(|(g, i)| TerrainSource::Script(g, i))
        };
        let Some(source) = source else {
            continue;
        };
        cache.request(source, ORBIT_RES);
        if id == 0 {
            // The homeworld's origin and cities need the full planet.
            cache.request(source, crate::planet_query::FACE_RES);
        }
        let Some(generated) = cache.get(source, ORBIT_RES) else {
            continue;
        };
        let eco = ecos.get(&id).copied();
        let base = body.map_or((0.5, 0.5), |b| (b.2, b.3));
        let climate = Climate {
            info: generated.info,
            base_water: generated.wta[0],
            now: eco.map_or(base, |e| (e.temperature, e.atmosphere)),
            t_score: eco.map_or(body.map_or(0, |b| b.4), |e| e.t_score()),
        };
        let mut look = look_for(id == 0, theme.as_ref(), ramps.as_deref(), climate);
        if look.fluid == Fluid::Water {
            if let Some(sea) = cache.sea_color(index, generated.sky.water_type, look.climate.0) {
                look.water = sea;
            }
        }
        let key = look.key(climate.t_score);
        if applied == Some(key) {
            continue;
        }
        jobs.push((entity, source, look, key, radius, material, false));
    }
    // Moons.
    let moons: Vec<(Entity, crate::space::Moon, Option<LookKey>)> = world
        .query_filtered::<(Entity, &crate::space::Moon, Option<&OrbitTerrain>), With<Mesh3d>>()
        .iter(world)
        .map(|(e, m, t)| (e, *m, t.map(|t| t.key)))
        .collect();
    for (entity, moon, applied) in moons {
        if applied.is_some() {
            continue;
        }
        let (group, instance) = moon.script.unwrap_or_else(|| moon_fallback(&moon));
        let source = TerrainSource::Script(group, instance);
        cache.request(source, ORBIT_RES);
        let Some(generated) = cache.get(source, ORBIT_RES) else {
            continue;
        };
        let (water, temperature, atmosphere) = pool_entry(group, instance).unwrap_or((generated.wta[0], 0.5, 0.0));
        let climate = Climate {
            info: generated.info,
            base_water: water,
            now: (temperature, atmosphere),
            t_score: crate::space_worldgen::t_score(temperature, atmosphere),
        };
        let mut look = look_for(false, theme.as_ref(), ramps.as_deref(), climate);
        if look.fluid == Fluid::Water {
            if let Some(sea) = cache.sea_color(index, generated.sky.water_type, look.climate.0) {
                look.water = sea;
            }
        }
        let key = look.key(climate.t_score);
        // The shared moon mesh has radius 0.9; the entity scale sizes it.
        jobs.push((entity, source, look, key, 0.9, None, true));
    }
    for (entity, source, look, key, radius, material, moon) in jobs {
        let Some(generated) = cache.get(source, ORBIT_RES) else {
            continue;
        };
        let vkey = (source, key, radius.to_bits());
        let Some(visual) = cache.visuals.get(&vkey).cloned() else {
            cache.request_bake(vkey, generated.planet.clone(), &look, radius);
            continue;
        };
        // Worlds keep their material handle (space_visuals caches it per
        // world); moons share one, so they get their own.
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let handle = match material {
            Some(h) if !moon => h,
            _ => materials.add(StandardMaterial::default()),
        };
        if let Some(mut m) = materials.get_mut(&handle) {
            m.base_color = Color::WHITE;
            m.base_color_texture = Some(visual.image.clone());
            m.perceptual_roughness = 0.9;
            m.reflectance = 0.05;
            m.emissive_texture = visual.emissive.clone();
            m.emissive = if visual.emissive.is_some() {
                LinearRgba::rgb(3.0, 3.0, 3.0)
            } else {
                LinearRgba::BLACK
            };
        }
        // From orbit the world keeps the scene's halo (its strength follows
        // the atmosphere score in `space_visuals::update`); airless worlds
        // have none. (The planet view's atmosphere shell has world-space
        // uniforms: rewriting them every frame for orbiting worlds cost ~2 ms
        // per world.)
        let halos: Vec<Entity> = world
            .query_filtered::<(Entity, &ChildOf), With<crate::space_visuals::PlanetAtmosphereHalo>>()
            .iter(world)
            .filter(|(_, c)| c.parent() == entity)
            .map(|(e, _)| e)
            .collect();
        let halo = if look.thickness > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        for e in halos {
            world.entity_mut(e).insert(halo);
        }
        if std::env::var_os("OPENSPORE_SPACE_TRACE").is_some() {
            let id = world.get::<crate::persistence::WorldId>(entity).map(|w| w.0);
            eprintln!(
                "space orbit: world {id:?} moon {moon} {source:?} {} fluid {:?} water {:.2} air {:.2} ramp {:?} land {:.2}",
                generated.name,
                look.fluid,
                look.water_level,
                look.thickness,
                look.ramp[20].map(|v| (v * 100.0).round() / 100.0),
                crate::planet_gen::land_fraction(&generated.planet),
            );
        }
        if impostor::enabled() && world.contains_resource::<Assets<impostor::ImpostorMaterial>>() {
            // The retail impostor program draws the body (a child with the
            // baked mesh); the body keeps its PBR material handle for the
            // systems that read it, without a mesh of its own.
            // ADAPTED: airless worlds keep the earlier black scatter (the bake
            // at A 0 is not black with the installed sunset table, 0.71 at
            // p 0.99; how retail blacks it out is not traced).
            let scatter =
                if look.thickness > 0.0 { cache.scatter_cube(world, index, &generated.sky, &look) } else { None };
            let material =
                impostor::material(&look, visual.image.clone(), visual.emissive.clone(), scatter.as_ref().map(|s| s.0.clone()));
            let material = world.resource_mut::<Assets<impostor::ImpostorMaterial>>().add(material);
            let existing = world
                .query_filtered::<(Entity, &ChildOf), With<OrbitImpostorBody>>()
                .iter(world)
                .find(|(_, c)| c.parent() == entity)
                .map(|(e, _)| e);
            let draw = (Mesh3d(visual.mesh.clone()), MeshMaterial3d(material));
            match existing {
                Some(child) => {
                    world.entity_mut(child).insert(draw);
                }
                None => {
                    world.spawn((
                        OrbitImpostorBody,
                        draw,
                        Transform::default(),
                        Visibility::Inherited,
                        bevy::light::NotShadowCaster,
                        ChildOf(entity),
                    ));
                }
            }
            // The atmosphere impostor ("planetAtmoImpostor", 1.2 x, the same
            // scatter cube); a black cube (no atmosphere) draws nothing. It
            // replaces the ADAPTED halo.
            let atmosphere = scatter
                .as_ref()
                .filter(|(_, black)| !black && look.thickness > 0.0)
                .map(|(cube, _)| cube.clone())
                .filter(|_| world.contains_resource::<Assets<impostor::AtmosphereImpostorMaterial>>());
            let existing_atmosphere = world
                .query_filtered::<(Entity, &ChildOf), With<impostor::OrbitAtmosphereImpostor>>()
                .iter(world)
                .find(|(_, c)| c.parent() == entity)
                .map(|(e, _)| e);
            if std::env::var_os("OPENSPORE_SPACE_TRACE").is_some() {
                let id = world.get::<crate::persistence::WorldId>(entity).map(|w| w.0);
                eprintln!(
                    "space orbit: world {id:?} atmosphere impostor {} (air {:.2}, cube {:?})",
                    atmosphere.is_some(),
                    look.thickness,
                    scatter.as_ref().map(|s| s.1)
                );
            }
            match (atmosphere.clone(), cache.atmosphere_mesh(world, radius)) {
                (Some(cube), Some(mesh)) => {
                    let material =
                        world.resource_mut::<Assets<impostor::AtmosphereImpostorMaterial>>().add(impostor::atmosphere_material(cube));
                    let draw = (Mesh3d(mesh), MeshMaterial3d(material));
                    match existing_atmosphere {
                        Some(child) => {
                            world.entity_mut(child).insert(draw);
                        }
                        None => {
                            world.spawn((
                                impostor::OrbitAtmosphereImpostor,
                                Name::new("orbit atmosphere (planetAtmoImpostor)"),
                                draw,
                                Transform::default(),
                                Visibility::Inherited,
                                bevy::light::NotShadowCaster,
                                ChildOf(entity),
                            ));
                        }
                    }
                    let halos: Vec<Entity> = world
                        .query_filtered::<(Entity, &ChildOf), With<crate::space_visuals::PlanetAtmosphereHalo>>()
                        .iter(world)
                        .filter(|(_, c)| c.parent() == entity)
                        .map(|(e, _)| e)
                        .collect();
                    for e in halos {
                        world.entity_mut(e).insert(Visibility::Hidden);
                    }
                }
                _ => {
                    if let Some(child) = existing_atmosphere {
                        world.entity_mut(child).despawn();
                    }
                }
            }
            world.entity_mut(entity).remove::<Mesh3d>().insert((
                MeshMaterial3d(handle),
                OrbitTerrain {
                    source,
                    key,
                    look,
                    radius,
                },
            ));
            continue;
        }
        world.entity_mut(entity).insert((
            Mesh3d(visual.mesh.clone()),
            MeshMaterial3d(handle),
            OrbitTerrain {
                source,
                key,
                look,
                radius,
            },
        ));
    }
    place_orbit_cities(world, &mut cache, seed);
    world.insert_resource(cache);
}

fn city_keys() -> [ResourceKey; 5] {
    [
        crate::city_presentation::CITY_DISTRICT_CITY_HALL_SOURCE,
        crate::city_presentation::CITY_DISTRICT_HOUSE_SOURCE,
        crate::city_presentation::CITY_DISTRICT_INDUSTRY_SOURCE,
        crate::city_presentation::CITY_DISTRICT_ENTERTAINMENT_SOURCE,
        crate::city_presentation::gun_source(crate::civ::Approach::Military, false).0,
    ]
}

// ---------------------------------------------------------------------------
// Colonies
// ---------------------------------------------------------------------------

/// The colony sites of a world by a seeded draw: land points flat enough for a
/// city, CityFlattenSize apart. The sites the player placed with the colony
/// tool (`CityLayout::site`) are not drawn: see [`colony_sites_with`].
pub fn colony_sites(planet: &Planet, water_level: f32, seed: u64, count: usize) -> Vec<Vec3> {
    colony_sites_with(planet, water_level, seed, count, &[])
}

/// `count` colony sites, the first ones as the player placed them (`placed[i]`
/// is where the colony tool landed for city `i`, retail: the colony goes where
/// the pack drops). The cities without a placement (older saves, colonies not
/// founded with the tool) are ADAPTED: a seeded draw of land points flat
/// enough for a city, CityFlattenSize apart from each other and from the
/// placed ones.
pub fn colony_sites_with(
    planet: &Planet,
    water_level: f32,
    seed: u64,
    count: usize,
    placed: &[Option<Vec3>],
) -> Vec<Vec3> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0xC010_51E5);
    let mut out: Vec<Option<Vec3>> = (0..count).map(|i| placed.get(i).copied().flatten()).collect();
    let spacing = crate::civ_rules::CITY_FLATTEN_SIZE * 2.4;
    let mut best: Option<(f32, Vec3)> = None;
    for _ in 0..6000 {
        let Some(slot) = out.iter().position(Option::is_none) else {
            break;
        };
        let z: f32 = rng.random_range(-0.85..0.85);
        let a: f32 = rng.random_range(0.0..std::f32::consts::TAU);
        let r = (1.0 - z * z).sqrt();
        let dir = Vec3::new(r * a.cos(), z, r * a.sin());
        let h = planet.h_at(dir);
        if best.is_none_or(|(bh, _)| h > bh) {
            best = Some((h, dir));
        }
        if h < water_level + 0.04 {
            continue;
        }
        if planet.ground_normal(dir).dot(dir) < 0.95 {
            continue;
        }
        if out.iter().flatten().any(|d| d.angle_between(dir) * planet.radius < spacing) {
            continue;
        }
        out[slot] = Some(dir);
    }
    // A water world: the highest ground found.
    let mut last: Option<Vec3> = None;
    out.into_iter()
        .map(|site| {
            let dir = site.unwrap_or_else(|| match best {
                Some((_, d)) if last.is_none() => d,
                _ => {
                    let from = last.unwrap_or(Vec3::Y);
                    let (t, _) = crate::planet_query::tangent_basis(from);
                    (from + t * (spacing / planet.radius)).normalize()
                }
            });
            last = Some(dir);
            dir
        })
        .collect()
}

/// The planet-local direction where the colony tool lands: the aim point of
/// the planet view (the cursor ray's hit on the world, else the ground ahead
/// of the UFO; `space_tool_fx::planet_aim`), for the world in view. `None` away
/// from the planet view or when the aim point is under water (the colony goes
/// to a seeded land site then, ADAPTED: retail's placement test was not
/// traced).
pub fn colony_landing(
    view: Option<&crate::space_view::SpaceView>,
    ground: Option<&PlanetViewGround>,
    ray: Option<Ray3d>,
    world: u32,
) -> Option<Vec3> {
    let pv = view?.planet.as_ref().filter(|p| p.world == world)?;
    let ground = ground?;
    let aim = crate::space_tool_fx::planet_aim(pv, Some(ground), ray);
    if aim.kind != crate::space_tool_fx::HitKind::Ground {
        return None;
    }
    ground.local_dir(aim.point)
}

/// What stands in one city on a world.
#[derive(Clone, Debug, PartialEq)]
enum Plot {
    House,
    Factory,
    Entertainment,
    Turret,
}

/// How many cities a world shows (`world_cities` for a world that is not the
/// homeworld): the colonies, or the layouts when more were planned.
fn space_city_count(campaign: &crate::space::SpaceCampaign, world_id: u32) -> usize {
    let colonies = campaign.colony_count(world_id) as usize;
    if colonies == 0 {
        0
    } else {
        colonies.max(campaign.city_layouts.get(&world_id).map_or(0, |l| l.len()))
    }
}

/// The cities of a world: (planet-local direction, plots around the hall).
fn world_cities(
    world_id: u32,
    run: Option<&crate::game::RunProgress>,
    planet: &Planet,
    water_level: f32,
    home_origin: Option<Vec3>,
) -> Vec<(Vec3, Vec<Option<Plot>>)> {
    let Some(run) = run else {
        return Vec::new();
    };
    // The homeworld: the Civ stage's cities where they stood (the Civ planet
    // frame takes the stage origin to +Y).
    if world_id == 0 {
        if let Some(origin) = home_origin {
            let frame = PlanetFrame::new(planet.radius, origin);
            let cities: Vec<_> = run
                .civ_campaign
                .cities
                .iter()
                .map(|c| {
                    let dir = frame.dir_to_local((c.center() - frame.centre).normalize_or(Vec3::Y));
                    let plots = c
                        .plots
                        .iter()
                        .map(|p| {
                            p.map(|b| match b {
                                crate::civ::Building::House => Plot::House,
                                crate::civ::Building::Factory => Plot::Factory,
                                crate::civ::Building::Entertainment => Plot::Entertainment,
                            })
                        })
                        .collect();
                    (dir, plots)
                })
                .collect();
            if !cities.is_empty() {
                return cities;
            }
        }
    }
    let colonies = run.space_campaign.colony_count(world_id) as usize;
    let layouts = run.space_campaign.city_layouts.get(&world_id);
    let count = if colonies == 0 { 0 } else { colonies.max(layouts.map_or(0, |l| l.len())) };
    if count == 0 {
        return Vec::new();
    }
    // Where the colony tool landed (`CityLayout::site`); the other cities by the draw.
    let placed: Vec<Option<Vec3>> = (0..count)
        .map(|i| {
            layouts
                .and_then(|l| l.get(i))
                .and_then(|l| l.site)
                .and_then(|s| Vec3::from_array(s).try_normalize())
        })
        .collect();
    let sites = colony_sites_with(planet, water_level, world_id as u64, count, &placed);
    sites
        .into_iter()
        .enumerate()
        .map(|(i, dir)| {
            let plots = layouts
                .and_then(|l| l.get(i))
                .map(|l| {
                    l.slots
                        .iter()
                        .map(|s| {
                            s.map(|b| match b {
                                crate::space::CityBuilding::House => Plot::House,
                                crate::space::CityBuilding::Factory => Plot::Factory,
                                crate::space::CityBuilding::Entertainment => Plot::Entertainment,
                                crate::space::CityBuilding::Turret => Plot::Turret,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            (dir, plots)
        })
        .collect()
}

/// The orbit view's colony markers stand on the colonies' sites.
fn place_orbit_cities(world: &mut World, cache: &mut SpacePlanets, seed: u64) {
    let markers: Vec<(Entity, u8, Entity)> = world
        .query::<(Entity, &crate::space_visuals::ColonyCityMarker, &ChildOf)>()
        .iter(world)
        .map(|(e, m, c)| (e, m.slot, c.parent()))
        .collect();
    if markers.is_empty() {
        return;
    }
    for (marker, slot, parent) in markers {
        let Some(terrain) = world.get::<OrbitTerrain>(parent).cloned() else {
            continue;
        };
        let Some(id) = world.get::<crate::persistence::WorldId>(parent).map(|w| w.0) else {
            continue;
        };
        let Some(generated) = cache.get(terrain.source, ORBIT_RES) else {
            continue;
        };
        let origin = if id == 0 {
            cache
                .get(TerrainSource::Home(seed), crate::planet_query::FACE_RES)
                .and_then(|g| g.origin)
        } else {
            None
        };
        let count = 3 + usize::from(origin.is_some()) * 7;
        // A new colony of the world re-reads the sites (the tool placed it).
        let have = world
            .get_resource::<crate::game::RunProgress>()
            .map_or(0, |r| space_city_count(&r.space_campaign, id));
        let entry = cache
            .sites
            .get(&id)
            .filter(|(c, d)| *c == count && (id == 0 || d.len() == have))
            .cloned();
        let dirs = match entry {
            Some((_, d)) => d,
            None => {
                let cities = world_cities(
                    id,
                    world.get_resource::<crate::game::RunProgress>(),
                    &generated.planet,
                    terrain.look.water_level,
                    origin,
                );
                let d: Vec<Vec3> = cities.into_iter().map(|(d, _)| d).collect();
                cache.sites.insert(id, (count, d.clone()));
                d
            }
        };
        let Some(dir) = dirs.get(slot as usize) else {
            continue;
        };
        let p = &generated.planet;
        let r = terrain.radius * p.height(*dir).max(p.water_radius()) / p.radius;
        if let Some(mut t) = world.get_mut::<Transform>(marker) {
            let want = *dir * r;
            if t.translation.distance_squared(want) > 1.0e-8 {
                t.translation = want;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Planet view
// ---------------------------------------------------------------------------

/// The surface of the world under the UFO.
#[derive(Resource)]
struct SurfaceView {
    world: u32,
    planet_entity: Entity,
    source: TerrainSource,
    res: usize,
    key: LookKey,
    planet: Arc<Planet>,
    root: Entity,
    land: Option<Handle<crate::terrain::TerrainMaterial>>,
    fallback: Handle<StandardMaterial>,
    atmosphere: Option<Handle<AtmosphereMaterial>>,
    chunks: HashMap<NodeKey, Entity>,
    meshes: HashMap<NodeKey, (Handle<Mesh>, Vec3)>,
    probe: crate::space_view::TerrainProbe,
    look: Look,
    /// City sites (planet-local directions).
    cities: Vec<Vec3>,
    /// The decoded planet look (`sky`): TerrainState, TerrainWater sea and
    /// TerrainAtmosphere shell; `None` keeps the adapted stand-ins.
    sky: Option<sky::SpaceSky>,
    sea: Option<Handle<crate::water::WaterMaterial>>,
    shell: Option<Handle<crate::planet_sky::RetailAtmosphereMaterial>>,
    /// The weather clouds (cloudVanilla puffs, `clouds`).
    clouds: Option<Handle<clouds::CloudMaterial>>,
    /// The ice / lava sea (TerrainBelowIce / Lava, `fluid`).
    fluid: Option<Handle<fluid::FluidMaterial>>,
}

/// The planet view's surface for Swarm decals (`swarm_fx::decal`): the viewed
/// world's height function (the one the chunks draw) in the surface root's
/// frame. Present while the planet view runs.
#[derive(Resource, Clone)]
pub struct PlanetViewGround {
    planet: Arc<Planet>,
    centre: Vec3,
    rotation: Quat,
    unit: f32,
}

impl PlanetViewGround {
    /// Scene units per retail unit.
    pub fn unit(&self) -> f32 {
        self.unit
    }

    /// The surface under a world point, along the radial direction: the
    /// surface point (world) and the outward normal there (the radial,
    /// as the terrain decals use, UpdateDecal 0xFAE7B0). The ground height
    /// `Planet::height`, or for water decals not below the sea surface.
    /// `None` at the planet's centre.
    pub fn surface_under(&self, world: Vec3, water: bool) -> Option<(Vec3, Vec3)> {
        let local = self.rotation.inverse() * (world - self.centre) / self.unit.max(1.0e-6);
        let dir = local.try_normalize()?;
        let mut radius = self.planet.height(dir);
        if water {
            radius = radius.max(self.planet.water_radius());
        }
        Some((self.centre + self.rotation * (dir * radius * self.unit), self.rotation * dir))
    }

    /// The planet-local direction of a world point (the frame of the height
    /// field, city sites and the minimap). `None` at the planet's centre.
    pub fn local_dir(&self, world: Vec3) -> Option<Vec3> {
        (self.rotation.inverse() * (world - self.centre)).try_normalize()
    }
}

/// What the HUD's planet minimap reads of the world in view: the retail
/// minimap window asks the planet model for the terrain and the world's cities
/// (0xE0D2C0 / 0xE0B560, `space_hud::minimap`). Present while the planet view
/// runs; replaced when the terrain, the look or the cities change.
#[derive(Resource, Clone)]
pub struct PlanetMinimapSource {
    pub world: u32,
    pub planet: Arc<Planet>,
    pub look: Look,
    /// The surface root's frame (`PlanetViewGround`).
    pub centre: Vec3,
    pub rotation: Quat,
    pub unit: f32,
    /// City sites: planet-local directions.
    pub cities: Vec<Vec3>,
}

/// Keep [`PlanetMinimapSource`] equal to the planet view's world.
fn update_minimap_source(world: &mut World, view: &SurfaceView, centre: Vec3, rotation: Quat, unit: f32) {
    let same = world.get_resource::<PlanetMinimapSource>().is_some_and(|s| {
        s.world == view.world
            && Arc::ptr_eq(&s.planet, &view.planet)
            && s.centre == centre
            && s.rotation == rotation
            && s.unit == unit
            && s.cities == view.cities
            && s.look == view.look
    });
    if !same {
        world.insert_resource(PlanetMinimapSource {
            world: view.world,
            planet: view.planet.clone(),
            look: view.look.clone(),
            centre,
            rotation,
            unit,
            cities: view.cities.clone(),
        });
    }
}

/// Marks the planet-view surface root and its parts.
#[derive(Component)]
pub struct SpaceSurface;

/// The sky haze inside the atmosphere (planet view).
#[derive(Component)]
pub struct SpaceSkyHaze;

/// A colony / city building on the planet-view surface.
#[derive(Component)]
pub struct SurfaceCityPart {
    pub city: usize,
}

/// The scale a solar-view body's own size had when the scale graphs first
/// saw it (`body_scale` multiplies it by the graph).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BodyScaleBase(pub Vec3);

/// The scale graphs of the Space objects (`SpaceGalaxyEffect`, 0x1034640,
/// `stars::scale_graph_value`): a planet, moon, gas giant or star object is
/// drawn at its own size times the graph at the distance from the camera
/// (retail units; callers 0xC84BA3 / 0xE97CA8 for the planets, 0xC865EA for
/// the star). In the planet view (`GetCurrentContext() == Planet`) the other
/// bodies take the scalar companions (planet 0.5, moon 0.4, gas giant 1).
/// ADAPTED: the base scale (`+0x184` of the retail object) is taken to be the
/// scene's own display size; the distance is the scene distance over
/// `SOLAR_SCALE`. `OPENSPORE_BODY_SCALE=0` switches it off (development).
#[allow(clippy::type_complexity)]
fn body_scale(
    mut commands: Commands,
    view: Option<Res<crate::space_view::SpaceView>>,
    camera: Query<&Transform, With<crate::GameCamera>>,
    mut bodies: Query<
        (
            Entity,
            &mut Transform,
            Option<&BodyScaleBase>,
            Option<&crate::game::Planet>,
            Option<&crate::space::Moon>,
            Option<&crate::space::GasGiant>,
            Option<&Name>,
        ),
        (With<crate::space::SystemBody>, Without<crate::GameCamera>),
    >,
) {
    use crate::space_galaxy::stars::{scale_graph_value, ScaleKind};
    use crate::space_view::SpaceZoom;
    let (Some(view), Ok(camera)) = (view, camera.single()) else {
        return;
    };
    let zoom = view.zoom();
    if zoom == SpaceZoom::Galaxy || std::env::var_os("OPENSPORE_BODY_SCALE").is_some_and(|v| v == "0") {
        return;
    }
    let from_planet = zoom == SpaceZoom::Planet;
    for (entity, mut transform, base, planet, moon, giant, name) in &mut bodies {
        let kind = if planet.is_some() {
            ScaleKind::Planet
        } else if moon.is_some() {
            ScaleKind::Moon
        } else if giant.is_some() {
            ScaleKind::GasGiant
        } else if name.is_some_and(|n| n.as_str() == "star core") {
            ScaleKind::Star
        } else {
            continue;
        };
        let Some(base) = base else {
            commands.entity(entity).insert(BodyScaleBase(transform.scale));
            continue;
        };
        let distance = camera.translation.distance(transform.translation) / crate::space_view::SOLAR_SCALE;
        // 0xC865EA passes the planet-context flag 0 for the star.
        let flag = from_planet && kind != ScaleKind::Star;
        let want = base.0 * scale_graph_value(kind, distance, flag);
        if (transform.scale - want).abs().max_element() > 1.0e-5 {
            transform.scale = want;
        }
    }
}

/// Keep the chunk mesh cache bounded.
const MESH_CACHE: usize = 1500;

fn teardown(world: &mut World) {
    show_moons(world);
    let scaled: Vec<Entity> = world
        .query_filtered::<Entity, With<PlanetViewScaled>>()
        .iter(world)
        .collect();
    for e in scaled {
        unscale_ship(world, e);
    }
    world.remove_resource::<PlanetViewGround>();
    world.remove_resource::<PlanetMinimapSource>();
    if let Some(view) = world.remove_resource::<SurfaceView>() {
        if let Ok(e) = world.get_entity_mut(view.root) {
            e.despawn();
        }
        if let Ok(mut e) = world.get_entity_mut(view.planet_entity) {
            e.insert(Visibility::Inherited);
        }
    }
    let haze: Vec<Entity> = world
        .query_filtered::<Entity, With<SpaceSkyHaze>>()
        .iter(world)
        .collect();
    for e in haze {
        world.entity_mut(e).insert(Visibility::Hidden);
    }
}

/// Per frame (after the Space camera): draw the real surface of the world the
/// UFO is over, refine its chunks for the camera, keep the material uniforms
/// on the orbiting world, hand the ground to the planet camera and fade the
/// sky into the atmosphere by altitude.
pub fn planet_view(world: &mut World) {
    if !in_space(world) || !render_app(world) {
        if world.contains_resource::<SurfaceView>() {
            teardown(world);
        }
        return;
    }
    let pv = world
        .get_resource::<crate::space_view::SpaceView>()
        .and_then(|v| v.planet.clone());
    let Some(pv) = pv else {
        if world.contains_resource::<SurfaceView>() {
            teardown(world);
        }
        return;
    };
    // The world and its generated terrain.
    let target = world
        .query::<(Entity, &crate::persistence::WorldId, &OrbitTerrain)>()
        .iter(world)
        .find(|(_, id, _)| id.0 == pv.world)
        .map(|(e, _, t)| (e, t.clone()));
    let Some((planet_entity, terrain)) = target else {
        if world.contains_resource::<SurfaceView>() {
            teardown(world);
        }
        return;
    };
    capture_colonies(world, pv.world);
    let mut cache = world.remove_resource::<SpacePlanets>().unwrap_or_default();
    cache.request(terrain.source, crate::planet_query::FACE_RES);
    let (generated, res) = match cache.get(terrain.source, crate::planet_query::FACE_RES) {
        Some(g) => {
            cache.touch_full(terrain.source);
            (g, crate::planet_query::FACE_RES)
        }
        None => match cache.get(terrain.source, ORBIT_RES) {
            Some(g) => (g, ORBIT_RES),
            None => {
                world.insert_resource(cache);
                return;
            }
        },
    };
    let seed = world
        .get_resource::<crate::game::RunProgress>()
        .map_or(0, |r| r.species_world.seed);
    // A colony founded in the planet view appears where the tool landed: the
    // city sites are read at the build, so a changed colony count rebuilds
    // (the homeworld's cities are the Civ stage's and do not follow it).
    let colonies = world
        .get_resource::<crate::game::RunProgress>()
        .map_or(0, |r| space_city_count(&r.space_campaign, pv.world));
    let rebuild = world.get_resource::<SurfaceView>().is_none_or(|v| {
        v.world != pv.world || v.res != res || v.key != terrain.key || v.source != terrain.source
            || world.get_entity(v.root).is_err()
            || (pv.world != 0 && v.cities.len() != colonies)
    });
    if rebuild {
        let fresh = world.get_resource::<SurfaceView>().is_none_or(|v| v.world != pv.world);
        teardown(world);
        build_surface(world, &mut cache, &pv, planet_entity, &terrain, generated, res, seed);
        // `OPENSPORE_SPACE_AT_CITY=<n>`: arrive over city n of the world
        // (development captures of the colonies).
        let at_city = std::env::var("OPENSPORE_SPACE_AT_CITY")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok());
        capture_overrides(world, fresh);
        if let (true, Some(n)) = (fresh, at_city) {
            let rotation = world.get::<Transform>(planet_entity).map_or(Quat::IDENTITY, |t| t.rotation);
            let dir = world
                .get_resource::<SurfaceView>()
                .and_then(|v| v.cities.get(n).copied())
                .map(|d| rotation * d);
            if let (Some(dir), Some(mut sv)) = (dir, world.get_resource_mut::<crate::space_view::SpaceView>()) {
                if let Some(p) = sv.planet.as_mut() {
                    // Stand back from the hall along the heading so the
                    // chase camera frames the city.
                    p.up = dir.normalize();
                    p.heading = crate::space_view::tangent(p.heading, p.up);
                }
            }
        }
    }
    world.insert_resource(cache);
    capture_overrides(world, false);
    sky::frame_stats();
    let Some(mut view) = world.remove_resource::<SurfaceView>() else {
        return;
    };
    let unit = pv.unit();
    let centre = pv.center;
    // The viewed world holds still (`space::space_orbits`): its frame is the
    // surface's frame.
    let rotation = world
        .get::<Transform>(view.planet_entity)
        .map_or(Quat::IDENTITY, |t| t.rotation);
    if let Some(mut t) = world.get_mut::<Transform>(view.root) {
        let want = Transform {
            translation: centre,
            rotation,
            scale: Vec3::splat(unit),
        };
        if *t != want {
            *t = want;
        }
    }
    // The ground the Swarm decals drape over (`swarm_fx::decal`).
    let same = world.get_resource::<PlanetViewGround>().is_some_and(|g| {
        Arc::ptr_eq(&g.planet, &view.planet) && g.centre == centre && g.rotation == rotation && g.unit == unit
    });
    if !same {
        world.insert_resource(PlanetViewGround { planet: view.planet.clone(), centre, rotation, unit });
    }
    if let Some(mut v) = world.get_mut::<Visibility>(view.planet_entity) {
        if *v != Visibility::Hidden {
            *v = Visibility::Hidden;
        }
    }
    // Uniforms in world space follow the world (written only when it moved:
    // a material rewrite costs a bind group).
    if let Some(land) = view.land.clone() {
        let want = centre.extend(view.planet.water_radius() * unit);
        let mut mats = world.resource_mut::<Assets<crate::terrain::TerrainMaterial>>();
        if mats.get(&land).is_some_and(|m| m.extension.planet != want) {
            if let Some(mut m) = mats.get_mut(&land) {
                m.extension.planet = want;
            }
        }
    }
    // Retail terrain lighting only (planet_light.rs) when the decoded planet
    // look is off (`OPENSPORE_RETAIL_SKY=0`); otherwise `sky` below.
    if let (Some(land), true) = (view.land.clone(), view.sky.is_none()) {
        let want = crate::planet_light::space_view_sky(world, centre);
        let mut mats = world.resource_mut::<Assets<crate::terrain::TerrainMaterial>>();
        if mats.get(&land).is_some_and(|m| m.extension.sky != want) {
            if let Some(mut m) = mats.get_mut(&land) {
                m.extension.sky = want;
            }
        }
    }
    if let Some(atm) = view.atmosphere.clone() {
        let want = centre.extend(view.planet.water_radius().max(view.planet.radius) * unit);
        let shell = view.planet.radius * shell_ratio(&view.planet) * unit;
        let mut mats = world.resource_mut::<Assets<AtmosphereMaterial>>();
        if mats.get(&atm).is_some_and(|m| m.extension.sphere != want || m.extension.params.y != shell) {
            if let Some(mut m) = mats.get_mut(&atm) {
                m.extension.sphere = want;
                m.extension.params.y = shell;
            }
        }
    }
    // The planet camera and the UFO stand on this ground.
    if let Some(mut sv) = world.get_resource_mut::<crate::space_view::SpaceView>() {
        if let Some(p) = sv.planet.as_mut() {
            let same = p
                .terrain
                .as_ref()
                .is_some_and(|t| Arc::ptr_eq(&t.0, &view.probe.0));
            if !same {
                p.terrain = Some(view.probe.clone());
            }
        }
    }
    // Chunks for the camera.
    let camera = world
        .query_filtered::<&Transform, With<crate::GameCamera>>()
        .iter(world)
        .next()
        .map(|t| t.translation);
    if let Some(camera) = camera {
        let local = rotation.inverse() * (camera - centre) / unit.max(1.0e-6);
        let wanted = crate::planet_render::select_nodes(&view.planet, local);
        let wanted_set: HashSet<NodeKey> = wanted.iter().copied().collect();
        let stale: Vec<NodeKey> = view.chunks.keys().filter(|k| !wanted_set.contains(k)).copied().collect();
        for key in stale {
            if let Some(e) = view.chunks.remove(&key) {
                if let Ok(em) = world.get_entity_mut(e) {
                    em.despawn();
                }
            }
        }
        if view.meshes.len() > MESH_CACHE {
            let keep: HashSet<NodeKey> = view.chunks.keys().copied().collect();
            view.meshes.retain(|k, _| keep.contains(k) || wanted_set.contains(k));
        }
        for key in wanted {
            if view.chunks.contains_key(&key) {
                continue;
            }
            let (mesh, at) = match view.meshes.get(&key) {
                Some(m) => m.clone(),
                None => {
                    let (mesh, at) = crate::planet_render::chunk_mesh(&view.planet, key);
                    let handle = world.resource_mut::<Assets<Mesh>>().add(mesh);
                    view.meshes.insert(key, (handle.clone(), at));
                    (handle, at)
                }
            };
            let mut e = world.spawn((
                SpaceSurface,
                crate::planet_render::PlanetChunk(key),
                Mesh3d(mesh),
                NotShadowCaster,
                Transform::from_translation(at),
                ChildOf(view.root),
            ));
            match view.land.clone() {
                Some(land) => e.insert(MeshMaterial3d(land)),
                None => e.insert(MeshMaterial3d(view.fallback.clone())),
            };
            view.chunks.insert(key, e.id());
        }
    }
    // The decoded planet look: the TerrainState of this frame into the land,
    // the sea and the atmosphere shell (space_planets::sky).
    if let (Some(mut sky), Some(camera)) = (view.sky.take(), camera) {
        let state = sky.update(world, centre, unit, &view.planet, camera);
        if let Some(land) = view.land.clone() {
            if let Some(mut m) = world.resource_mut::<Assets<crate::terrain::TerrainMaterial>>().get_mut(&land) {
                m.extension.sky = state;
                if m.extension.scatter != sky.scatter {
                    m.extension.scatter = sky.scatter.clone();
                    m.extension.below_texture = sky.below.clone();
                    m.base.fog_enabled = false;
                }
            }
        }
        if let Some(sea) = view.sea.clone() {
            let want = centre.extend(view.planet.water_radius() * unit);
            if let Some(mut m) = world.resource_mut::<Assets<crate::water::WaterMaterial>>().get_mut(&sea) {
                m.extension.sky = state;
                if m.extension.planet != want {
                    m.extension.planet = want;
                }
            }
        }
        if let Some(shell) = view.shell.clone() {
            if let Some(mut m) = world.resource_mut::<Assets<crate::planet_sky::RetailAtmosphereMaterial>>().get_mut(&shell) {
                m.extension.sky = state;
            }
        }
        if let Some(c) = view.clouds.clone() {
            // OPENSPORE_CLOUD_STATIC=1 (development): the first state only.
            static WRITES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let stat = std::env::var("OPENSPORE_CLOUD_STATIC").is_ok_and(|v| v == "1");
            if !stat || WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 5 {
                if let Some(mut m) = world.resource_mut::<Assets<clouds::CloudMaterial>>().get_mut(&c) {
                    m.sky = state;
                }
            }
        }
        if let Some(f) = view.fluid.clone() {
            if let Some(mut m) = world.resource_mut::<Assets<fluid::FluidMaterial>>().get_mut(&f) {
                m.extension.sky = state;
            }
        }
        view.sky = Some(sky);
    }
    // The sky inside the atmosphere (planetAtmosphereStart / End fade the
    // stars by altitude): an ADAPTED haze of the air colour around the camera
    // (the decoded shell is the sky when `sky` is on).
    let amount = if view.sky.is_some() {
        0.0
    } else {
        crate::space_view::atmosphere_amount(pv.altitude) * view.look.thickness / 1.75
    };
    // The haze shell sits just beyond the world's horizon, so the sky, the
    // star and the other bodies are seen through the air but the ground in
    // view is not.
    let haze_radius = camera.map_or(400.0, |c| {
        let r = view.planet.radius * unit;
        let d = c.distance(centre).max(r * 1.001);
        ((d * d - r * r).sqrt() * 1.25).clamp(r * 0.2, 400.0)
    });
    update_haze(world, camera, amount, view.look.air, haze_radius);
    // ADAPTED: the scene's moons orbit 2-3 world radii out (retail moons are
    // far away); from the surface they would fill the sky, so the viewed
    // world's moons wait out the planet view.
    let moons: Vec<(Entity, bool)> = world
        .query::<(Entity, &crate::space::Moon, &Visibility)>()
        .iter(world)
        .map(|(e, m, v)| (e, m.host == view.planet_entity && *v != Visibility::Hidden))
        .collect();
    for (e, hide) in moons {
        if hide {
            world.entity_mut(e).insert((Visibility::Hidden, HiddenMoon));
        }
    }
    scale_ships(world, &pv);
    // The solar view's orbit rings would cut through the world and its sky.
    let rings: Vec<Entity> = world
        .query_filtered::<(Entity, &Visibility), With<crate::space_scene::OrbitRing>>()
        .iter(world)
        .filter(|(_, v)| **v != Visibility::Hidden)
        .map(|(e, _)| e)
        .collect();
    for e in rings {
        world.entity_mut(e).insert((Visibility::Hidden, HiddenRing));
    }
    if std::env::var_os("OPENSPORE_SPACE_TRACE").is_some() {
        trace_view(world, &view, &pv, camera);
    }
    update_minimap_source(world, &view, centre, rotation, unit);
    world.insert_resource(view);
}

/// An NPC ship drawn at the planet view's retail proportion.
#[derive(Component)]
pub struct PlanetViewScaled;

/// ADAPTED: pirates and escorts near the viewed world are drawn at the UFO's
/// retail size (their models are fitted to the solar scene: 1.8 / 2.4 scene
/// units would be hundreds of retail units over the world). Their flight
/// paths stay the solar-scene AI's.
fn scale_ships(world: &mut World, pv: &crate::space_view::PlanetView) {
    use crate::space_view::{SHIP_MODEL_SIZE, UFO_RETAIL_SIZE};
    let reach = pv.radius + crate::space::action_range() * 3.0;
    let ships: Vec<(Entity, f32, bool, bool)> = world
        .query_filtered::<(
            Entity,
            &Transform,
            Has<crate::space::Pirate>,
            Has<PlanetViewScaled>,
        ), Or<(With<crate::space::Pirate>, With<crate::space::EscortWingman>)>>()
        .iter(world)
        .map(|(e, t, pirate, scaled)| (e, t.translation.distance(pv.center), pirate, scaled))
        .collect();
    for (e, distance, pirate, scaled) in ships {
        let fit = if pirate { 1.8 } else { SHIP_MODEL_SIZE };
        if distance < reach {
            let s = UFO_RETAIL_SIZE * pv.unit() / fit;
            if let Some(mut t) = world.get_mut::<Transform>(e) {
                if (t.scale.x - s).abs() > 1.0e-3 * s {
                    t.scale = Vec3::splat(s);
                }
            }
            if !scaled {
                world.entity_mut(e).insert(PlanetViewScaled);
            }
        } else if scaled {
            unscale_ship(world, e);
        }
    }
}

fn unscale_ship(world: &mut World, e: Entity) {
    if let Some(mut t) = world.get_mut::<Transform>(e) {
        t.scale = Vec3::ONE;
    }
    world.entity_mut(e).remove::<PlanetViewScaled>();
}

/// A moon hidden while its world is viewed from close by.
#[derive(Component)]
pub struct HiddenMoon;

/// An orbit ring hidden during the planet view.
#[derive(Component)]
pub struct HiddenRing;

fn show_moons(world: &mut World) {
    let hidden: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<HiddenMoon>, With<HiddenRing>)>>()
        .iter(world)
        .collect();
    for e in hidden {
        world
            .entity_mut(e)
            .remove::<(HiddenMoon, HiddenRing)>()
            .insert(Visibility::Inherited);
    }
}

/// `OPENSPORE_SPACE_TRACE=1`: the planet view's scale, ship and camera, and
/// the drawn meshes near the camera (every 30 frames).
fn trace_view(world: &mut World, view: &SurfaceView, pv: &crate::space_view::PlanetView, camera: Option<Vec3>) {
    static FRAME: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 30 != 0 {
        return;
    }
    let ship = world
        .query_filtered::<&Transform, With<crate::game::Player>>()
        .iter(world)
        .next()
        .map(|t| (t.translation, t.scale.x));
    eprintln!(
        "space view: world {} alt {:.0} unit {:.5} centre {:.2} cam {:?} ship {:?} chunks {} ground {:.1}",
        pv.world,
        pv.altitude,
        pv.unit(),
        pv.center,
        camera,
        ship,
        view.chunks.len(),
        pv.ground(pv.up)
    );
    let Some(camera) = camera else {
        return;
    };
    let near: Vec<String> = world
        .query::<(
            &GlobalTransform,
            &Mesh3d,
            &InheritedVisibility,
            Option<&Name>,
            (Has<crate::space::Moon>, Has<crate::game::Planet>, Has<crate::space::Pirate>, Has<SpaceSurface>),
        )>()
        .iter(world)
        .filter(|(t, _, v, _, (_, _, _, surface))| {
            v.get() && !surface && t.translation().distance(camera) < 6.0 && t.compute_transform().scale.x > 0.1
        })
        .take(16)
        .map(|(t, _, _, n, (moon, planet, pirate, _))| {
            format!(
                "{} moon {moon} planet {planet} pirate {pirate} d{:.2} s{:.3}",
                n.map_or("?".to_string(), |n| n.to_string()),
                t.translation().distance(camera),
                t.compute_transform().scale.x,
            )
        })
        .collect();
    eprintln!("space view near camera: {near:?}");
    let clouds: Vec<String> = world
        .query::<(&MeshMaterial3d<clouds::CloudMaterial>, &InheritedVisibility, &ViewVisibility, &GlobalTransform, Option<&bevy::camera::primitives::Aabb>)>()
        .iter(world)
        .map(|(m, iv, vv, t, aabb)| {
            format!(
                "material {} inherited {} view {} at {:.2} scale {:.4} aabb {:?}",
                world.resource::<Assets<clouds::CloudMaterial>>().get(&m.0).is_some(),
                iv.get(),
                vv.get(),
                t.translation(),
                t.compute_transform().scale.x,
                aabb.map(|a| (a.center, a.half_extents))
            )
        })
        .collect();
    eprintln!("space view clouds: {clouds:?}");
}

fn update_haze(world: &mut World, camera: Option<Vec3>, amount: f32, air: [f32; 3], radius: f32) {
    let existing = world
        .query_filtered::<(Entity, &MeshMaterial3d<StandardMaterial>), With<SpaceSkyHaze>>()
        .iter(world)
        .next()
        .map(|(e, m)| (e, m.0.clone()));
    let (entity, material) = match existing {
        Some(x) => x,
        None => {
            if amount <= 0.0 {
                return;
            }
            let Ok(mesh) = Sphere::new(1.0).mesh().ico(3) else {
                return;
            };
            let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
            let material = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
                base_color: Color::srgba(air[0], air[1], air[2], 0.0),
                unlit: true,
                fog_enabled: false,
                cull_mode: None,
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
            let e = world
                .spawn((
                    StageOwned,
                    SpaceSkyHaze,
                    NotShadowCaster,
                    Name::new("planet view sky haze (adapted)"),
                    Mesh3d(mesh),
                    MeshMaterial3d(material.clone()),
                    Transform::IDENTITY,
                    Visibility::Hidden,
                ))
                .id();
            (e, material)
        }
    };
    let alpha = (amount * 0.92).clamp(0.0, 0.92);
    if let Some(mut m) = world.resource_mut::<Assets<StandardMaterial>>().get_mut(&material) {
        let want = Color::srgba(air[0] * 0.9, air[1] * 0.9, air[2] * 0.95, alpha);
        if m.base_color != want {
            m.base_color = want;
        }
    }
    if let Some(at) = camera {
        if let Some(mut t) = world.get_mut::<Transform>(entity) {
            t.translation = at;
            t.scale = Vec3::splat(radius);
        }
    }
    let visible = if alpha > 0.002 { Visibility::Inherited } else { Visibility::Hidden };
    if let Some(mut v) = world.get_mut::<Visibility>(entity) {
        if *v != visible {
            *v = visible;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_surface(
    world: &mut World,
    cache: &mut SpacePlanets,
    pv: &crate::space_view::PlanetView,
    planet_entity: Entity,
    terrain: &OrbitTerrain,
    generated: Arc<Generated>,
    res: usize,
    seed: u64,
) {
    let started = std::time::Instant::now();
    let mut times: Vec<(&str, u128)> = Vec::new();
    let mut lap = std::time::Instant::now();
    let mut mark = |name: &'static str, times: &mut Vec<(&'static str, u128)>| {
        times.push((name, lap.elapsed().as_millis()));
        lap = std::time::Instant::now();
    };
    let mut look = terrain.look.clone();
    // `OPENSPORE_SPACE_FLUID=ice|lava` (development captures of the ice / lava
    // seas on a world with water).
    match std::env::var("OPENSPORE_SPACE_FLUID").as_deref() {
        Ok("ice") => look.fluid = Fluid::Ice,
        Ok("lava") => look.fluid = Fluid::Lava,
        _ => {}
    }
    let home_origin = if pv.world == 0 {
        cache
            .get(TerrainSource::Home(seed), crate::planet_query::FACE_RES)
            .and_then(|g| g.origin)
    } else {
        None
    };
    // City sites from the orbit planet (stable across resolutions).
    let site_planet = cache
        .get(terrain.source, ORBIT_RES)
        .map(|g| g.planet.clone())
        .unwrap_or_else(|| generated.planet.clone());
    let cities = world_cities(
        pv.world,
        world.get_resource::<crate::game::RunProgress>(),
        &site_planet,
        look.water_level,
        home_origin,
    );
    // The view planet: the generated cube with the city flatteners
    // (AddPlayerModification, CityFlattenSize 75) and the terraformed water.
    let base = &generated.planet;
    let planet = if cities.is_empty() && (look.water_level - base.water_level).abs() < 1.0e-4 {
        generated.planet.clone()
    } else {
        let mut cube = base.cube.clone();
        for (dir, _) in &cities {
            crate::planet_gen::flatten_pad(&mut cube, *dir, crate::civ_rules::CITY_FLATTEN_SIZE * 0.5, base.radius);
        }
        Arc::new(Planet::new(cube, base.radius, look.water_level))
    };
    mark("planet", &mut times);
    let unit = pv.unit();
    let rotation = world
        .get::<Transform>(planet_entity)
        .map_or(Quat::IDENTITY, |t| t.rotation);
    let root = world
        .spawn((
            StageOwned,
            SpaceSurface,
            Name::new(format!("planet view surface (world {})", pv.world)),
            Transform::from_translation(pv.center)
                .with_rotation(rotation)
                .with_scale(Vec3::splat(unit)),
            Visibility::Visible,
        ))
        .id();
    let phase = match terrain.source {
        TerrainSource::Home(s) => (s ^ (s >> 32)) as u32 as f32 / u32::MAX as f32 * 6.28,
        TerrainSource::Script(g, i) => ((g ^ i.rotate_left(7)) % 6283) as f32 / 1000.0,
    };
    let land = if look.home {
        crate::terrain::space_planet_material(world, &planet, pv.center, unit, phase, None, None, None)
    } else {
        crate::terrain::space_planet_material(
            world,
            &planet,
            pv.center,
            unit,
            phase,
            Some(&look.ramp),
            Some(look.beach),
            Some(look.cliff),
        )
    };
    mark("material", &mut times);
    // The decoded planet look (TerrainState with the world's tables, the
    // TerrainWater sea and the TerrainAtmosphere shell): `sky`.
    let mut space_sky = if sky::enabled() {
        let inputs = crate::planet_sky::SkyInputs {
            temperature: look.climate.0.clamp(0.0, 1.0),
            atmosphere: look.climate.1.clamp(0.0, 1.0),
            ..generated.sky
        };
        sky::SpaceSky::load(world, inputs, generated.below, look.air, planet.altitude_range)
    } else {
        None
    };
    mark("sky", &mut times);
    let mid = look.ramp[look.ramp.len() / 3];
    let fallback = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color: Color::srgb(mid[0], mid[1], mid[2]),
        perceptual_roughness: 0.92,
        ..default()
    });
    // The sea: TerrainWater with the decoded look; else (and for ice / lava)
    // the sphere shaded by depth per vertex.
    let (gmin, _) = planet.ranges.global();
    let sea = match (planet.water_level > gmin && look.fluid == Fluid::Water, space_sky.as_ref()) {
        (true, Some(s)) => sky::sea_material(world, &planet, rotation, pv.center, unit, phase, s),
        _ => None,
    };
    // Ice / lava worlds: TerrainBelowIce / TerrainBelowLava on the sphere.
    let fluid_sea = match (planet.water_level > gmin, look.fluid, space_sky.as_ref()) {
        (true, Fluid::Ice | Fluid::Lava, Some(s)) => {
            let detail = land.as_ref().and_then(|h| {
                world.resource::<Assets<crate::terrain::TerrainMaterial>>().get(h).map(|m| m.extension.detail_texture.clone())
            });
            fluid::material(world, &planet, rotation, look.fluid == Fluid::Lava, s, detail)
        }
        _ => None,
    };
    if let Some(handle) = fluid_sea.clone() {
        let mesh = world.resource_mut::<Assets<Mesh>>().add(crate::planet_render::water_mesh(&planet));
        world.spawn((
            SpaceSurface,
            Name::new(if look.fluid == Fluid::Lava { "planet view lava (TerrainBelowLava)" } else { "planet view ice (TerrainBelowIce)" }),
            NotShadowCaster,
            Mesh3d(mesh),
            MeshMaterial3d(handle),
            Transform::IDENTITY,
            ChildOf(root),
        ));
    } else if let Some(handle) = sea.clone() {
        let mesh = world.resource_mut::<Assets<Mesh>>().add(crate::planet_render::water_mesh(&planet));
        world.spawn((
            SpaceSurface,
            Name::new("planet view sea (TerrainWater)"),
            NotShadowCaster,
            Mesh3d(mesh),
            MeshMaterial3d(handle),
            Transform::IDENTITY,
            ChildOf(root),
        ));
    } else if planet.water_level > gmin {
        let mut mesh = crate::planet_render::water_mesh(&planet);
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            let shallow = [0, 1, 2].map(|k| look.water[k] + (look.beach[k] - look.water[k]) * 0.45);
            let deep = look.water.map(|v| v * 0.55);
            let colors: Vec<[f32; 4]> = pos
                .iter()
                .map(|p| {
                    let dir = Vec3::from_array(*p).normalize();
                    let depth = planet.water_radius() - planet.height(dir);
                    let t = (depth / 15.0).clamp(0.0, 1.0);
                    let c = [0, 1, 2].map(|k| shallow[k] + (deep[k] - shallow[k]) * t);
                    let l = Color::srgb(c[0], c[1], c[2]).to_linear();
                    [l.red, l.green, l.blue, 1.0]
                })
                .collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        }
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let material = match look.fluid {
            Fluid::Water => StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.12,
                reflectance: 0.5,
                ..default()
            },
            Fluid::Ice => StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.55,
                reflectance: 0.3,
                ..default()
            },
            Fluid::Lava => StandardMaterial {
                base_color: Color::srgb(0.25, 0.08, 0.02),
                emissive: LinearRgba::rgb(4.0, 1.1, 0.15),
                perceptual_roughness: 0.8,
                ..default()
            },
        };
        let material = world.resource_mut::<Assets<StandardMaterial>>().add(material);
        world.spawn((
            SpaceSurface,
            Name::new("planet view sea"),
            NotShadowCaster,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            ChildOf(root),
        ));
    }
    // The atmosphere shell: TerrainAtmosphere with the decoded look.
    let shell = space_sky.as_ref().and_then(|s| sky::shell(world, &planet, s)).map(|(mesh, material)| {
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        world.spawn((
            SpaceSurface,
            NotShadowCaster,
            Name::new("planet view atmosphere (TerrainAtmosphere)"),
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::IDENTITY,
            ChildOf(root),
        ));
        material
    });
    // The weather clouds over worlds with air (`clouds`; `OPENSPORE_SPACE_CLOUDS=0`
    // leaves them out).
    let cloud_material = match (space_sky.as_ref(), look.thickness > 0.0) {
        (Some(s), true) if !std::env::var("OPENSPORE_SPACE_CLOUDS").is_ok_and(|v| v == "0") => {
            clouds::material(world, generated.cloud_type, look.climate.0, s.scatter.clone())
        }
        _ => None,
    };
    if let Some(material) = cloud_material.clone() {
        let seed = match terrain.source {
            TerrainSource::Home(s) => s,
            TerrainSource::Script(g, i) => (g as u64) << 32 | i as u64,
        };
        let mesh = world.resource_mut::<Assets<Mesh>>().add(clouds::mesh(&clouds::puffs(&planet, seed)));
        world.spawn((
            SpaceSurface,
            NotShadowCaster,
            Name::new("planet view clouds (cloudVanilla)"),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            ChildOf(root),
        ));
    }
    let atmosphere = if look.thickness > 0.0 && space_sky.is_none() {
        Sphere::new(planet.radius * shell_ratio(&planet)).mesh().ico(5).ok().map(|mesh| {
            let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
            let mat = world.resource_mut::<Assets<AtmosphereMaterial>>().add(shell_material(
                &look,
                pv.center,
                planet.water_radius().max(planet.radius) * unit,
                planet.radius * shell_ratio(&planet) * unit,
            ));
            world.spawn((
                SpaceSurface,
                NotShadowCaster,
                Name::new("planet view atmosphere shell (adapted)"),
                Mesh3d(mesh),
                MeshMaterial3d(mat.clone()),
                Transform::IDENTITY,
                ChildOf(root),
            ));
            mat
        })
    } else {
        None
    };
    mark("sea+air", &mut times);
    spawn_cities(world, root, &planet, &cities);
    mark("cities", &mut times);
    // The ground under a world direction: the world's frame at entry (it
    // holds still while viewed).
    let probe_planet = planet.clone();
    let to_local = world
        .get::<Transform>(planet_entity)
        .map_or(Quat::IDENTITY, |t| t.rotation)
        .inverse();
    let probe = crate::space_view::TerrainProbe(Arc::new(move |dir: Vec3| {
        let local = to_local * dir;
        probe_planet.height(local).max(probe_planet.water_radius()) - probe_planet.radius
    }));
    info!(
        "space planets: planet view of world {} ({}^2, {} cities) built in {} ms {:?}; decoded look {} sea {} fluid {} shell {} clouds {} (type {})",
        pv.world,
        res,
        cities.len(),
        started.elapsed().as_millis(),
        times,
        space_sky.is_some(),
        sea.is_some(),
        fluid_sea.is_some(),
        shell.is_some(),
        cloud_material.is_some(),
        generated.cloud_type,
    );
    world.insert_resource(SurfaceView {
        world: pv.world,
        planet_entity,
        source: terrain.source,
        res,
        key: terrain.key,
        planet,
        root,
        land,
        fallback,
        atmosphere,
        chunks: HashMap::new(),
        meshes: HashMap::new(),
        probe,
        look,
        cities: cities.iter().map(|(d, _)| *d).collect(),
        sky: space_sky.take(),
        sea,
        shell,
        clouds: cloud_material,
        fluid: fluid_sea,
    });
}

/// Render-app registration (from `planet_render::build`): the weather cloud
/// material and its shader.
pub fn build_render(app: &mut App) {
    bevy::asset::embedded_asset!(app, "shaders/space_clouds.wesl");
    bevy::asset::embedded_asset!(app, "shaders/space_fluid.wesl");
    bevy::asset::embedded_asset!(app, "shaders/space_impostor.wesl");
    bevy::asset::embedded_asset!(app, "shaders/space_atmo_impostor.wesl");
    crate::world_presentation::dev_shader_override(app, "space_clouds.wesl");
    crate::world_presentation::dev_shader_override(app, "space_fluid.wesl");
    crate::world_presentation::dev_shader_override(app, "space_impostor.wesl");
    crate::world_presentation::dev_shader_override(app, "space_atmo_impostor.wesl");
    clouds::build(app);
    fluid::build(app);
    impostor::build(app);
    crate::space_galaxy::build_render(app);
}

/// Development captures of the colonies: `OPENSPORE_SPACE_COLONIES=<n>` gives
/// the viewed world n colonies, `OPENSPORE_SPACE_COLONY_SITES=x,y,z;x,y,z`
/// the planet-local directions the colony tool landed at (the first colonies;
/// the others stay at the seeded sites). Idempotent: runs every frame.
fn capture_colonies(world: &mut World, id: u32) {
    let Some(n) = std::env::var("OPENSPORE_SPACE_COLONIES").ok().and_then(|v| v.trim().parse::<u8>().ok()) else {
        return;
    };
    if id == 0 {
        return;
    }
    let sites: Vec<Vec3> = std::env::var("OPENSPORE_SPACE_COLONY_SITES")
        .map(|v| {
            v.split(';')
                .filter_map(|p| {
                    let c: Vec<f32> = p.split(',').filter_map(|c| c.trim().parse().ok()).collect();
                    (c.len() == 3).then(|| Vec3::new(c[0], c[1], c[2]).normalize_or(Vec3::Y))
                })
                .collect()
        })
        .unwrap_or_default();
    let Some(mut run) = world.get_resource_mut::<crate::game::RunProgress>() else {
        return;
    };
    let campaign = &mut run.space_campaign;
    if campaign.colony_count(id) >= n && campaign.colonies.contains(&id) {
        return;
    }
    let index = campaign.ensure_ecology(id);
    campaign.ecology[index].colony_count = n;
    campaign.own(id);
    let layouts = campaign.ensure_city_layouts(id);
    for (layout, site) in layouts.iter_mut().zip(&sites) {
        layout.site = Some(site.to_array());
    }
}

/// Development captures: `OPENSPORE_SPACE_SUN_DEG=<deg>` puts the UFO that many
/// degrees from the point under the star (0 noon, 180 midnight) when it
/// arrives; `OPENSPORE_SPACE_ALTITUDE=<retail units>` holds its altitude.
fn capture_overrides(world: &mut World, fresh: bool) {
    let sun_deg = std::env::var("OPENSPORE_SPACE_SUN_DEG").ok().and_then(|v| v.trim().parse::<f32>().ok());
    let altitude = std::env::var("OPENSPORE_SPACE_ALTITUDE").ok().and_then(|v| v.trim().parse::<f32>().ok());
    if sun_deg.is_none() && altitude.is_none() {
        return;
    }
    let Some(mut sv) = world.get_resource_mut::<crate::space_view::SpaceView>() else {
        return;
    };
    let Some(p) = sv.planet.as_mut() else {
        return;
    };
    if let (Some(deg), true) = (sun_deg, fresh) {
        let sun = (-p.center).normalize_or(Vec3::Y);
        let axis = sun.cross(if sun.y.abs() > 0.9 { Vec3::X } else { Vec3::Y }).normalize();
        p.up = (Quat::from_axis_angle(axis, deg.to_radians()) * sun).normalize();
        p.heading = crate::space_view::tangent(p.heading, p.up);
    }
    if let Some(a) = altitude {
        p.altitude = a;
        p.auto = None;
    }
}

/// City sizes in retail units, as the Civ city draws them
/// (`civ_planet::CITY_SCALE`: the plot ring at RadiusFromCityHall 15, the
/// district and hall footprints scaled with it; CityFlattenSize 75 is the
/// pad). ADAPTED: Space colonies reuse the Civ city's proportions.
const HALL_WIDTH: f32 = crate::city_presentation::CITY_CENTRAL_HALL_FOOTPRINT * crate::civ_planet::CITY_SCALE;
const PLOT_WIDTH: f32 = crate::city_presentation::CITY_DISTRICT_WIDTH * crate::civ_planet::CITY_SCALE;
const TURRET_WIDTH: f32 = PLOT_WIDTH * 0.6;
const PLOT_RING: f32 = crate::civ_rules::RADIUS_FROM_CITY_HALL;

/// Installed city models for the planet view (the Civ city's district
/// sources, the castle gun for turrets).
#[derive(Resource, Default)]
struct CityArt(HashMap<ResourceKey, Option<crate::OriginalModel>>);

fn city_model(world: &mut World, key: ResourceKey) -> Option<crate::OriginalModel> {
    if let Some(m) = world.get_resource::<CityArt>().and_then(|a| a.0.get(&key).cloned()) {
        return m;
    }
    let mut meshes = world.remove_resource::<Assets<Mesh>>()?;
    let mut images = world.remove_resource::<Assets<Image>>();
    let mut materials = world.remove_resource::<Assets<StandardMaterial>>();
    let model = match (images.as_mut(), materials.as_mut(), world.get_resource_mut::<crate::Originals>()) {
        (Some(images), Some(materials), Some(mut originals)) if !originals.resource_index.records.is_empty() => {
            crate::baked_model::ensure(&mut originals, key, &mut meshes, images, materials)
                .ok()
                .filter(|m| !m.surfaces.is_empty())
        }
        _ => None,
    };
    world.insert_resource(meshes);
    if let Some(i) = images {
        world.insert_resource(i);
    }
    if let Some(m) = materials {
        world.insert_resource(m);
    }
    world.get_resource_or_insert_with(CityArt::default).0.insert(key, model.clone());
    model
}

/// Bounds of a model's combined mesh.
fn model_bounds(world: &World, model: &crate::OriginalModel) -> Option<(Vec3, Vec3)> {
    let mesh = world.resource::<Assets<Mesh>>().get(&model.handle)?;
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        return None;
    };
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for p in v {
        min = min.min(Vec3::from_array(*p));
        max = max.max(Vec3::from_array(*p));
    }
    (min.x.is_finite() && max.x > min.x).then_some((min, max))
}

fn spawn_cities(world: &mut World, root: Entity, planet: &Planet, cities: &[(Vec3, Vec<Option<Plot>>)]) {
    if cities.is_empty() {
        return;
    }
    let hall_key = crate::city_presentation::CITY_DISTRICT_CITY_HALL_SOURCE;
    let key_for = |p: &Plot| match p {
        Plot::House => crate::city_presentation::CITY_DISTRICT_HOUSE_SOURCE,
        Plot::Factory => crate::city_presentation::CITY_DISTRICT_INDUSTRY_SOURCE,
        Plot::Entertainment => crate::city_presentation::CITY_DISTRICT_ENTERTAINMENT_SOURCE,
        Plot::Turret => crate::city_presentation::gun_source(crate::civ::Approach::Military, false).0,
    };
    for (city, (dir, plots)) in cities.iter().enumerate() {
        let up = dir.normalize();
        let (t, b) = crate::planet_query::tangent_basis(up);
        let mut items: Vec<(ResourceKey, Vec3, f32, f32)> = vec![(hall_key, up, HALL_WIDTH, 0.0)];
        for (i, plot) in plots.iter().enumerate() {
            let Some(plot) = plot else {
                continue;
            };
            let angle = i as f32 / plots.len().max(6) as f32 * std::f32::consts::TAU;
            let offset = (t * angle.cos() + b * angle.sin()) * PLOT_RING;
            let at = (up * planet.radius + offset).normalize();
            let width = if *plot == Plot::Turret { TURRET_WIDTH } else { PLOT_WIDTH };
            // Fronts face outward.
            items.push((key_for(plot), at, width, angle + std::f32::consts::FRAC_PI_2));
        }
        for (key, at, width, yaw) in items {
            let Some(model) = city_model(world, key) else {
                continue;
            };
            let Some((min, max)) = model_bounds(world, &model) else {
                continue;
            };
            let span = (max.x - min.x).max(max.z - min.z).max(1.0e-3);
            let scale = width / span;
            let centre = (min + max) * 0.5;
            let ground = planet.height(at).max(planet.water_radius());
            let rotation = Quat::from_rotation_arc(Vec3::Y, at) * Quat::from_rotation_y(yaw);
            // Base on the ground, sunk a little against slopes.
            let offset = Vec3::new(-centre.x, -min.y, -centre.z) * scale - Vec3::Y * 0.4;
            let transform = Transform {
                translation: at * ground + rotation * offset,
                rotation,
                scale: Vec3::splat(scale),
            };
            for surface in &model.surfaces {
                world.spawn((
                    SpaceSurface,
                    SurfaceCityPart { city },
                    crate::material_binding::OriginalSurfaceChild {
                        source_section: surface.source_section,
                        state_sections: surface.state_sections.clone(),
                    },
                    Name::new(format!("colony city {} {:08X}", city + 1, key.instance_id)),
                    Mesh3d(surface.handle.clone()),
                    MeshMaterial3d(surface.material.clone()),
                    transform,
                    ChildOf(root),
                ));
            }
        }
    }
}

pub struct SpacePlanetsPlugin;

impl Plugin for SpacePlanetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                orbit_planets.after(crate::space_visuals::prepare),
                planet_view.after(crate::space_view::camera),
            )
                .run_if(in_state(GameStage::Space)),
        )
        .add_systems(
            Update,
            body_scale
                .after(crate::space_view::camera)
                .run_if(in_state(GameStage::Space)),
        )
        .add_systems(OnExit(GameStage::Space), teardown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet_query::{HeightCube, PLANET_RADIUS};

    fn bumpy(water: f32) -> Planet {
        let res = 32;
        let mut cube = HeightCube::filled(res, 0.0);
        for face in 0..6 {
            for row in 0..res {
                for col in 0..res {
                    let d = crate::planet_query::texel_dir(face, row, col, res);
                    let i = cube.index(face, row, col);
                    cube.raw_mut()[i] = crate::planet_query::h_to_raw((d.x * 3.0).sin() * 0.4 + d.y * 0.2);
                }
            }
        }
        Planet::new(cube, PLANET_RADIUS, water)
    }

    fn climate(t_score: u8) -> Climate {
        Climate {
            info: [8.0, 54.0],
            base_water: 0.5,
            now: (0.5, 0.3),
            t_score,
        }
    }

    #[test]
    fn the_atmosphere_curve_and_cloud_bands_follow_the_data() {
        assert_eq!(atmosphere_thickness(0.05), 0.0);
        assert!((atmosphere_thickness(0.5) - 1.5).abs() < 1.0e-4);
        assert!((atmosphere_thickness(0.95) - 1.75).abs() < 1.0e-4);
        // The solar-view cloud effects are empty in the installed data.
        assert_eq!(cloud_alpha(0.05), 0.0);
        assert_eq!(cloud_alpha(0.8), 0.0);
        assert_eq!(cloud_color(0.3), [0.95; 3]);
        assert_eq!(cloud_color(0.7), [0.625, 0.525, 0.475]);
        assert_eq!(cloud_color(0.95), [0.3, 0.1, 0.0]);
    }

    #[test]
    fn terraforming_brings_water_to_dry_worlds() {
        // A barren world in vacuum shows dry basins.
        assert_eq!(climate_water(0.3, 0.12, 0.05), 0.0);
        // Terraformed to the bullseye: at least the perfect-score water.
        assert!((climate_water(0.3, 0.5, 0.5) - MIN_WATER_WITH_PERFECT_SCORE).abs() < 1.0e-5);
        // A living world keeps its script's water; never above the cap.
        assert_eq!(climate_water(0.5, 0.5, 0.5), 0.5);
        assert!(climate_water(0.6, 0.5, 0.5) <= MAX_WATER);
        // Boiled off by heat, fading from the low threshold.
        assert_eq!(climate_water(0.5, 0.95, 0.5), 0.0);
        let warm = climate_water(0.5, 0.85, 0.5);
        assert!(warm > 0.0 && warm < 0.5, "{warm}");
    }

    #[test]
    fn the_water_range_is_the_retail_rule_of_0xfc20a0() {
        // The bullseye: the perfect-score water is the least, MaxWater the most.
        let (lo, hi) = water_range(0.5, 0.5);
        assert!((lo - MIN_WATER_WITH_PERFECT_SCORE).abs() < 1.0e-6 && (hi - MAX_WATER).abs() < 1.0e-6, "{lo} {hi}");
        // A thin cold world: nothing, and a cap that does not rise above it.
        assert_eq!(water_range(0.2, 0.05), (0.0, 0.0));
        // The atmosphere's water (A - 0.2) x 0.8 / 0.8 at a mild heat, faded by the heat.
        let (mild, _) = water_range(0.6, 0.9);
        assert!((mild - 0.7).abs() < 1.0e-6, "{mild}");
        let (hot, _) = water_range(0.85, 0.9);
        assert!((hot - 0.35).abs() < 1.0e-6, "half faded at the middle of the heat band: {hot}");
        assert_eq!(water_range(0.95, 0.9).0, 0.0);
    }

    #[test]
    fn a_very_hot_world_shows_lava_coverage() {
        // Below the lava threshold no lava; above it up to 0.4 of the surface.
        assert!(climate_water(0.0, 0.9, 0.5) < 0.05, "only the bullseye's faint water");
        assert!((climate_water(0.0, 0.975, 0.5) - 0.2).abs() < 1.0e-5);
        assert!((climate_water(0.5, 1.0, 0.5) - LAVA_MAX_COVERAGE).abs() < 1.0e-5);
    }

    #[test]
    fn the_orbit_bake_key_follows_the_sea_colour() {
        // The sea colour comes from the water table by the temperature score
        // (`SpacePlanets::sea_color`): a new column must re-bake the atlas.
        let a = look_for(false, None, None, climate(2));
        let mut b = a.clone();
        b.water = [0.63, 0.21, 0.22];
        assert_ne!(a.key(2), b.key(2));
        let mut c = b.clone();
        c.water = [0.6301, 0.2101, 0.2201];
        assert_eq!(b.key(2), c.key(2), "sub-8-bit changes keep the bake");
    }

    /// Flat installed-style ramps: dead red, living green, ice blue, lava white; band colours
    /// by row (beach rows 0 .. 3: sand tones, the 4 .. 7 cliff greys).
    fn flat_ramps() -> Ramps {
        let flat = |bgra: [u8; 4], rows: usize| -> Vec<u8> {
            bgra.iter().copied().cycle().take(rows * RAMP_SIZE * 4).collect()
        };
        let mut band_image = Vec::new();
        for row in 0..simdata::BAND_IMAGE_ROWS {
            let v = 20 * row as u8 + 10;
            band_image.extend(flat([v, v, v, 255], 1));
        }
        Ramps::from_raw(RawRamps {
            dead: flat([40, 40, 200, 255], RAMP_SIZE),
            living: flat([40, 200, 40, 250], RAMP_SIZE),
            ice: flat([200, 40, 40, 255], RAMP_SIZE),
            lava: flat([220, 220, 220, 255], RAMP_SIZE),
            bands: band_image,
        })
    }

    #[test]
    fn the_look_follows_the_retail_life_table_and_freezes_or_melts() {
        let r = flat_ramps();
        let at = |t: f32, a: f32| {
            let mut c = climate(2);
            c.now = (t, a);
            look_for(false, None, Some(&r), c)
        };
        // Temperate and moist (column 127 at full weight): the living ramp at every value.
        let living = at(0.5, 0.5);
        assert!(living.ramp[10][1] > living.ramp[10][0], "{:?}", living.ramp[10]);
        assert!(living.ramp[60][1] > living.ramp[60][0], "{:?}", living.ramp[60]);
        // (the top four table rows of the column go back to the dead ramp, 0xFB84B0)
        assert!(living.ramp[63][0] > living.ramp[63][1], "{:?}", living.ramp[63]);
        // Warm (column ~180, in the living ramp's skewed edge): the low values keep the dead
        // ramp, the high values show the living one (the skew shifts the colours up).
        let warm = at(0.75, 0.5);
        assert!(warm.ramp[10][0] > warm.ramp[10][1], "{:?}", warm.ramp[10]);
        assert!(warm.ramp[60][1] > warm.ramp[60][0], "{:?}", warm.ramp[60]);
        // The atmosphere weights the climate column in the middle band: with no air the base
        // column 192 (dead) wins.
        let dry = at(0.5, 0.0);
        assert!(dry.ramp[10][0] > dry.ramp[10][1], "{:?}", dry.ramp[10]);
        // the key follows the column, the fluid the thresholds
        assert_ne!(living.key(2), warm.key(2));
        assert_ne!(living.key(2), dry.key(2));
        assert!(living.life_column > 127.0 && warm.life_column > 170.0);
        assert_eq!(at(0.1, 0.4).fluid, Fluid::Ice);
        assert_eq!(at(0.97, 0.4).fluid, Fluid::Lava);
        // beaches and cliffs are the band image's rows of the temperature zone
        let (b, c) = (living.beach, living.cliff);
        assert!(b != c);
        // The homeworld keeps the script's water and the theme's ramp.
        let home = look_for(true, None, Some(&r), climate(3));
        assert!(home.home && home.water_level == 0.0 && home.life_column == 0.0);
    }

    #[test]
    fn the_planet_info_x_picks_the_ramp_column_of_every_ramp() {
        // The retail table is built for one column: a ramp whose columns differ shows it.
        let mut raw = flat_ramps().raw.unwrap();
        for row in 0..RAMP_SIZE {
            for col in 0..RAMP_SIZE {
                let i = (row * RAMP_SIZE + col) * 4;
                raw.dead[i + 2] = (col * 4) as u8; // red grows with the column
            }
        }
        let r = Ramps::from_raw(raw);
        let red = |x: f32| {
            let mut c = climate(0);
            c.info = [x, 1.0]; // y is not used
            c.now = (0.7, 0.0);
            look_for(false, None, Some(&r), c).ramp[10][0]
        };
        assert!(red(60.0) > red(5.0));
        let mut a = climate(0);
        a.now = (0.7, 0.0);
        a.info = [30.0, 5.0];
        let mut b = a;
        b.info = [30.0, 60.0];
        assert_eq!(look_for(false, None, Some(&r), a).ramp, look_for(false, None, Some(&r), b).ramp);
    }

    /// Needs the installed game: the real ramps and band colours build the table the retail
    /// code builds (hashes from `scripts\hashes.py`, the retail routines run on the same
    /// images under an emulator).
    #[test]
    #[ignore = "requires installed original Spore resources"]
    fn the_installed_ramps_build_the_retail_table() {
        fn fnv(bytes: &[u8]) -> u64 {
            bytes.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01B3))
        }
        let install = crate::spore_install::discover().expect("installed Spore assets");
        let index = ResourceIndex::load(&install.packages);
        let ramps = Ramps::load(&index).expect("planet colour ramps");
        let raw = ramps.raw.as_ref().expect("raw images");
        assert_eq!(raw.bands.len(), RAMP_SIZE * simdata::BAND_IMAGE_ROWS * 4);
        for (a, b, hash) in [
            (20usize, 30usize, 0x3D1F_F2A6_E284_1951u64),
            (0, 0, 0xCF65_9766_D3EF_991E),
            (63, 63, 0x2D6D_42F7_A45B_FB92),
            (17, 40, 0xC42B_BE40_2CCB_D018),
        ] {
            let table = simdata::LifeTable::build(&raw.dead, &raw.living, &raw.ice, &raw.lava, a, b);
            assert_eq!(fnv(table.bytes()), hash, "columns {a} / {b}");
        }
    }

    #[test]
    fn the_orbit_mesh_and_atlas_follow_the_height_field() {
        let planet = bumpy(0.0);
        let look = look_for(false, None, None, climate(2));
        let mesh = orbit_mesh(&planet, 3.0, look.water_level);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("positions");
        };
        assert_eq!(pos.len(), 6 * (ORBIT_CELLS + 1) * (ORBIT_CELLS + 1));
        for p in pos.iter().step_by(7) {
            let v = Vec3::from_array(*p);
            let expect =
                3.0 * (PLANET_RADIUS + planet.altitude_range * planet.h_at(v).max(look.water_level)) / PLANET_RADIUS;
            assert!((v.length() - expect).abs() < 1.0e-3);
        }
        let (rgba, glow) = orbit_atlas(&planet, &look);
        assert_eq!(rgba.len(), ORBIT_RES * 3 * ORBIT_RES * 2 * 4);
        assert!(glow.is_none());
        // Sea and land differ.
        let sea = look.surface(&planet, Vec3::new(-1.0, -0.6, 0.0).normalize()).0;
        let land = look.surface(&planet, Vec3::new(0.5, 0.8, 0.0).normalize()).0;
        assert_ne!(sea, land);
        // Atlas uvs stay inside their tile.
        let [u, v] = atlas_uv(5, 1.0, 1.0);
        assert!(u <= 1.0 && v <= 1.0 && u > 2.0 / 3.0 && v > 0.5);
    }

    #[test]
    fn the_orbit_beach_band_follows_the_beach_pass() {
        // TerrainBrushTextureBeachShader 0x7000002D: full beach colour up to
        // (mBeachLevel + mWaterLevel) x altitude range, gone one unit above it.
        let planet = bumpy(0.0);
        let look = look_for(false, None, None, climate(2));
        let cos_min = crate::planet_query::MIN_CLIFF_GRADIENT_DEG.to_radians().cos();
        let (mut beach, mut inland) = (0, 0);
        for face in 0..6 {
            for row in 0..48 {
                for col in 0..48 {
                    let dir = face_point(face, col as f32 / 47.0, row as f32 / 47.0).normalize();
                    let above = (planet.h_at(dir) - look.water_level) * planet.altitude_range;
                    let flat = planet.ground_normal(dir).dot(dir) > cos_min + 0.02;
                    let c = look.surface(&planet, dir).0;
                    if above > 0.0 && above < 2.4 && flat {
                        assert_eq!(c, look.beach, "{above}");
                        beach += 1;
                    } else if above > 3.6 && flat {
                        assert_ne!(c, look.beach, "{above}");
                        inland += 1;
                    }
                }
            }
        }
        assert!(beach > 0 && inland > 0, "{beach} {inland}");
    }

    #[test]
    fn colony_sites_are_dry_flat_and_apart() {
        let planet = bumpy(0.0);
        let sites = colony_sites(&planet, 0.0, 7, 3);
        assert_eq!(sites.len(), 3);
        for (i, a) in sites.iter().enumerate() {
            assert!(!planet.is_in_water(*a * PLANET_RADIUS));
            for b in &sites[i + 1..] {
                assert!(a.angle_between(*b) * PLANET_RADIUS >= crate::civ_rules::CITY_FLATTEN_SIZE * 2.4 - 1.0);
            }
        }
        assert_eq!(sites, colony_sites(&planet, 0.0, 7, 3), "deterministic");
    }

    #[test]
    fn colony_sites_keep_where_the_colony_tool_landed() {
        let planet = bumpy(0.0);
        let spacing = crate::civ_rules::CITY_FLATTEN_SIZE * 2.4;
        let landed = Vec3::new(0.3, 0.5, -0.8).normalize();
        // The second city was placed by the tool; the others are drawn around it.
        let sites = colony_sites_with(&planet, 0.0, 7, 3, &[None, Some(landed)]);
        assert_eq!(sites.len(), 3);
        assert_eq!(sites[1], landed, "the placed site is kept as it is");
        for (i, a) in sites.iter().enumerate() {
            for b in &sites[i + 1..] {
                assert!(a.angle_between(*b) * PLANET_RADIUS >= spacing - 1.0, "{a:?} {b:?}");
            }
        }
        // Without placements the draw is the old one.
        assert_eq!(colony_sites_with(&planet, 0.0, 7, 3, &[]), colony_sites(&planet, 0.0, 7, 3));
        // Every city placed: no draw at all.
        let all = [Some(Vec3::X), Some(Vec3::Y), Some(Vec3::Z)];
        assert_eq!(colony_sites_with(&planet, 0.0, 7, 3, &all), vec![Vec3::X, Vec3::Y, Vec3::Z]);
    }

    #[test]
    fn the_colony_tool_lands_at_the_planet_views_aim_point() {
        let planet = Arc::new(bumpy(-1.0));
        let unit = 2.0;
        let centre = Vec3::new(10.0, -20.0, 5.0);
        let rotation = Quat::from_rotation_y(0.7);
        let ground = PlanetViewGround { planet: planet.clone(), centre, rotation, unit };
        let radius = PLANET_RADIUS * unit;
        let mut sv = crate::space_view::SpaceView::default();
        // The ship above the pole of the planet frame (world +y rotated).
        let ship = centre + rotation * Vec3::Y * (radius + 50.0);
        sv.enter_planet(3, centre, radius, ship, -Vec3::Z);
        // A cursor ray that hits the world a little off the ship's nadir.
        let local_target = Vec3::new(0.15, 1.0, 0.0).normalize();
        let target = centre + rotation * local_target * radius;
        let origin = target + (target - centre).normalize() * 300.0;
        let ray = Ray3d::new(origin, Dir3::new(centre - origin).unwrap());
        let dir = colony_landing(Some(&sv), Some(&ground), Some(ray), 3).expect("land under the aim point");
        assert!(dir.angle_between(local_target) < 0.02, "{dir:?} vs {local_target:?}");
        // Another world, or no planet view: no landing (the seeded site is used).
        assert!(colony_landing(Some(&sv), Some(&ground), Some(ray), 4).is_none());
        assert!(colony_landing(None, Some(&ground), Some(ray), 3).is_none());
        // Under the sea the colony is not placed (a dry world is needed).
        let sea = Arc::new(bumpy(1.0));
        let wet = PlanetViewGround { planet: sea, centre, rotation, unit };
        assert!(colony_landing(Some(&sv), Some(&wet), Some(ray), 3).is_none());
    }

    #[test]
    fn the_city_count_follows_the_colonies() {
        let mut campaign = crate::space::SpaceCampaign::default();
        assert_eq!(space_city_count(&campaign, 4), 0);
        let index = campaign.ensure_ecology(4);
        campaign.ecology[index].colony_count = 2;
        campaign.ensure_city_layouts(4);
        assert_eq!(space_city_count(&campaign, 4), 2);
        campaign.city_layouts.get_mut(&4).unwrap().push(crate::space::CityLayout::new());
        assert_eq!(space_city_count(&campaign, 4), 3, "a planned layout shows too");
    }

    #[test]
    #[ignore = "requires installed original Spore resources"]
    fn installed_pool_scripts_generate_distinct_worlds() {
        let install = crate::spore_install::discover().expect("installed Spore assets");
        let index = ResourceIndex::load(&install.packages);
        let ramps = Ramps::load(&index).expect("planet colour ramps");
        assert_eq!(ramps.dead.len(), 64 * 64);
        let mut library = None;
        let mut seen = Vec::new();
        for pool in crate::space_worldgen_data::POOLS {
            let e = pool[pool.len() / 2];
            let started = std::time::Instant::now();
            let g = generate(&index, &mut library, TerrainSource::Script(e.0, e.1), ORBIT_RES)
                .unwrap_or_else(|err| panic!("{:08x}!{:08x}: {err}", e.0, e.1));
            let land = crate::planet_gen::land_fraction(&g.planet);
            println!("{} land {land:.2} info {:?} in {:?}", g.name, g.info, started.elapsed());
            assert!(g.planet.cube.res() == ORBIT_RES);
            seen.push(g.planet.cube.raw().iter().map(|&v| v as u64).sum::<u64>());
        }
        seen.dedup();
        assert_eq!(seen.len(), 4, "four different worlds");
        // Census of the 354 pool scripts at 32^2 (a few seconds): every one
        // generates; how many come out under / above their water line or
        // pressed flat against the cube's range (the planet engine's brush
        // blend is a stand-in, planet-re 4F-1).
        let (mut wet, mut dry, mut flat, mut total) = (0, 0, 0, 0);
        for pool in crate::space_worldgen_data::POOLS {
            for e in pool.iter() {
                let g = generate(&index, &mut library, TerrainSource::Script(e.0, e.1), 32)
                    .unwrap_or_else(|err| panic!("{:08x}!{:08x}: {err}", e.0, e.1));
                let land = crate::planet_gen::land_fraction(&g.planet);
                let (lo, hi) = g.planet.ranges.global();
                total += 1;
                wet += usize::from(land < 0.01);
                dry += usize::from(land > 0.99);
                flat += usize::from(hi - lo < 0.05);
            }
        }
        println!("pool census: {total} scripts, {wet} all under water, {dry} all land, {flat} flat (< 0.05 h)");
        assert_eq!(total, 354);
        let home = generate(&index, &mut library, TerrainSource::Home(7), ORBIT_RES).expect("homeworld");
        assert_eq!(home.planet.cube.res(), ORBIT_RES);
    }
}
