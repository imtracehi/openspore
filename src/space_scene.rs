//! Solar-system presentation: the star with its glow, corona and flare, thin
//! orbit rings, sun-lit planets with a day and a night side, moons, gas
//! giants, an asteroid belt and the starry sky with the galactic band.
//!
//! Original rasters are used where the install has them (`solar_glow`,
//! `solar_corona`, `sg_star_twinkler2`, `ring_textureMap`, the galaxy map
//! rasters); every one has a generated stand-in so headless runs and partial
//! installs still draw a complete scene. Placement, lighting and the sky are
//! presentation adaptations, not Spore's native solar-system renderer.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::asset_catalog::{ResourceIndex, RW4};
use crate::dbpf::ResourceKey;
use crate::game::{Planet, Player};
use crate::space::{GasGiant, Moon, SystemBody};
use crate::space_galaxy::{BodyPlan, SystemPlan};
use crate::stages::StageOwned;

/// An installed raster looked up by its registered name.
fn named_raster(name: &str) -> ResourceKey {
    ResourceKey {
        group_id: 0,
        instance_id: crate::world_objects::name_id(name),
        type_id: RW4,
    }
}

/// One texture of the Space scene: the original raster when installed,
/// otherwise a generated stand-in.
#[derive(Clone, Debug)]
pub struct SpaceTexture {
    pub image: Handle<Image>,
    /// The original raster's key, or `None` for a generated stand-in.
    pub source: Option<ResourceKey>,
}

/// Rasters shared by the solar-system and galaxy views, built once.
#[derive(Resource, Clone, Debug)]
pub struct SpaceArt {
    pub gas_giant_bases: Vec<SpaceTexture>,
    pub glow: SpaceTexture,
    pub corona: SpaceTexture,
    pub flare: SpaceTexture,
    pub star_sprite: SpaceTexture,
    pub galaxy: Option<SpaceTexture>,
    pub nebulae: Option<SpaceTexture>,
    pub route: SpaceTexture,
    pub ring: SpaceTexture,
    /// Soft flat disc for territory tints and selection halos.
    pub disc: SpaceTexture,
    pub moon: Handle<Image>,
    pub sky: Handle<Image>,
}

fn image(width: u32, height: u32, rgba: Vec<u8>, repeat_u: bool) -> Image {
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let mode = if repeat_u {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// White texels whose alpha falls off from the centre: `(1 - r)^power`.
fn radial(size: u32, power: f32, rays: f32) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let half = size as f32 * 0.5;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5 - half) / half;
            let dy = (y as f32 + 0.5 - half) / half;
            let r = (dx * dx + dy * dy).sqrt();
            let mut a = (1.0 - r).max(0.0).powf(power);
            if rays > 0.0 {
                let angle = dy.atan2(dx);
                let spikes = (angle * 6.0).cos().abs().powf(18.0) * 0.6
                    + (angle * 11.0 + 0.7).cos().abs().powf(30.0) * 0.4;
                a = (a + spikes * rays * (1.0 - r).max(0.0).powf(1.6)).min(1.0);
            }
            out.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
        }
    }
    out
}

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Smooth value noise, wrapping horizontally every `period` cells.
fn value_noise(x: f32, y: f32, seed: u32, period: i32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let wrap = |v: i32| if period > 0 { v.rem_euclid(period) } else { v };
    let a = hash2(wrap(xi), yi, seed);
    let b = hash2(wrap(xi + 1), yi, seed);
    let c = hash2(wrap(xi), yi + 1, seed);
    let d = hash2(wrap(xi + 1), yi + 1, seed);
    let (u, v) = (s(fx), s(fy));
    (a + (b - a) * u) * (1.0 - v) + (c + (d - c) * u) * v
}

fn fbm(x: f32, y: f32, seed: u32, period: i32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * value_noise(x * freq, y * freq, seed + o * 17, period * freq as i32);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// Equirectangular night sky: black with a dusty, tinted galactic band.
fn sky_texture() -> Image {
    let (w, h) = (1024u32, 512u32);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    // The band's great circle is tilted so it crosses the downward-looking
    // system view diagonally.
    let pole = Vec3::new(0.35, 0.55, 0.76).normalize();
    for y in 0..h {
        let lat = (0.5 - (y as f32 + 0.5) / h as f32) * std::f32::consts::PI;
        for x in 0..w {
            let lon = (x as f32 + 0.5) / w as f32 * std::f32::consts::TAU;
            let dir = Vec3::new(lon.cos() * lat.cos(), lat.sin(), lon.sin() * lat.cos());
            let band = 1.0 - dir.dot(pole).abs();
            let core = band.powf(9.0);
            let u = x as f32 / w as f32 * 16.0;
            let v = y as f32 / h as f32 * 8.0;
            let dust = fbm(u, v, 7, 16, 5);
            let wisps = fbm(u * 2.3 + 3.0, v * 2.3, 31, 37, 4);
            let lanes = (fbm(u * 1.4, v * 1.4, 91, 22, 4) - 0.45).max(0.0) * 2.2;
            let density = (core * (0.55 + dust) - lanes * core).max(0.0);
            let glow = (density * 1.25).min(1.0);
            let nebula = ((wisps - 0.52).max(0.0) * 3.0).powf(1.6) * (0.25 + band.powf(4.0));
            // Warm core light, cool outer arms and magenta nebulae.
            let r = glow * 0.36 + nebula * 0.40;
            let g = glow * 0.30 + nebula * 0.10;
            let b = glow * 0.42 + nebula * 0.55;
            out.extend_from_slice(&[
                (r.min(1.0) * 255.0) as u8,
                (g.min(1.0) * 255.0) as u8,
                (b.min(1.0) * 255.0) as u8,
                255,
            ]);
        }
    }
    image(w, h, out, true)
}

/// A pale cratered moon surface.
fn moon_texture() -> Image {
    let (w, h) = (128u32, 64u32);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let n = fbm(x as f32 / 8.0, y as f32 / 8.0, 5, 16, 4);
            let craters = (fbm(x as f32 / 3.0, y as f32 / 3.0, 77, 43, 2) - 0.6).max(0.0) * 2.0;
            let v = (0.42 + n * 0.35 - craters * 0.3).clamp(0.0, 1.0);
            out.extend_from_slice(&[
                (v * 255.0) as u8,
                (v * 0.96 * 255.0) as u8,
                (v * 0.9 * 255.0) as u8,
                255,
            ]);
        }
    }
    image(w, h, out, true)
}

