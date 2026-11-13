//! Planet rendering: the cube-sphere quadtree of height-displaced chunks
//! (cTerrainSphere / cTerrainSphereQuad, `notes/planet-re-notes.md` 1C), the
//! water sphere, an atmosphere shell, and the camera-up / fog adaptations.
//!
//! Original layout (VERIFIED): 6 root quads per face set, each a quadtree of
//! 16 x 16-cell chunks (mPlanetLODChunkRes); at 512 height texels per face
//! edge the finest level is 32 chunks per edge (levels 0..=5, the same nodes
//! as the cHeightRanges tree). ADAPTED: vertices are displaced on the CPU,
//! LOD is split by camera distance (no geomorph, skirts hide cracks), the
//! land material is the existing TerrainAbove port with per-face tiling, the
//! water and atmosphere materials are stand-ins (TerrainWater /
//! TerrainAtmosphere programs are not decoded).
use crate::planet_query::{face_point, Planet, StagePlanet, PLANET_RADIUS};
use crate::stages::StageOwned;
use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor,
        TextureViewDimension,
    },
    shader::ShaderRef,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Cells per chunk edge (mPlanetLODChunkRes).
pub const CHUNK_CELLS: usize = 16;
/// Deepest quadtree level: 2^5 = 32 chunks of 16 cells = 512 texels.
pub const MAX_LEVEL: u8 = 5;
/// Arc length of a cube-face edge on the R = 500 sphere (pi/2 R ~ 785):
/// the native detail tiling constants are repeats per face edge.
pub const FACE_EDGE_UNITS: f32 = PLANET_RADIUS * std::f32::consts::FRAC_PI_2;
/// ADAPTED split rule: a node splits while the camera is closer than this
/// many node edge lengths.
const SPLIT_FACTOR: f32 = 2.0;
/// Water depth (units) mapped to 1.0 in the depth cube.
pub const DEPTH_SCALE: f32 = 30.0;
const DEPTH_CUBE_RES: u32 = 256;
const WATER_CELLS: usize = 64;

/// One quadtree node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKey {
    pub face: u8,
    pub level: u8,
    pub x: u16,
    pub y: u16,
}

impl NodeKey {
    /// (u0, v0, size) in face uv.
    pub fn uv_rect(&self) -> (f32, f32, f32) {
        let s = 1.0 / (1u32 << self.level) as f32;
        (self.x as f32 * s, self.y as f32 * s, s)
    }
    pub fn children(&self) -> [NodeKey; 4] {
        let (x, y, level) = (self.x * 2, self.y * 2, self.level + 1);
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| NodeKey {
            face: self.face,
            level,
            x: x + dx,
            y: y + dy,
        })
    }
}

/// Node bounds in planet-local space.
struct NodeBounds {
    centre_dir: Vec3,
    /// Largest angle between the centre direction and the node corners.
    angle: f32,
    r_lo: f32,
    r_hi: f32,
}

fn node_bounds(planet: &Planet, key: NodeKey) -> NodeBounds {
    let (u0, v0, s) = key.uv_rect();
    let face = key.face as usize;
    let centre_dir = face_point(face, u0 + s * 0.5, v0 + s * 0.5).normalize();
    let mut angle = 0.0f32;
    for (du, dv) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.0), (0.0, 0.5), (1.0, 0.5), (0.5, 1.0)] {
        let d = face_point(face, u0 + s * du, v0 + s * dv).normalize();
        angle = angle.max(d.dot(centre_dir).clamp(-1.0, 1.0).acos());
    }
    let (lo, hi) = planet
        .ranges
        .node_at(key.level as usize, face, key.x as usize, key.y as usize);
    let alt = planet.altitude_range;
    NodeBounds {
        centre_dir,
        angle,
        r_lo: planet.radius + alt * lo.min(planet.water_level),
        r_hi: planet.radius + alt * hi.max(planet.water_level),
    }
}

