//! Land surfaces of the Creature, Tribal and Civilization stages. With the
//! installed game the stages play on the original's spherical planet
//! (`planet_data` / `planet_gen` / `planet_query` / `planet_render`): the
//! homeworld's terrain script is regenerated into the 6 x 512^2 height cube,
//! the planet centre sits at (0, -R, 0) with the stage origin at its +Y pole,
//! and `LandSurface` is a thin flat-world facade over it. Without installed
//! data (headless tests) the deterministic prototype island below remains:
//! new terrain generation, not a decoder for Spore's planet shaders.
use crate::{
    game::{Player, RunProgress},
    stages::{GameStage, StageOwned},
    world_presentation::tune,
};
use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat},
    shader::ShaderRef,
};

const CELLS: usize = 128;
const BASE_Y: f32 = -0.6;
const SEA_Y: f32 = -0.9;
/// Depth below BASE_Y of the generated sea floor beyond the coastline.
const SHELF_DEPTH: f32 = 1.6;
/// Preview size of one native planet cube face in world units. Native detail
/// tiling constants are authored as repeats per face; this maps them onto the
/// flat preview island at a creature-stage ground scale.
const PREVIEW_FACE_WIDTH: f32 = 400.;
/// Terrain elevation (above the sea) covered by the planet color ramp.
const RAMP_ELEVATION_RANGE: f32 = 5.0;

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainLayers>;
/// Adapted layered terrain. Inputs are installed rasters and palette values;
/// their composition follows the decompiled native TerrainAbove pixel program
/// (ramp color times two-scale packed detail) plus the native cliff/beach
/// brush colors, evaluated per pixel instead of through a synthesized map.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TerrainLayers {
    /// x: coastline phase, y: cos(cliff start angle), z: detail present,
    /// w: coastline extent (0 disables the island coast mask).
    #[uniform(100)]
    pub parameters: Vec4,
    #[uniform(105)]
    pub beach_color: Vec4,
    #[uniform(106)]
    pub cliff_color: Vec4,
    /// Packed surface layers: R cliff_base, G beach_generic, B biome noise.
    #[texture(101)]
    #[sampler(102)]
    pub cliff_texture: Handle<Image>,
    /// The installed planet color ramp as a 256x1 sRGB lookup.
    #[texture(103)]
    #[sampler(104)]
    pub ramp_texture: Handle<Image>,
    /// Packed detail: RG AboveDetail2 (G, B), BA AboveDetail1 (G, B).
    #[texture(107)]
    #[sampler(108)]
    pub detail_texture: Handle<Image>,
    /// World-space frequency: x high detail, y low detail, z cliff, w near.
    #[uniform(109)]
    pub tiling: Vec4,
    /// x: cos(full cliff angle), y: ramp elevation range, z: ramp noise
    /// (munge) strength, w: detail contrast.
    #[uniform(110)]
    pub shaping: Vec4,
    /// x: bump strength, y: near-detail strength, z: sand mix, w: ramp present.
    #[uniform(111)]
    pub surface: Vec4,
    /// x: dirt tint in dark detail, y: ramp desaturation, z: albedo scale,
    /// w: cliff texture contrast.
    #[uniform(112)]
    pub grading: Vec4,
    /// Planet mode: xyz planet centre (world), w water radius; zero for the
    /// flat island.
    #[uniform(113)]
    pub planet: Vec4,
    /// x: beach / wet-sand band scale (1 on the island).
    #[uniform(114)]
    pub planet_bands: Vec4,
    /// Retail TerrainState (`planet_sky::SkyState`; slot 15 x = 1 enables
    /// the in-scatter fog and the sea floor on the stage planets).
    #[uniform(115)]
    pub sky: crate::planet_sky::SkyState,
    #[texture(116, dimension = "cube")]
    #[sampler(117)]
    pub scatter: Handle<Image>,
    /// terrainThemeBelow (seabed raster).
    #[texture(118)]
    #[sampler(119)]
    pub below_texture: Handle<Image>,
}
impl MaterialExtension for TerrainLayers {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/terrain.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
}
pub struct TerrainPlugin;
impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/terrain.wesl");
        crate::world_presentation::dev_shader_override(app, "terrain.wesl");
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
        crate::planet_render::build(app);
        // The Creature Creator's handle overdraw pass (`creature_handles::OverdrawMaterial`).
        crate::creature_handles::build(app);
    }
}

#[derive(Component)]
pub struct WorldGround;
#[derive(Component)]
pub struct OriginalTerrainTexture {
    pub definition: crate::dbpf::ResourceKey,
    pub texture: crate::dbpf::ResourceKey,
    pub palette: crate::dbpf::ResourceKey,
    pub cliff: Option<crate::dbpf::ResourceKey>,
    pub beach: Option<crate::dbpf::ResourceKey>,
}
/// Procedural distant relief is a preview composition, not saved or native
/// planet geometry. The palette and optional surface detail keep its materials
/// tied to installed resources without implying an original terrain mesh.
#[derive(Component)]
pub struct AdaptedDistantLandform {
    pub seed: u64,
    pub palette: crate::dbpf::ResourceKey,
    pub detail_texture: Option<crate::dbpf::ResourceKey>,
}
#[derive(Component)]
pub struct Grounded {
    clearance: f32,
}

impl Grounded {
    pub fn new(clearance: f32) -> Self {
        Self { clearance }
    }
}

/// Grounded along the planet's local up (the Civilization stage's 3D
/// placement, planet-re 4C): on the planet `ground` puts the entity on
/// `ToSurface` of its own radial line (the water surface over the sea) plus
/// its clearance along the local up, wherever it is on the sphere, and keeps
/// its world position otherwise (a translation off the surface is read as its
/// clearance). Without a planet it is grounded like any other entity.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Radial;

#[derive(Resource, Clone)]
pub struct LandSurface {
    pub stage: GameStage,
    pub seed: u64,
    extent: f32,
    heights: Vec<f32>,
    /// The stage planet; `None` for the prototype island.
    planet: Option<std::sync::Arc<crate::planet_query::StagePlanet>>,
}

/// The homeworld generated for a save seed, kept across stage changes (the
/// homeworld keeps its terrain script through the stages, planet-re 2A).
#[derive(Resource, Clone)]
pub struct PlanetCache {
    pub seed: u64,
    pub script: crate::planet_data::PlanetScript,
    pub base: std::sync::Arc<crate::planet_query::Planet>,
    /// Planet-local direction of the stage origin.
    pub origin: Vec3,
    pub stats: crate::planet_gen::GenStats,
}

/// Interpret installed palette channels through the preview's sRGB color
/// path, consistent with its ordinary preview constants. Native palette
/// transfer behavior remains unverified.
fn preview_srgb_palette_color(color: [f32; 3]) -> Color {
    Color::srgb(color[0], color[1], color[2])
}

/// The seed-warped radial island edge shared by the CPU surface, the terrain
/// and water shaders and the world map. Positive inside the coastline.
pub fn coast_depth(p: Vec2, extent: f32, phase: f32) -> f32 {
    let radial_noise = (p.x * 0.31 + phase).sin() * 1.05
        + (p.y * 0.27 - phase * 0.7).cos() * 0.8
        + ((p.x + p.y) * 0.16 + phase * 1.4).sin() * 0.65;
    extent * 0.86 + radial_noise - p.length()
}

/// Shelf profile below BASE_Y: zero inland, sinking under the cosmetic sea a
/// little outside the analytic coastline (the waterline sits near -0.3).
fn shore_drop(coast_depth: f32) -> f32 {
    SHELF_DEPTH * smooth((0.9 - coast_depth) / 4.5)
}

fn lattice(seed: u32, x: i32, z: i32) -> f32 {
    let mut h = seed ^ (x as u32).wrapping_mul(0x8DA6_B343) ^ (z as u32).wrapping_mul(0xD816_3841);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h as f32 / u32::MAX as f32
}

/// Smooth value noise in [0, 1].
fn value_noise(seed: u32, p: Vec2) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec2::splat(3.) - 2. * f);
    let (x, z) = (i.x as i32, i.y as i32);
    let a = lattice(seed, x, z);
    let b = lattice(seed, x + 1, z);
    let c = lattice(seed, x, z + 1);
    let d = lattice(seed, x + 1, z + 1);
    let near = a + (b - a) * u.x;
    let far = c + (d - c) * u.x;
    near + (far - near) * u.y
}

/// Fractal value noise in roughly [-1, 1].
fn fbm(seed: u32, p: Vec2, octaves: u32) -> f32 {
    let (mut sum, mut amplitude, mut frequency, mut norm) = (0., 0.5, 1., 0.);
    for octave in 0..octaves {
        sum += amplitude * (value_noise(seed.wrapping_add(octave * 101), p * frequency) * 2. - 1.);
        norm += amplitude;
        amplitude *= 0.5;
        frequency *= 2.03;
    }
    sum / norm
}

