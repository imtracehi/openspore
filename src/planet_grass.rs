//! Planet grass: the theme's grass ambient effect drawn with the retail grass
//! material (`notes/planet-look-notes.md` section 10, `planet-flora-notes.md`
//! F4). Creature, Tribal and Civilization stage planets.
//!
//! Original data / code (VERIFIED):
//! - The theme's `terrainScriptAmbientEffectSelect2` 0x040CF84F lists eleven
//!   grass effects (0x22375781 ... 0x852AA314, planet effdir
//!   0x40450200!0x5F848353 effects 227 .. 277). Each is a terrainDistribute
//!   with two levels, type -1 (any cell): level 4 (16 x 16 cells per cube
//!   face, distance 80, facing 0.6) -> distribute D526 (density 1000, square
//!   source 1.0 = the cell, sizes [0.8, 0.6, 0.7, 0.8, 0.9], colours 0.75 /
//!   0.8 / 0.7, alpha [0.255, 1], heading 0..1 turn, 30 clusters of 0.1278,
//!   emit map Grass 0x02C9C763 in [0.4, 10000], colour map Color 0x016F280B,
//!   material 0x52B2AD1C); level 3 (8 x 8 cells, distance 170) -> D529 (sizes
//!   [1.05, 2.0], 50 clusters of 0.0707, material 0xE27C98BD). The variants
//!   differ only in the strip texture (grass_<name>_strip, 64 x 512, 8 cells).
//! - Materials (effdir resources, shader 0xF0FB7DEE = grassVanilla.smt,
//!   builder grassVanilla 0x700000E4): customParams c0 = (startFade, endFade,
//!   tilt deg, startFadeIn), c1 = (startScale, endScale, scaleFactor,
//!   endFadeIn); 0x52B2AD1C = (10, 70, 15, 10) (100, 1000, 1, 13), 0xE27C98BD
//!   = (70, 120, 35, 20) (100, 1000, 1, 30).
//! - Shader (HLSL fragments 0x40212002): grassVanillaVS (size 1.5, fade by
//!   camera distance x (id 0.5 + 0.5), fake AO 1.25 - v^2 by 0.5 fade, scale
//!   by distance and fade, uv sway 0.1 sin(t + 10 w) (1 - v), terrain vertex
//!   lighting at the root, quad along the heading tangent and the normal
//!   tilted by id x tilt, strip cell floor(id 7.999) / 8) and grassVanillaPS
//!   (alpha > 0.3 mask, diffuse x c1 + c2, night luminance x tint, clip at
//!   64.5 / 255).
//! - Grass map sampler 0xF70FB0: live / dead >= 0.95, slope below
//!   mMinCliffGradient, above the water + beach level (0.025), plant map set.
//! - The cDistGrid cell test of 0xF66790 (as `planet_flora`).
//!
//! ADAPTED / INFERRED:
//! - INFERRED: blade sizes are world units (the particle size is not scaled
//!   by the cell); a cluster is a disc of clusterRadius x the cell half width
//!   around a uniform centre; the normal is the sphere direction; the sway
//!   phase uses the blade id (retail: the particle position w).
//! - ADAPTED: the variant is drawn from the planet seed (retail: the global
//!   RNG at load); the live / dead map is "live" everywhere (its table
//!   0xFB9E40 is not decoded); the plant map = land outside the stage's nests,
//!   villages, cities and geysers (as `planet_flora`); no trampling (the
//!   `tramp` creature list); the Color map is the land shader's ramp colour at
//!   the root elevation without its noise patches.
//!
//! Opt-in: `OPENSPORE_PLANET_GRASS=1` (frame cost, see `enabled`).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::dbpf::ResourceKey;
use crate::planet_query::{face_point, tangent_basis, Planet, StagePlanet};
use crate::stages::GameStage;

/// One terrainDistribute level of the grass effect.
#[derive(Clone, Copy, Debug)]
pub struct GrassLevel {
    /// Cells per cube-face edge (2^level).
    pub cells: usize,
    pub distance: f32,
    pub facing: f32,
    pub density: usize,
    pub sizes: &'static [f32],
    pub clusters: usize,
    pub cluster_radius: f32,
    /// Material customParams c0, c1.
    pub params: [[f32; 4]; 2],
}

