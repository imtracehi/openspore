//! Weather clouds of the Space planet view (helper `space-visuals`,
//! `notes/space-visuals-notes.md`).
//!
//! Original data (VERIFIED, planet effect directory 0x40450202!0x5F848353,
//! decoded by `space-visuals/re/clouds.py`; code 0xF9A530 / 0xFC2ED0 /
//! 0xE978C0):
//! - weatherMidAtmoEffect (`weather` 0x0243AD2B!0x12E95823) = effect
//!   0x03671D85: distribute components of density 75 (cloud effect 20) and
//!   35 + 10 (cloud effect 21, emit map 0x0379A0AA), 16 clusters.
//! - the cloud components (type 0x2B) of effects 20 / 21: life 30..50 s, rate
//!   0.35 / s, preroll 40 s, emit volume +-50 x +-50 x +-1, speed 1..3, size
//!   keys 60 75 60 56 (vary 0.25), the alpha keys below, pinned 15 units over
//!   the planet surface map 0x03598109; material 0x5995959E = shader
//!   0x213D3024 (cloudVanilla) with c0 (0.3, 8, 1) and the cloud_set1 side /
//!   top strips.
//! - weatherCloudType (theme property 0x03B4DE49, 1..6) selects the cloud set
//!   cloudDefault / Curly / Alien / Web / Pollen / Sponge (table 0x014914CC).
//! - the cloud colour by temperature band <= 0.2 / 0.4 / 0.6 / 0.8 / 1.0:
//!   weatherIce / Cold / Warm / Hot / LavaCloudColor (cPlanetGfx_CreateCloudsEffect).
//! - shading: cloudVanillaHighVS / PS (`shaders/space_clouds.wesl`).
//! - the solar-system view's planet clouds (SG_solar_planet_clouds_thin /
//!   medium / thickAtmo~, 0x6B306EEC / EB / EA) are EMPTY effects in the
//!   installed data: retail draws no clouds on the orbiting worlds.
//!
//! ADAPTED: the Swarm cloud simulation (emit maps, advection, the surface pin
//! map) is not ported: the distribute's emitters are spread over the sphere in
//! 16 clusters, each holds rate x mean life puffs at the steady state (the
//! preroll), puffs drift at their emit speed along a fixed heading and cycle
//! through their life in the vertex shader; the set -> strip mapping is by name
//! (cloudSponge -> the swirl strips, INFERRED).
use crate::planet_query::Planet;
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Emitters of weatherMidAtmoEffect's distributes (75 + 35 + 10).
pub const EMITTERS: [usize; 3] = [75, 35, 10];
/// numClusters of the distributes.
pub const CLUSTERS: usize = 16;
/// Cloud component of effect 20 (VERIFIED values).
pub const LIFE: (f32, f32) = (30.0, 50.0);
pub const RATE: f32 = 0.35;
pub const EMIT_HALF: f32 = 50.0;
pub const SPEED: (f32, f32) = (1.0, 3.0);
pub const PIN_OFFSET: f32 = 15.0;
pub const SIZE_KEYS: [f32; 4] = [60.0, 75.0, 60.0, 56.0];
pub const SIZE_VARY: f32 = 0.25;
pub const ALPHA_KEYS: [f32; 13] = [0.0, 0.2, 0.5, 0.3, 0.7, 1.0, 1.0, 0.9, 1.0, 0.8, 0.6, 0.7, 0.0];
/// Material 0x5995959E customParams c0: animBlendRate, stripLength, topFadeAmount.
pub const CUSTOM: [f32; 3] = [0.3, 8.0, 1.0];
/// ADAPTED: the angular radius of a cluster (radians).
const CLUSTER_RADIUS: f32 = 0.35;

/// weatherIce / Cold / Warm / Hot / LavaCloudColor (`weather` prop, VERIFIED;
/// the same defaults in 0xE978C0) by the temperature band of 0xE978C0.
pub fn cloud_color(temperature: f32) -> [f32; 3] {
    if temperature <= 0.6 {
        [0.95; 3]
    } else if temperature <= 0.8 {
        [0.625, 0.525, 0.475]
    } else {
        [0.3, 0.1, 0.0]
    }
}

/// The side / top strips of a weatherCloudType (1..6).
pub fn strips(cloud_type: u8) -> (&'static str, &'static str) {
    match cloud_type {
        2 => ("cloud_curly_side_strip", "cloud_curly_top_strip"),
        3 => ("cloud_alien_side_strip", "cloud_alien_top_strip"),
        4 => ("cloud_web_side_strip", "cloud_web_top_strip"),
        5 => ("cloud_pollen_side_strip", "cloud_pollen_top_strip"),
        6 => ("cloud_swirl_side_strip", "cloud_swirl_top_strip"),
        _ => ("cloud_set1_side_strip", "cloud_set1_top_strip"),
    }
}

