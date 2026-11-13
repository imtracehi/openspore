//! The retail planet look on the Space planet view (helper `space-visuals`,
//! `notes/space-visuals-notes.md`): the stage planets' TerrainState
//! (`planet_sky.rs`) for the world under the UFO, so the land, the sea
//! (TerrainWater), the sea floor (TerrainBelowRefract), the in-scatter fog and
//! the TerrainAtmosphere shell run the same decoded programs as in the
//! Creature..Civ stages.
//!
//! Original data / code (VERIFIED, see the notes): the in-scatter cube and its
//! lobes (0xF9D960 / 0xFBC740), the TerrainState slots (0xFBF540 / 0xF98910),
//! kFogByElevSpace 0xA219CC45 (PlanetMaterial dump), the water colour table by
//! terrainWaterType, the sky tables by terrainAtmosphereType, the PCA water,
//! the atmosphere_packed_curves; T / A = the world's current temperature /
//! atmosphere scores (cTerrainStateMgr +0x30 / +0x2C hold the planet's scores).
//!
//! ADAPTED: the TerrainState is kept in planet units and the shaders divide
//! world positions by `unit` (sky[9].w), because the Space view draws the
//! planet at `PlanetView::unit` scene units per retail unit; the local hour of
//! kFogDistBiasByTimeOfDay is read from the sun angle (morning side); the
//! atmosphere-type sky table is opt-in (`OPENSPORE_PLANET_SKY=1`) as on the
//! stages, otherwise lobe 1 takes the world's air colour (the user-colour
//! branch); the sea's depth cube is baked once in the world orientation at
//! entry (the viewed world holds still, `space::space_orbits`).
use crate::{
    planet_query::Planet,
    planet_sky::{self as ps, SkyInputs, SkyState, SkyTables, SKY_SLOTS},
    stages::GameStage,
};
use bevy::prelude::*;

/// The decoded planet look is on unless `OPENSPORE_RETAIL_SKY=0` (the
/// adapted Space shell, haze and Bevy-lit sea stay available for A/B checks).
pub fn enabled() -> bool {
    !std::env::var("OPENSPORE_RETAIL_SKY").is_ok_and(|v| v == "0")
}

/// Water depth cube resolution of the Space sea (planet_render uses 256).
const DEPTH_RES: usize = 256;

/// Live sky of the planet under the UFO.
pub struct SpaceSky {
    pub inputs: SkyInputs,
    tables: SkyTables,
    pca_frames: Vec<[f32; 68]>,
    pub pca: [Handle<Image>; 2],
    /// terrainThemeBelow of the world's theme (the sea floor raster).
    pub below: Handle<Image>,
    pub curves: Handle<Image>,
    pub scatter: Handle<Image>,
    user_air: Option<[f32; 3]>,
    baked_p: f32,
    /// kAtmThicknessByAtmosphereScore(A) x altRange, planet units.
    pub thickness: f32,
    pub water_color: [f32; 3],
}