pub const LEVELS: [GrassLevel; 2] = [
    GrassLevel {
        cells: 16,
        distance: 80.0,
        facing: 0.6,
        density: 1000,
        sizes: &[0.8, 0.6, 0.7, 0.8, 0.9],
        clusters: 30,
        cluster_radius: 0.1278,
        params: [[10.0, 70.0, 15.0, 10.0], [100.0, 1000.0, 1.0, 13.0]],
    },
    GrassLevel {
        cells: 8,
        distance: 170.0,
        facing: 0.6,
        density: 1000,
        sizes: &[1.05, 2.0],
        clusters: 50,
        cluster_radius: 0.07071,
        params: [[70.0, 120.0, 35.0, 20.0], [100.0, 1000.0, 1.0, 30.0]],
    },
];
/// D526 / D529 colour and alpha curves.
pub const COLORS: [f32; 3] = [0.75, 0.8, 0.7];
pub const ALPHAS: [f32; 2] = [0.255, 1.0];
/// cTerrainMapSet beach level above the water (h scale).
pub const BEACH_LEVEL: f32 = 0.025;
/// Planet config kMinCliffGradient (degrees, `world_catalog::PlanetMaterials`).
pub const MIN_CLIFF_GRADIENT: f32 = 32.0;
/// The eleven Select2 grass effects in theme order and their strip rasters.
pub const VARIANTS: [(u32, u32); 11] = [
    (0x22375781, 0xFFBF6D93), // grass_arizona_strip
    (0x0F2B4748, 0x9EB73060), // grass_claymation_strip
    (0xB3DA8BDB, 0x5DC051EB), // grass_conglomerate_strip
    (0x7512ADE2, 0x8BFA003C), // grass_coral_strip
    (0x4063120F, 0x959B3D41), // grass_crystal_strip
    (0xFBB290FF, 0xF78D2AA9), // grass_granite_strip
    (0x79157766, 0xE3D31A58), // grass_organic_strip
    (0x40690BA3, 0xBC85574F), // grass_shitaake_strip
    (0xDAA91DDC, 0x5DC75FEE), // grass_sandstone_strip
    (0x8A6735F2, 0x03600B8E), // grass_spikes_strip
    (0x852AA314, 0x3E4431AE), // grass_swiss_strip
];
/// Cell meshes kept after they leave the view.
const MESH_CACHE: usize = 96;

/// Opt-in for now (`OPENSPORE_PLANET_GRASS=1`): the first cut costs ~3 ms per
/// frame in the dev build (16.7 -> ~20 ms, Creature start view).
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OPENSPORE_PLANET_GRASS").is_ok_and(|v| !v.is_empty() && v != "0"))
}

/// Piecewise-linear curve over t in 0..1 (Swarm keyed curves).
fn curve(values: &[f32], t: f32) -> f32 {
    match values.len() {
        0 => 1.0,
        1 => values[0],
        n => {
            let x = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let i = (x.floor() as usize).min(n - 2);
            values[i] + (values[i + 1] - values[i]) * (x - i as f32)
        }
    }
}

/// The Grass effect map (0xF70FB0) at a direction: above the water + beach
/// level and below the cliff gradient (plant map / live-dead tested by the
/// caller).
pub fn grass_map(planet: &Planet, dir: Vec3) -> bool {
    if planet.h_at(dir) <= planet.water_level + BEACH_LEVEL {
        return false;
    }
    slope_degrees(planet, dir) < MIN_CLIFF_GRADIENT
}

/// Ground slope (degrees from level) by central differences over ~1 unit.
pub fn slope_degrees(planet: &Planet, dir: Vec3) -> f32 {
    let (t, b) = tangent_basis(dir);
    let step = 1.0 / planet.radius;
    let h = |d: Vec3| planet.height(d.normalize());
    let dx = (h(dir + t * step) - h(dir - t * step)) * 0.5;
    let dy = (h(dir + b * step) - h(dir - b * step)) * 0.5;
    (dx * dx + dy * dy).sqrt().atan().to_degrees()
}