/// cloudVanilla on the planet view.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct CloudMaterial {
    #[uniform(0)]
    pub sky: crate::planet_sky::SkyState,
    /// c0, cloud colour + temperature, alpha key count, size keys.
    #[uniform(1)]
    pub params: [Vec4; 4],
    #[uniform(2)]
    pub alpha_keys: [Vec4; 4],
    #[texture(3)]
    #[sampler(4)]
    pub side: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    pub top: Handle<Image>,
    #[texture(7, dimension = "cube")]
    #[sampler(8)]
    pub scatter: Handle<Image>,
    /// Blend (development: `OPENSPORE_CLOUD_DEBUG=2` draws them masked).
    pub alpha_mode: AlphaMode,
}

impl Material for CloudMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://openspore/shaders/space_clouds.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/space_clouds.wesl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// One cloud puff of the steady state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Puff {
    /// Planet-local site (on the pinned surface).
    pub site: Vec3,
    /// Drift, planet units per second (tangent).
    pub drift: Vec3,
    pub id: f32,
    pub phase: f32,
    pub life: f32,
    pub size: f32,
    /// Terrain height above R under the puff.
    pub ground: f32,
}

fn tangent_frame(up: Vec3) -> (Vec3, Vec3) {
    let helper = if up.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
    let a = up.cross(helper).normalize();
    (a, up.cross(a))
}

fn random_dir(rng: &mut ChaCha8Rng) -> Vec3 {
    loop {
        let v = Vec3::new(rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0));
        let l = v.length_squared();
        if l > 1e-4 && l <= 1.0 {
            return v / l.sqrt();
        }
    }
}

/// The puffs of a world's weather (seeded per world).
pub fn puffs(planet: &Planet, seed: u64) -> Vec<Puff> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0xC10D_5EED);
    let centres: Vec<Vec3> = (0..CLUSTERS).map(|_| random_dir(&mut rng)).collect();
    let per_emitter = (RATE * (LIFE.0 + LIFE.1) * 0.5).round() as usize;
    let mut out = Vec::new();
    for _ in 0..EMITTERS.iter().sum::<usize>() {
        let centre = centres[rng.random_range(0..CLUSTERS)];
        let (a, b) = tangent_frame(centre);
        let ang = rng.random_range(0.0..CLUSTER_RADIUS);
        let turn = rng.random_range(0.0..std::f32::consts::TAU);
        let emitter = (centre * ang.cos() + (a * turn.cos() + b * turn.sin()) * ang.sin()).normalize();
        let (ea, eb) = tangent_frame(emitter);
        let heading = rng.random_range(0.0..std::f32::consts::TAU);
        for _ in 0..per_emitter {
            let offset = ea * rng.random_range(-EMIT_HALF..EMIT_HALF) + eb * rng.random_range(-EMIT_HALF..EMIT_HALF);
            let dir = (emitter * planet.radius + offset).normalize();
            let ground = planet.height(dir).max(planet.water_radius());
            let (ta, tb) = tangent_frame(dir);
            let speed = rng.random_range(SPEED.0..SPEED.1);
            out.push(Puff {
                site: dir * (ground + PIN_OFFSET),
                drift: (ta * heading.cos() + tb * heading.sin()) * speed,
                id: rng.random_range(0.0..1.0),
                phase: rng.random_range(0.0..1.0),
                life: rng.random_range(LIFE.0..LIFE.1),
                size: 1.0 + rng.random_range(-SIZE_VARY..SIZE_VARY),
                ground: planet.height(dir) - planet.radius,
            });
        }
    }
    out
}

/// Four corners per puff; the vertex shader places and animates them.
pub fn mesh(puffs: &[Puff]) -> Mesh {
    let n = puffs.len() * 4;
    let mut pos = Vec::with_capacity(n);
    let mut nrm = Vec::with_capacity(n);
    let mut tan = Vec::with_capacity(n);
    let mut uv = Vec::with_capacity(n);
    let mut uvb = Vec::with_capacity(n);
    let mut col = Vec::with_capacity(n);
    let mut idx: Vec<u32> = Vec::with_capacity(puffs.len() * 6);
    for p in puffs {
        let base = pos.len() as u32;
        for c in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
            pos.push(p.site.to_array());
            nrm.push(p.drift.to_array());
            tan.push([1.0, 0.0, 0.0, 1.0]);
            uv.push(c);
            uvb.push([p.id, p.phase]);
            col.push([p.life, p.size, p.ground, 1.0]);
        }
        // Counter-clockwise as the camera sees the quad (the vertex shader
        // spans u to the right and v downward on screen).
        idx.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tan);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uvb);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