/// Leaf nodes to draw for a camera at planet-local `camera`.
pub fn select_nodes(planet: &Planet, camera: Vec3) -> Vec<NodeKey> {
    let d = camera.length().max(1.0);
    let cam_dir = camera / d;
    // The occluder is the lowest terrain sphere: the decoded TerrainWater is
    // see-through (the sea floor shows under it), so the water sphere no
    // longer hides the terrain below its horizon (it left holes in the floor).
    let (gmin, _) = planet.ranges.global();
    let r_floor = planet.radius + planet.altitude_range * gmin;
    let mut out = Vec::new();
    let mut stack: Vec<NodeKey> = (0..6)
        .map(|face| NodeKey {
            face,
            level: 0,
            x: 0,
            y: 0,
        })
        .collect();
    while let Some(key) = stack.pop() {
        let b = node_bounds(planet, key);
        // Horizon culling against the lowest terrain sphere.
        if d > r_floor {
            let alpha = cam_dir.dot(b.centre_dir).clamp(-1.0, 1.0).acos();
            let theta = (r_floor / d).clamp(-1.0, 1.0).acos()
                + (r_floor / b.r_hi.max(r_floor)).clamp(-1.0, 1.0).acos();
            if alpha - b.angle > theta + 0.01 {
                continue;
            }
        }
        let edge = FACE_EDGE_UNITS / (1u32 << key.level) as f32;
        let mid = b.centre_dir * (b.r_lo + b.r_hi) * 0.5;
        let radius = b.r_hi * b.angle.sin() + (b.r_hi - b.r_lo) * 0.5 + b.r_hi * (1.0 - b.angle.cos());
        let dist = (camera.distance(mid) - radius).max(0.0);
        // Settings > Graphics > Planet Quality (terrainLODThreshold).
        if key.level < MAX_LEVEL && dist < SPLIT_FACTOR * crate::front_end::graphics::planet_lod_scale() * edge {
            stack.extend(key.children());
        } else {
            out.push(key);
        }
    }
    out.sort();
    out
}