/// A grid cell of one level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellKey {
    pub level: u8,
    pub face: u8,
    pub x: u16,
    pub y: u16,
}

impl CellKey {
    fn n(&self) -> usize {
        LEVELS[self.level as usize].cells
    }
    pub fn dir(&self) -> Vec3 {
        let n = self.n() as f32;
        face_point(self.face as usize, (self.x as f32 + 0.5) / n, (self.y as f32 + 0.5) / n).normalize()
    }
    /// Seed index of 0xF64240: n y + n^2 face + x.
    pub fn index(&self) -> u64 {
        let n = self.n() as u64;
        n * self.y as u64 + n * n * self.face as u64 + self.x as u64
    }
}

/// The cDistGrid activity test (0xF66790) for a cell of `level` with ground
/// radii `rmin..rmax`.
pub fn cell_active(level: &GrassLevel, dir: Vec3, rmin: f32, rmax: f32, camera: Vec3) -> bool {
    let r = camera.length().clamp(rmin, rmax);
    let to_cam = camera - dir * r;
    let d = to_cam.length();
    if d > level.distance {
        return false;
    }
    let facing = 0.5 - 0.5 * dir.dot(to_cam / d.max(1e-4));
    facing <= level.facing
}

/// One blade: root (planet-local), up, heading tangent, id, size, colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blade {
    pub root: Vec3,
    pub up: Vec3,
    pub tangent: Vec3,
    pub id: f32,
    pub size: f32,
    pub color: [f32; 4],
}

/// The blades of one cell, deterministic in `seed`. `ground(dir, radius)`
/// gives the Color map value (display, half scale); `clear` = clearings.
pub fn cell_blades(
    planet: &Planet,
    key: CellKey,
    seed: u64,
    clear: &[(Vec3, f32)],
    ground: &dyn Fn(Vec3, f32) -> [f32; 3],
) -> Vec<Blade> {
    use rand::{Rng, SeedableRng};
    let level = &LEVELS[key.level as usize];
    let n = level.cells as f32;
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(
        seed ^ 0x6A55_0000_0000_0000 ^ ((key.level as u64) << 40) ^ key.index().wrapping_mul(0x9E37_79B9_7F4A_7C15),
    );
    let (u0, v0) = (key.x as f32 / n, key.y as f32 / n);
    let half = 0.5 / n;
    let centres: Vec<(f32, f32)> = (0..level.clusters.max(1))
        .map(|_| (u0 + rng.random::<f32>() / n, v0 + rng.random::<f32>() / n))
        .collect();
    let mut out = Vec::with_capacity(level.density);
    for i in 0..level.density {
        // Draw every number first so filtering never shifts the others.
        let (a, r) = (rng.random::<f32>(), rng.random::<f32>());
        let (yaw, size_t, color_t, alpha_t, id) =
            (rng.random::<f32>(), rng.random::<f32>(), rng.random::<f32>(), rng.random::<f32>(), rng.random::<f32>());
        let (cu, cv) = centres[i % centres.len()];
        let angle = a * std::f32::consts::TAU;
        // Source units: the square source 1.0 spans the cell (half width = 1).
        let radius = r.sqrt() * level.cluster_radius * half;
        let (u, v) = (cu + angle.cos() * radius, cv + angle.sin() * radius);
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            continue;
        }
        let dir = face_point(key.face as usize, u, v).normalize();
        if !grass_map(planet, dir) {
            continue;
        }
        let height = planet.height(dir);
        let root = dir * height;
        if clear.iter().any(|(c, rad)| root.distance(*c) < *rad) {
            continue;
        }
        let (t, b) = tangent_basis(dir);
        let heading = yaw * std::f32::consts::TAU;
        let tangent = t * heading.cos() + b * heading.sin();
        let g = ground(dir, height);
        let c = curve(&COLORS, color_t);
        out.push(Blade {
            root,
            up: dir,
            tangent,
            id,
            size: curve(level.sizes, size_t),
            color: [g[0] * c, g[1] * c, g[2] * c, curve(&ALPHAS, alpha_t)],
        });
    }
    out
}