fn strip_image(index: &crate::asset_catalog::ResourceIndex, name: &str) -> Option<Image> {
    let key = crate::dbpf::ResourceKey {
        group_id: 0,
        instance_id: crate::world_objects::name_id(name),
        type_id: crate::asset_catalog::RW4,
    };
    // Display values: the retail program works in gamma space (the shader
    // converts its result to linear).
    crate::planet_sky::rw4_image(index, key, false, false)
}

/// The cloud material of a world (`None` without the installed strips or the
/// material plugin).
pub fn material(
    world: &mut World,
    cloud_type: u8,
    temperature: f32,
    scatter: Handle<Image>,
) -> Option<Handle<CloudMaterial>> {
    if !world.contains_resource::<Assets<CloudMaterial>>() {
        return None;
    }
    // OPENSPORE_CLOUD_DEBUG (development): 1 every puff opaque and full size,
    // 2 the same masked, 3 the same with white strips.
    let debug = std::env::var("OPENSPORE_CLOUD_DEBUG").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let (side, top) = strips(cloud_type);
    let (side, top) = {
        let index = &world.get_resource::<crate::Originals>()?.resource_index;
        (strip_image(index, side)?, strip_image(index, top)?)
    };
    let (side, top) = {
        let mut images = world.resource_mut::<Assets<Image>>();
        if debug == 3 {
            let white = || {
                Image::new_fill(
                    bevy::render::render_resource::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    bevy::render::render_resource::TextureDimension::D2,
                    &[255, 255, 255, 255],
                    bevy::render::render_resource::TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::default(),
                )
            };
            (images.add(white()), images.add(white()))
        } else {
            (images.add(side), images.add(top))
        }
    };
    let c = cloud_color(temperature);
    let mut alpha = [Vec4::ZERO; 4];
    for (i, a) in ALPHA_KEYS.iter().enumerate() {
        alpha[i / 4][i % 4] = *a;
    }
    let params = [
        Vec4::new(
            CUSTOM[0],
            CUSTOM[1],
            CUSTOM[2],
            if debug > 0 { 1.0 } else { 0.0 },
        ),
        Vec4::new(c[0], c[1], c[2], temperature),
        Vec4::new(ALPHA_KEYS.len() as f32, 0.0, 0.0, 0.0),
        Vec4::from_array(SIZE_KEYS),
    ];
    Some(world.resource_mut::<Assets<CloudMaterial>>().add(CloudMaterial {
        sky: [Vec4::ZERO; crate::planet_sky::SKY_SLOTS],
        params,
        alpha_keys: alpha,
        side,
        top,
        scatter,
        alpha_mode: if debug == 2 { AlphaMode::Mask(0.5) } else { AlphaMode::Blend },
    }))
}

/// Register the material (render apps: `planet_render::build`).
pub fn build(app: &mut App) {
    app.add_plugins(MaterialPlugin::<CloudMaterial>::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_colours_follow_the_temperature_bands() {
        assert_eq!(cloud_color(0.1), [0.95; 3]);
        assert_eq!(cloud_color(0.6), [0.95; 3]);
        assert_eq!(cloud_color(0.7), [0.625, 0.525, 0.475]);
        assert_eq!(cloud_color(0.85), [0.3, 0.1, 0.0]);
    }

    #[test]
    fn puffs_sit_over_the_ground_in_clusters() {
        let planet = Planet::flat(0.2);
        let a = puffs(&planet, 7);
        assert_eq!(a, puffs(&planet, 7));
        assert_eq!(a.len(), 120 * 14);
        for p in &a {
            let alt = p.site.length() - planet.height(p.site.normalize());
            assert!((alt - PIN_OFFSET).abs() < 0.5, "{alt}");
            assert!(p.drift.dot(p.site.normalize()).abs() < 1e-3);
            assert!((LIFE.0..LIFE.1).contains(&p.life));
        }
        let m = mesh(&a[..10]);
        assert_eq!(m.count_vertices(), 40);
        assert_eq!(strips(1).0, "cloud_set1_side_strip");
        assert_eq!(strips(6).1, "cloud_swirl_top_strip");
    }
}