/// Horizontal cloud bands for a gas giant, in two related hues.
fn gas_giant_texture(seed: u32) -> Image {
    let (w, h) = (256u32, 128u32);
    let mut rng = ChaCha8Rng::seed_from_u64(seed as u64 ^ 0x6A5);
    let palettes: [[[f32; 3]; 2]; 5] = [
        [[0.86, 0.70, 0.50], [0.62, 0.40, 0.26]],
        [[0.55, 0.70, 0.86], [0.28, 0.38, 0.62]],
        [[0.82, 0.78, 0.62], [0.58, 0.52, 0.40]],
        [[0.78, 0.55, 0.72], [0.45, 0.28, 0.48]],
        [[0.62, 0.80, 0.70], [0.30, 0.48, 0.44]],
    ];
    let [light, dark] = palettes[rng.random_range(0..palettes.len())];
    let freq = rng.random_range(5.0..11.0);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = y as f32 / h as f32;
            let swirl = fbm(x as f32 / 16.0, y as f32 / 6.0, seed, 16, 4);
            let band = 0.5 + 0.5 * ((v * freq + swirl * 1.6) * std::f32::consts::TAU).sin();
            let t = (band * 0.8 + swirl * 0.2).clamp(0.0, 1.0);
            let c: Vec<u8> = (0..3)
                .map(|i| ((dark[i] + (light[i] - dark[i]) * t).clamp(0.0, 1.0) * 255.0) as u8)
                .collect();
            out.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    image(w, h, out, true)
}

/// Concentric translucent ring bands (used when `ring_textureMap` is absent).
fn ring_bands() -> Vec<u8> {
    let (w, h) = (256u32, 8u32);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..h {
        for x in 0..w {
            let t = x as f32 / w as f32;
            let a =
                (0.35 + 0.65 * value_noise(t * 40.0, 0.5, 3, 0)) * (t * (1.0 - t) * 4.0).min(1.0);
            out.extend_from_slice(&[230, 215, 190, (a * 200.0) as u8]);
        }
    }
    out
}

/// `SG_galaxy_arms_color`: the 32-bit colour map of the galaxy (header 20
/// bytes: big-endian width at 4 and height at 8; BGRA pixels). It spans the
/// same +-1000 rectangle as the star emit maps. Its flat grey background is
/// made transparent so it can be added under the stars.
fn colour_map(index: &ResourceIndex, images: &mut Assets<Image>) -> Option<SpaceTexture> {
    let key = crate::space_galaxy::GALAXY_ARMS_COLOR_MAP;
    let bytes = index.read(key).ok()?;
    if bytes.len() < 20 {
        return None;
    }
    let width = u32::from_be_bytes(bytes[4..8].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[8..12].try_into().ok()?);
    let count = (width as usize).checked_mul(height as usize)?;
    let data = bytes.get(20..20 + count * 4)?;
    let background = 0x2A as f32 / 255.0;
    let mut rgba = Vec::with_capacity(count * 4);
    for (i, px) in data.chunks_exact(4).enumerate() {
        let (b, g, r) = (px[0], px[1], px[2]);
        let luma = r.max(g).max(b) as f32 / 255.0;
        let keep = ((luma - background) / (1.0 - background)).clamp(0.0, 1.0);
        // The map's rim is a ring of rectangular dust tiles beyond the stars
        // (they lie past 700 units): fade it out.
        let (x, y) = ((i % width as usize) as f32, (i / width as usize) as f32);
        let r_norm = ((x + 0.5) / width as f32 - 0.5).hypot((y + 0.5) / height as f32 - 0.5) * 2.0;
        let rim = 1.0 - ((r_norm - 0.60) / 0.12).clamp(0.0, 1.0);
        rgba.extend_from_slice(&[r, g, b, (keep.sqrt() * rim * 255.0) as u8]);
    }
    Some(SpaceTexture {
        image: images.add(image(width, height, rgba, false)),
        source: Some(key),
    })
}