impl LandSurface {
    pub fn generate(stage: GameStage, seed: u64, pads: &[(Vec2, f32)]) -> Self {
        let extent = match stage {
            GameStage::Civilization => 48.,
            GameStage::Tribal => 45.,
            _ => 42.,
        };
        let phase = (seed ^ (seed >> 32)) as u32 as f32 / u32::MAX as f32 * 6.28;
        let noise_seed = (seed ^ (seed >> 29)) as u32;
        let mut heights = Vec::with_capacity((CELLS + 1).pow(2));
        for z in 0..=CELLS {
            for x in 0..=CELLS {
                let p = Vec2::new(x as f32, z as f32) / CELLS as f32 * extent * 2.
                    - Vec2::splat(extent);
                let rolling = ((p.x * 0.16 + phase).sin() * (p.y * 0.13 - phase).cos() + 1.) * 0.95
                    + (p.x * 0.37 + p.y * 0.24 + phase).sin() * 0.18;
                // Domain-warped fractal relief adds broad uplands and a few
                // steep, rocky ridges so the native cliff layer has slopes to
                // dress (Spore's kMinCliffGradient starts at 32 degrees).
                let warp = Vec2::new(
                    fbm(noise_seed ^ 0x51, p * 0.05, 2),
                    fbm(noise_seed ^ 0xA3, p * 0.05, 2),
                ) * 6.;
                let uplands = (fbm(noise_seed ^ 0x17, (p + warp) * 0.045, 4) * 0.5 + 0.5)
                    .clamp(0., 1.)
                    .powf(1.6)
                    * tune("UPLAND", 2.2);
                let ridge_line = 1. - fbm(noise_seed ^ 0x2B, (p + warp) * 0.034, 3).abs();
                let ridge_mask = smooth((value_noise(noise_seed ^ 0x3C, p * 0.028) - 0.42) / 0.25);
                let ridges = smooth((ridge_line - 0.62) / 0.3) * ridge_mask * tune("RIDGE", 2.6);
                // Terrace part of the uplands into flat benches separated by
                // short steep risers, the cliff bands typical of Spore land.
                let raised = uplands + ridges;
                let step = tune("TERRACE_STEP", 1.1);
                let level = raised / step;
                let riser = smooth((level.fract() - 0.72) / 0.28);
                let terraced = (level.floor() + riser) * step;
                let terrace_mask = smooth((value_noise(noise_seed ^ 0x4D, p * 0.04) - 0.35) / 0.3)
                    * tune("TERRACE", 1.0);
                let relief = rolling.max(0.) * 0.8 + raised + (terraced - raised) * terrace_mask;
                // A seed-warped, radial island edge creates coves and points;
                // past the coast the shelf sinks below the cosmetic sea.
                let coast = coast_depth(p, extent, phase);
                let edge = smooth(coast / tune("EDGE", 8.0));
                let pad = pads
                    .iter()
                    .map(|(c, r)| smooth((p.distance(*c) - r) / tune("PAD_FADE", 7.)))
                    .fold(1.0, f32::min);
                heights.push(BASE_Y + relief * edge * pad - shore_drop(coast));
            }
        }
        Self {
            stage,
            seed,
            extent,
            heights,
            planet: None,
        }
    }

    /// The facade over a stage planet: `height(x, z)` is the world y of the
    /// planet's `ToSurface` surface on the vertical line through (x, z).
    pub fn on_planet(
        stage: GameStage,
        seed: u64,
        planet: std::sync::Arc<crate::planet_query::StagePlanet>,
    ) -> Self {
        let extent = match stage {
            GameStage::Civilization => 48.,
            GameStage::Tribal => 45.,
            _ => 42.,
        };
        Self {
            stage,
            seed,
            extent,
            heights: Vec::new(),
            planet: Some(planet),
        }
    }

    /// The stage planet, when the stage plays on the original sphere.
    pub fn planet(&self) -> Option<&crate::planet_query::StagePlanet> {
        self.planet.as_deref()
    }

    /// The shared handle of the stage planet (stage code keeps it).
    pub fn planet_arc(&self) -> Option<std::sync::Arc<crate::planet_query::StagePlanet>> {
        self.planet.clone()
    }

    pub fn extent(&self) -> f32 {
        self.extent
    }

    /// Ground height under (x, z): the sea floor under water on the planet
    /// (`height` stands on the water surface there, as retail ToSurface).
    pub fn ground_height(&self, x: f32, z: f32) -> f32 {
        match &self.planet {
            Some(p) if x.is_finite() && z.is_finite() => p.ground_y(x, z),
            _ => self.height(x, z),
        }
    }

    /// Whether (x, z) lies over the sea (`IsInWater`); never on the island.
    pub fn is_water(&self, x: f32, z: f32) -> bool {
        self.planet.as_ref().is_some_and(|p| {
            x.is_finite() && z.is_finite() && p.ground_y(x, z) < p.surface_y(x, z) - 1e-4
        })
    }

    /// Sample the exact two triangles used by mesh(), including edges.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        if !x.is_finite() || !z.is_finite() {
            return BASE_Y;
        }
        if let Some(planet) = &self.planet {
            return planet.surface_y(x, z);
        }
        let p = ((Vec2::new(x, z) + Vec2::splat(self.extent)) / (2. * self.extent) * CELLS as f32)
            .clamp(Vec2::ZERO, Vec2::splat(CELLS as f32));
        let ix = (p.x.floor() as usize).min(CELLS - 1);
        let iz = (p.y.floor() as usize).min(CELLS - 1);
        let (u, v) = (p.x - ix as f32, p.y - iz as f32);
        let a = self.heights[iz * (CELLS + 1) + ix];
        let b = self.heights[iz * (CELLS + 1) + ix + 1];
        let c = self.heights[(iz + 1) * (CELLS + 1) + ix];
        let d = self.heights[(iz + 1) * (CELLS + 1) + ix + 1];
        if u + v <= 1. {
            a + (b - a) * u + (c - a) * v
        } else {
            d + (c - d) * (1. - u) + (b - d) * (1. - v)
        }
    }

    pub fn mesh(&self) -> Mesh {
        self.mesh_with_palette(None)
    }

    /// A preview-only vertex ramp from the installed theme. Slope and beach
    /// masks are derived from geometry by terrain.wesl so the authored colors
    /// remain independent of the procedural height field.
    pub fn mesh_with_palette(&self, palette: Option<&crate::world_catalog::PlanetPalette>) -> Mesh {
        if self.heights.is_empty() {
            // The planet is drawn by planet_render, not as one island mesh.
            return Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        }
        let mut positions = Vec::with_capacity(self.heights.len());
        let mut uvs = Vec::with_capacity(self.heights.len());
        let mut colors = Vec::with_capacity(self.heights.len());
        for z in 0..=CELLS {
            for x in 0..=CELLS {
                let px = x as f32 / CELLS as f32 * self.extent * 2. - self.extent;
                let pz = z as f32 / CELLS as f32 * self.extent * 2. - self.extent;
                let h = self.heights[z * (CELLS + 1) + x];
                positions.push([px, h, pz]);
                uvs.push([x as f32 / CELLS as f32, z as f32 / CELLS as f32]);
                let rise = ((h - SEA_Y) / RAMP_ELEVATION_RANGE).clamp(0., 1.);
                let grass = match self.stage {
                    GameStage::Tribal => [0.30, 0.36, 0.12],
                    GameStage::Civilization => [0.23, 0.34, 0.16],
                    _ => [0.15, 0.34, 0.10],
                };
                let fallback = [
                    grass[0] + rise * 0.10,
                    grass[1] + rise * 0.06,
                    grass[2] + rise * 0.03,
                ];
                // Both procedural fallback constants and installed palette
                // values use the preview's sRGB color mapping.
                let color = palette.map_or_else(
                    || Color::srgb(fallback[0], fallback[1], fallback[2]),
                    |p| preview_srgb_palette_color(p.sample(rise)),
                );
                colors.push(color.to_linear().to_f32_array());
            }
        }
        let mut indices = Vec::with_capacity(CELLS * CELLS * 6);
        for z in 0..CELLS {
            for x in 0..CELLS {
                let a = (z * (CELLS + 1) + x) as u32;
                let c = a + (CELLS + 1) as u32;
                indices.extend([a, c, a + 1, a + 1, c, c + 1]);
            }
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(Indices::U32(indices));
        mesh.compute_smooth_normals();
        mesh
    }
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn deterministic_unit(seed: u64, index: u32, salt: u32) -> f32 {
    let mut value = (seed as u32)
        .wrapping_add((seed >> 32) as u32)
        .wrapping_add(index.wrapping_mul(0x9E37_79B9))
        .wrapping_add(salt.wrapping_mul(0x85EB_CA6B));
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846C_A68B);
    value ^= value >> 16;
    value as f32 / u32::MAX as f32
}