/// Four vertices per blade: position = root, normal = up, tangent = heading,
/// uv = corner, uv_b = (id, size), colour = particle colour (display).
pub fn blade_mesh(blades: &[Blade]) -> Mesh {
    let mut pos = Vec::with_capacity(blades.len() * 4);
    let mut nrm = Vec::with_capacity(blades.len() * 4);
    let mut tan = Vec::with_capacity(blades.len() * 4);
    let mut uv = Vec::with_capacity(blades.len() * 4);
    let mut uvb = Vec::with_capacity(blades.len() * 4);
    let mut col = Vec::with_capacity(blades.len() * 4);
    let mut idx = Vec::with_capacity(blades.len() * 6);
    for (i, b) in blades.iter().enumerate() {
        for corner in [[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]] {
            pos.push(b.root.to_array());
            nrm.push(b.up.to_array());
            tan.push([b.tangent.x, b.tangent.y, b.tangent.z, 1.0]);
            uv.push(corner);
            uvb.push([b.id, b.size]);
            col.push(b.color);
        }
        let k = (i * 4) as u32;
        idx.extend_from_slice(&[k, k + 1, k + 2, k, k + 2, k + 3]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tan);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uvb);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

/// The grassVanilla material.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct GrassMaterial {
    /// The stage planet's TerrainState (`planet_sky::SkyState`).
    #[uniform(0)]
    pub sky: crate::planet_sky::SkyState,
    /// customParams c0, c1.
    #[uniform(1)]
    pub params: [Vec4; 2],
    #[texture(2)]
    #[sampler(3)]
    pub strip: Handle<Image>,
}

impl Material for GrassMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://openspore/shaders/planet_grass.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/planet_grass.wesl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Mask(64.5 / 255.0)
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

#[derive(Component)]
pub struct PlanetGrassRoot;

struct CellLive {
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    blades: usize,
    last_used: u64,
}

#[derive(Resource)]
pub struct PlanetGrass {
    render_root: Entity,
    root: Entity,
    stage: GameStage,
    seed: u64,
    materials: [Handle<GrassMaterial>; 2],
    cells: HashMap<CellKey, CellLive>,
    radii: HashMap<CellKey, (f32, f32)>,
    clear: Vec<(Vec3, f32)>,
    last_camera: Option<Vec3>,
    frame: u64,
    ramp: Option<RampSampler>,
}

/// The land shader's Color map stand-in: its ramp texture, elevation range,
/// desaturation and albedo scale (`terrain.wesl`), times the retail diffuse
/// scale (sky slot 9 z).
#[derive(Clone)]
struct RampSampler {
    texels: Vec<[f32; 3]>,
    range: f32,
    desat: f32,
    albedo: f32,
    water_radius: f32,
}

impl RampSampler {
    fn sample(&self, height: f32, scale: f32) -> [f32; 3] {
        let control = ((height - self.water_radius) / self.range.max(1e-3)).clamp(0.004, 0.996);
        let x = control * (self.texels.len() - 1) as f32;
        let i = (x.floor() as usize).min(self.texels.len().saturating_sub(2));
        let t = x - i as f32;
        let lin: [f32; 3] = std::array::from_fn(|c| {
            let a = self.texels[i][c];
            let b = self.texels[(i + 1).min(self.texels.len() - 1)][c];
            a + (b - a) * t
        });
        let lum = lin[0] * 0.2126 + lin[1] * 0.7152 + lin[2] * 0.0722;
        std::array::from_fn(|c| ((lin[c] + (lum - lin[c]) * self.desat) * self.albedo).max(0.0).powf(1.0 / 2.2) * scale)
    }
}