impl SpaceArt {
    /// Decode the installed rasters once; generate the rest.
    pub fn load(index: &ResourceIndex, images: &mut Assets<Image>) -> Self {
        let galaxy_map = colour_map(index, images);
        let mut original = |name: &str, repeat: bool| -> Option<SpaceTexture> {
            let key = named_raster(name);
            let bytes = index.read(key).ok()?;
            let mips = crate::texture::decode_rw4_mips(&bytes)
                .or_else(|| crate::texture::decode_rw4(&bytes).map(|base| vec![base]))?;
            let base = mips.first()?;
            let mut raster = image(base.width, base.height, base.rgba.clone(), repeat);
            raster.texture_descriptor.mip_level_count = mips.len() as u32;
            raster.data = Some(mips.into_iter().flat_map(|mip| mip.rgba).collect());
            Some(SpaceTexture {
                image: images.add(raster),
                source: Some(key),
            })
        };
        let glow = original("solar_glow", false);
        let corona = original("solar_corona", false);
        let flare = original("sg_star_twinkler2", false);
        let star_sprite = original("sg_star_single", false);
        let galaxy = galaxy_map.or_else(|| original("SG_galaxy_arms_colored2", false));
        let nebulae = original("sg_galaxy_nebulaes_topdown", false);
        let route = original("sg_galaxy_travel_line", true);
        let ring = original("ring_textureMap", false);
        let gas_giant_bases = [
            "sg_gasgiant_deeppurple_base",
            "sg_gasgiant_icewater_base",
            "sg_gasgiant_quasiuranus_base",
            "sg_gasgiant_quasijupiter_base",
            "sg_gasgiant_liquidmars_base",
        ]
        .into_iter()
        .filter_map(|name| original(name, true))
        .collect();
        let mut generated = |rgba: Vec<u8>, w: u32, h: u32| SpaceTexture {
            image: images.add(image(w, h, rgba, false)),
            source: None,
        };
        let glow = glow.unwrap_or_else(|| generated(radial(128, 2.4, 0.0), 128, 128));
        let corona = corona.unwrap_or_else(|| generated(radial(256, 3.0, 1.0), 256, 256));
        let flare = flare.unwrap_or_else(|| generated(radial(128, 4.0, 1.4), 128, 128));
        let star_sprite = star_sprite.unwrap_or_else(|| generated(radial(64, 2.0, 0.0), 64, 64));
        let route = route.unwrap_or_else(|| generated(radial(32, 1.0, 0.0), 32, 32));
        let ring = ring.unwrap_or_else(|| generated(ring_bands(), 256, 8));
        let disc = generated(radial(64, 1.3, 0.0), 64, 64);
        Self {
            gas_giant_bases,
            glow,
            corona,
            flare,
            star_sprite,
            galaxy,
            nebulae,
            route,
            ring,
            disc,
            moon: images.add(moon_texture()),
            sky: images.add(sky_texture()),
        }
    }
}

/// Faces the camera every frame, optionally spinning about the view axis.
#[derive(Component, Clone, Copy)]
pub struct Billboard {
    pub spin: f32,
    pub phase: f32,
}

/// Camera-centred sky (dome and star points) for the solar-system view.
#[derive(Component)]
pub struct SkyLayer;

/// A thin orbit ring, tinted by the state of the world it traces.
#[derive(Component)]
pub struct OrbitRing {
    pub body: Entity,
}

/// Shared orbit-ring materials by world state.
#[derive(Resource, Clone)]
pub struct OrbitRingMaterials {
    pub plain: Handle<StandardMaterial>,
    pub surveyed: Handle<StandardMaterial>,
    pub owned: Handle<StandardMaterial>,
}

/// Stellar light for one body: a spotlight on the star side aimed at the
/// body, so every world shows a lit day side and a dark night side at the
/// same brightness whatever its orbit (an inverse-square point light at the
/// star would blow out inner worlds and darken outer ones).
#[derive(Component)]
pub struct Sunlight {
    pub body: Entity,
    /// Radius the cone must cover around the body (its moons included).
    pub cover: f32,
}

/// The ship's share of starlight, aimed at it from the star's side.
#[derive(Component)]
pub struct ShipSunlight;

/// Distance of each body's sunlight from the body.
const SUNLIGHT_DISTANCE: f32 = 70.0;
/// Illuminance delivered to sunlit surfaces (lux at the target).
const SUNLIGHT_LUX: f32 = 11_000.0;

fn sunlight_intensity(distance: f32, lux: f32) -> f32 {
    lux * 4.0 * std::f32::consts::PI * distance * distance
}