/// A low-poly but smooth, isolated horizon island. Its shore ring sits just
/// above the cosmetic sea and its interior rises gradually into broad hills.
fn distant_landform_mesh(
    seed: u64,
    index: u32,
    radius: f32,
    peak: f32,
    palette: &crate::world_catalog::PlanetPalette,
) -> Mesh {
    const SEGMENTS: usize = 40;
    const RINGS: usize = 12;
    let phase = deterministic_unit(seed, index, 1) * std::f32::consts::TAU;
    let uv_offset = Vec2::new(
        deterministic_unit(seed, index, 2) * 32.,
        deterministic_unit(seed, index, 3) * 32.,
    );
    let mut positions = Vec::with_capacity(1 + SEGMENTS * RINGS);
    let mut uvs = Vec::with_capacity(1 + SEGMENTS * RINGS);
    let mut colors = Vec::with_capacity(1 + SEGMENTS * RINGS);
    let color_at = |height: f32| {
        let color = palette.sample(((height - SEA_Y) / peak).clamp(0., 1.));
        preview_srgb_palette_color(color).to_linear().to_f32_array()
    };
    // Leave only a tiny positive offset above the sea plane: enough to avoid
    // depth fighting at the waterline, while preventing an obvious air gap.
    let shoreline = SEA_Y + 0.008;
    let center_height = shoreline + peak * 0.82;
    positions.push([0., center_height, 0.]);
    uvs.push(uv_offset.to_array());
    colors.push(color_at(center_height));

    for ring in 1..=RINGS {
        let radial = ring as f32 / RINGS as f32;
        let profile = (1. - smooth(radial)).powf(1.2);
        for segment in 0..SEGMENTS {
            let angle = segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let edge =
                1. + 0.055 * (angle * 3. + phase).sin() + 0.035 * (angle * 5. - phase * 0.7).cos();
            let local_radius = radius * radial * edge;
            let ridge = (0.72
                + 0.17 * (angle * 2. + phase).cos()
                + 0.08 * (angle * 5. - phase).sin()
                + 0.035 * (radial * 18. + angle * 3. + phase).sin())
            .clamp(0.38, 1.08);
            let height = shoreline + peak * profile * ridge;
            let x = local_radius * angle.cos();
            let z = local_radius * angle.sin();
            positions.push([x, height, z]);
            uvs.push((Vec2::new(x, z) * 0.045 + uv_offset).to_array());
            colors.push(color_at(height));
        }
    }

    let mut indices = Vec::with_capacity(SEGMENTS * (1 + (RINGS - 1) * 2) * 3);
    for segment in 0..SEGMENTS {
        let a = 1 + segment;
        let b = 1 + (segment + 1) % SEGMENTS;
        // Winding faces upward (Y+).
        indices.extend([0_u32, b as u32, a as u32]);
    }
    for ring in 1..RINGS {
        let inner = 1 + (ring - 1) * SEGMENTS;
        let outer = 1 + ring * SEGMENTS;
        for segment in 0..SEGMENTS {
            let next = (segment + 1) % SEGMENTS;
            let a = inner + segment;
            let b = inner + next;
            let c = outer + segment;
            let d = outer + next;
            indices.extend([a as u32, b as u32, c as u32, b as u32, d as u32, c as u32]);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    mesh
}

/// Decode an installed terrain raster with all of its authored mips.
/// Luminance rasters (D3DFMT_L8, used by cliff_base and beach_generic) are
/// expanded to grey RGBA; the generic RW4 decoder only handles color formats.
fn decode_terrain_mips(bytes: &[u8]) -> Option<Vec<crate::texture::Pixels>> {
    if let Some(mips) = crate::texture::decode_rw4_mips(bytes) {
        return Some(mips);
    }
    let texture = crate::native_texture::decode(bytes, None).ok()?;
    if texture.format != 50 || texture.faces.len() != 1 {
        return None;
    }
    texture.faces[0]
        .iter()
        .enumerate()
        .map(|(level, mip)| {
            let width = (texture.width >> level).max(1);
            let height = (texture.height >> level).max(1);
            let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
            for row in 0..height as usize {
                let start = row * mip.row_pitch;
                for &l in mip.bytes.get(start..start + width as usize)? {
                    rgba.extend([l, l, l, 255]);
                }
            }
            Some(crate::texture::Pixels {
                width,
                height,
                rgba,
            })
        })
        .collect()
}

fn read_terrain_raster(
    world: &World,
    key: Option<crate::dbpf::ResourceKey>,
) -> Option<Vec<crate::texture::Pixels>> {
    let key = key?;
    let index = &world.get_resource::<crate::Originals>()?.resource_index;
    let bytes = if key.type_id != 0 {
        index.read(key).ok()?
    } else {
        let matches = [crate::asset_catalog::RW4, crate::decode::TYPE_GMDL]
            .into_iter()
            .map(|type_id| crate::dbpf::ResourceKey { type_id, ..key })
            .filter(|candidate| index.records.contains_key(candidate))
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return None;
        }
        index.read(matches[0]).ok()?
    };
    decode_terrain_mips(&bytes)
}

/// The native noise modifier is an 8-bit world image (biome_noise_modifier
/// .8bitImage); a same-instance RW4 raster also exists but is not the input
/// named by cTerrainStateMgr::TerrainTextures.
fn read_terrain_noise(
    world: &World,
    key: Option<crate::dbpf::ResourceKey>,
) -> Option<crate::texture::Pixels> {
    let key = crate::dbpf::ResourceKey {
        type_id: 0x03E421EC,
        ..key?
    };
    let index = &world.get_resource::<crate::Originals>()?.resource_index;
    let bitmap = crate::bitmap::parse(key.type_id, &index.read(key).ok()?).ok()?;
    Some(crate::texture::Pixels {
        width: bitmap.width,
        height: bitmap.height,
        rgba: bitmap.rgba8(),
    })
}

/// Wrapping bilinear resample, used only to give packed layers one size.
fn resample(source: &crate::texture::Pixels, size: u32) -> crate::texture::Pixels {
    if source.width == size && source.height == size {
        return source.clone();
    }
    let (w, h) = (source.width as usize, source.height as usize);
    let texel = |x: usize, y: usize, c: usize| source.rgba[((y % h) * w + x % w) * 4 + c] as f32;
    let mut rgba = Vec::with_capacity(size as usize * size as usize * 4);
    for y in 0..size as usize {
        for x in 0..size as usize {
            let u = (x as f32 + 0.5) / size as f32 * w as f32 - 0.5 + w as f32;
            let v = (y as f32 + 0.5) / size as f32 * h as f32 - 0.5 + h as f32;
            let (x0, y0) = (u.floor() as usize, v.floor() as usize);
            let (fx, fy) = (u.fract(), v.fract());
            for c in 0..4 {
                let top = texel(x0, y0, c) * (1. - fx) + texel(x0 + 1, y0, c) * fx;
                let bottom = texel(x0, y0 + 1, c) * (1. - fx) + texel(x0 + 1, y0 + 1, c) * fx;
                rgba.push((top * (1. - fy) + bottom * fy).round() as u8);
            }
        }
    }
    crate::texture::Pixels {
        width: size,
        height: size,
        rgba,
    }
}

/// Box-filtered mip chain for packed layers whose sources have no common
/// authored chain.
fn box_mips(base: crate::texture::Pixels) -> Vec<crate::texture::Pixels> {
    let mut mips = vec![base];
    while mips.last().is_some_and(|m| m.width > 1 || m.height > 1) {
        let last = mips.last().unwrap();
        let (w, h) = ((last.width / 2).max(1), (last.height / 2).max(1));
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h as usize {
            for x in 0..w as usize {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(last.width as usize - 1);
                        let sy = (y * 2 + dy).min(last.height as usize - 1);
                        sum += last.rgba[(sy * last.width as usize + sx) * 4 + c] as u32;
                    }
                    rgba.push((sum / 4) as u8);
                }
            }
        }
        mips.push(crate::texture::Pixels {
            width: w,
            height: h,
            rgba,
        });
    }
    mips
}

fn layer_image(
    mips: Vec<crate::texture::Pixels>,
    format: TextureFormat,
    address_mode: ImageAddressMode,
) -> Option<Image> {
    let base = mips.first()?;
    let mut image = Image::new(
        Extent3d {
            width: base.width,
            height: base.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        base.rgba.clone(),
        format,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address_mode,
        address_mode_v: address_mode,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: if mips.len() > 1 { 8 } else { 1 },
        ..default()
    });
    image.texture_descriptor.mip_level_count = mips.len() as u32;
    image.data = Some(mips.into_iter().flat_map(|m| m.rgba).collect());
    Some(image)
}

/// Pack the two native above-ground detail rasters into one linear texture,
/// level by level. Both are data (packed greyscale channels), not color.
/// AboveDetail2's red channel is a constant 0.518 that the native program
/// pairs with the low-tiling sample; the shader restores that constant.
fn pack_detail(
    detail: &[crate::texture::Pixels],
    near: Option<&[crate::texture::Pixels]>,
) -> Vec<crate::texture::Pixels> {
    detail
        .iter()
        .enumerate()
        .map(|(level, mip)| {
            let near = near
                .and_then(|n| n.get(level))
                .filter(|n| n.width == mip.width && n.height == mip.height);
            let rgba = mip
                .rgba
                .chunks_exact(4)
                .enumerate()
                .flat_map(|(i, p)| {
                    let (g, b) =
                        near.map_or((128, 128), |n| (n.rgba[i * 4 + 1], n.rgba[i * 4 + 2]));
                    [p[1], p[2], g, b]
                })
                .collect();
            crate::texture::Pixels {
                width: mip.width,
                height: mip.height,
                rgba,
            }
        })
        .collect()
}

/// Pack cliff, beach and noise greyscale layers into one 256x256 texture.
fn pack_surface_layers(
    cliff: Option<&crate::texture::Pixels>,
    beach: Option<&crate::texture::Pixels>,
    noise: Option<&crate::texture::Pixels>,
) -> Vec<crate::texture::Pixels> {
    const SIZE: u32 = 256;
    let flat = crate::texture::Pixels {
        width: 1,
        height: 1,
        rgba: vec![160, 160, 160, 255],
    };
    let [cliff, beach, noise] =
        [cliff, beach, noise].map(|layer| resample(layer.unwrap_or(&flat), SIZE));
    let rgba = (0..(SIZE * SIZE) as usize)
        .flat_map(|i| [cliff.rgba[i * 4], beach.rgba[i * 4], noise.rgba[i * 4], 255])
        .collect();
    box_mips(crate::texture::Pixels {
        width: SIZE,
        height: SIZE,
        rgba,
    })
}

/// The installed elevation ramp as a lookup texture, sampled with the same
/// preview interpolation as the vertex colors.
fn ramp_pixels(palette: &crate::world_catalog::PlanetPalette) -> crate::texture::Pixels {
    let rgba = (0..256)
        .flat_map(|i| {
            let c = palette.sample(i as f32 / 255.);
            [
                (c[0].clamp(0., 1.) * 255.).round() as u8,
                (c[1].clamp(0., 1.) * 255.).round() as u8,
                (c[2].clamp(0., 1.) * 255.).round() as u8,
                255,
            ]
        })
        .collect();
    crate::texture::Pixels {
        width: 256,
        height: 1,
        rgba,
    }
}

/// beach_generic, the native TerrainTextures default beach raster. The home
/// theme does not override terrainThemeBeach.
const BEACH_GENERIC: crate::dbpf::ResourceKey = crate::dbpf::ResourceKey {
    group_id: 0,
    instance_id: 0x9D64F524,
    type_id: crate::asset_catalog::RW4,
};

/// Shared layered-terrain inputs for the playable island and horizon islands.
#[derive(Clone)]
struct TerrainInputs {
    detail_key: Option<crate::dbpf::ResourceKey>,
    detail: Handle<Image>,
    layers: Handle<Image>,
    ramp: Handle<Image>,
}