/// A land chunk mesh in planet-local space relative to the returned centre.
pub fn chunk_mesh(planet: &Planet, key: NodeKey) -> (Mesh, Vec3) {
    let n = CHUNK_CELLS;
    let (u0, v0, s) = key.uv_rect();
    let face = key.face as usize;
    let centre_dir = face_point(face, u0 + s * 0.5, v0 + s * 0.5).normalize();
    let centre = centre_dir * planet.height(centre_dir);
    let verts = (n + 1) * (n + 1);
    let mut positions = Vec::with_capacity(verts + 4 * (n + 1));
    let mut normals = Vec::with_capacity(verts + 4 * (n + 1));
    let mut uvs = Vec::with_capacity(verts + 4 * (n + 1));
    let mut dirs = Vec::with_capacity(verts);
    for j in 0..=n {
        for i in 0..=n {
            let u = u0 + s * i as f32 / n as f32;
            let v = v0 + s * j as f32 / n as f32;
            let dir = face_point(face, u, v).normalize();
            positions.push((dir * planet.height(dir) - centre).to_array());
            normals.push(planet.ground_normal(dir).to_array());
            uvs.push([u * FACE_EDGE_UNITS, v * FACE_EDGE_UNITS]);
            dirs.push(dir);
        }
    }
    let mut indices: Vec<u32> = Vec::with_capacity(n * n * 6 + 4 * n * 12);
    for j in 0..n {
        for i in 0..n {
            let a = (j * (n + 1) + i) as u32;
            let b = a + 1;
            let c = a + (n + 1) as u32;
            let d = c + 1;
            // du x dv is outward on every face (axis table winding).
            indices.extend([a, b, c, b, d, c]);
        }
    }
    // Skirts: the border dropped toward the centre hides LOD cracks.
    let skirt = (FACE_EDGE_UNITS / (1u32 << key.level) as f32) * 0.06 + 1.5;
    let border: Vec<usize> = (0..=n)
        .map(|i| i)
        .chain((0..=n).map(|j| j * (n + 1) + n))
        .chain((0..=n).rev().map(|i| n * (n + 1) + i))
        .chain((0..=n).rev().map(|j| j * (n + 1)))
        .collect();
    let mut prev: Option<(u32, u32)> = None;
    for &k in &border {
        let top = k as u32;
        let p = Vec3::from_array(positions[k]) + centre - dirs[k] * skirt - centre;
        let bottom = positions.len() as u32;
        positions.push(p.to_array());
        normals.push(normals[k]);
        uvs.push(uvs[k]);
        if let Some((pt, pb)) = prev {
            // Both windings: the wall is seen from either side.
            indices.extend([pt, top, pb, top, bottom, pb, pt, pb, top, top, pb, bottom]);
        }
        prev = Some((top, bottom));
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    (mesh, centre)
}

/// The water sphere (planet-local, centred on the planet).
pub fn water_mesh(planet: &Planet) -> Mesh {
    let n = WATER_CELLS;
    let r = planet.water_radius();
    let mut positions = Vec::with_capacity(6 * (n + 1) * (n + 1));
    let mut normals = Vec::with_capacity(positions.capacity());
    let mut uvs = Vec::with_capacity(positions.capacity());
    let mut indices: Vec<u32> = Vec::with_capacity(6 * n * n * 6);
    for face in 0..6 {
        let base = positions.len() as u32;
        for j in 0..=n {
            for i in 0..=n {
                let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                let dir = face_point(face, u, v).normalize();
                positions.push((dir * r).to_array());
                normals.push(dir.to_array());
                uvs.push([u * FACE_EDGE_UNITS, v * FACE_EDGE_UNITS]);
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
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Water depth below the sea surface as an R8 cube map in WORLD orientation
/// (cube direction = world direction from the planet centre), so the water
/// shader can shade shallows and foam per pixel.
pub fn depth_cube_image(stage: &StagePlanet) -> Image {
    let n = DEPTH_CUBE_RES as usize;
    let water = stage.planet.water_radius();
    let mut data = Vec::with_capacity(6 * n * n);
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let s = 2.0 * (x as f32 + 0.5) / n as f32 - 1.0;
                let t = 2.0 * (y as f32 + 0.5) / n as f32 - 1.0;
                // wgpu / D3D cube face order and orientation.
                let d = match face {
                    0 => Vec3::new(1.0, -t, -s),
                    1 => Vec3::new(-1.0, -t, s),
                    2 => Vec3::new(s, 1.0, t),
                    3 => Vec3::new(s, -1.0, -t),
                    4 => Vec3::new(s, -t, 1.0),
                    _ => Vec3::new(-s, -t, -1.0),
                };
                let local = stage.frame.dir_to_local(d.normalize());
                let depth = water - stage.planet.height(local);
                data.push(((depth / DEPTH_SCALE).clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
    }
    cube_image(DEPTH_CUBE_RES, data)
}

/// A single-channel cube image (also the 1x1 neutral cube of flat water).
pub fn cube_image(size: u32, data: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::default(),
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

// ---------------------------------------------------------------------------
// Atmosphere shell (stand-in for TerrainAtmosphere 0x18EDB1F3).

pub type AtmosphereMaterial = ExtendedMaterial<StandardMaterial, AtmosphereShell>;

/// ADAPTED rim-glow shell around the planet: brightest where the view grazes
/// the shell (the limb seen from high up, a haze band at the horizon from the
/// ground). Colour = the palette AirColor.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct AtmosphereShell {
    /// xyz planet centre (world), w planet radius.
    #[uniform(100)]
    pub sphere: Vec4,
    /// rgb colour (linear), a strength.
    #[uniform(101)]
    pub color: Vec4,
    /// x rim exponent, y shell radius.
    #[uniform(102)]
    pub params: Vec4,
}
impl MaterialExtension for AtmosphereShell {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/planet_atmosphere.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

#[derive(Component)]
pub struct PlanetAtmosphereShell;

/// One drawn land chunk.
#[derive(Component)]
pub struct PlanetChunk(pub NodeKey);

/// Live renderer state of the stage planet.
#[derive(Resource)]
pub struct PlanetRender {
    pub stage: Arc<StagePlanet>,
    /// Land root (the stage's WorldGround entity, transformed to the frame).
    pub root: Entity,
    pub land_material: Handle<crate::terrain::TerrainMaterial>,
    pub chunks: HashMap<NodeKey, Entity>,
    pub meshes: HashMap<NodeKey, (Handle<Mesh>, Vec3)>,
    /// Stage defaults of the camera fog (start, end) before altitude scaling.
    pub fog_base: Option<(f32, f32)>,
    /// Stage defaults of the sun (colour, illuminance) before time of day.
    pub sun_base: Option<(Color, f32)>,
    /// Stage default of the camera's ambient brightness before time of day
    /// and the value last written.
    pub ambient_base: Option<(f32, f32)>,
    pub last_nodes: usize,
}

/// Keep the mesh cache bounded (oldest entries are rebuilt on demand).
const MESH_CACHE: usize = 2000;

/// Per frame: orient the game camera to the local up, then rebuild the
/// visible chunk set for its position, scale the sky shell and the fog with
/// altitude. Runs in PostUpdate before transform propagation.
pub fn update(world: &mut World) {
    let Some(render) = world.get_resource::<PlanetRender>() else {
        return;
    };
    if world.get_entity(render.root).is_err() {
        world.remove_resource::<PlanetRender>();
        return;
    }
    let stage = render.stage.clone();
    let cameras: Vec<Entity> = world
        .query_filtered::<Entity, With<crate::GameCamera>>()
        .iter(world)
        .collect();
    let Some(&camera) = cameras.first() else {
        return;
    };
    // Cameras take up = normalize(pos - centre) (planet-re 4E step 3).
    let cam_pos = {
        let Some(mut t) = world.get_mut::<Transform>(camera) else {
            return;
        };
        let up = stage.frame.up(t.translation);
        let forward = *t.forward();
        if forward.cross(up).length_squared() > 1e-8 {
            let target = t.translation + forward;
            t.look_at(target, up);
        }
        t.translation
    };
    let local_cam = stage.frame.to_local(cam_pos);
    let altitude = local_cam.length() - stage.planet.water_radius();

    // Sky shell: the preview sky sphere follows the camera at radius 280; on
    // the planet it must enclose the far horizon (below the 1000 far plane).
    let horizon = (2.0 * PLANET_RADIUS * altitude.max(1.0) + altitude.max(1.0).powi(2)).sqrt();
    for mut t in world
        .query_filtered::<&mut Transform, With<crate::world_presentation::PreviewAtmosphere>>()
        .iter_mut(world)
    {
        t.scale = Vec3::splat(3.3);
    }
    // ADAPTED fog by camera elevation (kFogByElev* distance factors grow
    // with height): keep the stage's near fog, but never fog out the
    // planet's limb when the camera is high.
    let fog_base = world.resource::<PlanetRender>().fog_base;
    if let Some(mut fog) = world.get_mut::<DistanceFog>(camera) {
        if let FogFalloff::Linear { start, end } = &mut fog.falloff {
            let (s0, e0) = fog_base.unwrap_or((*start, *end));
            let alt = altitude.max(0.0);
            *start = s0.max(alt * 1.3);
            *end = e0.max(horizon * 1.5 + alt);
            if fog_base.is_none() {
                world.resource_mut::<PlanetRender>().fog_base = Some((s0, e0));
            }
        }
    }

    drive_sun(world);
    let started = std::time::Instant::now();
    let wanted = select_nodes(&stage.planet, local_cam);
    let mut render = world.remove_resource::<PlanetRender>().unwrap();
    let wanted_set: std::collections::HashSet<NodeKey> = wanted.iter().copied().collect();
    let stale: Vec<NodeKey> = render
        .chunks
        .keys()
        .filter(|k| !wanted_set.contains(k))
        .copied()
        .collect();
    for key in stale {
        if let Some(e) = render.chunks.remove(&key) {
            if let Ok(em) = world.get_entity_mut(e) {
                em.despawn();
            }
        }
    }
    let missing: Vec<NodeKey> = wanted
        .iter()
        .filter(|k| !render.chunks.contains_key(k))
        .copied()
        .collect();
    if render.meshes.len() + missing.len() > MESH_CACHE {
        let keep: std::collections::HashSet<NodeKey> = render.chunks.keys().copied().collect();
        render.meshes.retain(|k, _| keep.contains(k));
    }
    for key in missing {
        let (mesh, centre) = match render.meshes.get(&key) {
            Some(m) => m.clone(),
            None => {
                let (mesh, centre) = chunk_mesh(&stage.planet, key);
                let handle = world.resource_mut::<Assets<Mesh>>().add(mesh);
                render.meshes.insert(key, (handle.clone(), centre));
                (handle, centre)
            }
        };
        let e = world
            .spawn((
                PlanetChunk(key),
                Mesh3d(mesh),
                MeshMaterial3d(render.land_material.clone()),
                NotShadowCaster,
                Transform::from_translation(centre),
                ChildOf(render.root),
            ))
            .id();
        render.chunks.insert(key, e);
    }
    if std::env::var_os("OPENSPORE_PLANET_TRACE").is_some() {
        let queries = crate::planet_query::FACADE_QUERIES.swap(0, std::sync::atomic::Ordering::Relaxed);
        eprintln!(
            "planet: {} chunks ({} new), altitude {:.1}, lod {:?}, facade queries {}",
            wanted.len(),
            wanted.len().saturating_sub(render.last_nodes.min(wanted.len())),
            altitude,
            started.elapsed(),
            queries
        );
    }
    render.last_nodes = wanted.len();
    world.insert_resource(render);
}

/// The sun follows the stage clock (`planet_time`): Creature's
/// (`SpeciesWorld::time_of_day`, creatureStartTimeOfDay 7.0, hourly
/// multipliers) and the Tribal / Civ clocks of `planet_sky` (Tribe = the
/// Creature table, CityCiv its own). The stage origin is the +Y pole, so the
/// local frame there is the world frame.
fn drive_sun(world: &mut World) {
    if std::env::var("OPENSPORE_PLANET_TIME").is_ok_and(|v| v == "0") {
        return;
    }
    // Creature: the ecology clock; Tribal / Civ: `planet_sky::StageClock`.
    let Some(hour) = crate::planet_sky::stage_hour(world) else {
        return;
    };
    let Some(sun) = world
        .query_filtered::<Entity, (With<DirectionalLight>, With<crate::GameSun>)>()
        .iter(world)
        .next()
    else {
        return;
    };
    let base = world.resource::<PlanetRender>().sun_base;
    let base = match base {
        Some(b) => b,
        None => {
            let Some(light) = world.get::<DirectionalLight>(sun) else {
                return;
            };
            let b = (light.color, light.illuminance);
            world.resource_mut::<PlanetRender>().sun_base = Some(b);
            b
        }
    };
    let azimuth = crate::world_presentation::tune("SUN_AZ", -55.).to_radians();
    let to_sun = crate::planet_time::sun_direction(hour, azimuth);
    let retail_light = !std::env::var("OPENSPORE_RETAIL_LIGHT").is_ok_and(|v| v == "0");
    let (tint, brightness) = if retail_light {
        // Retail terrain light curve + model day / transition / night weights
        // (planet_light.rs; ADAPTED onto Bevy's sun and ambient).
        let terrain = world.get_resource::<crate::planet_light::TerrainLighting>().cloned().unwrap_or_default();
        let (tint, ratio) = crate::planet_light::model_light(&terrain, &Default::default(), to_sun.y);
        (tint, ratio)
    } else {
        let weights = crate::planet_time::light_weights(to_sun.y);
        let tint = crate::planet_time::light_color(weights);
        let brightness = tint[0].max(tint[1]).max(tint[2]).max(1e-3);
        (tint.map(|t| t / brightness), brightness)
    };
    let c = base.0.to_linear();
    let dir = crate::planet_time::shading_direction(hour, azimuth);
    if let Some(mut light) = world.get_mut::<DirectionalLight>(sun) {
        light.color = Color::linear_rgb(c.red * tint[0], c.green * tint[1], c.blue * tint[2]);
        light.illuminance = base.1 * brightness;
    }
    if retail_light {
        // The camera's ambient fill follows the same curve.
        let cameras: Vec<Entity> = world.query_filtered::<Entity, With<crate::GameCamera>>().iter(world).collect();
        for camera in cameras {
            let Some(current) = world.get::<AmbientLight>(camera).map(|a| a.brightness) else {
                continue;
            };
            // (base, last written): a write by the stage presentation in
            // between becomes the new base.
            let ambient = match world.resource::<PlanetRender>().ambient_base {
                Some((base, written)) if (written - current).abs() <= 1e-3 * written.abs().max(1.0) => base,
                _ => current,
            };
            world.resource_mut::<PlanetRender>().ambient_base = Some((ambient, ambient * brightness));
            if let Some(mut a) = world.get_mut::<AmbientLight>(camera) {
                a.brightness = ambient * brightness;
            }
        }
    }
    if let Some(mut t) = world.get_mut::<Transform>(sun) {
        let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        t.rotation = Transform::IDENTITY.looking_to(-dir, up).rotation;
    }
}

/// The stage planet turned to a new frame (the Creature stage's floating
/// origin, `creature_planet::reanchor`): the land root takes the frame (the
/// chunks are its children and keep their meshes) and the sea's depth cube,
/// which is in world orientation, is rebuilt.
pub fn reframe(world: &mut World, stage: Arc<StagePlanet>) {
    let Some(mut render) = world.get_resource_mut::<PlanetRender>() else {
        return;
    };
    render.stage = stage.clone();
    let root = render.root;
    if let Some(mut t) = world.get_mut::<Transform>(root) {
        t.translation = stage.frame.centre;
        t.rotation = stage.frame.rotation;
    }
    let seas: Vec<Handle<crate::water::WaterMaterial>> = world
        .query_filtered::<&MeshMaterial3d<crate::water::WaterMaterial>, With<crate::terrain::WorldGround>>()
        .iter(world)
        .map(|m| m.0.clone())
        .collect();
    if seas.is_empty() {
        return;
    }
    let image = depth_cube_image(&stage);
    for sea in seas {
        let cube = world
            .resource::<Assets<crate::water::WaterMaterial>>()
            .get(&sea)
            .map(|m| m.extension.depth_cube.clone());
        if let Some(cube) = cube {
            if let Some(mut slot) = world.resource_mut::<Assets<Image>>().get_mut(&cube) {
                *slot = image.clone();
            }
        }
    }
}

/// Spawn the planet's water sphere and atmosphere shell for a stage; the
/// land chunks appear with the first `update`.
pub fn spawn_static(
    world: &mut World,
    stage: &StagePlanet,
    air_color: [f32; 3],
) {
    let frame_transform = Transform {
        translation: stage.frame.centre,
        rotation: stage.frame.rotation,
        scale: Vec3::ONE,
    };
    let Some(mesh) = world
        .get_resource_mut::<Assets<Mesh>>()
        .map(|mut m| {
            let mut sphere = Sphere::new(PLANET_RADIUS + stage.planet.altitude_range * 0.45)
                .mesh()
                .ico(5)
                .unwrap();
            sphere.asset_usage = RenderAssetUsages::default();
            m.add(sphere)
        })
    else {
        return;
    };
    let Some(material) = world
        .get_resource_mut::<Assets<AtmosphereMaterial>>()
        .map(|mut m| {
            let air = Color::srgb(air_color[0], air_color[1], air_color[2]).to_linear();
            m.add(AtmosphereMaterial {
                base: StandardMaterial {
                    base_color: Color::WHITE,
                    unlit: true,
                    fog_enabled: false,
                    cull_mode: None,
                    alpha_mode: AlphaMode::Add,
                    ..default()
                },
                extension: AtmosphereShell {
                    sphere: stage.frame.centre.extend(stage.planet.radius),
                    color: Vec4::new(air.red, air.green, air.blue, crate::world_presentation::tune("ATM", 0.5)),
                    params: Vec4::new(2.0, PLANET_RADIUS + stage.planet.altitude_range * 0.45, 0.0, 0.0),
                },
            })
        })
    else {
        return;
    };
    world.spawn((
        StageOwned,
        PlanetAtmosphereShell,
        NotShadowCaster,
        Name::new("planet atmosphere shell (adapted)"),
        Mesh3d(mesh),
        MeshMaterial3d(material),
        frame_transform,
    ));
}

/// Register the renderer (called from `TerrainPlugin`).
pub fn build(app: &mut App) {
    bevy::asset::embedded_asset!(app, "shaders/planet_atmosphere.wesl");
    crate::world_presentation::dev_shader_override(app, "planet_atmosphere.wesl");
    app.add_plugins(MaterialPlugin::<AtmosphereMaterial>::default());
    crate::planet_sky::build(app);
    crate::planet_flora::build(app);
    crate::planet_grass::plugin(app);
    crate::space_planets::build_render(app);
    app.add_systems(
        PostUpdate,
        update.before(bevy::transform::TransformSystems::Propagate),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet_query::{HeightCube, PlanetFrame};

    fn bumpy() -> Planet {
        let mut cube = HeightCube::filled(64, 0.0);
        for face in 0..6 {
            for row in 0..64 {
                for col in 0..64 {
                    let d = crate::planet_query::texel_dir(face, row, col, 64);
                    let i = cube.index(face, row, col);
                    cube.raw_mut()[i] = crate::planet_query::h_to_raw((d.x * 7.0).sin() * 0.2 + 0.05);
                }
            }
        }
        Planet::new(cube, PLANET_RADIUS, 0.0)
    }

    #[test]
    fn lod_refines_near_the_camera_and_culls_the_far_side() {
        let planet = bumpy();
        let ground = Vec3::Y * planet.height(Vec3::Y);
        let near = select_nodes(&planet, ground + Vec3::Y * 12.0);
        assert!(near.iter().any(|k| k.level == MAX_LEVEL));
        // Nothing from the opposite face.
        assert!(near.iter().all(|k| k.face != 5));
        assert!(near.len() < 400, "{} chunks", near.len());
        let orbit = select_nodes(&planet, Vec3::Y * 3000.0);
        assert!(orbit.iter().all(|k| k.level <= 2));
        assert!(orbit.iter().all(|k| k.face != 5));
        // Leaves tile their faces without overlap: areas sum to <= 6.
        let area: f32 = near.iter().map(|k| k.uv_rect().2.powi(2)).sum();
        assert!(area <= 6.0 + 1e-4);
    }

    #[test]
    fn chunk_vertices_lie_on_the_height_field() {
        let planet = bumpy();
        let key = NodeKey { face: 4, level: 3, x: 3, y: 4 };
        let (mesh, centre) = chunk_mesh(&planet, key);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("positions");
        };
        for p in pos.iter().take((CHUNK_CELLS + 1) * (CHUNK_CELLS + 1)) {
            let world = Vec3::from_array(*p) + centre;
            assert!((world.length() - planet.height(world)).abs() < 1e-2);
        }
        let water = water_mesh(&planet);
        assert!(water.count_vertices() == 6 * (WATER_CELLS + 1) * (WATER_CELLS + 1));
        let stage = StagePlanet {
            frame: PlanetFrame::new(PLANET_RADIUS, Vec3::new(0.2, 0.9, 0.1)),
            planet,
        };
        let cube = depth_cube_image(&stage);
        assert_eq!(cube.texture_descriptor.size.depth_or_array_layers, 6);
    }
}