/// A flat annulus in the XZ plane, `segments` quads around.
pub fn annulus(inner: f32, outer: f32, segments: u32) -> Mesh {
    let mut positions = Vec::with_capacity(segments as usize * 2 + 2);
    let mut uvs = Vec::with_capacity(segments as usize * 2 + 2);
    let mut normals = Vec::with_capacity(segments as usize * 2 + 2);
    let mut indices = Vec::with_capacity(segments as usize * 6);
    for i in 0..=segments {
        let a = i as f32 / segments as f32 * std::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        positions.push([c * inner, 0.0, s * inner]);
        positions.push([c * outer, 0.0, s * outer]);
        uvs.push([0.0, i as f32 / segments as f32]);
        uvs.push([1.0, i as f32 / segments as f32]);
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        if i < segments {
            let b = i * 2;
            indices.extend_from_slice(&[b, b + 1, b + 2, b + 1, b + 3, b + 2]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn additive(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
    texture: Option<Handle<Image>>,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        base_color_texture: texture,
        alpha_mode: AlphaMode::Add,
        unlit: true,
        fog_enabled: false,
        cull_mode: None,
        ..default()
    })
}

/// A small low-poly rock: an octahedron with jittered corners.
fn push_rock(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    centre: Vec3,
    size: f32,
    rng: &mut impl Rng,
) {
    let corners = [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
    ]
    .map(|c| c * size * rng.random_range(0.6..1.3));
    let faces = [
        (0, 2, 4),
        (4, 2, 1),
        (1, 2, 5),
        (5, 2, 0),
        (4, 3, 0),
        (1, 3, 4),
        (5, 3, 1),
        (0, 3, 5),
    ];
    for (a, b, c) in faces {
        let (pa, pb, pc) = (corners[a], corners[b], corners[c]);
        let n = (pb - pa).cross(pc - pa).normalize_or(Vec3::Y);
        let base = positions.len() as u32;
        for p in [pa, pb, pc] {
            positions.push((centre + p).to_array());
            normals.push(n.to_array());
        }
        indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
}

/// Every procedural piece of a star system that needs no per-body entity:
/// the star (sphere, glow, corona, flare), orbit rings, the asteroid belt,
/// sunlight for each body and for the ship.
pub fn spawn_system_visuals(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    art: Option<&SpaceArt>,
    plan: &SystemPlan,
    bodies: &[(Entity, BodyPlan)],
) {
    let kind = plan.kind;
    let [r, g, b] = kind.color();
    let star_radius = plan.star_radius;
    // Bevy ignores emissive on unlit materials. Keep the lit path so the
    // star's own emission reaches the HDR output beneath the original glows.
    let black_hole = kind == crate::space_galaxy::StarKind::BlackHole;
    let core = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: if black_hole {
            LinearRgba::BLACK
        } else {
            LinearRgba::rgb(r * 20.0, g * 18.0, b * 16.0)
        },
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        unlit: false,
        fog_enabled: false,
        ..default()
    });
    commands.spawn((
        StageOwned,
        SystemBody,
        NotShadowCaster,
        Name::new("star core"),
        Mesh3d(meshes.add(Sphere::new(star_radius).mesh().uv(48, 24))),
        MeshMaterial3d(core),
        Transform::IDENTITY,
    ));
    if black_hole {
        // The accretion disk of a black hole: a hot ring around the dark sphere.
        commands.spawn((
            StageOwned,
            SystemBody,
            NotShadowCaster,
            NotShadowReceiver,
            Name::new("accretion disk"),
            Mesh3d(meshes.add(annulus(star_radius * 1.4, star_radius * 3.2, 128))),
            MeshMaterial3d(additive(
                materials,
                Color::srgba(r * 1.2, g * 0.9, b * 1.1, 0.9),
                art.map(|a| a.ring.image.clone()),
            )),
            Transform::from_rotation(Quat::from_rotation_x(0.25)),
        ));
    }
    if let Some(companion) = plan.companion {
        // The second star of a binary system, beside the first.
        let [cr, cg, cb] = companion.kind.color();
        commands.spawn((
            StageOwned,
            SystemBody,
            NotShadowCaster,
            Name::new("companion star"),
            Mesh3d(meshes.add(Sphere::new(companion.radius).mesh().uv(32, 16))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: LinearRgba::rgb(cr * 20.0, cg * 18.0, cb * 16.0),
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                fog_enabled: false,
                ..default()
            })),
            Transform::from_xyz(companion.offset, 0.0, 0.0),
        ));
        commands.spawn((
            StageOwned,
            SystemBody,
            NotShadowCaster,
            NotShadowReceiver,
            Billboard {
                spin: 0.0,
                phase: 0.0,
            },
            Name::new("companion glow"),
            Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
            MeshMaterial3d(additive(
                materials,
                Color::srgba(cr, cg, cb, 0.85),
                art.map(|a| a.glow.image.clone()),
            )),
            Transform::from_xyz(companion.offset, 0.0, 0.0)
                .with_scale(Vec3::splat(companion.radius * 14.0)),
        ));
    }
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    let layers: [(Option<&SpaceTexture>, f32, f32, f32, [f32; 3], f32); 4] = [
        // texture, size (star radii), spin, phase, colour mix, strength
        (art.map(|a| &a.glow), 16.0, 0.0, 0.0, [r, g, b], 0.85),
        (art.map(|a| &a.glow), 6.0, 0.0, 0.0, [1.0, 0.95, 0.85], 1.0),
        (art.map(|a| &a.corona), 7.5, 0.02, 0.4, [r, g, b], 0.9),
        (
            art.map(|a| &a.flare),
            13.0,
            0.0,
            0.0,
            [r * 0.6 + 0.4, g * 0.6 + 0.4, b * 0.6 + 0.4],
            0.55,
        ),
    ];
    for (texture, size, spin, phase, color, strength) in layers {
        if black_hole {
            // No corona or flare around a black hole: its disk is the light.
            continue;
        }
        let material = additive(
            materials,
            Color::srgba(color[0], color[1], color[2], strength),
            texture.map(|t| t.image.clone()),
        );
        commands.spawn((
            StageOwned,
            SystemBody,
            NotShadowCaster,
            NotShadowReceiver,
            Billboard { spin, phase },
            Name::new("star glow"),
            Mesh3d(quad.clone()),
            MeshMaterial3d(material),
            Transform::from_scale(Vec3::splat(star_radius * size)),
        ));
    }
    // Orbit rings, faint and thin as on Spore's system map.
    let rings = OrbitRingMaterials {
        plain: additive(materials, Color::srgba(0.42, 0.55, 0.78, 0.30), None),
        surveyed: additive(materials, Color::srgba(0.90, 0.75, 0.35, 0.38), None),
        owned: additive(materials, Color::srgba(0.35, 0.95, 0.55, 0.42), None),
    };
    for (entity, body) in bodies {
        let width = 0.10 + body.orbit * 0.0016;
        commands.spawn((
            StageOwned,
            SystemBody,
            NotShadowCaster,
            NotShadowReceiver,
            OrbitRing { body: *entity },
            Mesh3d(meshes.add(annulus(body.orbit - width, body.orbit + width, 256))),
            MeshMaterial3d(rings.plain.clone()),
            Transform::IDENTITY,
        ));
        let cover = body.radius * 1.2
            + if body.moons > 0 {
                body.radius * 1.9 + 2.2 + body.moons as f32 * 2.1 + 1.0
            } else {
                0.0
            };
        let [lr, lg, lb] = kind.light();
        commands.spawn((
            StageOwned,
            SystemBody,
            Sunlight {
                body: *entity,
                cover,
            },
            SpotLight {
                color: Color::srgb(lr, lg, lb),
                intensity: sunlight_intensity(SUNLIGHT_DISTANCE, SUNLIGHT_LUX),
                range: SUNLIGHT_DISTANCE + cover * 2.0 + 6.0,
                radius: 0.0,
                shadow_maps_enabled: false,
                inner_angle: (cover * 0.8 / SUNLIGHT_DISTANCE).atan(),
                outer_angle: (cover * 1.15 / SUNLIGHT_DISTANCE).atan(),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
    }
    commands.insert_resource(rings);
    let [lr, lg, lb] = kind.light();
    commands.spawn((
        StageOwned,
        SystemBody,
        ShipSunlight,
        SpotLight {
            color: Color::srgb(lr, lg, lb),
            intensity: sunlight_intensity(6.0, SUNLIGHT_LUX * 0.8),
            range: 9.0,
            radius: 0.0,
            shadow_maps_enabled: false,
            inner_angle: 0.35,
            outer_angle: 0.55,
            ..default()
        },
        Transform::IDENTITY,
    ));
    // Asteroid belt: one mesh of a few hundred tumbling rocks.
    for (belt_index, belt) in plan.belts.iter().copied().enumerate() {
        let mut rng = ChaCha8Rng::seed_from_u64(plan.star as u64 ^ 0xA57E ^ ((belt_index as u64) << 32));
        let (mut positions, mut normals, mut indices) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..420 {
            let a = rng.random_range(0.0..std::f32::consts::TAU);
            let d = belt + rng.random_range(-2.6f32..2.6) * rng.random_range(0.3f32..1.0);
            let centre = Vec3::new(a.cos() * d, rng.random_range(-0.6..0.6), a.sin() * d);
            let size = rng.random_range(0.08f32..0.42).powf(1.4) + 0.06;
            push_rock(
                &mut positions,
                &mut normals,
                &mut indices,
                centre,
                size,
                &mut rng,
            );
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_indices(Indices::U32(indices));
        commands.spawn((
            StageOwned,
            SystemBody,
            NotShadowCaster,
            AsteroidBelt {
                speed: crate::space_galaxy::orbit_speed(belt),
                radius: belt,
                seed: plan.star.wrapping_add(belt_index as u32),
            },
            Name::new("asteroid belt"),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.55, 0.48, 0.42),
                perceptual_roughness: 1.0,
                // Rocks are too small to catch a body's sunlight cone.
                emissive: LinearRgba::rgb(0.05, 0.04, 0.035),
                ..default()
            })),
            Transform::IDENTITY,
        ));
    }
}