fn load_terrain_inputs(
    world: &mut World,
    theme: &crate::world_presentation::LandTheme,
) -> TerrainInputs {
    let detail_key = theme.textures.above_detail.map(|mut key| {
        if key.type_id == 0 {
            key.type_id = crate::asset_catalog::RW4
        }
        key
    });
    let detail_mips = read_terrain_raster(world, detail_key);
    let near_mips = read_terrain_raster(world, theme.textures.above_detail_near);
    let cliff = read_terrain_raster(world, theme.textures.cliff);
    let beach = read_terrain_raster(world, theme.textures.beach.or(Some(BEACH_GENERIC)));
    let noise = read_terrain_noise(world, theme.textures.above_detail_noise);
    if detail_mips.is_none()
        && world
            .get_resource::<crate::Originals>()
            .is_some_and(|o| !o.resource_index.records.is_empty())
    {
        warn!("original terrain detail: missing or undecodable above-ground detail");
    }
    let detail = detail_mips
        .as_deref()
        .map(|mips| pack_detail(mips, near_mips.as_deref()))
        .unwrap_or_else(|| {
            vec![crate::texture::Pixels {
                width: 1,
                height: 1,
                rgba: vec![132, 132, 128, 128],
            }]
        });
    let layers = pack_surface_layers(
        cliff.as_ref().and_then(|m| m.first()),
        beach.as_ref().and_then(|m| m.first()),
        noise.as_ref(),
    );
    let mut images = world.resource_mut::<Assets<Image>>();
    TerrainInputs {
        detail_key: detail_mips.as_ref().and(detail_key),
        detail: images
            .add(layer_image(detail, TextureFormat::Rgba8Unorm, ImageAddressMode::Repeat).unwrap()),
        layers: images
            .add(layer_image(layers, TextureFormat::Rgba8Unorm, ImageAddressMode::Repeat).unwrap()),
        ramp: images.add(
            layer_image(
                vec![ramp_pixels(&theme.palette)],
                TextureFormat::Rgba8UnormSrgb,
                ImageAddressMode::ClampToEdge,
            )
            .unwrap(),
        ),
    }
}

fn terrain_layers(
    inputs: &TerrainInputs,
    theme: &crate::world_presentation::LandTheme,
    phase: f32,
    coast_extent: f32,
) -> TerrainLayers {
    let materials = &theme.materials;
    let linear = |c: [f32; 3]| {
        let c = preview_srgb_palette_color(c).to_linear();
        Vec4::new(c.red, c.green, c.blue, 1.)
    };
    TerrainLayers {
        parameters: Vec4::new(
            phase,
            (materials.min_cliff_gradient + tune("CLIFF_BIAS", 0.))
                .to_radians()
                .cos(),
            if inputs.detail_key.is_some() { 1. } else { 0. },
            coast_extent,
        ),
        beach_color: linear(theme.palette.beach),
        cliff_color: linear(theme.palette.cliff),
        cliff_texture: inputs.layers.clone(),
        ramp_texture: inputs.ramp.clone(),
        detail_texture: inputs.detail.clone(),
        tiling: Vec4::new(
            materials.above_detail_high_tiling / PREVIEW_FACE_WIDTH * tune("TILE_HIGH", 1.0),
            materials.above_detail_low_tiling / PREVIEW_FACE_WIDTH * tune("TILE_LOW", 1.0),
            materials.cliff_tiling / PREVIEW_FACE_WIDTH * tune("TILE_CLIFF", 1.8),
            materials.above_detail_high_tiling / PREVIEW_FACE_WIDTH * tune("TILE_NEAR", 4.0),
        ),
        shaping: Vec4::new(
            (materials.max_cliff_gradient + tune("CLIFF_BIAS", 0.))
                .to_radians()
                .cos(),
            RAMP_ELEVATION_RANGE,
            tune("MUNGE", 0.22),
            tune("DETAIL_CONTRAST", 1.1),
        ),
        surface: Vec4::new(
            tune("BUMP", 0.06),
            tune("NEAR_DETAIL", 0.8),
            tune("SAND", 0.65),
            1.,
        ),
        grading: Vec4::new(
            tune("DIRT", 0.35),
            tune("DESAT", 0.2),
            tune("ALBEDO", 0.9),
            tune("CLIFF_CONTRAST", 1.3),
        ),
        planet: Vec4::ZERO,
        planet_bands: Vec4::new(1., 0., 0., 0.),
        sky: [Vec4::ZERO; crate::planet_sky::SKY_SLOTS],
        scatter: crate::planet_sky::NEUTRAL_CUBE,
        below_texture: Handle::default(),
    }
}

/// Planet-mode material inputs: per-face tiling over the real ~785-unit face
/// edge (PREVIEW_FACE_WIDTH was the island stand-in), the colour ramp over
/// the altitude range above the water (h_w..1), and beach bands in units of
/// mBeachLevel 0.025 x altRange.
fn planet_layers(
    inputs: &TerrainInputs,
    theme: &crate::world_presentation::LandTheme,
    phase: f32,
    planet: &crate::planet_query::StagePlanet,
) -> TerrainLayers {
    planet_layers_at(inputs, theme, phase, &planet.planet, planet.frame.centre, 0.0)
}

/// `planet_layers` for a planet centred at `centre` (world) and drawn at
/// `unit` world units per planet unit (`planet_bands.y`; 0 = the stages'
/// 1:1 planet, the shader then measures in world units as before).
fn planet_layers_at(
    inputs: &TerrainInputs,
    theme: &crate::world_presentation::LandTheme,
    phase: f32,
    p: &crate::planet_query::Planet,
    centre: Vec3,
    unit: f32,
) -> TerrainLayers {
    let mut layers = terrain_layers(inputs, theme, phase, 0.);
    let face = crate::planet_render::FACE_EDGE_UNITS;
    let m = &theme.materials;
    layers.tiling = Vec4::new(
        m.above_detail_high_tiling / face * tune("TILE_HIGH", 1.0),
        m.above_detail_low_tiling / face * tune("TILE_LOW", 1.0),
        m.cliff_tiling / face * tune("TILE_CLIFF", 1.8),
        m.above_detail_high_tiling / face * tune("TILE_NEAR", 4.0),
    );
    layers.shaping.y = p.altitude_range * (1.0 - p.water_level) * tune("RAMP_SPAN", 0.6);
    let scale = if unit > 0.0 { unit } else { 1.0 };
    layers.planet = centre.extend(p.water_radius() * scale);
    layers.planet_bands = Vec4::new(
        crate::planet_query::BEACH_LEVEL * p.altitude_range / 0.5,
        unit,
        0.,
        0.,
    );
    layers
}

/// Cached land-theme inputs of the Space planet view (decoded once).
#[derive(Resource, Clone)]
struct SpaceTerrainInputs(TerrainInputs, crate::world_presentation::LandTheme);

fn space_terrain_inputs(world: &mut World) -> Option<SpaceTerrainInputs> {
    if let Some(c) = world.get_resource::<SpaceTerrainInputs>() {
        return Some(c.clone());
    }
    let theme = world
        .get_resource::<crate::world_presentation::LandTheme>()
        .cloned()
        .or_else(|| {
            crate::world_presentation::LandTheme::load(&world.get_resource::<crate::Originals>()?.resource_index)
                .ok()
        })?;
    let inputs = load_terrain_inputs(world, &theme);
    let c = SpaceTerrainInputs(inputs, theme);
    world.insert_resource(c.clone());
    Some(c)
}

/// Decode the Space planet view's land inputs ahead of the first descent
/// (`space_planets`; a few hundred ms of raster decoding in the dev profile).
pub fn warm_space_planet_material(world: &mut World) -> bool {
    world.contains_resource::<Assets<Image>>() && space_terrain_inputs(world).is_some()
}

/// The Space planet view (`space_planets`): the planet-mode land material
/// for `planet` centred at `centre` and drawn at `unit` world units per
/// planet unit, with the planet's own colour ramp (sRGB, low to high ground)
/// and beach / cliff colours when given (else the land theme's, as the
/// homeworld in the earlier stages). `None` without the installed land theme
/// or render assets (headless).
#[allow(clippy::too_many_arguments)]
pub fn space_planet_material(
    world: &mut World,
    planet: &crate::planet_query::Planet,
    centre: Vec3,
    unit: f32,
    phase: f32,
    ramp: Option<&[[f32; 3]]>,
    beach: Option<[f32; 3]>,
    cliff: Option<[f32; 3]>,
) -> Option<Handle<TerrainMaterial>> {
    if !world.contains_resource::<Assets<TerrainMaterial>>()
        || !world.contains_resource::<Assets<Image>>()
    {
        return None;
    }
    let SpaceTerrainInputs(inputs, theme) = space_terrain_inputs(world)?;
    let mut layers = planet_layers_at(&inputs, &theme, phase, planet, centre, unit);
    let linear = |c: [f32; 3]| {
        let c = preview_srgb_palette_color(c).to_linear();
        Vec4::new(c.red, c.green, c.blue, 1.)
    };
    if let Some(ramp) = ramp.filter(|r| r.len() >= 2) {
        let n = ramp.len() - 1;
        let rgba = (0..256)
            .flat_map(|i| {
                let t = i as f32 / 255. * n as f32;
                let (a, b) = (ramp[(t.floor() as usize).min(n)], ramp[(t.ceil() as usize).min(n)]);
                let f = t.fract();
                let c = [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * f);
                [
                    (c[0].clamp(0., 1.) * 255.).round() as u8,
                    (c[1].clamp(0., 1.) * 255.).round() as u8,
                    (c[2].clamp(0., 1.) * 255.).round() as u8,
                    255,
                ]
            })
            .collect();
        let pixels = crate::texture::Pixels { width: 256, height: 1, rgba };
        let image = layer_image(vec![pixels], TextureFormat::Rgba8UnormSrgb, ImageAddressMode::ClampToEdge)?;
        layers.ramp_texture = world.resource_mut::<Assets<Image>>().add(image);
    }
    if let Some(c) = beach {
        layers.beach_color = linear(c);
    }
    if let Some(c) = cliff {
        layers.cliff_color = linear(c);
    }
    Some(world.resource_mut::<Assets<TerrainMaterial>>().add(TerrainMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: None,
            perceptual_roughness: 0.92,
            reflectance: 0.3,
            ..default()
        },
        extension: layers,
    }))
}