fn ramp_sampler(world: &World, sp: &StagePlanet) -> Option<RampSampler> {
    let render = world.get_resource::<crate::planet_render::PlanetRender>()?;
    let material = world.get_resource::<Assets<crate::terrain::TerrainMaterial>>()?.get(&render.land_material)?;
    let image = world.get_resource::<Assets<Image>>()?.get(&material.extension.ramp_texture)?;
    let data = image.data.as_ref()?;
    let w = image.width() as usize;
    if w < 2 || data.len() < w * 4 {
        return None;
    }
    let srgb = |v: u8| Color::srgb_u8(v, 0, 0).to_linear().red;
    let texels = (0..w).map(|i| [srgb(data[i * 4]), srgb(data[i * 4 + 1]), srgb(data[i * 4 + 2])]).collect();
    let e = &material.extension;
    Some(RampSampler {
        texels,
        range: e.shaping.y,
        desat: e.grading.y,
        albedo: e.grading.z,
        water_radius: sp.planet.water_radius(),
    })
}

/// Where no grass grows (ADAPTED plant map, the same clearings as the
/// planet flora): nests / home, villages, cities and geysers.
fn clearings(world: &World, stage: &GameStage, sp: &StagePlanet) -> Vec<(Vec3, f32)> {
    let Some(run) = world.get_resource::<crate::game::RunProgress>() else {
        return Vec::new();
    };
    let local = |p: Vec3| {
        let world_point = if p.y >= sp.frame.centre.y { Vec3::new(p.x, sp.surface_y(p.x, p.z), p.z) } else { p };
        let l = sp.frame.to_local(world_point);
        l.normalize_or(Vec3::Y) * sp.planet.height(l)
    };
    let mut out = Vec::new();
    match stage {
        GameStage::Creature => {
            out.push((local(Vec3::from_array(run.creature_lifecycle.home)), 8.0));
            out.extend(run.species_world.nests.iter().map(|n| (local(Vec3::from_array(n.position)), 6.0)));
        }
        GameStage::Tribal => {
            let layout = crate::tribal_planet::current();
            out.push((local(Vec3::new(layout.origin.x, 0.0, layout.origin.y)), 14.0));
            out.extend(layout.villages.iter().map(|v| (local(*v), 12.0)));
        }
        GameStage::Civilization => {
            let campaign = &run.civ_campaign;
            out.extend(campaign.cities.iter().map(|c| (local(c.center()), 12.0 * crate::civ_planet::CITY_SCALE)));
            out.extend(
                (0..crate::civ::GEYSER_COUNT).map(|g| (local(campaign.geyser_at(g)), 4.0 * crate::civ_planet::CITY_SCALE)),
            );
        }
        _ => {}
    }
    out
}