/// The belt's slow rotation.
#[derive(Component)]
pub struct AsteroidBelt {
    pub speed: f32,
    pub radius: f32,
    pub seed: u32,
}

/// Exact installed base raster on adapted UV-sphere geometry. Native cloud,
/// scattering, UV distortion and full gas-giant shader execution remain open.
#[derive(Component, Clone)]
pub struct OriginalGasGiantArt {
    pub source: ResourceKey,
    pub image: Handle<Image>,
}

fn gas_giant_mesh(radius: f32) -> Mesh {
    // Bevy's UV sphere has its latitude poles on Z. Match this world's Y-up
    // axis without rotating the orbital actor or its independent ring plane.
    Sphere::new(radius)
        .mesh()
        .uv(64, 32)
        .rotated_by(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
}

/// Dressing for the bodies the star-system spawner leaves bare: gas giants
/// (banded sphere and ring) and moons. Runs once per new body.
pub fn dress_bodies(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    art: Option<Res<SpaceArt>>,
    giants: Query<(Entity, &GasGiant), Without<Mesh3d>>,
    moons: Query<Entity, (With<Moon>, Without<Mesh3d>)>,
    mut shared_moon: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    for (entity, giant) in &giants {
        let native = art.as_ref().and_then(|art| {
            let count = art.gas_giant_bases.len();
            (count > 0).then(|| art.gas_giant_bases[giant.seed as usize % count].clone())
        });
        let texture = native
            .as_ref()
            .map(|art| art.image.clone())
            .unwrap_or_else(|| images.add(gas_giant_texture(giant.seed)));
        commands.entity(entity).insert((
            Mesh3d(meshes.add(gas_giant_mesh(giant.radius))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(texture),
                perceptual_roughness: 0.85,
                reflectance: 0.05,
                ..default()
            })),
        ));
        if let Some(SpaceTexture {
            image,
            source: Some(source),
        }) = native
        {
            commands
                .entity(entity)
                .insert(OriginalGasGiantArt { source, image });
        }
        if giant.ring {
            let ring = art.as_ref().map(|a| a.ring.image.clone());
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(1.0, 0.95, 0.85, 0.85),
                base_color_texture: ring,
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 1.0,
                cull_mode: None,
                double_sided: true,
                ..default()
            });
            let mesh = meshes.add(annulus(giant.radius * 1.35, giant.radius * 2.3, 128));
            commands.entity(entity).with_children(|p| {
                p.spawn((
                    StageOwned,
                    NotShadowCaster,
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_rotation(
                        Quat::from_rotation_x(0.42) * Quat::from_rotation_z(0.2),
                    ),
                ));
            });
        }
    }
    if moons.is_empty() {
        return;
    }
    let (mesh, material) = shared_moon
        .get_or_insert_with(|| {
            (
                meshes.add(Sphere::new(0.9).mesh().uv(32, 16)),
                materials.add(StandardMaterial {
                    base_color_texture: art.as_ref().map(|a| a.moon.clone()),
                    perceptual_roughness: 1.0,
                    reflectance: 0.02,
                    ..default()
                }),
            )
        })
        .clone();
    for entity in &moons {
        commands
            .entity(entity)
            .insert((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone())));
    }
}