fn spawn_distant_landforms(
    world: &mut World,
    surface: &LandSurface,
    palette: &crate::world_catalog::PlanetPalette,
    material: Option<Handle<TerrainMaterial>>,
    detail_key: Option<crate::dbpf::ResourceKey>,
) {
    let Some(material) = material else {
        // Headless stage simulation has no render asset resources.
        return;
    };

    let phase = deterministic_unit(surface.seed, 0, 9) * std::f32::consts::TAU;
    const ISLANDS: u32 = 11;
    for index in 0..ISLANDS {
        // Jitter the nominal spacing so the horizon reads as scattered land,
        // not a symmetric enclosure around the playable island.
        let angle = phase
            + index as f32 / ISLANDS as f32 * std::f32::consts::TAU
            + (deterministic_unit(surface.seed, index, 13) - 0.5) * 0.24;
        let distance = 88. + deterministic_unit(surface.seed, index, 10) * 66.;
        let radius = 9. + deterministic_unit(surface.seed, index, 11) * 12.;
        let peak = 5. + deterministic_unit(surface.seed, index, 12) * 10.;
        let mesh = distant_landform_mesh(surface.seed, index, radius, peak, palette);
        let Some(mesh) = world
            .get_resource_mut::<Assets<Mesh>>()
            .map(|mut assets| assets.add(mesh))
        else {
            return;
        };
        let mut entity = world.spawn((
            StageOwned,
            AdaptedDistantLandform {
                seed: surface.seed ^ u64::from(index),
                palette: palette.key,
                detail_texture: detail_key,
            },
            NotShadowCaster,
            Name::new(format!("adapted distant landform {index}")),
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(angle.cos() * distance, 0., angle.sin() * distance),
        ));
        // Existing source markers describe only the installed texture inputs;
        // this marker above separately identifies generated geometry.
        if let Some(texture) = detail_key {
            entity.insert(OriginalTerrainTexture {
                definition: crate::world_presentation::MASTER_THEME,
                texture,
                palette: palette.key,
                cliff: None,
                beach: None,
            });
        }
    }
}

/// Stage setup and checkpoint restoration finish first. Reconstruct terrain
/// from the saved ecology seed and stage sites, then establish clearances.
pub fn prepare(world: &mut World) {
    let _layout = crate::tribal_planet::scope_world(world);
    let stage = world.resource::<State<GameStage>>().get().clone();
    let run = world.resource::<RunProgress>();
    let seed = run.species_world.seed;
    let mut pads = vec![(Vec2::new(0., 8.), 3.), (Vec2::ZERO, 4.)];
    let mut sphere_pads: Vec<(Vec3, f32)> = Vec::new();
    match stage {
        GameStage::Creature => {
            // The player's nest (wherever it is now) and every herd nest.
            let home = run.creature_lifecycle.home;
            pads = vec![(Vec2::new(home[0], home[2]), 3.)];
            pads.extend(
                run.species_world
                    .nests
                    .iter()
                    .map(|n| (Vec2::new(n.position[0], n.position[2]), 3.)),
            )
        }
        GameStage::Tribal => pads.extend((0..5).map(|i| {
            let p = crate::tribal::village_position(i);
            (Vec2::new(p.x, p.z), 4.)
        })),
        GameStage::Civilization => {
            pads.extend((0..5).map(|i| {
                let p = crate::civ::city_position(i);
                (Vec2::new(p.x, p.z), 5.)
            }));
            // On the planet: the city flatteners at the cities' world points
            // (`CityFlattenSize` 75 read as the pad's diameter).
            sphere_pads.extend(run.civ_campaign.cities.iter().map(|c| {
                (c.center(), crate::civ_rules::CITY_FLATTEN_SIZE * 0.5)
            }));
        }
        _ => {
            world.remove_resource::<LandSurface>();
            world.remove_resource::<crate::planet_render::PlanetRender>();
            return;
        }
    }
    world.remove_resource::<crate::planet_render::PlanetRender>();
    if let Some(planet) = stage_planet(world, seed, &pads, &sphere_pads) {
        prepare_planet(world, stage, seed, planet);
        return;
    }
    let surface = LandSurface::generate(stage, seed, &pads);
    let land_theme = world
        .get_resource::<crate::world_presentation::LandTheme>()
        .cloned()
        .or_else(|| {
            crate::world_presentation::LandTheme::load(
                &world.resource::<crate::Originals>().resource_index,
            )
            .ok()
        });
    let terrain_mesh = surface.mesh_with_palette(land_theme.as_ref().map(|t| &t.palette));
    let phase = (surface.seed ^ (surface.seed >> 32)) as u32 as f32 / u32::MAX as f32 * 6.28;
    let mesh = world.resource_mut::<Assets<Mesh>>().add(terrain_mesh);
    let use_layered_material = land_theme.is_some()
        && world.contains_resource::<Assets<TerrainMaterial>>()
        && world.contains_resource::<Assets<Image>>();
    let inputs = if use_layered_material {
        Some(load_terrain_inputs(world, land_theme.as_ref().unwrap()))
    } else {
        None
    };
    let detail_key = inputs.as_ref().and_then(|inputs| inputs.detail_key);
    let (material, landform_material) = if let Some(inputs) = &inputs {
        let theme = land_theme.as_ref().unwrap();
        let base = StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: None,
            perceptual_roughness: 0.92,
            reflectance: 0.3,
            ..default()
        };
        let mut materials = world.resource_mut::<Assets<TerrainMaterial>>();
        let island = materials.add(TerrainMaterial {
            base: base.clone(),
            extension: terrain_layers(inputs, theme, phase, surface.extent),
        });
        let landforms = materials.add(TerrainMaterial {
            base,
            extension: terrain_layers(inputs, theme, phase, 0.),
        });
        (Some(island), Some(landforms))
    } else {
        // The headless fallback does not install the render plugin.
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.95,
                ..default()
            });
        for e in world
            .query_filtered::<Entity, With<WorldGround>>()
            .iter(world)
            .collect::<Vec<_>>()
        {
            world.entity_mut(e).insert(MeshMaterial3d(material.clone()));
        }
        (None, None)
    };
    let grounds: Vec<Entity> = world
        .query_filtered::<Entity, With<WorldGround>>()
        .iter(world)
        .collect();
    for e in grounds {
        world
            .entity_mut(e)
            .insert((Mesh3d(mesh.clone()), Transform::IDENTITY));
        if let Some(material) = &material {
            // Only one material renderer may draw the ground. Keeping its
            // setup-time StandardMaterial also draws an opaque square over
            // the layered shader and hides its detail.
            world
                .entity_mut(e)
                .remove::<MeshMaterial3d<StandardMaterial>>();
            // The submerged outer shelf would only shadow the opaque sea.
            world.entity_mut(e).insert(NotShadowCaster);
            world.entity_mut(e).insert(MeshMaterial3d(material.clone()));
        } else {
            world
                .entity_mut(e)
                .remove::<MeshMaterial3d<TerrainMaterial>>();
        }
        if let Some(texture) = detail_key {
            world.entity_mut(e).insert(OriginalTerrainTexture {
                definition: crate::world_presentation::MASTER_THEME,
                texture,
                palette: crate::world_presentation::HOME_PALETTE,
                cliff: land_theme.as_ref().and_then(|t| t.textures.cliff),
                beach: land_theme.as_ref().and_then(|t| t.textures.beach),
            });
        }
    }
    if let Some(theme) = &land_theme {
        spawn_distant_landforms(
            world,
            &surface,
            &theme.palette,
            landform_material,
            detail_key,
        );
    }
    let water_extent = surface.extent;
    world.insert_resource(surface);
    // Cosmetic surrounding sea; game boundaries prevent land-stage swimming.
    let water = world
        .resource_mut::<Assets<Mesh>>()
        // The playable island is small; extend the cosmetic water to the
        // camera's fogged horizon instead of exposing the square plane edge.
        .add(Plane3d::default().mesh().size(640., 640.));
    let (water_mat, water_sources) = crate::water::material(
        world,
        land_theme.as_ref().map(|theme| &theme.palette),
        phase,
        water_extent,
    );
    world.spawn((
        StageOwned,
        WorldGround,
        Name::new("surrounding sea"),
        water_sources,
        Mesh3d(water),
        MeshMaterial3d(water_mat),
        Transform::from_xyz(0., SEA_Y, 0.),
    ));
    ground(world);
}

/// The stage planet for a save seed: the homeworld (a FirstGroup terrain
/// script chosen by the seed) generated once and cached, rotated so the stage
/// origin is +Y, with the stage's site pads levelled into a copy of its cube.
/// `None` without installed planet data (headless) or with
/// `OPENSPORE_FLAT_TERRAIN=1`.
fn stage_planet(
    world: &mut World,
    seed: u64,
    pads: &[(Vec2, f32)],
    sphere_pads: &[(Vec3, f32)],
) -> Option<std::sync::Arc<crate::planet_query::StagePlanet>> {
    if std::env::var("OPENSPORE_FLAT_TERRAIN").is_ok_and(|v| v != "0") {
        return None;
    }
    let cached = world
        .get_resource::<PlanetCache>()
        .filter(|c| c.seed == seed)
        .cloned();
    let cache = match cached {
        Some(cache) => cache,
        None => {
            let index = &world.get_resource::<crate::Originals>()?.resource_index;
            let cache = generate_planet_cache(index, seed)?;
            world.insert_resource(cache.clone());
            cache
        }
    };
    // Tribal: the planet levelled under the tribes, the hut at the pole
    // (`tribal_planet`, laid out when the stage was set up).
    if *world.resource::<State<GameStage>>().get() == GameStage::Tribal {
        if let Some(planet) = crate::tribal_planet::stage_planet(seed) {
            return Some(planet);
        }
    }
    let base = &cache.base;
    let frame = match world.resource::<State<GameStage>>().get() {
        GameStage::Creature => crate::creature_planet::creature_frame(
            &cache,
            world.resource::<RunProgress>().species_world.planet_frame,
        ),
        _ => crate::planet_query::PlanetFrame::new(base.radius, cache.origin),
    };
    let mut cube = base.cube.clone();
    // World-point pads (Civ cities anywhere on the hemisphere): half-widths.
    for (point, half_width) in sphere_pads {
        let dir = (*point - frame.centre).normalize_or(Vec3::Y);
        crate::planet_gen::flatten_pad(&mut cube, frame.dir_to_local(dir), *half_width, base.radius);
    }
    let pads = if sphere_pads.is_empty() { pads } else { &[] };
    for (centre, radius) in pads {
        // ADAPTED site pads (the island's flattened nest/village/city spots),
        // as AddPlayerModification flattener stamps on the sphere.
        let dir = (Vec3::new(centre.x, 0., centre.y) - frame.centre).normalize();
        crate::planet_gen::flatten_pad(
            &mut cube,
            frame.dir_to_local(dir),
            (radius * 2.5).max(6.),
            base.radius,
        );
    }
    let planet = crate::planet_query::Planet::new(cube, base.radius, base.water_level);
    Some(std::sync::Arc::new(crate::planet_query::StagePlanet { planet, frame }))
}