impl SpaceSky {
    /// Load the world's sky (tables by its atmosphere / water type, the shared
    /// curves and PCA water) and bake the first scatter cube.
    pub fn load(
        world: &mut World,
        inputs: SkyInputs,
        below_instance: Option<u32>,
        air: [f32; 3],
        altitude_range: f32,
    ) -> Option<Self> {
        let table_sky = std::env::var("OPENSPORE_PLANET_SKY").is_ok_and(|v| v == "1");
        let originals = world.get_resource::<crate::Originals>()?;
        let index = &originals.resource_index;
        let tables = SkyTables::load(index, inputs.atmosphere_type, inputs.water_type)?;
        let curves = index
            .read(ps::PACKED_CURVES)
            .ok()
            .and_then(|b| crate::texture::decode_rw4_mips(&b))
            .and_then(|m| m.into_iter().next())
            .map(|p| ps::image_rgba8(p.width, p.height, p.rgba, false))?;
        let key = |instance_id| crate::dbpf::ResourceKey { group_id: 0, instance_id, type_id: crate::asset_catalog::RW4 };
        let pca = [ps::rw4_image(index, ps::PCA0, true, false), ps::rw4_image(index, ps::PCA1, true, false)];
        let below = below_instance
            .and_then(|i| ps::rw4_image(index, key(i), true, true))
            .or_else(|| ps::rw4_image(index, key(ps::SEABED_DEFAULT), true, true));
        let pca_frames = index.read(ps::PCAW).ok().and_then(|b| ps::decode_pcaw(&b)).unwrap_or_default();
        let water_color = tables.water_color(&inputs);
        let user_air = (!table_sky).then_some(air);
        let p = 0.39;
        let cube = ps::scatter_cube(&tables.lobes(&inputs, p, user_air));
        if !world.contains_resource::<crate::planet_light::TerrainLighting>() {
            let lighting = world
                .get_resource::<crate::Originals>()
                .map(|o| crate::planet_light::TerrainLighting::load(&o.resource_index))
                .unwrap_or_default();
            world.insert_resource(lighting);
        }
        let thickness = ps::atmosphere_thickness(inputs.atmosphere) * altitude_range;
        info!(
            "space sky: T {:.2} A {:.2} atmosphere type {} water type {} thickness {:.1} water {:?} table sky {}",
            inputs.temperature, inputs.atmosphere, inputs.atmosphere_type, inputs.water_type, thickness, water_color, table_sky
        );
        let mut images = world.resource_mut::<Assets<Image>>();
        let pca = pca.map(|i| i.map(|i| images.add(i)).unwrap_or_default());
        let below = below.map(|i| images.add(i)).unwrap_or_default();
        let curves = images.add(curves);
        let scatter = images.add(cube);
        Some(Self {
            inputs,
            tables,
            pca_frames,
            pca,
            below,
            curves,
            scatter,
            user_air,
            baked_p: p,
            thickness,
            water_color,
        })
    }

    /// The TerrainState of this frame (0xFBF540 with the Space fog table),
    /// re-baking the scatter cube when the camera altitude row moves.
    pub fn update(&mut self, world: &mut World, centre: Vec3, unit: f32, planet: &Planet, camera: Vec3) -> SkyState {
        let unit = unit.max(1.0e-6);
        let local = (camera - centre) / unit;
        let altitude = local.length() - planet.radius;
        // The system's star is the scene origin.
        let sun = (-centre).normalize_or(Vec3::Y);
        let p = (altitude / ps::SKY_ALTITUDE_RANGE).clamp(0.0, 1.0) * 0.6 + 0.39;
        let lobes = self.tables.lobes(&self.inputs, p, self.user_air);
        if (p - self.baked_p).abs() > 1.0 / 128.0 {
            let cube = ps::scatter_cube(&lobes);
            if let Some(mut slot) = world.resource_mut::<Assets<Image>>().get_mut(&self.scatter) {
                *slot = cube;
            }
            self.baked_p = p;
        }
        let time = world.get_resource::<Time>().map_or(0.0, |t| t.elapsed_secs());
        // ADAPTED local hour for kFogDistBiasByTimeOfDay: 12 under the sun,
        // 6 on the terminator, 0 opposite (the morning half of the day).
        let cos = local.normalize_or(Vec3::Y).dot(sun).clamp(-1.0, 1.0);
        let hour = 12.0 - 12.0 * cos.acos() / std::f32::consts::PI;
        let sphere_w = ((1.0 - (altitude / (self.thickness + 1e-6)).clamp(0.0, 1.0)) + 1.0) * 0.5;
        let mut s = [Vec4::ZERO; SKY_SLOTS];
        s[0] = sun.extend(ps::CUTOFF);
        let w = self.water_color;
        s[1] = Vec4::new(w[0], w[1], w[2], 1.0);
        s[2] = centre.extend(planet.water_radius());
        s[3] = Vec4::new(planet.radius, planet.altitude_range, self.thickness, sphere_w);
        s[4] = ps::fog_parms(GameStage::Space, &self.inputs, &lobes, altitude, hour, sphere_w);
        s[5] = Vec4::new(ps::ATM_LUMINANCE_RANGE.0, ps::ATM_LUMINANCE_RANGE.1, 0.0, 0.0);
        let frame = ps::scatter_frame(sun);
        s[6] = frame[0];
        s[7] = frame[1];
        s[8] = frame[2];
        // w: world units per planet unit (the shaders measure in planet units).
        s[9] = Vec4::new(time, crate::planet_render::DEPTH_SCALE, crate::planet_light::land_diffuse_scale(), unit);
        if !self.pca_frames.is_empty() {
            let f = &self.pca_frames[((time * ps::PCA_FPS) as usize) % self.pca_frames.len()];
            let reg = |r: usize| Vec4::new(f[r * 4], f[r * 4 + 1], f[r * 4 + 2], f[r * 4 + 3]);
            s[10] = reg(0);
            s[11] = reg(2);
            s[12] = reg(4);
            s[13] = reg(6);
            s[14] = reg(16);
        }
        let debug = std::env::var("OPENSPORE_WATER_DEBUG").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let retail_light = !std::env::var("OPENSPORE_RETAIL_LIGHT").is_ok_and(|v| v == "0");
        s[15] = Vec4::new(1.0, if self.pca_frames.is_empty() { 0.0 } else { 1.0 }, debug, if retail_light { 1.0 } else { 0.0 });
        if let Some(lighting) = world.get_resource::<crate::planet_light::TerrainLighting>() {
            s[16..24].copy_from_slice(&lighting.slots());
        }
        s
    }
}