/// Camera-centred sky: a dome carrying the galactic band and a mesh of star
/// points, both following the camera so the sky never parallaxes.
pub fn spawn_sky(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    art: Option<&SpaceArt>,
) {
    let mut dome = Sphere::new(460.0).mesh().uv(64, 32);
    // Seen from inside: flip the winding so the inner face renders.
    if let Some(Indices::U32(indices)) = dome.indices_mut() {
        for tri in indices.chunks_exact_mut(3) {
            tri.swap(1, 2);
        }
    }
    commands.spawn((
        StageOwned,
        SkyLayer,
        NotShadowCaster,
        NotShadowReceiver,
        Name::new("space sky"),
        Mesh3d(meshes.add(dome)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: art.map(|a| a.sky.clone()),
            unlit: true,
            fog_enabled: false,
            ..default()
        })),
        Transform::IDENTITY,
    ));
    // Star points: small camera-facing quads on a sphere inside the dome.
    let mut rng = ChaCha8Rng::seed_from_u64(0x57A2);
    let (mut positions, mut colors, mut uvs, mut indices) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let pole = Vec3::new(0.35, 0.55, 0.76).normalize();
    let mut placed = 0;
    while placed < 2600 {
        let dir = Vec3::new(
            rng.random_range(-1.0f32..1.0),
            rng.random_range(-1.0f32..1.0),
            rng.random_range(-1.0f32..1.0),
        );
        if dir.length_squared() > 1.0 || dir.length_squared() < 0.01 {
            continue;
        }
        let dir = dir.normalize();
        // Denser toward the galactic band.
        let band = 1.0 - dir.dot(pole).abs();
        if rng.random::<f32>() > 0.25 + band.powf(5.0) * 0.75 {
            continue;
        }
        placed += 1;
        let centre = dir * 440.0;
        let bright = rng.random::<f32>().powf(5.0);
        let size = 0.55 + bright * 1.9;
        let right = dir.cross(Vec3::Y).normalize_or(Vec3::X) * size;
        let up = right.cross(dir).normalize() * size;
        let base = positions.len() as u32;
        for (corner, uv) in [
            (-right - up, [0.0, 1.0]),
            (right - up, [1.0, 1.0]),
            (right + up, [1.0, 0.0]),
            (-right + up, [0.0, 0.0]),
        ] {
            positions.push((centre + corner).to_array());
            uvs.push(uv);
        }
        let tint = match rng.random_range(0..10) {
            0..=1 => [1.0, 0.82, 0.62],
            2 => [0.68, 0.78, 1.0],
            3 => [1.0, 0.65, 0.55],
            _ => [0.92, 0.94, 1.0],
        };
        let level = 0.35 + bright * 0.65;
        for _ in 0..4 {
            colors.push([tint[0] * level, tint[1] * level, tint[2] * level, 1.0]);
        }
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    commands.spawn((
        StageOwned,
        SkyLayer,
        NotShadowCaster,
        NotShadowReceiver,
        Name::new("space star points"),
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(additive(
            materials,
            Color::WHITE,
            art.map(|a| a.star_sprite.image.clone()),
        )),
        Transform::IDENTITY,
    ));
}

/// Entities `present` only reads: everything but the pieces it moves.
type Static = (
    Without<Billboard>,
    Without<SkyLayer>,
    Without<Sunlight>,
    Without<ShipSunlight>,
    Without<AsteroidBelt>,
);
/// Pieces `present` moves never include the camera or the ship.
type Fixed = (Without<crate::GameCamera>, Without<Player>);

