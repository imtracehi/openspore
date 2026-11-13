//! Installed ocean rasters in an adapted Bevy material. Source pixels and mip
//! chains are retained; flow and lighting are preview mappings, not native PCA.
use crate::{asset_catalog::RW4, dbpf::ResourceKey, world_presentation::tune};
use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat},
    shader::ShaderRef,
};

pub const WATER_NORMAL: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0xB4914B57,
    type_id: RW4,
};
pub const FOAM_CUT: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0x9D8D0398,
    type_id: RW4,
};
pub const SEABED: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0xC66C3FCD,
    type_id: RW4,
};
#[derive(Clone)]
pub struct WaterRasterSource {
    pub name: &'static str,
    pub key: ResourceKey,
    pub image: Handle<Image>,
    pub dimensions: UVec2,
    pub linear: bool,
}
#[derive(Component, Clone)]
pub struct OriginalWaterSources(pub Vec<WaterRasterSource>);

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterWaves>;
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WaterWaves {
    #[uniform(100)]
    pub parameters: Vec4,
    #[uniform(101)]
    pub coast: Vec4,
    #[texture(102)]
    #[sampler(103)]
    pub normal_texture: Handle<Image>,
    #[texture(104)]
    #[sampler(105)]
    pub foam_texture: Handle<Image>,
    #[texture(106)]
    #[sampler(107)]
    pub seabed_texture: Handle<Image>,
    #[uniform(108)]
    pub native_layers: Vec4,
    #[uniform(109)]
    pub shallow_color: Vec4,
    #[uniform(110)]
    pub sky_color: Vec4,
    /// x: foam strength, y: sky fresnel strength, z: seabed visibility,
    /// w: shallow-shelf depth (world units) over which turquoise fades.
    #[uniform(111)]
    pub tuning: Vec4,
    /// Planet mode (`planet_render`): xyz planet centre, w water radius;
    /// zero for the flat island sea.
    #[uniform(112)]
    pub planet: Vec4,
    /// Water depth cube (R8, world orientation; a 1x1 neutral cube when flat).
    #[texture(113, dimension = "cube")]
    #[sampler(114)]
    pub depth_cube: Handle<Image>,
    /// Retail TerrainWater state (`planet_sky::SkyState`; slot 15 x = 1
    /// switches the planet sea to the decoded program).
    #[uniform(115)]
    pub sky: crate::planet_sky::SkyState,
    /// The in-scatter cube (fog colour and sky reflection).
    #[texture(116, dimension = "cube")]
    #[sampler(117)]
    pub scatter: Handle<Image>,
    /// PCAwater2_0 / PCAwater2_1 (PCA normal components).
    #[texture(118)]
    #[sampler(119)]
    pub pca0: Handle<Image>,
    #[texture(120)]
    #[sampler(121)]
    pub pca1: Handle<Image>,
}
impl Default for WaterWaves {
    fn default() -> Self {
        Self {
            parameters: Vec4::new(0., 0.035, 0.24, 0.),
            coast: Vec4::ZERO,
            normal_texture: Handle::default(),
            foam_texture: Handle::default(),
            seabed_texture: Handle::default(),
            native_layers: Vec4::ZERO,
            shallow_color: Vec4::new(0.10, 0.32, 0.35, 1.),
            sky_color: Vec4::new(0.42, 0.65, 0.82, 1.),
            tuning: Vec4::new(0.85, 0.65, 0.45, 1.6),
            planet: Vec4::ZERO,
            depth_cube: Handle::default(),
            sky: [Vec4::ZERO; crate::planet_sky::SKY_SLOTS],
            scatter: crate::planet_sky::NEUTRAL_CUBE,
            pca0: Handle::default(),
            pca1: Handle::default(),
        }
    }
}
impl MaterialExtension for WaterWaves {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/water.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}
pub struct WaterPlugin;
impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/water.wesl");
        crate::world_presentation::dev_shader_override(app, "water.wesl");
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
    }
}
pub fn animate(time: Res<Time>, mut materials: ResMut<Assets<WaterMaterial>>) {
    for (_, material) in materials.iter_mut() {
        material.extension.parameters.x = time.elapsed_secs();
    }
}