/// The homeworld for a save seed: its FirstGroup terrain script generated
/// and the stage origin chosen on it. `None` without installed planet data or
/// with `OPENSPORE_FLAT_TERRAIN=1`.
pub fn generate_planet_cache(index: &crate::asset_catalog::ResourceIndex, seed: u64) -> Option<PlanetCache> {
    if std::env::var("OPENSPORE_FLAT_TERRAIN").is_ok_and(|v| v != "0") {
        return None;
    }
    if index.records.is_empty() {
        return None;
    }
    let start = std::time::Instant::now();
    let scripts = crate::planet_data::first_group(index);
    let script = crate::planet_data::choose_homeworld(&scripts, seed)?.clone();
    let source = match crate::planet_data::PlanetSource::load(index, &script) {
        Ok(source) => source,
        Err(error) => {
            warn!("planet {}: {error}", script.description);
            return None;
        }
    };
    let loaded = start.elapsed().as_millis();
    let (planet, stats) = crate::planet_gen::build_planet(&source);
    let origin = crate::planet_query::choose_stage_origin(&planet, seed);
    info!(
        "planet: homeworld {} (terrain script {:08x}): {} entries, {} stamps, data {} ms, generation {} ms, total {} ms",
        script.description,
        script.terrain_script,
        stats.entries,
        stats.stamps,
        loaded,
        stats.millis,
        start.elapsed().as_millis()
    );
    Some(PlanetCache {
        seed,
        script,
        base: std::sync::Arc::new(planet),
        origin,
        stats,
    })
}

/// Planet-mode stage setup: the WorldGround entity becomes the planet's
/// chunk root (frame transform, land material), the sea is the water sphere,
/// and the atmosphere shell is added; `planet_render::update` draws chunks.
fn prepare_planet(
    world: &mut World,
    stage: GameStage,
    seed: u64,
    planet: std::sync::Arc<crate::planet_query::StagePlanet>,
) {
    let surface = LandSurface::on_planet(stage, seed, planet.clone());
    let land_theme = world
        .get_resource::<crate::world_presentation::LandTheme>()
        .cloned()
        .or_else(|| {
            crate::world_presentation::LandTheme::load(
                &world.resource::<crate::Originals>().resource_index,
            )
            .ok()
        });
    let phase = (seed ^ (seed >> 32)) as u32 as f32 / u32::MAX as f32 * 6.28;
    let frame_transform = Transform {
        translation: planet.frame.centre,
        rotation: planet.frame.rotation,
        scale: Vec3::ONE,
    };
    let grounds: Vec<Entity> = world
        .query_filtered::<Entity, With<WorldGround>>()
        .iter(world)
        .collect();
    let renderable = land_theme.is_some()
        && world.contains_resource::<Assets<TerrainMaterial>>()
        && world.contains_resource::<Assets<Image>>()
        && world.contains_resource::<Assets<Mesh>>();
    if renderable {
        let theme = land_theme.as_ref().unwrap();
        let inputs = load_terrain_inputs(world, theme);
        let material = world.resource_mut::<Assets<TerrainMaterial>>().add(TerrainMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: None,
                perceptual_roughness: 0.92,
                reflectance: 0.3,
                ..default()
            },
            extension: planet_layers(&inputs, theme, phase, &planet),
        });
        for &e in &grounds {
            let mut entity = world.entity_mut(e);
            entity.remove::<(Mesh3d, MeshMaterial3d<StandardMaterial>)>();
            entity.insert((
                MeshMaterial3d(material.clone()),
                NotShadowCaster,
                frame_transform,
                Visibility::Visible,
            ));
            if let Some(texture) = inputs.detail_key {
                entity.insert(OriginalTerrainTexture {
                    definition: crate::world_presentation::MASTER_THEME,
                    texture,
                    palette: crate::world_presentation::HOME_PALETTE,
                    cliff: theme.textures.cliff,
                    beach: theme.textures.beach,
                });
            }
        }
        if let Some(&root) = grounds.first() {
            world.insert_resource(crate::planet_render::PlanetRender {
                stage: planet.clone(),
                root,
                land_material: material,
                chunks: Default::default(),
                meshes: Default::default(),
                fog_base: None,
                sun_base: None,
                ambient_base: None,
                last_nodes: 0,
            });
        }
    } else {
        // Headless: nothing is drawn, the cuboid placeholder is irrelevant.
        for &e in &grounds {
            world.entity_mut(e).insert(frame_transform);
        }
    }
    world.insert_resource(surface);
    if world.contains_resource::<Assets<Mesh>>()
        && world.contains_resource::<Assets<crate::water::WaterMaterial>>()
        && world.contains_resource::<Assets<Image>>()
    {
        let water = world
            .resource_mut::<Assets<Mesh>>()
            .add(crate::planet_render::water_mesh(&planet.planet));
        let (water_mat, water_sources) = crate::water::material(
            world,
            land_theme.as_ref().map(|theme| &theme.palette),
            phase,
            0.,
        );
        crate::water::set_planet(
            world,
            &water_mat,
            planet.frame.centre,
            planet.planet.water_radius(),
            crate::planet_render::depth_cube_image(&planet),
            crate::planet_render::DEPTH_SCALE * tune("WATER_DEPTH_K", 0.3),
        );
        world.spawn((
            StageOwned,
            WorldGround,
            Name::new("surrounding sea"),
            water_sources,
            NotShadowCaster,
            Mesh3d(water),
            MeshMaterial3d(water_mat),
            frame_transform,
        ));
    }
    if renderable && world.contains_resource::<Assets<crate::planet_render::AtmosphereMaterial>>() {
        let air = land_theme
            .as_ref()
            .map_or([0.7, 0.85, 1.0], |theme| theme.palette.atmosphere);
        if !crate::planet_sky::spawn(world, &planet, air) {
            crate::planet_render::spawn_static(world, &planet, air);
        }
    }
    ground(world);
}

/// Run after simulation and before camera/part presentation. New respawns
/// receive a clearance once; saved heights are never accumulated twice.
///
/// On the planet the facade height is the sphere's ToSurface point on the
/// vertical line through the entity, so (x, z) never drift when stage code
/// rewrites y each frame, and entities are turned to the local up
/// (`GetOrientation` keeps the heading: minimal rotation of their up axis).
/// ADAPTED: the clearance is applied along world Y, which is the local up
/// within ~5 degrees over the stages' island-sized areas; 3D tangent-space
/// placement arrives with the stage migrations (planet-re 4E steps 4-6).
pub fn ground(world: &mut World) {
    let Some(surface) = world.get_resource::<LandSurface>() else {
        return;
    };
    if world.resource::<State<GameStage>>().get() != &surface.stage {
        return;
    }
    let surface = surface.clone();
    let stage = surface.stage.clone();
    let airborne_height = world
        .get_resource::<crate::creature::mobility::CreatureMobility>()
        .map_or(0.0, |mobility| {
            if mobility.height.is_finite() {
                mobility.height.max(0.0)
            } else {
                0.0
            }
        });
    let mut query = world.query_filtered::<(
        Entity,
        &Transform,
        Option<&Grounded>,
        Option<&Player>,
        Option<&crate::game::Npc>,
        Option<&crate::baked_model::NativeCreature>,
        Has<Radial>,
    ), (
        With<StageOwned>,
        Without<ChildOf>,
        Without<WorldGround>,
        Without<crate::creature_home::HomeNest>,
        // Swarm draw entities (screen quads, particle meshes) are placed by
        // the effect renderer, never on the ground.
        Without<crate::swarm_fx::render::SwarmDraw>,
    )>();
    let planet = surface.planet_arc();
    let mut radial_updates: Vec<(Entity, Vec3, f32)> = Vec::new();
    if let Some(sp) = planet.as_deref() {
        // `Radial` entities: ToSurface of their own radial line plus the
        // clearance along the local up (anywhere on the sphere).
        for (e, t, g, _, _, _, radial) in query.iter(world) {
            if !radial {
                continue;
            }
            let c = sp.frame.centre;
            if (t.translation - c).length_squared() < 1.0 {
                continue;
            }
            let up = (t.translation - c).normalize();
            let s = sp.to_surface_world(t.translation);
            let clearance = g.map_or_else(|| (t.translation - s).dot(up), |g| g.clearance);
            radial_updates.push((e, s + up * clearance, clearance));
        }
    }
    let updates: Vec<_> = query
        .iter(world)
        .filter(|(.., radial)| !(*radial && planet.is_some()))
        .map(|(e, t, g, player, npc, native, _)| {
            let h = surface.height(t.translation.x, t.translation.z);
            let clearance = if player.is_some() && stage == GameStage::Creature {
                // Creator changes may resize the body after stage entry.
                0.7 * t.scale.x - BASE_Y
            } else if native.is_some() {
                t.scale.y
            } else {
                g.map_or_else(
                    || {
                        // Locomotion systems write their actor baseline before this pass.
                        if npc.is_some() && stage == GameStage::Creature {
                            1.0
                        } else {
                            t.translation.y - BASE_Y
                        }
                    },
                    |g| g.clearance,
                )
            };
            let creature_player = player.is_some() && stage == GameStage::Creature;
            let height = if creature_player {
                airborne_height
            } else {
                0.0
            };
            let gait = if g.is_some() && creature_player && height <= 1e-4 {
                (t.translation.y - 0.7 * t.scale.x).clamp(-0.08, 0.08)
            } else {
                0.
            };
            (e, h + clearance + gait + height, clearance)
        })
        .collect();
    for (e, at, clearance) in radial_updates {
        if let Some(mut t) = world.get_mut::<Transform>(e) {
            if t.translation != at {
                t.translation = at;
            }
            let up = planet.as_deref().map_or(Vec3::Y, |sp| sp.frame.up(at));
            let tilt = Quat::from_rotation_arc((t.rotation * Vec3::Y).normalize_or(Vec3::Y), up);
            if tilt.w < 0.999_999_9 {
                t.rotation = (tilt * t.rotation).normalize();
            }
        }
        if world
            .get::<Grounded>(e)
            .is_none_or(|g| g.clearance != clearance)
        {
            world.entity_mut(e).insert(Grounded { clearance });
        }
    }
    let frame = surface.planet().map(|p| p.frame);
    for (e, y, clearance) in updates {
        if let Some(mut t) = world.get_mut::<Transform>(e) {
            // Beyond the planet's limb (below its centre) the vertical line
            // meets the near side: such an entity (left behind by the
            // Creature stage's floating origin) already stands on its ground.
            if frame.is_some_and(|f| t.translation.y < f.centre.y) {
                continue;
            }
            t.translation.y = y;
            if let Some(frame) = frame {
                let up = frame.up(t.translation);
                let tilt = Quat::from_rotation_arc((t.rotation * Vec3::Y).normalize_or(Vec3::Y), up);
                if tilt.w < 0.999_999_9 {
                    t.rotation = (tilt * t.rotation).normalize();
                }
            }
        }
        if world
            .get::<Grounded>(e)
            .is_none_or(|g| g.clearance != clearance)
        {
            world.entity_mut(e).insert(Grounded { clearance });
        }
    }
}

