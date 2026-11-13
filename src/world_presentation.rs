//! Installed planet presentation in the preview renderer. Authored values are
//! preserved; selecting a home theme, PBR exposure, fog and sky interpolation
//! are preview approximations, not the original atmosphere/lighting shaders.
use crate::{
    asset_catalog::{ResourceIndex, PROP},
    dbpf::ResourceKey,
    stages::GameStage,
    world_catalog::{PlanetMaterials, PlanetPalette, TerrainOverrides, WorldDescriptor},
};
use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    camera::Exposure,
    core_pipeline::tonemapping::Tonemapping,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::{CascadeShadowConfig, CascadeShadowConfigBuilder, NotShadowCaster},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::{
        render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat},
        view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
    },
    shader::ShaderRef,
};

/// Temporary preview tuning hook (development only).
pub(crate) fn tune(name: &str, default: f32) -> f32 {
    std::env::var(format!("OS_{name}"))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Temporary development override: re-register an embedded shader from disk.
pub(crate) fn dev_shader_override(app: &mut App, name: &str) {
    if let Ok(dir) = std::env::var("OS_SHADER_DIR") {
        if let Ok(bytes) = std::fs::read(std::path::Path::new(&dir).join(name)) {
            app.world_mut()
                .resource_mut::<bevy::asset::io::embedded::EmbeddedAssetRegistry>()
                .insert_asset(
                    std::path::PathBuf::new(),
                    std::path::Path::new(&format!("openspore/shaders/{name}")),
                    bytes,
                );
        }
    }
}

pub const HOME_PALETTE: ResourceKey = ResourceKey {
    group_id: 0x406B0100,
    instance_id: 0x6F78B3C5,
    type_id: PROP,
};
pub const MASTER_THEME: ResourceKey = ResourceKey {
    group_id: 0x42840100,
    instance_id: 0x6E660BEB,
    type_id: PROP,
};
pub const PLANET_LIGHTING: ResourceKey = ResourceKey {
    group_id: 0x40200100,
    instance_id: 0x5F848353,
    type_id: PROP,
};
pub const CLOUD_RASTER: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0xA50F5A46,
    type_id: crate::asset_catalog::RW4,
};

#[derive(Resource, Clone)]
pub struct LandTheme {
    pub palette: PlanetPalette,
    pub textures: TerrainOverrides,
    pub materials: PlanetMaterials,
}
impl LandTheme {
    pub fn load(index: &ResourceIndex) -> Result<Self, String> {
        let palette =
            PlanetPalette::from_properties(HOME_PALETTE, &index.properties(HOME_PALETTE)?)?;
        let textures = WorldDescriptor::parse(MASTER_THEME, index.properties(MASTER_THEME)?)?
            .terrain_overrides()?;
        Ok(Self {
            palette,
            textures,
            materials: PlanetMaterials::load(index),
        })
    }
}
#[derive(Component)]
pub struct PreviewAtmosphere;
/// Proven GPU source bound to the preview sky. The resource is globally named
/// `cloud_lit`; this does not claim that the selected planet theme references it.
#[derive(Component, Clone)]
pub struct OriginalCloudSource {
    pub key: ResourceKey,
    pub image: Handle<Image>,
    pub dimensions: UVec2,
}
#[derive(Component)]
struct PreviewFillLight;
#[derive(Resource)]
struct AppliedStage(GameStage);

/// Adapted atmospheric preview. The home palette and cloud raster are original
/// installed resources; sky projection and cloud motion are renderer mappings.
pub type PreviewSkyMaterial = ExtendedMaterial<StandardMaterial, PreviewSky>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct PreviewSky {
    #[uniform(100)]
    pub parameters: Vec4,
    #[uniform(101)]
    pub atmosphere: Vec4,
    #[texture(102)]
    #[sampler(103)]
    pub cloud_texture: Handle<Image>,
    #[uniform(104)]
    pub cloud_controls: Vec4,
    /// Linear zenith color; `w` scales the gradient exponent.
    #[uniform(105)]
    pub zenith: Vec4,
    /// Linear horizon/haze color, shared with the land distance fog.
    #[uniform(106)]
    pub horizon: Vec4,
    /// Direction toward the preview sun; `w` is the halo strength.
    #[uniform(107)]
    pub sun: Vec4,
}