/// Per-frame presentation of the system scene: billboards face the camera,
/// sunlight tracks each body, orbit rings follow world state, the sky
/// follows the camera and hides on the galaxy map.
pub fn present(
    time: Res<Time>,
    view: Option<Res<crate::space_view::SpaceView>>,
    rings: Option<Res<OrbitRingMaterials>>,
    camera: Query<&Transform, (With<crate::GameCamera>, Static)>,
    bodies: Query<(&Transform, Option<&Planet>), Static>,
    player: Query<&Transform, (With<Player>, Static)>,
    mut billboards: Query<(&mut Transform, &Billboard), Fixed>,
    mut sky: Query<(&mut Transform, &mut Visibility), (With<SkyLayer>, Without<Billboard>, Fixed)>,
    mut lights: Query<(&mut Transform, &Sunlight), (Without<Billboard>, Without<SkyLayer>, Fixed)>,
    mut ship_light: Query<
        (&mut Transform, &mut SpotLight),
        (
            With<ShipSunlight>,
            Without<Sunlight>,
            Without<Billboard>,
            Without<SkyLayer>,
            Fixed,
        ),
    >,
    mut ring_materials: Query<(&OrbitRing, &mut MeshMaterial3d<StandardMaterial>)>,
    mut belts: Query<
        (&AsteroidBelt, &mut Transform),
        (
            Without<ShipSunlight>,
            Without<Sunlight>,
            Without<Billboard>,
            Without<SkyLayer>,
            Fixed,
        ),
    >,
) {
    let galaxy = view.as_ref().is_some_and(|v| v.galaxy);
    let Ok(camera) = camera.single() else {
        return;
    };
    let t = time.elapsed_secs();
    for (mut transform, board) in &mut billboards {
        transform.rotation = camera.rotation * Quat::from_rotation_z(board.phase + board.spin * t);
    }
    for (mut transform, mut visibility) in &mut sky {
        transform.translation = camera.translation;
        let want = if galaxy {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != want {
            *visibility = want;
        }
    }
    // Every system's star sits at the origin.
    let star_at = Vec3::ZERO;
    for (mut transform, light) in &mut lights {
        let Ok((body, _)) = bodies.get(light.body) else {
            continue;
        };
        let to_star = (star_at - body.translation).normalize_or(Vec3::Y);
        transform.translation = body.translation + to_star * SUNLIGHT_DISTANCE;
        transform.look_at(body.translation, Vec3::Y);
    }
    if let (Ok(ship), Ok((mut light, mut spot))) = (player.single(), ship_light.single_mut()) {
        let to_star = (star_at - ship.translation).normalize_or(Vec3::Y);
        light.translation = ship.translation + to_star * 6.0;
        light.look_at(ship.translation, Vec3::Y);
        // Over a world the ship is inside the world's own sunlight; its 6-unit
        // cone would light a bright disc on the surface under it.
        let over_planet = view.as_ref().is_some_and(|v| v.planet.is_some());
        let want = if over_planet {
            0.0
        } else {
            sunlight_intensity(6.0, SUNLIGHT_LUX * 0.8)
        };
        if spot.intensity != want {
            spot.intensity = want;
        }
    }
    if let Some(rings) = rings {
        for (ring, mut material) in &mut ring_materials {
            let want = match bodies.get(ring.body) {
                Ok((_, Some(p))) if p.colonized => &rings.owned,
                Ok((_, Some(p))) if p.surveyed => &rings.surveyed,
                _ => &rings.plain,
            };
            if material.0 != *want {
                material.0 = want.clone();
            }
        }
    }
    for (belt, mut transform) in &mut belts {
        transform.rotation = Quat::from_rotation_y(-belt.speed * t);
    }
}

/// Screen-space name tag under a world, as Spore labels the planets of the
/// system you fly through.
#[derive(Component)]
pub struct PlanetLabel {
    pub body: Entity,
}

/// One tag per campaign world: name and state, the nearest world in reach
/// also naming its next action. Hidden on the galaxy map and off screen.
pub fn planet_labels(
    mut commands: Commands,
    view: Option<Res<crate::space_view::SpaceView>>,
    run: Res<crate::game::RunProgress>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    planets: Query<(Entity, &Transform, &Planet, &crate::persistence::WorldId)>,
    player: Query<&Transform, (With<Player>, Without<Planet>)>,
    mut labels: Query<(Entity, &PlanetLabel, &mut Node, &mut Text, &mut TextColor)>,
) {
    let galaxy = view.as_ref().is_some_and(|v| v.galaxy);
    let zoom = view.as_ref().map(|v| v.zoom());
    let Ok((camera, at)) = cameras.single() else {
        return;
    };
    let ship = player.single().map(|t| t.translation).ok();
    // The world the ship can act on right now, if any.
    let nearest = ship.and_then(|s| {
        planets
            .iter()
            .filter(|(_, t, p, _)| {
                t.translation.distance(s) < p.radius + crate::space::action_range()
            })
            .min_by(|a, b| {
                a.1.translation
                    .distance(s)
                    .total_cmp(&b.1.translation.distance(s))
            })
            .map(|(e, ..)| e)
    });
    let mut labelled = Vec::new();
    for (entity, label, mut node, mut text, mut color) in &mut labels {
        let Ok((body, t, planet, id)) = planets.get(label.body) else {
            commands.entity(entity).despawn();
            continue;
        };
        labelled.push(body);
        let near = nearest == Some(body);
        // Over a planet only the world beneath the ship keeps its tag.
        let shown = !galaxy && (near || zoom != Some(crate::space_view::SpaceZoom::Planet));
        let anchor = t.translation - Vec3::Y * (planet.radius * 1.15 + 0.6);
        let screen = shown
            .then(|| camera.world_to_viewport(at, anchor).ok())
            .flatten();
        let display = if screen.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let Some(screen) = screen else {
            continue;
        };
        node.left = Val::Px(screen.x - 90.0);
        node.top = Val::Px(screen.y);
        let campaign = &run.space_campaign;
        let spice = crate::space::SpiceColor::for_world(id.0);
        let empire_opt = campaign.world_empire(id.0);
        let ecology = campaign.ecology_for(id.0);

        let state = if id.0 == 0 {
            "homeworld".to_string()
        } else if planet.colonized {
            format!("colony [T{}] • {}", ecology.t_score(), spice.label())
        } else if let Some(e) = empire_opt {
            let rel = campaign.relation_label(e);
            format!(
                "{} empire [{rel}] • {}",
                crate::space_galaxy::EMPIRES[e as usize].0,
                spice.label()
            )
        } else if campaign.terraformed.contains(&id.0) {
            format!("terraformed [T{}] • {}", ecology.t_score(), spice.label())
        } else if planet.surveyed {
            format!("surveyed • {}", spice.label())
        } else {
            "unexplored".to_string()
        };

        let hint = if !near {
            ""
        } else if planet.colonized {
            " • docked • B spice"
        } else if empire_opt.is_some() {
            " • F attack • K comms • C conquer"
        } else if !planet.surveyed {
            " • V scan"
        } else if !campaign.terraformed.contains(&id.0) {
            " • T tool • Y use"
        } else {
            " • C colonize"
        };
        let value = format!("{}\n{state}{hint}", planet.name);
        if text.0 != value {
            text.0 = value;
        }
        let want = if planet.colonized || id.0 == 0 {
            Color::srgb(0.55, 1.0, 0.65)
        } else if empire_opt.is_some() {
            Color::srgb(1.0, 0.45, 0.35)
        } else if planet.surveyed {
            Color::srgb(1.0, 0.86, 0.5)
        } else {
            Color::srgb(0.78, 0.86, 1.0)
        };
        if color.0 != want {
            color.0 = want;
        }
    }
    for (entity, _, _, _) in &planets {
        if labelled.contains(&entity) {
            continue;
        }
        commands.spawn((
            StageOwned,
            PlanetLabel { body: entity },
            Text::new(""),
            TextFont {
                font_size: bevy::text::FontSize::Px(12.0),
                ..default()
            },
            TextColor(Color::WHITE),
            TextShadow {
                offset: Vec2::splat(1.5),
                color: Color::linear_rgba(0.0, 0.0, 0.0, 0.85),
            },
            TextLayout::justify(Justify::Center),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(180.0),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_galaxy::StarKind;

    #[test]
    fn gas_giant_latitudes_follow_world_up() {
        use bevy::mesh::VertexAttributeValues;
        let mesh = gas_giant_mesh(3.0);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions")
        };
        let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("UVs")
        };
        for (position, uv) in positions.iter().zip(uvs) {
            let latitude_height = 3.0 * (std::f32::consts::PI * uv[1]).cos();
            assert!((position[1] - latitude_height).abs() < 1e-5);
            assert!((Vec3::from_array(*position).length() - 3.0).abs() < 1e-5);
        }
    }

    #[test]
    fn generated_stand_ins_cover_every_space_texture() {
        let mut images = Assets::<Image>::default();
        let art = SpaceArt::load(&ResourceIndex::default(), &mut images);
        for texture in [
            &art.glow,
            &art.corona,
            &art.flare,
            &art.star_sprite,
            &art.route,
            &art.ring,
            &art.disc,
        ] {
            assert!(texture.source.is_none(), "no install, no original key");
            assert!(images.get(&texture.image).is_some());
        }
        assert!(art.galaxy.is_none() && art.nebulae.is_none());
        let sky = images.get(&art.sky).unwrap();
        assert_eq!((sky.width(), sky.height()), (1024, 512));
    }

    #[test]
    fn orbit_annulus_is_a_closed_flat_ring() {
        let mesh = annulus(9.0, 10.0, 64);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        assert_eq!(positions.len(), 130);
        for p in positions {
            let r = (p[0] * p[0] + p[2] * p[2]).sqrt();
            assert!((8.99..=10.01).contains(&r));
            assert_eq!(p[1], 0.0);
        }
        // The first and last spokes coincide, so the ring has no gap.
        assert!((positions[0][0] - positions[128][0]).abs() < 1e-4);
    }

    #[test]
    fn sunlight_delivers_the_same_light_at_every_orbit() {
        let near = sunlight_intensity(SUNLIGHT_DISTANCE, SUNLIGHT_LUX)
            / (4.0 * std::f32::consts::PI * SUNLIGHT_DISTANCE * SUNLIGHT_DISTANCE);
        assert!((near - SUNLIGHT_LUX).abs() < 1.0);
        assert!(StarKind::Blue.radius() > StarKind::Red.radius());
    }
}