/// Keep checkpoint positions in the historical flat-world coordinate basis.
/// Reloading applies this stage's surface once, including for older saves.
pub fn checkpoint_position(world: &World, entity: Entity, position: Vec3) -> [f32; 3] {
    let mut position = position;
    // Radial entities keep their 3D position (it is on its own radial line).
    let radial = world.get::<Radial>(entity).is_some()
        && world.get_resource::<LandSurface>().is_some_and(|s| s.planet().is_some());
    if world.get::<Grounded>(entity).is_some() && !radial {
        if let Some(surface) = world
            .get_resource::<LandSurface>()
            .filter(|s| s.planet().is_none_or(|p| position.y >= p.frame.centre.y))
        {
            position.y -= surface.height(position.x, position.z) - BASE_Y;
            if surface.stage == GameStage::Creature && world.get::<Player>(entity).is_some() {
                // Airborne velocity is transient. Resume a save on its standing
                // baseline rather than preserving height without its motion.
                position.y -= world
                    .get_resource::<crate::creature::mobility::CreatureMobility>()
                    .map_or(0.0, |mobility| mobility.height);
            }
        }
    }
    position.to_array()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_palette_channels_use_srgb_preview_mapping_and_stay_in_range() {
        let authored = [0.5, 0.25, 0.75];
        let palette = crate::world_catalog::PlanetPalette {
            key: crate::dbpf::ResourceKey {
                group_id: 0x406B0100,
                instance_id: 1,
                type_id: crate::asset_catalog::PROP,
            },
            elevations: vec![0., 1.],
            colors: vec![authored, authored],
            water: authored,
            atmosphere: authored,
            cliff: authored,
            beach: authored,
        };
        let surface = LandSurface::generate(GameStage::Creature, 123, &[]);
        let mesh = surface.mesh_with_palette(Some(&palette));
        let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("terrain colors")
        };
        let expected = Color::srgb(authored[0], authored[1], authored[2])
            .to_linear()
            .to_f32_array();
        assert_eq!(
            [colors[0][0], colors[0][1], colors[0][2]],
            [expected[0], expected[1], expected[2]]
        );
        assert_eq!(colors[0][3], 1.);
        assert!(colors
            .iter()
            .flatten()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
    }

    #[test]
    fn deterministic_surface_flat_sites_and_finite_edges() {
        let pads = [(Vec2::ZERO, 4.)];
        let a = LandSurface::generate(GameStage::Creature, 123, &pads);
        let b = LandSurface::generate(GameStage::Creature, 123, &pads);
        let c = LandSurface::generate(GameStage::Creature, 987, &pads);
        assert_eq!(a.heights, b.heights);
        assert_ne!(a.heights, c.heights);
        assert_eq!(a.height(0., 0.), BASE_Y);
        assert_eq!(a.height(1., 1.), BASE_Y);
        assert_eq!(a.height(f32::NAN, 0.), BASE_Y);
        assert_eq!(a.height(100., 100.), a.height(a.extent, a.extent));
        assert_eq!(a.height(a.extent, a.extent), BASE_Y - SHELF_DEPTH);
        assert!(a
            .heights
            .iter()
            .all(|h| h.is_finite() && *h >= BASE_Y - SHELF_DEPTH && *h <= BASE_Y + 7.));
    }
    #[test]
    fn triangles_and_cpu_height_agree() {
        let a = LandSurface::generate(GameStage::Creature, 123, &[]);
        let mesh = a.mesh();
        assert_eq!(mesh.count_vertices(), (CELLS + 1).pow(2));
        for z in 0..CELLS {
            for x in 0..CELLS {
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("terrain positions")
                };
                let [px, _, pz] = positions[z * (CELLS + 1) + x];
                assert!((a.height(px, pz) - a.heights[z * (CELLS + 1) + x]).abs() < 1e-5);
            }
        }
        let step = a.extent * 2. / CELLS as f32;
        let h = a.heights[0] * 0.5 + a.heights[1] * 0.25 + a.heights[CELLS + 1] * 0.25;
        assert!((a.height(-a.extent + step * 0.25, -a.extent + step * 0.25) - h).abs() < 1e-5);
    }
    #[test]
    fn moving_on_hills_and_cold_checkpoint_reload_do_not_accumulate_height() {
        for stage in [
            GameStage::Creature,
            GameStage::Tribal,
            GameStage::Civilization,
        ] {
            let mut app = crate::runtime::headless_app(RunProgress::default());
            app.world_mut()
                .resource_mut::<NextState<GameStage>>()
                .set(stage.clone());
            app.update();
            assert_eq!(app.world().resource::<State<GameStage>>().get(), &stage);
            let player = app
                .world_mut()
                .query_filtered::<Entity, With<Player>>()
                .single(app.world())
                .unwrap();
            let clearance = app.world().get::<Grounded>(player).unwrap().clearance;
            {
                let mut t = app.world_mut().get_mut::<Transform>(player).unwrap();
                t.translation.x = 13.;
                t.translation.z = 15.;
            }
            app.update();
            let p = app.world().get::<Transform>(player).unwrap().translation;
            let h = app.world().resource::<LandSurface>().height(p.x, p.z);
            assert!((p.y - h - clearance).abs() < 1e-5);
            let expected = app.world().resource::<LandSurface>().heights.clone();
            crate::persistence::capture(app.world_mut());
            let run = app.world().resource::<RunProgress>().clone();
            assert!(
                (run.checkpoint.as_ref().unwrap().player.position[1] - (clearance + BASE_Y)).abs()
                    < 1e-5
            );
            let json = serde_json::to_string(&run).unwrap();
            let mut cold = crate::runtime::headless_app(serde_json::from_str(&json).unwrap());
            cold.world_mut()
                .resource_mut::<NextState<GameStage>>()
                .set(stage.clone());
            cold.update();
            assert_eq!(cold.world().resource::<LandSurface>().heights, expected);
            for _ in 0..5 {
                cold.update();
            }
            let restored = cold
                .world_mut()
                .query_filtered::<&Transform, With<Player>>()
                .single(cold.world())
                .unwrap()
                .translation;
            let restored_player = cold
                .world_mut()
                .query_filtered::<Entity, With<Player>>()
                .single(cold.world())
                .unwrap();
            let restored_clearance = cold
                .world()
                .get::<Grounded>(restored_player)
                .unwrap()
                .clearance;
            let restored_height = cold
                .world()
                .resource::<LandSurface>()
                .height(restored.x, restored.z);
            assert!(
                (restored.y - restored_height - restored_clearance).abs() < 1e-4,
                "{stage:?}: restored position {restored:?} is not grounded at {restored_height} with clearance {restored_clearance}"
            );
            cold.world_mut()
                .resource_mut::<NextState<GameStage>>()
                .set(GameStage::Space);
            cold.update();
            assert!(!cold.world().contains_resource::<LandSurface>());
            assert_eq!(
                cold.world_mut()
                    .query_filtered::<Entity, With<WorldGround>>()
                    .iter(cold.world())
                    .count(),
                0
            );
        }
    }

    #[test]
    fn resized_creature_preserves_clearance_and_walking_bob_on_hills() {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        app.update();
        let player = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(app.world())
            .unwrap();
        {
            let mut t = app.world_mut().get_mut::<Transform>(player).unwrap();
            t.scale = Vec3::splat(1.7);
            t.translation.x = 13.;
            t.translation.z = 15.;
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        for _ in 0..5 {
            app.update();
        }
        let p = app.world().get::<Transform>(player).unwrap().translation;
        let h = app.world().resource::<LandSurface>().height(p.x, p.z);
        let bob = (app.world().resource::<Time>().elapsed_secs() * 11.).sin() * 0.06;
        assert!((p.y - h - (0.7 * 1.7 - BASE_Y) - bob).abs() < 1e-4);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyW);
        app.update();
        crate::persistence::capture(app.world_mut());
        assert!(
            (app.world()
                .resource::<RunProgress>()
                .checkpoint
                .as_ref()
                .unwrap()
                .player
                .position[1]
                - 0.7 * 1.7)
                .abs()
                < 1e-4
        );
    }
}