/// `OPENSPORE_FRAME_STATS=1`: mean / max wall-clock frame time of each 60
/// planet-view frames (the frame-time report).
pub fn frame_stats() {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("OPENSPORE_FRAME_STATS").is_some()) {
        return;
    }
    static SAMPLES: std::sync::Mutex<Vec<f32>> = std::sync::Mutex::new(Vec::new());
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let now = std::time::Instant::now();
    let dt = LAST.lock().unwrap().replace(now).map_or(0.0, |t| (now - t).as_secs_f32() * 1000.0);
    let mut s = SAMPLES.lock().unwrap();
    s.push(dt);
    if s.len() == 60 {
        let mean = s.iter().sum::<f32>() / 60.0;
        let max = s.iter().cloned().fold(0.0, f32::max);
        eprintln!("space frame stats: mean {mean:.2} ms, max {max:.2} ms (60 frames)");
        s.clear();
    }
}

/// Water depth below the sea surface (planet units / `DEPTH_SCALE`) as an R8
/// cube in WORLD orientation for a planet drawn with `rotation`
/// (`planet_render::depth_cube_image` for the stage frame).
pub fn depth_cube(planet: &Planet, rotation: Quat) -> Image {
    let n = DEPTH_RES;
    let water = planet.water_radius();
    let to_local = rotation.inverse();
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
                let depth = water - planet.height(to_local * d.normalize());
                data.push(((depth / crate::planet_render::DEPTH_SCALE).clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
    }
    crate::planet_render::cube_image(n as u32, data)
}

/// The sea sphere of the Space view: the stage planets' TerrainWater material
/// (water.rs, decoded program when sky[15].x = 1) on `planet_render::water_mesh`.
pub fn sea_material(
    world: &mut World,
    planet: &Planet,
    rotation: Quat,
    centre: Vec3,
    unit: f32,
    phase: f32,
    sky: &SpaceSky,
) -> Option<Handle<crate::water::WaterMaterial>> {
    if !world.contains_resource::<Assets<crate::water::WaterMaterial>>() {
        return None;
    }
    let (handle, _) = crate::water::material(world, None, phase, 0.0);
    let cube = depth_cube(planet, rotation);
    crate::water::set_planet(world, &handle, centre, planet.water_radius() * unit, cube, crate::planet_render::DEPTH_SCALE);
    if let Some(mut m) = world.resource_mut::<Assets<crate::water::WaterMaterial>>().get_mut(&handle) {
        m.base.alpha_mode = AlphaMode::Premultiplied;
        m.base.fog_enabled = false;
        m.extension.scatter = sky.scatter.clone();
        m.extension.pca0 = sky.pca[0].clone();
        m.extension.pca1 = sky.pca[1].clone();
    }
    Some(handle)
}

/// The TerrainAtmosphere shell (planet_sky.wesl) at R + thickness, planet
/// units (the caller parents it to the scaled surface root).
pub fn shell(world: &mut World, planet: &Planet, sky: &SpaceSky) -> Option<(Mesh, Handle<ps::RetailAtmosphereMaterial>)> {
    if sky.thickness <= 0.0 || !world.contains_resource::<Assets<ps::RetailAtmosphereMaterial>>() {
        return None;
    }
    let mesh = Sphere::new(planet.radius + sky.thickness).mesh().ico(5).ok()?;
    let material = world.resource_mut::<Assets<ps::RetailAtmosphereMaterial>>().add(ps::RetailAtmosphereMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            fog_enabled: false,
            // Back faces only (the stages' shell): the far side of the shell
            // is the sky from the ground and the limb from space.
            cull_mode: Some(bevy::render::render_resource::Face::Front),
            alpha_mode: AlphaMode::Blend,
            ..default()
        },
        extension: ps::RetailAtmosphere {
            sky: [Vec4::ZERO; SKY_SLOTS],
            scatter: sky.scatter.clone(),
            curves: sky.curves.clone(),
        },
    });
    Some((mesh, material))
}