fn strip_image(world: &World, instance: u32) -> Option<Image> {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let originals = world.get_resource::<crate::Originals>()?;
    let key = ResourceKey { group_id: 0, instance_id: instance, type_id: crate::asset_catalog::RW4 };
    let mips = crate::texture::decode_rw4_mips(&originals.resource_index.read(key).ok()?)?;
    let base = mips.first()?;
    // Display values (the retail shader works in gamma space).
    let mut image = Image::new(
        Extent3d { width: base.width, height: base.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        base.rgba.clone(),
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = mips.len() as u32;
    image.data = Some(mips.into_iter().flat_map(|m| m.rgba).collect());
    Some(image)
}

fn build(world: &mut World, render_root: Entity, sp: &StagePlanet, stage: GameStage) -> Option<PlanetGrass> {
    let seed = world.get_resource::<crate::terrain::PlanetCache>().map_or(0, |c| c.seed);
    let variant = VARIANTS[(seed % VARIANTS.len() as u64) as usize];
    let image = strip_image(world, variant.1)?;
    let strip = world.resource_mut::<Assets<Image>>().add(image);
    let sky = world
        .get_resource::<crate::planet_sky::PlanetSky>()
        .map_or([Vec4::ZERO; crate::planet_sky::SKY_SLOTS], |s| s.state);
    let materials = LEVELS.map(|level| {
        world.resource_mut::<Assets<GrassMaterial>>().add(GrassMaterial {
            sky,
            params: level.params.map(Vec4::from_array),
            strip: strip.clone(),
        })
    });
    let root = world
        .spawn((
            PlanetGrassRoot,
            Name::new("planet grass (grassVanilla)"),
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(render_root),
        ))
        .id();
    let clear = clearings(world, &stage, sp);
    info!("planet grass: variant {:08X} (strip {:08X}), {} clearings", variant.0, variant.1, clear.len());
    Some(PlanetGrass {
        render_root,
        root,
        stage,
        seed,
        materials,
        cells: HashMap::new(),
        radii: HashMap::new(),
        clear,
        last_camera: None,
        frame: 0,
        ramp: ramp_sampler(world, sp),
    })
}

/// Ground radius range of a cell (5 x 5 samples).
fn cell_radii(planet: &Planet, key: CellKey) -> (f32, f32) {
    let n = LEVELS[key.level as usize].cells as f32;
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for j in 0..5 {
        for i in 0..5 {
            let u = (key.x as f32 + i as f32 / 4.0) / n;
            let v = (key.y as f32 + j as f32 / 4.0) / n;
            let h = planet.height(face_point(key.face as usize, u, v).normalize());
            lo = lo.min(h);
            hi = hi.max(h);
        }
    }
    (lo, hi.max(planet.water_radius()))
}

/// Per frame (PostUpdate, after the planet renderer and the sky): activate
/// the cells near the camera, build their blade meshes, and hand the sky
/// state to the materials.
pub fn update(world: &mut World) {
    if !enabled() {
        return;
    }
    let Some((render_root, sp)) = world
        .get_resource::<crate::planet_render::PlanetRender>()
        .map(|r| (r.root, r.stage.clone()))
    else {
        world.remove_resource::<PlanetGrass>();
        return;
    };
    let Some(stage) = world.get_resource::<State<GameStage>>().map(|s| s.get().clone()) else {
        return;
    };
    if !matches!(stage, GameStage::Creature | GameStage::Tribal | GameStage::Civilization)
        || !world.contains_resource::<crate::planet_sky::PlanetSky>()
        || !world.contains_resource::<Assets<GrassMaterial>>()
    {
        return;
    }
    let stale = world
        .get_resource::<PlanetGrass>()
        .is_none_or(|g| g.render_root != render_root || g.stage != stage || world.get_entity(g.root).is_err());
    if stale {
        if let Some(old) = world.remove_resource::<PlanetGrass>() {
            if let Ok(e) = world.get_entity_mut(old.root) {
                e.despawn();
            }
        }
        match build(world, render_root, &sp, stage.clone()) {
            Some(g) => world.insert_resource(g),
            None => return,
        }
    }
    capture_view(world, &sp);
    let Some(camera) = world
        .query_filtered::<&Transform, With<crate::GameCamera>>()
        .iter(world)
        .next()
        .map(|t| t.translation)
    else {
        return;
    };
    let sky = world.resource::<crate::planet_sky::PlanetSky>().state;
    let diffuse_scale = if sky[9].z > 0.0 { sky[9].z } else { 0.5 };
    world.resource_scope(|world, mut grass: Mut<PlanetGrass>| {
        for handle in grass.materials.clone() {
            if let Some(mut m) = world.resource_mut::<Assets<GrassMaterial>>().get_mut(&handle) {
                m.sky = sky;
            }
        }
        if grass.ramp.is_none() {
            grass.ramp = ramp_sampler(world, &sp);
        }
        let local_camera = sp.frame.to_local(camera);
        if grass.last_camera.is_some_and(|c| c.distance_squared(local_camera) < 0.25) {
            return;
        }
        grass.last_camera = Some(local_camera);
        grass.frame += 1;
        let frame = grass.frame;
        // Active cells of both levels.
        let mut active = Vec::new();
        for (li, level) in LEVELS.iter().enumerate() {
            let n = level.cells;
            let cell_half = std::f32::consts::FRAC_PI_4 * sp.planet.radius / n as f32 * 1.5;
            for face in 0..6u8 {
                for y in 0..n as u16 {
                    for x in 0..n as u16 {
                        let key = CellKey { level: li as u8, face, x, y };
                        let dir = key.dir();
                        // Cheap reject before the exact test.
                        if (local_camera - dir * sp.planet.radius).length() - cell_half - sp.planet.altitude_range
                            > level.distance
                        {
                            continue;
                        }
                        let (rmin, rmax) = *grass.radii.entry(key).or_insert_with(|| cell_radii(&sp.planet, key));
                        if cell_active(level, dir, rmin, rmax, local_camera) {
                            active.push(key);
                        }
                    }
                }
            }
        }
        let ramp = grass.ramp.clone();
        let ground = move |_dir: Vec3, height: f32| -> [f32; 3] {
            ramp.as_ref().map_or([0.25, 0.35, 0.15], |r| r.sample(height, diffuse_scale))
        };
        for key in &active {
            let seed = grass.seed;
            let level = key.level as usize;
            let root = grass.root;
            let material = grass.materials[level].clone();
            let clear = grass.clear.clone();
            let live = grass.cells.entry(*key).or_insert(CellLive { entity: None, mesh: None, blades: 0, last_used: frame });
            live.last_used = frame;
            if live.entity.is_some() {
                continue;
            }
            if live.mesh.is_none() {
                let blades = cell_blades(&sp.planet, *key, seed, &clear, &ground);
                live.blades = blades.len();
                if !blades.is_empty() {
                    live.mesh = Some(world.resource_mut::<Assets<Mesh>>().add(blade_mesh(&blades)));
                }
            }
            if let Some(mesh) = live.mesh.clone() {
                let e = world
                    .spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                        bevy::light::NotShadowCaster,
                        ChildOf(root),
                    ))
                    .id();
                live.entity = Some(e);
            }
        }
        // Cells that left the view: despawn, keep a bounded mesh cache.
        let mut gone = Vec::new();
        for live in grass.cells.values_mut() {
            if live.last_used != frame {
                if let Some(e) = live.entity.take() {
                    gone.push(e);
                }
            }
        }
        for e in gone {
            if let Ok(e) = world.get_entity_mut(e) {
                e.despawn();
            }
        }
        if grass.cells.len() > MESH_CACHE {
            let mut idle: Vec<(CellKey, u64)> =
                grass.cells.iter().filter(|(_, l)| l.entity.is_none()).map(|(k, l)| (*k, l.last_used)).collect();
            idle.sort_by_key(|(_, f)| *f);
            let excess = grass.cells.len() - MESH_CACHE;
            for (k, _) in idle.into_iter().take(excess) {
                grass.cells.remove(&k);
            }
        }
        stats(&grass);
    });
}