#[cfg(test)]
mod inspect_inputs {
    use crate::{asset_catalog::ResourceIndex, dbpf::ResourceKey};
    use std::fmt::Write;
    #[test]
    #[ignore = "temporary terrain input inspection"]
    fn zz_dump_terrain_inputs() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let out = std::path::Path::new("caps/inspect");
        std::fs::create_dir_all(out).unwrap();
        let mut report = String::new();
        let prop = crate::asset_catalog::PROP;
        let mut dump = |report: &mut String, label: &str, key: ResourceKey| {
            writeln!(
                report,
                "== {label} {:08X}!{:08X}",
                key.group_id, key.instance_id
            )
            .unwrap();
            let list = match index.read(key).and_then(|b| crate::prop::parse(&b)) {
                Ok(l) => l,
                Err(e) => {
                    writeln!(report, "  error {e}").unwrap();
                    return;
                }
            };
            for (id, p) in &list.0 {
                let vals: Vec<String> = p
                    .values
                    .iter()
                    .map(|b| {
                        let q = crate::prop::Property {
                            kind: p.kind,
                            array: false,
                            values: vec![b.clone()],
                        };
                        if let Some(v) = q.number() {
                            format!("{v}")
                        } else if let Some(v) = q.vector3() {
                            format!("{v:?}")
                        } else if let Some(v) = q.color_rgb() {
                            format!("c{v:?}")
                        } else if let Some(v) = q.vector4() {
                            format!("{v:?}")
                        } else if let Some(k) = q.key() {
                            format!("{:08X}!{:08X}.{:08X}", k.group_id, k.instance_id, k.type_id)
                        } else if let Some(t) = q.text() {
                            format!("{:?}", t.2)
                        } else if p.kind == 0x30 && b.len() >= 8 {
                            format!(
                                "v2[{}, {}]",
                                f32::from_le_bytes(b[0..4].try_into().unwrap()),
                                f32::from_le_bytes(b[4..8].try_into().unwrap())
                            )
                        } else {
                            format!("raw{:02X?}", b)
                        }
                    })
                    .collect();
                writeln!(
                    report,
                    "  {id:08X} kind {:04X} arr {} = {}",
                    p.kind,
                    p.array,
                    vals.join(", ")
                )
                .unwrap();
            }
        };
        let k = |g: u32, i: u32| ResourceKey {
            group_id: g,
            instance_id: i,
            type_id: prop,
        };
        dump(&mut report, "master theme", k(0x42840100, 0x6E660BEB));
        dump(&mut report, "home palette", k(0x406B0100, 0x6F78B3C5));
        dump(&mut report, "planet lighting", k(0x40200100, 0x5F848353));
        dump(&mut report, "planet materials", k(0x0243AD2B, 0xD985994C));
        dump(&mut report, "planet config", k(0x0243AD2B, 0x5F848353));
        let mut groups: Vec<_> = index
            .records
            .keys()
            .filter(|r| r.type_id == prop && (r.group_id == 0x0243AD2B || r.group_id == 0x40200100))
            .copied()
            .collect();
        groups.sort_by_key(|r| (r.group_id, r.instance_id));
        for r in &groups {
            writeln!(report, "prop {:08X}!{:08X}", r.group_id, r.instance_id).unwrap();
        }
        for r in groups.iter().filter(|r| r.group_id == 0x40200100) {
            dump(&mut report, "lighting", *r);
        }
        for (name, inst) in [
            ("detail2", 0xE2D70BADu32),
            ("intertiling", 0xA5AB24F5),
            ("shannon", 0x84613AC8),
            ("cliff_base", 0x3F05C913),
            ("beach_generic", 0x9D64F524),
            ("seabed", 0xC66C3FCD),
            ("atm_curves", 0xDEF55713),
            ("noise", 0xC5D262E4),
            ("ramps_living", 0x2EC45F93),
        ] {
            let matches: Vec<_> = index
                .records
                .keys()
                .filter(|r| r.instance_id == inst)
                .copied()
                .collect();
            for m in matches {
                writeln!(
                    report,
                    "tex {name} {:08X}!{:08X}.{:08X}",
                    m.group_id, m.instance_id, m.type_id
                )
                .unwrap();
                let bytes = index.read(m).unwrap();
                let (w, h, rgba, mips) = if crate::bitmap::supported(m.type_id) {
                    let b = crate::bitmap::parse(m.type_id, &bytes).unwrap();
                    (b.width, b.height, b.rgba8(), 1)
                } else if let Some(mips) = crate::texture::decode_rw4_mips(&bytes) {
                    (
                        mips[0].width,
                        mips[0].height,
                        mips[0].rgba.clone(),
                        mips.len(),
                    )
                } else {
                    let n = crate::native_texture::decode(&bytes, None).unwrap();
                    writeln!(
                        report,
                        "  L8? {}x{} fmt {:08X} mips {}",
                        n.width,
                        n.height,
                        n.format,
                        n.faces[0].len()
                    )
                    .unwrap();
                    let file = std::fs::File::create(out.join(format!("{name}_l8.png"))).unwrap();
                    let mut enc = png::Encoder::new(file, n.width, n.height);
                    enc.set_color(png::ColorType::Grayscale);
                    enc.set_depth(png::BitDepth::Eight);
                    enc.write_header()
                        .unwrap()
                        .write_image_data(&n.faces[0][0].bytes)
                        .unwrap();
                    let vals: Vec<f64> = n.faces[0][0].bytes.iter().map(|v| *v as f64).collect();
                    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
                    let sd = (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                        / vals.len() as f64)
                        .sqrt();
                    writeln!(report, "  l8 mean {mean:.1} sd {sd:.1}").unwrap();
                    continue;
                };
                writeln!(report, "  {w}x{h} mips {mips}").unwrap();
                for c in 0..4 {
                    let vals: Vec<f64> = rgba.chunks_exact(4).map(|p| p[c] as f64).collect();
                    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
                    let sd = (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                        / vals.len() as f64)
                        .sqrt();
                    let min = vals.iter().cloned().fold(255., f64::min);
                    let max = vals.iter().cloned().fold(0., f64::max);
                    writeln!(
                        report,
                        "  ch{c} mean {mean:.1} sd {sd:.1} min {min} max {max}"
                    )
                    .unwrap();
                    let gray: Vec<u8> = rgba.chunks_exact(4).map(|p| p[c]).collect();
                    let file = std::fs::File::create(
                        out.join(format!("{name}_{:08X}_ch{c}.png", m.type_id)),
                    )
                    .unwrap();
                    let mut enc = png::Encoder::new(file, w, h);
                    enc.set_color(png::ColorType::Grayscale);
                    enc.set_depth(png::BitDepth::Eight);
                    enc.write_header().unwrap().write_image_data(&gray).unwrap();
                }
                let file =
                    std::fs::File::create(out.join(format!("{name}_{:08X}_rgba.png", m.type_id)))
                        .unwrap();
                let mut enc = png::Encoder::new(file, w, h);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                let opaque: Vec<u8> = rgba
                    .chunks_exact(4)
                    .flat_map(|p| [p[0], p[1], p[2], 255])
                    .collect();
                enc.write_header()
                    .unwrap()
                    .write_image_data(&opaque)
                    .unwrap();
            }
        }
        std::fs::write(out.join("report.txt"), report).unwrap();
        let shaders = out.join("shaders");
        std::fs::create_dir_all(&shaders).unwrap();
        let mut keys: Vec<_> = index
            .records
            .keys()
            .copied()
            .filter(|k| {
                k.type_id == crate::shader_pack::TYPE_ID
                    && k.group_id == crate::shader_pack::GROUP_COMPILED
            })
            .collect();
        keys.sort_by_key(|k| k.instance_id);
        let mut meta = String::new();
        for key in keys {
            let pack = crate::shader_pack::parse(&index.read(key).unwrap()).unwrap();
            for (i, c) in pack.pixel.iter().enumerate() {
                std::fs::write(
                    shaders.join(format!("frag_{}_ps_{i}.bin", pack.name)),
                    &c.program,
                )
                .unwrap();
            }
            for (i, c) in pack.vertex.iter().enumerate() {
                std::fs::write(
                    shaders.join(format!("frag_{}_vs_{i}.bin", pack.name)),
                    &c.program,
                )
                .unwrap();
            }
            for st in &pack.standards {
                let name = st.header.name.clone().unwrap_or_default();
                if !(name.contains("Terrain")
                    || name.contains("Skybox")
                    || name.contains("planetLitModel")
                    || name.contains("Atmo"))
                {
                    continue;
                }
                for (i, e) in st.entries.iter().enumerate() {
                    let base = format!("{}_{:08X}_{}_{i}", name, st.header.id, pack.name);
                    std::fs::write(shaders.join(format!("{base}.vs")), &e.vertex).unwrap();
                    std::fs::write(shaders.join(format!("{base}.ps")), &e.pixel).unwrap();
                    writeln!(
                        meta,
                        "{base} rt {} vs_uniforms {:?} ps_uniforms {:?}",
                        e.render_type,
                        e.vertex_uniforms
                            .iter()
                            .map(|u| (u.data_index, u.register, u.register_size, u.flags))
                            .collect::<Vec<_>>(),
                        e.pixel_uniforms
                            .iter()
                            .map(|u| (u.data_index, u.register, u.register_size, u.flags))
                            .collect::<Vec<_>>()
                    )
                    .unwrap();
                }
            }
        }
        std::fs::write(shaders.join("meta.txt"), meta).unwrap();
    }
}

#[cfg(test)]
mod relief_stats {
    use super::*;
    #[test]
    #[ignore = "temporary relief statistics"]
    fn zz_relief_stats() {
        for stage in [
            GameStage::Creature,
            GameStage::Tribal,
            GameStage::Civilization,
        ] {
            let pads = [(Vec2::new(0., 8.), 3.), (Vec2::ZERO, 4.)];
            let s = LandSurface::generate(stage.clone(), 0x1234_5678_9abc, &pads);
            let step = s.extent * 2. / CELLS as f32;
            let mut hist = [0usize; 10];
            let (mut max_h, mut land) = (f32::MIN, 0);
            for z in 0..CELLS {
                for x in 0..CELLS {
                    let h = s.heights[z * (CELLS + 1) + x];
                    if h < SEA_Y {
                        continue;
                    }
                    land += 1;
                    max_h = max_h.max(h);
                    let dx = (s.heights[z * (CELLS + 1) + x + 1] - h) / step;
                    let dz = (s.heights[(z + 1) * (CELLS + 1) + x] - h) / step;
                    let deg = (dx * dx + dz * dz).sqrt().atan().to_degrees();
                    hist[((deg / 6.) as usize).min(9)] += 1;
                }
            }
            println!("{stage:?} land {land} max_h {max_h:.2} slope hist (6deg bins) {hist:?}");
        }
    }
}