/// weatherCloudType 0x03B4DE49 (int 1..6, theme property) and the cloud set
/// names of the table 0x014914CC it indexes (0xFC2ED0, VERIFIED):
/// cloudDefault, cloudCurly, cloudAlien, cloudWeb, cloudPollen, cloudSponge.
pub const PROP_WEATHER_CLOUD_TYPE: u32 = 0x03B4_DE49;

/// terrainAtmosphereType / terrainWaterType of a Space world whose script has
/// neither property: 11 (0xF35450 starts both at 0xB, VERIFIED).
pub const SPACE_DEFAULT_TYPE: u32 = 11;

/// The world's sky inputs, sea-floor raster and cloud type from its terrain
/// script (on the generation worker).
pub fn script_inputs(
    index: &crate::asset_catalog::ResourceIndex,
    d: &crate::planet_data::PlanetDescriptor,
) -> (SkyInputs, Option<u32>, u8) {
    let inputs = SkyInputs::from_descriptor_or(d, SPACE_DEFAULT_TYPE);
    let below = d
        .properties
        .0
        .get(&crate::world_catalog::BELOW_TEXTURE)
        .and_then(|p| p.key())
        .map(|k| k.instance_id);
    let cloud = d
        .properties
        .0
        .get(&PROP_WEATHER_CLOUD_TYPE)
        .and_then(|p| p.number())
        .or_else(|| ps::theme_twin(index, d).and_then(|t| t.0.get(&PROP_WEATHER_CLOUD_TYPE).and_then(|p| p.number())))
        .map_or(1, |v| (v as i32).clamp(1, 6) as u8);
    (inputs, below, cloud)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet_query::HeightCube;

    #[test]
    fn depth_cube_follows_the_planet_rotation() {
        // A world with one deep basin around local +X.
        let res = 32;
        let mut cube = HeightCube::filled(res, 0.2);
        for face in 0..6 {
            for row in 0..res {
                for col in 0..res {
                    let d = crate::planet_query::texel_dir(face, row, col, res);
                    if d.x > 0.9 {
                        let i = cube.index(face, row, col);
                        cube.raw_mut()[i] = crate::planet_query::h_to_raw(-0.9);
                    }
                }
            }
        }
        let planet = Planet::new(cube, 500.0, 0.0);
        let rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let image = depth_cube(&planet, rotation);
        let data = image.data.as_ref().unwrap();
        let n = DEPTH_RES;
        // The basin now faces world +Y (face 2, centre texel).
        let up = data[2 * n * n + (n / 2) * n + n / 2];
        let side = data[(n / 2) * n + n / 2];
        assert!(up > 200 && side == 0, "up {up} side {side}");
    }
}
