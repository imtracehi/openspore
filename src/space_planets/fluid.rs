//! The ice and lava seas of the Space planet view (helper `space-visuals`,
//! `notes/planet-look-notes.md` 11, `notes/space-visuals-notes.md`): TerrainBelowIce / TerrainBelowLava
//! (`shaders/space_fluid.wesl`) on the water sphere.
//!
//! The retail programs read the renderer's per-face textures: the height map
//! (Sampler1, .x = (h + 1) / 2) and, for the ice, its normal map. The mesh
//! planet has no per-face textures, so the height comes from a cube baked from
//! the planet's height field in the world orientation of the view (ADAPTED:
//! the other channels of the ice program's Sampler1 and the normal map are not
//! known / not baked). Textures (loader 0xFBA300, VERIFIED names): ice_ramp
//! 0x3C9C0255, ice_lowfrequency 0x3CEF83D5, lava_ramp 0xA3973EA2, lava_detail
//! 0x08C05AD9 (INFERRED: the lava program's Sampler5); the ice program's
//! near octave is the land's detail raster, the lava's low frequency the
//! theme's below raster (one copy per face, as TerrainBelowRefract).
use crate::{dbpf::ResourceKey, planet_query::Planet};
use bevy::{
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

pub const ICE_RAMP: u32 = 0x3C9C_0255;
pub const ICE_LOW_FREQUENCY: u32 = 0x3CEF_83D5;
pub const LAVA_RAMP: u32 = 0xA397_3EA2;
pub const LAVA_DETAIL: u32 = 0x08C0_5AD9;
const HEIGHT_RES: usize = 256;

pub type FluidMaterial = ExtendedMaterial<StandardMaterial, SpaceFluid>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct SpaceFluid {
    #[uniform(100)]
    pub sky: crate::planet_sky::SkyState,
    /// x: 1 ice, 2 lava.
    #[uniform(101)]
    pub mode: Vec4,
    #[texture(102, dimension = "cube")]
    #[sampler(103)]
    pub height: Handle<Image>,
    #[texture(104, dimension = "cube")]
    #[sampler(105)]
    pub scatter: Handle<Image>,
    #[texture(106)]
    #[sampler(107)]
    pub ramp: Handle<Image>,
    #[texture(108)]
    #[sampler(109)]
    pub noise: Handle<Image>,
    #[texture(110)]
    #[sampler(111)]
    pub detail: Handle<Image>,
    #[texture(112)]
    #[sampler(113)]
    pub pca0: Handle<Image>,
    #[texture(114)]
    #[sampler(115)]
    pub pca1: Handle<Image>,
}

impl MaterialExtension for SpaceFluid {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/space_fluid.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// (h + 1) / 2 of the terrain as an R8 cube in WORLD orientation.
pub fn height_cube(planet: &Planet, rotation: Quat) -> Image {
    let n = HEIGHT_RES;
    let to_local = rotation.inverse();
    let mut data = Vec::with_capacity(6 * n * n);
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let s = 2.0 * (x as f32 + 0.5) / n as f32 - 1.0;
                let t = 2.0 * (y as f32 + 0.5) / n as f32 - 1.0;
                let d = match face {
                    0 => Vec3::new(1.0, -t, -s),
                    1 => Vec3::new(-1.0, -t, s),
                    2 => Vec3::new(s, 1.0, t),
                    3 => Vec3::new(s, -1.0, -t),
                    4 => Vec3::new(s, -t, 1.0),
                    _ => Vec3::new(-s, -t, -1.0),
                };
                let h = planet.h_at(to_local * d.normalize());
                data.push((((h + 1.0) * 0.5).clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
    }
    crate::planet_render::cube_image(n as u32, data)
}

fn raster(world: &World, instance_id: u32, srgb: bool) -> Option<Image> {
    let index = &world.get_resource::<crate::Originals>()?.resource_index;
    let key = ResourceKey { group_id: 0, instance_id, type_id: crate::asset_catalog::RW4 };
    let mut image = crate::planet_sky::rw4_image(index, key, true, srgb)?;
    // The ramps are looked up at the edge column: clamp them.
    if instance_id == ICE_RAMP || instance_id == LAVA_RAMP {
        use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageFilterMode};
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..default()
        });
    }
    Some(image)
}

/// The ice (`lava` false) or lava sea material of a world.
#[allow(clippy::too_many_arguments)]
pub fn material(
    world: &mut World,
    planet: &Planet,
    rotation: Quat,
    lava: bool,
    sky: &super::sky::SpaceSky,
    land_detail: Option<Handle<Image>>,
) -> Option<Handle<FluidMaterial>> {
    if !world.contains_resource::<Assets<FluidMaterial>>() {
        return None;
    }
    let (ramp, noise) = if lava { (LAVA_RAMP, LAVA_DETAIL) } else { (ICE_RAMP, ICE_LOW_FREQUENCY) };
    // Display values (the retail programs work in gamma space).
    let ramp = raster(world, ramp, false)?;
    let noise = raster(world, noise, false)?;
    let height = height_cube(planet, rotation);
    let (ramp, noise, height) = {
        let mut images = world.resource_mut::<Assets<Image>>();
        (images.add(ramp), images.add(noise), images.add(height))
    };
    let detail = if lava { sky.below.clone() } else { land_detail.unwrap_or_else(|| sky.below.clone()) };
    Some(world.resource_mut::<Assets<FluidMaterial>>().add(FluidMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            fog_enabled: false,
            ..default()
        },
        extension: SpaceFluid {
            sky: [Vec4::ZERO; crate::planet_sky::SKY_SLOTS],
            mode: Vec4::new(if lava { 2.0 } else { 1.0 }, 0.0, 0.0, 0.0),
            height,
            scatter: sky.scatter.clone(),
            ramp,
            noise,
            detail,
            pca0: sky.pca[0].clone(),
            pca1: sky.pca[1].clone(),
        },
    }))
}

/// Register the material (render apps).
pub fn build(app: &mut App) {
    app.add_plugins(MaterialPlugin::<FluidMaterial>::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_cube_stores_half_offset_heights() {
        let planet = Planet::flat(0.5);
        let image = height_cube(&planet, Quat::IDENTITY);
        let data = image.data.as_ref().unwrap();
        assert!(data.iter().all(|&v| (v as i32 - 191).abs() <= 1), "{}", data[0]);
    }
}