/// `OPENSPORE_CAPTURE_VIEW=grass[,height,back]` (captures): the camera close
/// to the blade nearest the stage origin, looking at it.
fn capture_view(world: &mut World, sp: &StagePlanet) {
    let Ok(spec) = std::env::var("OPENSPORE_CAPTURE_VIEW") else {
        return;
    };
    let mut parts = spec.split(',');
    if parts.next() != Some("grass") {
        return;
    }
    let height: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1.6);
    let back: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(5.0);
    static TARGET: std::sync::OnceLock<Option<Vec3>> = std::sync::OnceLock::new();
    let Some(grass) = world.get_resource::<PlanetGrass>() else {
        return;
    };
    let (seed, clear) = (grass.seed, grass.clear.clone());
    let target = *TARGET.get_or_init(|| {
        let origin = sp.frame.dir_to_local(Vec3::Y);
        let (face, u, v) = crate::planet_query::face_uv(origin);
        let n = LEVELS[0].cells as f32;
        let mut best: Option<(f32, Vec3)> = None;
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let x = ((u * n).floor() as i32 + dx).clamp(0, n as i32 - 1) as u16;
                let y = ((v * n).floor() as i32 + dy).clamp(0, n as i32 - 1) as u16;
                let key = CellKey { level: 0, face: face as u8, x, y };
                for b in cell_blades(&sp.planet, key, seed, &clear, &|_, _| [0.3; 3]) {
                    let d = b.root.normalize().distance(origin);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, b.root));
                    }
                }
            }
        }
        best.map(|(_, p)| p)
    });
    let Some(target) = target else {
        return;
    };
    let up = target.normalize();
    let (t, _) = tangent_basis(up);
    let eye_dir = (target + t * back).normalize();
    let eye_local = eye_dir * (sp.planet.height(eye_dir) + height);
    let eye = sp.frame.to_world(eye_local);
    let look = sp.frame.to_world(target + up * 0.6);
    let eye_up = sp.frame.up(eye);
    let cameras: Vec<Entity> = world.query_filtered::<Entity, With<crate::GameCamera>>().iter(world).collect();
    for c in cameras {
        if let Some(mut tr) = world.get_mut::<Transform>(c) {
            *tr = Transform::from_translation(eye).looking_at(look, eye_up);
        }
    }
}