fn read_cloud_mips(index: &ResourceIndex) -> Option<Vec<crate::texture::Pixels>> {
    let bytes = index.read(CLOUD_RASTER).ok()?;
    crate::texture::decode_rw4_mips(&bytes)
}

fn image_from_mips(mips: Vec<crate::texture::Pixels>) -> Option<(Image, UVec2)> {
    let base = mips.first()?;
    let size = UVec2::new(base.width, base.height);
    let mut image = Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        base.rgba.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image.texture_descriptor.mip_level_count = mips.len() as u32;
    image.data = Some(mips.into_iter().flat_map(|mip| mip.rgba).collect());
    Some((image, size))
}

fn transparent_cloud_fallback() -> (Image, UVec2) {
    image_from_mips(vec![crate::texture::Pixels {
        width: 1,
        height: 1,
        rgba: vec![255, 255, 255, 0],
    }])
    .expect("one-pixel cloud fallback is non-empty")
}

impl MaterialExtension for PreviewSky {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/world_sky.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

pub struct PreviewSkyPlugin;
impl Plugin for PreviewSkyPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/world_sky.wesl");
        dev_shader_override(app, "world_sky.wesl");
        app.add_plugins(MaterialPlugin::<PreviewSkyMaterial>::default());
    }
}

fn follow_sky(world: &mut World, camera: Entity) {
    let editor_open = world
        .get_resource::<crate::editor::EditorState>()
        .is_some_and(|e| e.open);
    let center = world
        .get::<Transform>(camera)
        .map(|transform| transform.translation)
        .unwrap_or_default();
    let elapsed = world
        .get_resource::<Time>()
        .map(Time::elapsed_secs)
        .unwrap_or_default();
    let mut sky_materials = Vec::new();
    for (mut transform, material, mut visibility) in world
        .query_filtered::<(
            &mut Transform,
            &MeshMaterial3d<PreviewSkyMaterial>,
            &mut Visibility,
        ), With<PreviewAtmosphere>>()
        .iter_mut(world)
    {
        transform.translation = center;
        *visibility = if editor_open {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        sky_materials.push(material.0.clone());
    }
    if let Some(mut materials) = world.get_resource_mut::<Assets<PreviewSkyMaterial>>() {
        for handle in sky_materials {
            if let Some(mut material) = materials.get_mut(&handle) {
                material.extension.parameters = Vec4::new(center.x, center.y, center.z, elapsed);
            }
        }
    }
}

fn rgb(p: &crate::prop::PropertyList, id: u32) -> Option<[f32; 3]> {
    p.0.get(&id)?
        .color_rgb()
        .filter(|v| v.iter().all(|v| v.is_finite() && *v >= 0.))
}
/// Interpret installed PROP float channels through the preview's sRGB color
/// path, consistently with its ordinary preview constants. The native
/// lighting/sky transfer function has not been recovered.
fn preview_srgb_rgb(color: [f32; 3]) -> Color {
    Color::srgb(color[0], color[1], color[2])
}
fn strength(p: &crate::prop::PropertyList, id: u32) -> Option<f32> {
    p.0.get(&id)?.number().filter(|v| v.is_finite() && *v >= 0.)
}
fn authored_light_direction(p: &crate::prop::PropertyList, id: u32) -> Option<Vec3> {
    let property = p.0.get(&id)?;
    if property.array || property.values.len() != 1 {
        return None;
    }
    let direction = Vec3::from_array(property.vector3()?);
    let length_squared = direction.length_squared();
    if !direction.is_finite()
        || !length_squared.is_finite()
        || !(1.0e-8..=1.0e8).contains(&length_squared)
    {
        return None;
    }
    Some(direction / length_squared.sqrt())
}

/// Adapt the lighting property's direction to Bevy's local -Z light-ray axis.
/// These renderer-facing fields use a separate convention from mesh positions:
/// the selected installed daylight state has its sun at (0, 1, 0). Applying
/// the GMDL Z-up conversion made it graze the horizon in a GPU comparison.
/// Treating this direction as Y-up/toward the light remains a preview mapping.
fn bevy_light_transform(source_direction: Vec3) -> Transform {
    let up = if source_direction.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    Transform::IDENTITY.looking_to(-source_direction, up)
}

fn default_sun_rotation() -> Quat {
    Transform::from_xyz(6.0, 12.0, 4.0)
        .looking_at(Vec3::ZERO, Vec3::Y)
        .rotation
}

/// Late-morning preview elevation and azimuth (from +Z, the default camera
/// side, toward +X). The selected lighting state authors its sun at the
/// planet-local zenith; the native renderer derives the visible sun from
/// planet rotation and time of day, which this preview does not simulate.
fn land_sun_elevation() -> f32 {
    tune("SUN_EL", 42.).to_radians()
}
fn land_sun_azimuth() -> f32 {
    tune("SUN_AZ", -55.).to_radians()
}

/// Tilt the authored planet-local sun to the fixed preview time of day. An
/// authored zenith sun lands exactly at the preview elevation/azimuth.
fn land_sun_direction(authored: Option<Vec3>) -> Vec3 {
    let authored = authored.unwrap_or(Vec3::Y);
    let azimuth = land_sun_azimuth();
    let horizontal = Vec3::new(azimuth.sin(), 0., azimuth.cos());
    let axis = Vec3::Y.cross(horizontal).normalize();
    let tilt = std::f32::consts::FRAC_PI_2 - land_sun_elevation();
    (Quat::from_axis_angle(axis, tilt) * authored).normalize()
}

/// Preview sky/haze colors derived from the authored AirColor (sRGB
/// convention). The zenith deepens it and the horizon pales it toward the
/// bright scattering band seen at the original planet's limb.
struct LandAtmosphere {
    zenith: LinearRgba,
    horizon: LinearRgba,
    /// Fraction of the authored AirColor mixed (linearly) into the horizon
    /// color for the distance haze; the sky uses the same mix at altitude 0.
    haze_mix: f32,
    air: LinearRgba,
}
impl LandAtmosphere {
    fn haze(&self) -> LinearRgba {
        let h = self
            .horizon
            .to_vec3()
            .lerp(self.air.to_vec3(), self.haze_mix);
        LinearRgba::rgb(h.x, h.y, h.z)
    }
}
fn land_atmosphere(air: [f32; 3]) -> LandAtmosphere {
    let air = Vec3::from_array(air).clamp(Vec3::ZERO, Vec3::ONE);
    let zenith = air.powf(tune("ZEN_POW", 1.7)) * tune("ZEN_SCALE", 0.86);
    let horizon = air.lerp(Vec3::ONE, tune("HOR_MIX", 0.52));
    let linear = |c: Vec3| Color::srgb(c.x, c.y, c.z).to_linear();
    LandAtmosphere {
        zenith: linear(zenith),
        horizon: linear(horizon),
        haze_mix: tune("HAZE_MIX", 0.22),
        air: linear(air),
    }
}

fn land_tonemapping() -> Tonemapping {
    match tune("TONE", 0.) as i32 {
        1 => Tonemapping::AcesFitted,
        2 => Tonemapping::AgX,
        3 => Tonemapping::KhronosPbrNeutral,
        4 => Tonemapping::SomewhatBoringDisplayTransform,
        5 => Tonemapping::BlenderFilmic,
        6 => Tonemapping::ReinhardLuminance,
        _ => Tonemapping::TonyMcMapface,
    }
}

fn land_color_grading() -> ColorGrading {
    let section = ColorGradingSection {
        saturation: tune("SAT", 1.0),
        contrast: tune("CONTRAST", 1.0),
        ..default()
    };
    ColorGrading {
        global: ColorGradingGlobal {
            post_saturation: tune("POST_SAT", 1.0),
            ..default()
        },
        shadows: section,
        midtones: section,
        highlights: section,
    }
}

/// Fit the shadow cascades to the small preview island instead of Bevy's
/// kilometre-scale default so actor and flora shadows stay crisp.
fn land_shadow_cascades() -> CascadeShadowConfig {
    CascadeShadowConfigBuilder {
        num_cascades: 3,
        minimum_distance: 0.1,
        first_cascade_far_bound: 18.,
        maximum_distance: 140.,
        overlap_proportion: 0.2,
    }
    .build()
}

/// Run after stage entry/terrain preparation. Menus, Cell and Space restore
/// their ordinary lights and have no land atmosphere; their presentation is
/// owned by their respective stage systems.
pub fn update(world: &mut World) {
    let stage = world.resource::<State<GameStage>>().get().clone();
    let land = matches!(
        stage,
        GameStage::Creature | GameStage::Tribal | GameStage::Civilization
    );
    let cameras: Vec<_> = world
        .query_filtered::<Entity, With<crate::GameCamera>>()
        .iter(world)
        .collect();
    if cameras.is_empty() {
        return;
    }
    // Temporary development camera override: OS_CAM="x,y,z,tx,ty,tz".
    if let Some(v) = std::env::var("OS_CAM").ok().and_then(|s| {
        let v: Vec<f32> = s.split(',').filter_map(|c| c.trim().parse().ok()).collect();
        (v.len() == 6).then_some(v)
    }) {
        if land {
            world.insert_resource(crate::runtime::RenderSmokeCamera(
                Transform::from_xyz(v[0], v[1], v[2])
                    .looking_at(Vec3::new(v[3], v[4], v[5]), Vec3::Y),
            ));
        }
    }
    if world
        .get_resource::<AppliedStage>()
        .is_some_and(|s| s.0 == stage)
    {
        if land {
            follow_sky(world, cameras[0]);
        }
        return;
    }
    let old: Vec<_> = world
        .query_filtered::<Entity, Or<(With<PreviewAtmosphere>, With<PreviewFillLight>)>>()
        .iter(world)
        .collect();
    for entity in old {
        world.despawn(entity);
    }
    let theme = if land {
        world
            .get_resource::<LandTheme>()
            .cloned()
            .or_else(|| LandTheme::load(&world.resource::<crate::Originals>().resource_index).ok())
    } else {
        None
    };
    let lighting = theme
        .as_ref()
        .and_then(|theme| theme.textures.lighting_state)
        // ILightingWorld::SetLightingState consumes an instance in lighting_properties~.
        .and_then(|key| {
            world
                .resource::<crate::Originals>()
                .resource_index
                .properties(ResourceKey {
                    group_id: 0x40200100,
                    instance_id: key.instance_id,
                    type_id: PROP,
                })
                .ok()
        })
        .or_else(|| {
            if land {
                world
                    .resource::<crate::Originals>()
                    .resource_index
                    .properties(PLANET_LIGHTING)
                    .ok()
            } else {
                None
            }
        });
    let sun = if land {
        lighting
            .as_ref()
            .and_then(|p| rgb(p, 0x0100EAB9))
            .unwrap_or([1.; 3])
    } else {
        [1.; 3]
    };
    let sun_strength = if land {
        lighting
            .as_ref()
            .and_then(|p| strength(p, 0x0100EABA))
            .unwrap_or(1.)
    } else {
        1.
    };
    let atmosphere = theme
        .as_ref()
        .map(|theme| land_atmosphere(theme.palette.atmosphere));
    let sun_direction = land_sun_direction(
        lighting
            .as_ref()
            .and_then(|p| authored_light_direction(p, 0x0100EAB8)),
    );
    let suns: Vec<_> = world
        .query_filtered::<Entity, (
            With<DirectionalLight>,
            Without<PreviewFillLight>,
            With<crate::GameSun>,
        )>()
        .iter(world)
        .collect();
    for entity in suns {
        let mut entity = world.entity_mut(entity);
        if land {
            // The authored sun color is slightly pink once mapped through the
            // sRGB preview convention; warm it toward daylight white.
            let warm = Vec3::from_array(sun).lerp(Vec3::new(1.0, 0.95, 0.86), 0.6);
            let mut light = entity.get_mut::<DirectionalLight>().unwrap();
            light.color = preview_srgb_rgb(warm.to_array());
            // Bevy lux rather than the game's lighting units. The authored
            // strength scales a preview daylight level balanced against the
            // ambient fill: Spore's terrain shader keeps a soft ~2:1 lit to
            // shadow ratio (kDayLightColor 1.75 vs kDayShadowColor 0.85).
            light.illuminance = tune("SUN_LUX", 2_600.) * sun_strength;
            light.shadow_maps_enabled = true;
            entity.get_mut::<Transform>().unwrap().rotation =
                bevy_light_transform(sun_direction).rotation;
            entity.insert(land_shadow_cascades());
        } else {
            let mut light = entity.get_mut::<DirectionalLight>().unwrap();
            light.color = preview_srgb_rgb(sun);
            light.illuminance = 3000.;
            entity.get_mut::<Transform>().unwrap().rotation = default_sun_rotation();
            entity.insert(CascadeShadowConfig::default());
        }
    }
    for camera in cameras {
        if let Some(mut ambient) = world.get_mut::<AmbientLight>(camera) {
            if land {
                // The authored skylight is lilac; bias it toward the planet's
                // AirColor so shaded ground reads as a soft bluish sky fill.
                let skylight = lighting
                    .as_ref()
                    .and_then(|p| rgb(p, 0x0100EAB6))
                    .unwrap_or([1.; 3]);
                let air = theme
                    .as_ref()
                    .map_or([0.7, 0.85, 1.0], |theme| theme.palette.atmosphere);
                let fill = Vec3::from_array(skylight)
                    .lerp(Vec3::from_array(air).lerp(Vec3::ONE, 0.45), 0.7);
                ambient.color = preview_srgb_rgb(fill.to_array());
                ambient.brightness = tune("AMB", 1_900.)
                    * lighting
                        .as_ref()
                        .and_then(|p| strength(p, 0x0100EAB7))
                        .unwrap_or(1.);
            } else {
                ambient.color = preview_srgb_rgb([1.; 3]);
                ambient.brightness = 300.;
            }
        }
        if land {
            world.entity_mut(camera).insert((
                land_tonemapping(),
                Exposure {
                    ev100: tune("EV", Exposure::EV100_BLENDER),
                },
                land_color_grading(),
            ));
            if let Some(atmosphere) = &atmosphere {
                world.entity_mut(camera).insert(DistanceFog {
                    color: atmosphere.haze().into(),
                    directional_light_color: Color::srgba(1.0, 0.93, 0.78, tune("GLARE", 0.35)),
                    directional_light_exponent: 18.,
                    // Preview range adaptation of kFogMinDistance 70 /
                    // kFogMaxDensity: the island stays crisp while the
                    // horizon islands and sea fade into the authored air.
                    falloff: if tune("FOG_MODE", 1.) > 0.5 {
                        FogFalloff::Linear {
                            start: tune("FOG_START", 40.),
                            end: tune("FOG_END", 270.),
                        }
                    } else {
                        FogFalloff::from_visibility_squared(tune("FOG_VIS", 230.))
                    },
                });
            } else {
                world.entity_mut(camera).remove::<DistanceFog>();
            }
        } else {
            // Stage accents belong to UI presentation. Let Cell and Space
            // keep their own background and do not carry terrestrial haze.
            world.entity_mut(camera).remove::<DistanceFog>();
            world.entity_mut(camera).insert((
                Tonemapping::default(),
                Exposure::default(),
                ColorGrading::default(),
            ));
        }
    }
    if let Some(p) = &lighting {
        // Preserve authored color/intensity and map authored source vectors to
        // Bevy directional-light orientation. Coordinates/sign remain an
        // explicit preview adaptation, not native transform parity.
        for (direction_id, color_id, strength_id) in [
            (0x0100EABB, 0x0100EABC, 0x0100EABD),
            (0x0100EABE, 0x0100EABF, 0x0100EAC0),
            (0x0100EAC1, 0x0100EAC2, 0x0100EAC3),
        ] {
            if let (Some(direction), Some(color), Some(value)) = (
                authored_light_direction(p, direction_id),
                rgb(p, color_id),
                strength(p, strength_id),
            ) {
                world.spawn((
                    PreviewFillLight,
                    DirectionalLight {
                        color: preview_srgb_rgb(color),
                        illuminance: value * tune("FILL", 160.),
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    bevy_light_transform(direction),
                ));
            }
        }
    }
    if let Some(theme) = theme {
        // Asset resources are not guaranteed in headless stage simulations.
        // If the render plugin is absent, leave the fallback clear color only.
        let cloud_mips = read_cloud_mips(&world.resource::<crate::Originals>().resource_index);
        let (cloud_image, cloud_source) = match cloud_mips.and_then(image_from_mips) {
            Some((image, dimensions)) => (
                Some(image),
                Some(OriginalCloudSource {
                    key: CLOUD_RASTER,
                    image: Handle::default(),
                    dimensions,
                }),
            ),
            None => (Some(transparent_cloud_fallback().0), None),
        };
        let cloud_handle = if let (Some(image), Some(mut images)) =
            (cloud_image, world.get_resource_mut::<Assets<Image>>())
        {
            Some(images.add(image))
        } else {
            None
        };
        if let Some(cloud_handle) = cloud_handle {
            let source = cloud_source.map(|mut source| {
                source.image = cloud_handle.clone();
                source
            });
            let sky_material =
                world
                    .get_resource_mut::<Assets<PreviewSkyMaterial>>()
                    .map(|mut materials| {
                        materials.add(PreviewSkyMaterial {
                            base: StandardMaterial {
                                base_color: Color::WHITE,
                                unlit: true,
                                fog_enabled: false,
                                cull_mode: None,
                                ..default()
                            },
                            extension: PreviewSky {
                                parameters: Vec4::ZERO,
                                atmosphere: Vec4::new(
                                    theme.palette.atmosphere[0],
                                    theme.palette.atmosphere[1],
                                    theme.palette.atmosphere[2],
                                    if source.is_some() { 1. } else { 0. },
                                ),
                                cloud_texture: cloud_handle,
                                // Scatter the cloud sprite: azimuth cells per
                                // layer, opacity, and the altitude band (sine).
                                cloud_controls: Vec4::new(
                                    tune("CLOUD_CELLS", 14.),
                                    tune("CLOUD_OPACITY", 0.9),
                                    tune("CLOUD_LOW", 0.02),
                                    tune("CLOUD_HIGH", 0.42),
                                ),
                                zenith: atmosphere
                                    .as_ref()
                                    .map_or(Vec4::new(0.03, 0.15, 0.6, 1.), |a| {
                                        a.zenith.to_vec4().with_w(tune("SKY_POW", 0.55))
                                    }),
                                horizon: atmosphere
                                    .as_ref()
                                    .map_or(Vec4::new(0.5, 0.7, 0.9, 0.), |a| {
                                        a.horizon.to_vec4().with_w(a.haze_mix)
                                    }),
                                sun: sun_direction.extend(tune("SUN_HALO", 0.5)),
                            },
                        })
                    });
            if let Some(material) = sky_material {
                let mesh_handle = world.get_resource_mut::<Assets<Mesh>>().map(|mut meshes| {
                    let mut mesh = Sphere::new(280.).mesh().uv(64, 32);
                    mesh.asset_usage = RenderAssetUsages::default();
                    meshes.add(mesh)
                });
                if let Some(mesh) = mesh_handle {
                    let mut entity = world.spawn((
                        PreviewAtmosphere,
                        NotShadowCaster,
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                    ));
                    if let Some(source) = source {
                        entity.insert(source);
                    }
                }
            }
        }
        world.resource_mut::<ClearColor>().0 = Color::srgb(0.34, 0.57, 0.78);
    }
    world.insert_resource(AppliedStage(stage));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn direction_property(values: &[f32], array: bool) -> crate::prop::PropertyList {
        let mut bytes = Vec::new();
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let mut properties = BTreeMap::new();
        properties.insert(
            0x0100EAB8,
            crate::prop::Property {
                kind: 0x31,
                array,
                values: vec![bytes],
            },
        );
        crate::prop::PropertyList(properties)
    }

    #[test]
    fn authored_direction_is_normalized_and_maps_to_bevy_light_rays() {
        let properties = direction_property(&[0.0, 1.0, 0.0], false);
        let source = authored_light_direction(&properties, 0x0100EAB8).unwrap();
        assert_eq!(source, Vec3::Y);
        let transform = bevy_light_transform(source);
        assert!((transform.rotation * Vec3::NEG_Z).distance(Vec3::NEG_Y) < 1.0e-5);
    }

    #[test]
    fn malformed_or_zero_authored_directions_are_ignored() {
        let zero = direction_property(&[0.0, 0.0, 0.0], false);
        assert!(authored_light_direction(&zero, 0x0100EAB8).is_none());

        let non_finite = direction_property(&[f32::NAN, 1.0, 0.0], false);
        assert!(authored_light_direction(&non_finite, 0x0100EAB8).is_none());

        let array = direction_property(&[1.0, 0.0, 0.0], true);
        assert!(authored_light_direction(&array, 0x0100EAB8).is_none());
    }

    #[test]
    fn installed_world_colors_use_the_srgb_preview_mapping() {
        let source = [0.5, 0.25, 0.75];
        let actual = preview_srgb_rgb(source).to_linear().to_f32_array();
        let expected = Color::srgb(source[0], source[1], source[2])
            .to_linear()
            .to_f32_array();
        assert_eq!(
            [actual[0], actual[1], actual[2]],
            [expected[0], expected[1], expected[2]]
        );
        assert_ne!([actual[0], actual[1], actual[2]], source);
    }

    #[test]
    #[ignore = "requires the installed Spore resource packages"]
    fn original_cloud_rasters_decode_and_export_for_inspection() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = ResourceIndex::load(&install.packages);
        let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("clouds-066");
        std::fs::create_dir_all(&output).expect("create cloud preview directory");
        for (name, instance_id) in [
            ("cloud_lit", 0xA50F5A46),
            ("cloud_lit_2x2", 0xBC924977),
            ("cloud_lit_strip", 0xC44D1615),
        ] {
            let key = ResourceKey {
                group_id: 0,
                instance_id,
                type_id: crate::asset_catalog::RW4,
            };
            let bytes = index.read(key).unwrap_or_else(|e| panic!("{name}: {e}"));
            let mips = crate::texture::decode_rw4_mips(&bytes)
                .unwrap_or_else(|| panic!("{name} is not a decodable RW4 raster"));
            let base = &mips[0];
            let file = std::fs::File::create(output.join(format!("{name}.png")))
                .expect("create cloud preview");
            let mut encoder = png::Encoder::new(file, base.width, base.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .expect("write cloud PNG header")
                .write_image_data(&base.rgba)
                .expect("write cloud PNG pixels");
        }
    }
}