fn raster_image(mips: Vec<crate::texture::Pixels>, linear: bool) -> Option<(Image, UVec2)> {
    let first = mips.first()?;
    let dimensions = UVec2::new(first.width, first.height);
    let mut image = Image::new(
        Extent3d {
            width: first.width,
            height: first.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        first.rgba.clone(),
        if linear {
            TextureFormat::Rgba8Unorm
        } else {
            TextureFormat::Rgba8UnormSrgb
        },
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image.texture_descriptor.mip_level_count = mips.len() as u32;
    image.data = Some(mips.into_iter().flat_map(|p| p.rgba).collect());
    Some((image, dimensions))
}

/// The SDK documents foam and seabed as TerrainTextures inputs. The named
/// normal is an installed preview input, not a replacement for native PCA.
pub fn material(
    world: &mut World,
    palette: Option<&crate::world_catalog::PlanetPalette>,
    phase: f32,
    extent: f32,
) -> (Handle<WaterMaterial>, OriginalWaterSources) {
    let mut sources = vec![];
    let mut load = |name, key, linear, fallback| {
        let decoded = world
            .get_resource::<crate::Originals>()
            .and_then(|o| o.resource_index.read(key).ok())
            .and_then(|bytes| crate::texture::decode_rw4_mips(&bytes))
            .and_then(|mips| raster_image(mips, linear));
        let present = decoded.is_some();
        let (image, dimensions) = decoded.unwrap_or_else(|| {
            raster_image(
                vec![crate::texture::Pixels {
                    width: 1,
                    height: 1,
                    rgba: fallback,
                }],
                linear,
            )
            .unwrap()
        });
        let handle = world.resource_mut::<Assets<Image>>().add(image);
        if present {
            sources.push(WaterRasterSource {
                name,
                key,
                image: handle.clone(),
                dimensions,
                linear,
            });
        }
        (handle, if present { 1. } else { 0. })
    };
    let (normal_texture, normal) = load(
        "water_a_normals",
        WATER_NORMAL,
        true,
        vec![128, 128, 255, 255],
    );
    let (foam_texture, foam) = load("water_foamline", FOAM_CUT, true, vec![0, 0, 0, 255]);
    let (seabed_texture, seabed) = load(
        "seabed_grand_shallow",
        SEABED,
        false,
        vec![128, 128, 128, 255],
    );
    let water = Vec3::from_array(palette.map_or([0.06, 0.21, 0.30], |p| p.water));
    // Preview shading: the authored WaterColor is the deep tint; shallow
    // shelves read turquoise where the native refraction pass would show the
    // sandy seabed through it.
    let deep =
        water.lerp(Vec3::new(0.02, 0.42, 0.66), tune("WATER_CYAN", 0.45)) * tune("WATER_DEEP", 0.8);
    let shallow = water.lerp(Vec3::new(0.16, 0.86, 0.84), tune("WATER_TURQ", 0.62));
    let shallow = Color::srgb(shallow.x, shallow.y, shallow.z)
        .to_linear()
        .to_f32_array();
    // Same pale horizon the land sky derives from AirColor.
    let sky = Vec3::from_array(palette.map_or([0.67, 0.80, 0.91], |p| p.atmosphere))
        .lerp(Vec3::ONE, tune("HOR_MIX", 0.52));
    let sky = Color::srgb(sky.x, sky.y, sky.z).to_linear().to_f32_array();
    let depth_cube = world
        .resource_mut::<Assets<Image>>()
        .add(crate::planet_render::cube_image(1, vec![0; 6]));
    let handle = world
        .resource_mut::<Assets<WaterMaterial>>()
        .add(WaterMaterial {
            base: StandardMaterial {
                base_color: Color::srgb(deep.x, deep.y, deep.z),
                perceptual_roughness: tune("WATER_ROUGH", 0.16),
                reflectance: 0.5,
                ..default()
            },
            extension: WaterWaves {
                coast: Vec4::new(phase, extent, 1., 0.),
                normal_texture,
                foam_texture,
                seabed_texture,
                native_layers: Vec4::new(normal, foam, seabed, 0.),
                tuning: Vec4::new(
                    tune("WATER_FOAM", 0.85),
                    tune("WATER_FRESNEL", 0.65),
                    tune("WATER_BED", 0.45),
                    tune("WATER_SHALLOW", 1.6),
                ),
                shallow_color: Vec4::from_array(shallow),
                sky_color: Vec4::from_array(sky),
                depth_cube,
                ..default()
            },
        });
    (handle, OriginalWaterSources(sources))
}

/// Switch a water material to the planet's water sphere: shading in local up,
/// depth from the planet's depth cube (`depth_scale` units per 1.0).
pub fn set_planet(
    world: &mut World,
    handle: &Handle<WaterMaterial>,
    centre: Vec3,
    water_radius: f32,
    depth_cube: Image,
    depth_scale: f32,
) {
    let cube = world.resource_mut::<Assets<Image>>().add(depth_cube);
    if let Some(mut material) = world.resource_mut::<Assets<WaterMaterial>>().get_mut(handle) {
        material.extension.planet = centre.extend(water_radius);
        material.extension.depth_cube = cube;
        material.extension.coast.w = depth_scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_install_uses_explicit_neutral_layers() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<WaterMaterial>>();
        let (handle, sources) = material(&mut world, None, 0.4, 42.);
        assert!(sources.0.is_empty());
        let materials = world.resource::<Assets<WaterMaterial>>();
        let water = &materials.get(&handle).unwrap().extension;
        assert_eq!(water.native_layers, Vec4::ZERO);
        assert_eq!(water.coast, Vec4::new(0.4, 42., 1., 0.));
        let images = world.resource::<Assets<Image>>();
        assert_eq!(
            images.get(&water.normal_texture).unwrap().data.as_deref(),
            Some(&[128, 128, 255, 255][..])
        );
        assert_eq!(
            images
                .get(&water.normal_texture)
                .unwrap()
                .texture_descriptor
                .format,
            TextureFormat::Rgba8Unorm
        );
    }
    #[test]
    #[ignore = "requires installed original ocean rasters"]
    fn installed_water_rasters_retain_pixels_mips_and_channel_space() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        std::fs::create_dir_all("target/water-067").unwrap();
        let mut rows = vec![];
        for (name, key, linear) in [
            ("water_a_normals", WATER_NORMAL, true),
            ("water_foamline", FOAM_CUT, true),
            ("seabed_grand_shallow", SEABED, false),
        ] {
            assert_eq!(crate::world_objects::name_id(name), key.instance_id);
            let mips = crate::texture::decode_rw4_mips(&index.read(key).unwrap()).unwrap();
            assert!(mips[0].width > 1 && mips[0].height > 1);
            let expected = mips[0].rgba.clone();
            let levels = mips.len();
            let (image, dimensions) = raster_image(mips, linear).unwrap();
            assert_eq!(image.texture_descriptor.mip_level_count, levels as u32);
            assert_eq!(&image.data.as_ref().unwrap()[..expected.len()], expected);
            let mut png_image = image.clone();
            png_image.texture_descriptor.mip_level_count = 1;
            png_image.texture_descriptor.format = TextureFormat::Rgba8UnormSrgb;
            png_image.data.as_mut().unwrap().truncate(expected.len());
            png_image
                .try_into_dynamic()
                .unwrap()
                .save(format!("target/water-067/{name}.png"))
                .unwrap();
            rows.push(serde_json::json!({"name":name,"key":crate::resource_audit::key_json(key),"dimensions":dimensions.to_array(),"mip_levels":levels,"linear":linear}));
        }
        std::fs::write(
            "target/water-067/rasters.json",
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
    }
}