/// `OPENSPORE_GRASS_STATS=1`: drawn cells and blades when the set changes.
fn stats(grass: &PlanetGrass) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("OPENSPORE_GRASS_STATS").is_some()) {
        return;
    }
    let drawn: Vec<&CellLive> = grass.cells.values().filter(|l| l.entity.is_some()).collect();
    let blades: usize = drawn.iter().map(|l| l.blades).sum();
    eprintln!("grass stats: {} cells drawn, {} blades, {} cached", drawn.len(), blades, grass.cells.len());
}

/// Registered by `planet_render::build`.
pub fn plugin(app: &mut App) {
    bevy::asset::embedded_asset!(app, "shaders/planet_grass.wesl");
    crate::world_presentation::dev_shader_override(app, "planet_grass.wesl");
    app.add_plugins(MaterialPlugin::<GrassMaterial>::default());
    app.add_systems(
        PostUpdate,
        update
            .after(crate::planet_sky::update)
            .before(bevy::transform::TransformSystems::Propagate),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet_query::{h_to_raw, texel_dir, HeightCube, PLANET_RADIUS};

    fn land() -> Planet {
        let mut cube = HeightCube::filled(64, 0.0);
        for face in 0..6 {
            for row in 0..64 {
                for col in 0..64 {
                    let d = texel_dir(face, row, col, 64);
                    let i = cube.index(face, row, col);
                    cube.raw_mut()[i] = h_to_raw(if d.x > 0.0 { 0.2 } else { -0.3 });
                }
            }
        }
        Planet::new(cube, PLANET_RADIUS, 0.0)
    }

    #[test]
    fn cells_follow_the_distgrid_test() {
        let level = &LEVELS[0];
        let dir = Vec3::Y;
        let r = 500.0;
        assert!(cell_active(level, dir, r, r, Vec3::Y * (r + 20.0)));
        assert!(!cell_active(level, dir, r, r, Vec3::Y * (r + 120.0)));
        // Within the distance but seeing the cell from below its horizon:
        // facing = 0.5 + 0.5 x 60 / 116.6 = 0.76 > 0.6.
        assert!(!cell_active(&LEVELS[1], dir, r, r, Vec3::new(100.0, r - 60.0, 0.0)));
        // facing 0.59 (just inside).
        assert!(cell_active(&LEVELS[1], dir, r, r, Vec3::new(160.0, r - 30.0, 0.0)));
    }

    #[test]
    fn blades_grow_on_land_only_and_are_deterministic() {
        let planet = land();
        let key = CellKey { level: 0, face: 2, x: 8, y: 8 }; // +X face: land
        let ground = |_d: Vec3, _h: f32| [0.2, 0.3, 0.1];
        let a = cell_blades(&planet, key, 3, &[], &ground);
        let b = cell_blades(&planet, key, 3, &[], &ground);
        assert!(a.len() > 500, "{} blades", a.len());
        assert_eq!(a, b);
        let sea = CellKey { level: 0, face: 3, x: 8, y: 8 };
        assert!(cell_blades(&planet, sea, 3, &[], &ground).is_empty());
        assert!(a.iter().all(|b| (0.6..=0.9).contains(&b.size) && b.color[3] >= 0.255));
        let mesh = blade_mesh(&a);
        assert_eq!(mesh.count_vertices(), a.len() * 4);
    }

    #[test]
    fn curves_and_variants_are_the_effdir_values() {
        assert!((curve(LEVELS[0].sizes, 0.0) - 0.8).abs() < 1e-6);
        assert!((curve(LEVELS[1].sizes, 1.0) - 2.0).abs() < 1e-6);
        assert_eq!(VARIANTS.len(), 11);
        assert_eq!(LEVELS[0].params[0], [10.0, 70.0, 15.0, 10.0]);
    }
}
