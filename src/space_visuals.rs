//! Preview orbital surfaces from installed planet palettes and terrain atlases.
//! Mapping those assets onto spheres, cloud motion, and world selection remain
//! presentation adaptations rather than native planet generation.
use crate::{
    asset_catalog::{ResourceIndex, PROP},
    dbpf::ResourceKey,
    stages::StageOwned,
};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::NotShadowCaster,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::collections::HashMap;

#[derive(Component, Clone)]
pub struct PlanetSurface {
    pub planet_id: u32,
    /// Installed color palette used by the generated global surface map.
    pub theme: ResourceKey,
    /// Installed planet descriptor chosen for this preview world.
    pub terrain_definition: ResourceKey,
    /// Authored terrain detail raster blended into the global surface map.
    pub terrain_texture: Option<ResourceKey>,
    /// Original global cloud raster reused as this planet's cloud shell.
    pub cloud_texture: Option<ResourceKey>,
}
#[derive(Component, Clone)]
pub struct OriginalPlanetCloud {
    pub key: ResourceKey,
    pub image: Handle<Image>,
    pub dimensions: UVec2,
}

/// Marker for the 3D atmospheric halo glow shell of a planet.
#[derive(Component)]
pub struct PlanetAtmosphereHalo;

/// Marker for the 3D rotating cloud shell of a planet.
#[derive(Component)]
pub struct PlanetCloudLayer;

#[derive(Clone)]
struct CachedPlanetCloud {
    source: OriginalPlanetCloud,
    material: Handle<StandardMaterial>,
    mesh: Handle<Mesh>,
}

#[derive(Clone)]
struct CachedPlanetVisual {
    surface: PlanetSurface,
    material: Handle<StandardMaterial>,
    atmosphere: [f32; 3],
    cloud: Option<CachedPlanetCloud>,
}

/// Stable Space ids retain their selected source maps and material handles
/// across entity refreshes. Decoded sources are shared by their full DBPF key.
#[derive(Resource, Default)]
struct PlanetVisualCache {
    planets: HashMap<u32, CachedPlanetVisual>,
    terrain_pixels: HashMap<ResourceKey, crate::texture::Pixels>,
    cloud_image: Option<(Handle<Image>, UVec2)>,
    cloud_meshes: HashMap<u32, Handle<Mesh>>,
}
/// Exact installed raster used by a code-placed Space backdrop billboard.
/// The raster pixels are decoded from the named RW4 resource; placement and
/// projection in this preview are adapted and are not a native space scene.
#[derive(Component, Clone)]
pub struct OriginalSpaceBackdrop {
    pub key: ResourceKey,
    pub image: Handle<Image>,
    pub dimensions: [u32; 2],
}

#[derive(Component)]
pub(crate) struct SpaceBackdropPlacement {
    distance: f32,
    offset: Vec2,
    width: f32,
    roll: f32,
}

fn spawn_original_space_backdrops(world: &mut World) {
    if world
        .query_filtered::<Entity, With<SpaceBackdropPlacement>>()
        .iter(world)
        .next()
        .is_some()
    {
        return;
    }
    let Some(index) = world
        .get_resource::<crate::Originals>()
        .map(|originals| originals.resource_index.clone())
    else {
        return;
    };
    // The opaque black pixels in these original nebula plates disappear under
    // additive blending. Use one cool streak and two subdued warm plates.
    let sources = [
        (0xBEBC_288A, Vec2::ZERO, 112., 0., [0.42, 0.50, 0.70]),
        (
            0xBEBC_2889,
            Vec2::new(-48., 24.),
            46.,
            -0.20,
            [0.40, 0.25, 0.18],
        ),
        (
            0xBEBC_2889,
            Vec2::new(54., -26.),
            40.,
            0.24,
            [0.40, 0.25, 0.18],
        ),
    ];
    let mut decoded = Vec::new();
    for (instance_id, offset, width, roll, tint) in sources {
        let key = ResourceKey {
            group_id: 0,
            instance_id,
            type_id: crate::asset_catalog::RW4,
        };
        let Ok(bytes) = index.read(key) else {
            continue;
        };
        let Some(pixels) = crate::texture::decode_rw4(&bytes) else {
            warn!("original Space raster {key:?} did not decode as a standalone RW4 raster");
            continue;
        };
        decoded.push((key, pixels, offset, width, roll, tint));
    }
    let Some(mut images) = world.get_resource_mut::<Assets<Image>>() else {
        return;
    };
    let mut prepared = Vec::new();
    for (key, pixels, offset, width, roll, tint) in decoded {
        let dimensions = [pixels.width, pixels.height];
        let mut image = Image::new(
            Extent3d {
                width: pixels.width,
                height: pixels.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels.rgba,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..default()
        });
        let handle = images.add(image);
        prepared.push((key, handle, dimensions, offset, width, roll, tint));
    }
    drop(images);
    if prepared.is_empty() {
        return;
    }
    let Some(mut meshes) = world.get_resource_mut::<Assets<Mesh>>() else {
        return;
    };
    let mesh = meshes.add(Rectangle::new(1., 1.).mesh().build());
    drop(meshes);
    let Some(mut materials) = world.get_resource_mut::<Assets<StandardMaterial>>() else {
        return;
    };
    let prepared = prepared
        .into_iter()
        .map(|(key, image, dimensions, offset, width, roll, tint)| {
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(tint[0], tint[1], tint[2], 0.32),
                base_color_texture: Some(image.clone()),
                alpha_mode: AlphaMode::Add,
                unlit: true,
                fog_enabled: false,
                cull_mode: None,
                ..default()
            });
            (key, image, dimensions, offset, width, roll, material)
        })
        .collect::<Vec<_>>();
    drop(materials);
    for (key, image, dimensions, offset, width, roll, material) in prepared {
        world.spawn((
            StageOwned,
            NotShadowCaster,
            Name::new(format!("adapted original nebula {:08X}", key.instance_id)),
            OriginalSpaceBackdrop {
                key,
                image,
                dimensions,
            },
            SpaceBackdropPlacement {
                distance: 190.,
                offset,
                width,
                roll,
            },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_scale(Vec3::new(width, width, 1.)),
        ));
    }
}

pub fn prepare(world: &mut World) {
    if *world.resource::<State<crate::stages::GameStage>>().get() != crate::stages::GameStage::Space
    {
        return;
    }
    spawn_original_space_backdrops(world);
    let pending=world.query_filtered::<(Entity,&crate::persistence::WorldId,&crate::game::Planet),Without<PlanetSurface>>()
        .iter(world).map(|(e,id,p)|(e,id.0,p.radius)).collect::<Vec<_>>();
    if pending.is_empty() {
        return;
    }
    let index = world.resource::<crate::Originals>().resource_index.clone();
    let mut cache = world
        .remove_resource::<PlanetVisualCache>()
        .unwrap_or_default();
    if !pending.is_empty() {
        // Worlds of other star systems are rebuilt on every visit; keep only
        // the ones on screen so a long voyage does not pile up surface maps.
        let live = world
            .query_filtered::<&crate::persistence::WorldId, With<crate::game::Planet>>()
            .iter(world)
            .map(|id| id.0)
            .collect::<Vec<_>>();
        cache
            .planets
            .retain(|id, _| *id < crate::space_galaxy::FOREIGN_WORLD_BASE || live.contains(id));
    }
    let mut meshes = world.remove_resource::<Assets<Mesh>>().unwrap();
    let mut images = world.remove_resource::<Assets<Image>>().unwrap();
    let mut materials = world.remove_resource::<Assets<StandardMaterial>>().unwrap();
    for (entity, id, radius) in pending {
        let visual = cached_surface(
            &index,
            id,
            radius,
            &mut cache,
            &mut meshes,
            &mut images,
            &mut materials,
        );
        if let Some(visual) = visual {
            world.entity_mut(entity).insert((
                visual.surface.clone(),
                MeshMaterial3d(visual.material.clone()),
            ));
            let mut commands = world.commands();
            orbit_details(
                &mut commands,
                &mut meshes,
                &mut materials,
                entity,
                radius,
                visual.atmosphere,
            );
            if let Some(cloud) = visual.cloud {
                commands.entity(entity).with_children(|planet| {
                    planet.spawn((
                        StageOwned,
                        PlanetCloudLayer,
                        NotShadowCaster,
                        Name::new(format!(
                            "adapted original planet cloud {:08X}",
                            cloud.source.key.instance_id
                        )),
                        cloud.source,
                        Mesh3d(cloud.mesh),
                        MeshMaterial3d(cloud.material),
                    ));
                });
            }
        }
    }
    world.insert_resource(meshes);
    world.insert_resource(images);
    world.insert_resource(materials);
    world.insert_resource(cache);
}
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BeaconKind {
    Ownership,
    Survey,
    Artifact,
    Distress,
}

#[derive(Component)]
pub(crate) struct OwnershipBeacon;
#[derive(Component)]
pub(crate) struct SurveyBeacon;
#[derive(Component)]
pub(crate) struct ArtifactBeacon;
#[derive(Component)]
pub(crate) struct DistressBeacon;
/// Marker for 3D colony cities on a planet's surface (slot 0..3).
#[derive(Component)]
pub struct ColonyCityMarker {
    pub slot: u8,
}
pub fn palette(index: &ResourceIndex, id: u32) -> Option<crate::world_catalog::PlanetPalette> {
    let themes = [0x6F78B3C5, 0x6E78B253, 0x6D78B0E0];
    let key = ResourceKey {
        group_id: 0x406B0100,
        instance_id: themes[id as usize % themes.len()],
        type_id: PROP,
    };
    crate::world_catalog::PlanetPalette::from_properties(key, &index.properties(key).ok()?).ok()
}

const SPACE_TERRAIN_DEFINITIONS: [u32; 3] = [0x14E9_8A17, 0x1795_BBD2, 0x66AA_7D12];

fn terrain_source(index: &ResourceIndex, id: u32) -> (ResourceKey, Option<ResourceKey>) {
    let definition = ResourceKey {
        group_id: 0x4284_0100,
        instance_id: SPACE_TERRAIN_DEFINITIONS[id as usize % SPACE_TERRAIN_DEFINITIONS.len()],
        type_id: PROP,
    };
    let texture = index
        .properties(definition)
        .ok()
        .and_then(|properties| {
            crate::world_catalog::WorldDescriptor::parse(definition, properties).ok()
        })
        .and_then(|descriptor| descriptor.terrain_overrides().ok())
        .and_then(|overrides| overrides.above_detail)
        .and_then(|key| resolve_raster_key(index, key));
    (definition, texture)
}

fn resolve_raster_key(index: &ResourceIndex, mut key: ResourceKey) -> Option<ResourceKey> {
    if key.type_id != 0 {
        return index.records.contains_key(&key).then_some(key);
    }
    key.type_id = crate::asset_catalog::RW4;
    index.records.contains_key(&key).then_some(key)
}

fn cached_terrain_pixels(
    index: &ResourceIndex,
    key: ResourceKey,
    cache: &mut PlanetVisualCache,
) -> Option<crate::texture::Pixels> {
    if let Some(pixels) = cache.terrain_pixels.get(&key) {
        return Some(pixels.clone());
    }
    let bytes = index.read(key).ok()?;
    let pixels = crate::texture::decode_rw4_mips(&bytes)?
        .into_iter()
        .next()?;
    cache.terrain_pixels.insert(key, pixels.clone());
    Some(pixels)
}

fn cloud_source(
    index: &ResourceIndex,
    cache: &mut PlanetVisualCache,
    images: &mut Assets<Image>,
) -> Option<(ResourceKey, Handle<Image>, UVec2)> {
    let key = crate::world_presentation::CLOUD_RASTER;
    if let Some((image, dimensions)) = &cache.cloud_image {
        return Some((key, image.clone(), *dimensions));
    }
    let bytes = index.read(key).ok()?;
    let mips = crate::texture::decode_rw4_mips(&bytes)?;
    let base = mips.first()?;
    let dimensions = UVec2::new(base.width, base.height);
    let mut image = Image::new(
        Extent3d {
            width: base.width,
            height: base.height,
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
    let handle = images.add(image);
    cache.cloud_image = Some((handle.clone(), dimensions));
    Some((key, handle, dimensions))
}

fn sample_native_detail(pixels: &crate::texture::Pixels, u: f32, v: f32) -> [f32; 3] {
    let x = ((u * pixels.width as f32).floor() as i64).rem_euclid(pixels.width as i64) as usize;
    let y = ((v.clamp(0., 0.999_999) * pixels.height as f32).floor() as usize)
        .min(pixels.height.saturating_sub(1) as usize);
    let offset = (y * pixels.width as usize + x) * 4;
    [
        pixels.rgba[offset] as f32 / 255.,
        pixels.rgba[offset + 1] as f32 / 255.,
        pixels.rgba[offset + 2] as f32 / 255.,
    ]
}

fn cached_surface(
    index: &ResourceIndex,
    id: u32,
    radius: f32,
    cache: &mut PlanetVisualCache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<CachedPlanetVisual> {
    if let Some(visual) = cache.planets.get(&id) {
        return Some(visual.clone());
    }
    let visual = surface(index, id, radius, cache, meshes, images, materials)?;
    cache.planets.insert(id, visual.clone());
    Some(visual)
}

fn elevation(p: Vec3, seed: f32) -> f32 {
    let broad =
        (p.x * 4.3 + seed).sin() * (p.z * 3.5 - seed).cos() + 0.45 * (p.y * 6.3 + p.x * 2.1).sin();
    let ridges = 0.18 * (p.x * 17.0 - p.y * 8.0 + seed).sin() * (p.z * 12.0).cos();
    broad + ridges
}
pub fn texture(palette: &crate::world_catalog::PlanetPalette, id: u32) -> Image {
    texture_with_detail(palette, id, None)
}

fn texture_with_detail(
    palette: &crate::world_catalog::PlanetPalette,
    id: u32,
    native_detail: Option<&crate::texture::Pixels>,
) -> Image {
    let (width, height) = (512u32, 256u32);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    let seed = id as f32 * 1.713;
    for y in 0..height {
        for x in 0..width {
            let longitude = x as f32 / width as f32 * std::f32::consts::TAU;
            let latitude = (y as f32 / (height - 1) as f32 - 0.5) * std::f32::consts::PI;
            let p = Vec3::new(
                longitude.cos() * latitude.cos(),
                latitude.sin(),
                longitude.sin() * latitude.cos(),
            );
            let h = elevation(p, seed);
            let water = h < 0.05;
            let (low, high) = (
                *palette.elevations.first().unwrap(),
                *palette.elevations.last().unwrap(),
            );
            let land = palette.sample(low + (h.max(0.) / 1.65).clamp(0., 1.) * (high - low));
            let mut c = if water {
                palette.water.map(|v| v * 0.70)
            } else if h < 0.11 {
                palette.beach
            } else {
                land
            };
            let polar = ((p.y.abs() - 0.84) * 6.25).clamp(0., 1.);
            let fine = (p.x * 61. + p.z * 39. + p.y * 27.).sin() * 0.035;
            for value in &mut c {
                *value = (*value * (1. + fine)) * (1. - polar) + 0.91 * polar;
            }
            if !water {
                if let Some(detail) = native_detail {
                    let native = sample_native_detail(
                        detail,
                        longitude / std::f32::consts::TAU * 4.,
                        y as f32 / height as f32 * 2.,
                    );
                    for channel in 0..3 {
                        c[channel] = c[channel] * 0.78 + native[channel] * 0.22;
                    }
                }
            }
            pixels.extend_from_slice(&[
                (c[0].clamp(0., 1.) * 255.) as u8,
                (c[1].clamp(0., 1.) * 255.) as u8,
                (c[2].clamp(0., 1.) * 255.) as u8,
                255,
            ]);
        }
    }
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}
fn surface(
    index: &ResourceIndex,
    id: u32,
    radius: f32,
    cache: &mut PlanetVisualCache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<CachedPlanetVisual> {
    let palette = palette(index, id)?;
    let (terrain_definition, terrain_texture) = terrain_source(index, id);
    let native_detail = terrain_texture.and_then(|key| cached_terrain_pixels(index, key, cache));
    let image = images.add(texture_with_detail(&palette, id, native_detail.as_ref()));
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(image),
        perceptual_roughness: 0.9,
        reflectance: 0.05,
        ..default()
    });
    let cloud = cloud_source(index, cache, images).map(|(key, image, dimensions)| {
        let cloud_material = materials.add(StandardMaterial {
            base_color: Color::srgba(1., 1., 1., 0.28),
            base_color_texture: Some(image.clone()),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.,
            unlit: false,
            fog_enabled: false,
            cull_mode: None,
            ..default()
        });
        let mesh = cache
            .cloud_meshes
            .entry(radius.to_bits())
            .or_insert_with(|| meshes.add(Sphere::new(radius * 1.008).mesh().uv(48, 24)))
            .clone();
        CachedPlanetCloud {
            source: OriginalPlanetCloud {
                key,
                image,
                dimensions,
            },
            material: cloud_material,
            mesh,
        }
    });
    Some(CachedPlanetVisual {
        surface: PlanetSurface {
            planet_id: id,
            theme: palette.key,
            terrain_definition,
            terrain_texture,
            cloud_texture: cloud.as_ref().map(|cloud| cloud.source.key),
        },
        material,
        atmosphere: palette.atmosphere,
        cloud,
    })
}
pub fn orbit_details(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    planet: Entity,
    radius: f32,
    color: [f32; 3],
) {
    let atmosphere = materials.add(StandardMaterial {
        base_color: Color::srgba(color[0], color[1], color[2], 0.08),
        emissive: Color::srgb(color[0] * 0.12, color[1] * 0.12, color[2] * 0.12).into(),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        // Draw the far hemisphere: the opaque planet hides the center and
        // leaves a thin atmospheric limb instead of adding haze over its face.
        cull_mode: Some(bevy::render::render_resource::Face::Front),
        ..default()
    });
    let halo = meshes.add(Sphere::new(radius * 1.035).mesh().uv(48, 24));
    let ring = meshes.add(Torus::new(radius * 1.1, radius * 1.115));
    let beacon = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.95, 0.55),
        emissive: Color::srgb(0.1, 0.9, 0.3).into(),
        unlit: true,
        ..default()
    });
    let survey = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.7, 0.25),
        emissive: Color::srgb(0.3, 0.16, 0.025).into(),
        unlit: true,
        ..default()
    });
    let artifact_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.88, 0.25),
        emissive: Color::srgb(0.6, 0.45, 0.1).into(),
        unlit: true,
        ..default()
    });
    let distress_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.15, 0.15),
        emissive: Color::srgb(0.8, 0.1, 0.1).into(),
        unlit: true,
        ..default()
    });

    let city_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.92, 0.96, 1.0),
        emissive: Color::srgb(0.2, 0.45, 0.85).into(),
        unlit: false,
        ..default()
    });
    let city_mesh = meshes.add(Sphere::new(radius * 0.055).mesh().uv(16, 8));
    let city_locs = [
        Vec3::new(0.68, 0.42, 0.60).normalize(),
        Vec3::new(-0.72, 0.45, 0.52).normalize(),
        Vec3::new(0.20, -0.75, 0.63).normalize(),
    ];

    commands.entity(planet).with_children(|p| {
        p.spawn((
            StageOwned,
            PlanetAtmosphereHalo,
            NotShadowCaster,
            Mesh3d(halo),
            MeshMaterial3d(atmosphere),
            Transform::IDENTITY,
        ));
        p.spawn((
            StageOwned,
            OwnershipBeacon,
            BeaconKind::Ownership,
            NotShadowCaster,
            Mesh3d(ring.clone()),
            MeshMaterial3d(beacon),
            Transform::from_xyz(0., 0., 0.).with_rotation(Quat::from_rotation_x(0.22)),
            Visibility::Hidden,
        ));
        p.spawn((
            StageOwned,
            SurveyBeacon,
            BeaconKind::Survey,
            NotShadowCaster,
            Mesh3d(ring.clone()),
            MeshMaterial3d(survey),
            Transform::from_rotation(Quat::from_rotation_x(0.22)),
            Visibility::Hidden,
        ));
        p.spawn((
            StageOwned,
            ArtifactBeacon,
            BeaconKind::Artifact,
            NotShadowCaster,
            Mesh3d(ring.clone()),
            MeshMaterial3d(artifact_mat),
            Transform::from_rotation(Quat::from_rotation_z(0.35)),
            Visibility::Hidden,
        ));
        p.spawn((
            StageOwned,
            DistressBeacon,
            BeaconKind::Distress,
            NotShadowCaster,
            Mesh3d(ring),
            MeshMaterial3d(distress_mat),
            Transform::from_rotation(Quat::from_rotation_y(0.5)),
            Visibility::Hidden,
        ));
        for (slot, loc) in city_locs.iter().enumerate() {
            p.spawn((
                StageOwned,
                ColonyCityMarker { slot: slot as u8 },
                NotShadowCaster,
                Mesh3d(city_mesh.clone()),
                MeshMaterial3d(city_mat.clone()),
                Transform::from_translation(*loc * (radius * 1.002)),
                Visibility::Hidden,
            ));
        }
    });
}
pub fn update(
    time: Res<Time>,
    view: Option<Res<crate::space_view::SpaceView>>,
    run: Option<Res<crate::game::RunProgress>>,
    planets: Query<&crate::game::Planet>,
    planet_ids: Query<&crate::persistence::WorldId, With<crate::game::Planet>>,
    parents: Query<&ChildOf>,
    cameras: Query<&GlobalTransform, With<crate::GameCamera>>,
    mut backdrops: Query<
        (&mut Transform, &mut Visibility, &SpaceBackdropPlacement),
        (
            Without<OriginalPlanetCloud>,
            Without<BeaconKind>,
            Without<ColonyCityMarker>,
        ),
    >,
    mut clouds: Query<
        (&ChildOf, &mut Transform),
        (
            With<OriginalPlanetCloud>,
            Without<SpaceBackdropPlacement>,
            Without<BeaconKind>,
            Without<ColonyCityMarker>,
        ),
    >,
    mut beacons: Query<
        (&BeaconKind, &mut Visibility, Option<&mut Transform>, &ChildOf),
        (
            Without<SpaceBackdropPlacement>,
            Without<ColonyCityMarker>,
            Without<OriginalPlanetCloud>,
        ),
    >,
    mut city_markers: Query<
        (&ColonyCityMarker, &mut Visibility, &ChildOf),
        (
            Without<BeaconKind>,
            Without<SpaceBackdropPlacement>,
            Without<OriginalPlanetCloud>,
        ),
    >,
    atmospheres: Query<
        (&MeshMaterial3d<StandardMaterial>, &ChildOf),
        With<PlanetAtmosphereHalo>,
    >,
    cloud_layers: Query<
        (&MeshMaterial3d<StandardMaterial>, &ChildOf),
        With<PlanetCloudLayer>,
    >,
    surfaces: Query<
        (&MeshMaterial3d<StandardMaterial>, &PlanetSurface),
        Without<crate::space_planets::OrbitTerrain>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let _ = &parents;
    // Cosmetic orbital drift of the installed raster. Native weather/PCA and
    // planetary cloud generation are separate, unfinished systems.
    for (parent, mut transform) in &mut clouds {
        let phase = planet_ids
            .get(parent.parent())
            .map_or(0., |id| id.0 as f32 * 0.71);
        transform.rotation = Quat::from_rotation_y(phase + time.elapsed_secs() * 0.014);
    }
    if let Some(camera) = cameras.iter().next() {
        let (_, rotation, translation) = camera.to_scale_rotation_translation();
        let forward = rotation * Vec3::NEG_Z;
        let right = rotation * Vec3::X;
        let up = rotation * Vec3::Y;
        // The nebula plates belong to the solar-system sky; the galaxy map
        // draws its own original rasters.
        let hidden = view.as_ref().is_some_and(|v| v.galaxy);
        for (mut transform, mut visibility, placement) in &mut backdrops {
            transform.translation = translation
                + forward * placement.distance
                + right * placement.offset.x
                + up * placement.offset.y;
            transform.rotation = rotation * Quat::from_rotation_z(placement.roll);
            transform.scale = Vec3::new(placement.width, placement.width, 1.);
            let want = if hidden {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            if *visibility != want {
                *visibility = want;
            }
        }
    }
    for (kind, mut visible, mut transform, parent) in &mut beacons {
        let Ok(id) = planet_ids.get(parent.parent()) else {
            continue;
        };
        let world_id = id.0;
        match kind {
            BeaconKind::Ownership => {
                let empire_owns = run
                    .as_ref()
                    .and_then(|r| r.space_campaign.world_empire(world_id))
                    .is_some();
                let colonized = planets.get(parent.parent()).map_or(false, |p| p.colonized);
                *visible = if colonized || empire_owns {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            BeaconKind::Survey => {
                let colonized = planets.get(parent.parent()).map_or(false, |p| p.colonized);
                let surveyed = planets.get(parent.parent()).map_or(false, |p| p.surveyed);
                *visible = if surveyed && !colonized {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            BeaconKind::Artifact => {
                // A rare still on the world, or a tool artifact lying there
                // (`space_tool_artifacts`).
                let has_artifact = run.as_ref().map_or(false, |r| {
                    (crate::space::world_artifact(world_id).is_some()
                        && !r.space_campaign.collected_artifacts.contains(&world_id))
                        || crate::space_tool_artifacts::lying_on(&r.space_campaign, world_id)
                });
                *visible = if has_artifact {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            BeaconKind::Distress => {
                let has_distress = run.as_ref().map_or(false, |r| {
                    r.space_campaign
                        .active_distress
                        .as_ref()
                        .is_some_and(|d| d.world_id == world_id)
                });
                if has_distress {
                    let pulse = 1.0 + 0.18 * (time.elapsed_secs() * 5.0).sin();
                    if let Some(ref mut t) = transform {
                        t.scale = Vec3::splat(pulse);
                    }
                    *visible = Visibility::Inherited;
                } else {
                    *visible = Visibility::Hidden;
                }
            }
        }
    }
    if let Some(run) = run.as_ref() {
        for (marker, mut visible, parent) in &mut city_markers {
            if let Ok(id) = planet_ids.get(parent.parent()) {
                let world_id = id.0;
                let count = if run.space_campaign.colonies.contains(&world_id) {
                    run.space_campaign.ecology_for(world_id).colony_count
                } else if run.space_campaign.world_empire(world_id).is_some() {
                    1
                } else {
                    0
                };
                *visible = if marker.slot < count {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }

        // Dynamic Terraforming Visuals: atmosphere haze, clouds, and surface
        // biome tinting. Worlds drawn from their terrain script
        // (`space_planets`) carry their own atmosphere shell and re-bake
        // their surface; the halo strength follows
        // kAtmThicknessByAtmosphereScore, the hue stays ADAPTED.
        for (mat_handle, parent) in &atmospheres {
            if let Ok(id) = planet_ids.get(parent.parent()) {
                let ecology = run.space_campaign.ecology_for(id.0);
                if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
                    let thickness = crate::space_planets::atmosphere_thickness(ecology.atmosphere);
                    let alpha = 0.03 + 0.08 * thickness / 1.75;
                    let (r, g, b) = if ecology.temperature < 0.35 {
                        (0.35, 0.65, 0.95) // Icy azure / pale blue
                    } else if ecology.temperature <= 0.65 {
                        (0.22, 0.85, 0.95) // Temperate cyan
                    } else {
                        (0.95, 0.50, 0.22) // Scorched volcanic amber
                    };
                    mat.base_color = Color::srgba(r, g, b, alpha);
                    mat.emissive = Color::srgb(r * alpha * 1.5, g * alpha * 1.5, b * alpha * 1.5).into();
                }
            }
        }

        // Clouds seen from space: the thin / medium / thick cloud effect by
        // atmosphere score, coloured by the temperature band (weather data);
        // those effects are empty in the installed data, so `cloud_alpha` is 0.
        for (mat_handle, parent) in &cloud_layers {
            if let Ok(id) = planet_ids.get(parent.parent()) {
                let ecology = run.space_campaign.ecology_for(id.0);
                if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
                    let alpha = crate::space_planets::cloud_alpha(ecology.atmosphere);
                    let [r, g, b] = crate::space_planets::cloud_color(ecology.temperature);
                    let want = Color::srgba(r, g, b, alpha);
                    if mat.base_color != want {
                        mat.base_color = want;
                    }
                }
            }
        }

        for (mat_handle, surface) in &surfaces {
            let ecology = run.space_campaign.ecology_for(surface.planet_id);
            if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
                mat.base_color = match ecology.t_score() {
                    0 => Color::WHITE,
                    1 => Color::srgb(0.95, 1.04, 0.95),
                    2 => Color::srgb(0.90, 1.10, 0.92),
                    _ => Color::srgb(0.85, 1.18, 0.90),
                };
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write_png(path: &Path, pixels: &crate::texture::Pixels) {
        let file = std::fs::File::create(path).expect("create raster preview");
        let mut encoder = png::Encoder::new(file, pixels.width, pixels.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .expect("PNG header")
            .write_image_data(&pixels.rgba)
            .expect("PNG pixels");
    }

    #[test]
    fn procedural_height_is_finite_and_not_an_unrelated_planar_grid() {
        for seed in [0., 3., 19.] {
            for p in [Vec3::X, Vec3::Y, Vec3::Z, -Vec3::X] {
                assert!(elevation(p, seed).is_finite());
            }
        }
        assert_ne!(elevation(Vec3::X, 0.), elevation(Vec3::X, 3.));
    }

    #[test]
    fn installed_terrain_detail_changes_the_palette_map() {
        let palette = crate::world_catalog::PlanetPalette {
            key: ResourceKey {
                group_id: 1,
                instance_id: 2,
                type_id: PROP,
            },
            elevations: vec![0., 1.],
            colors: vec![[0.15, 0.25, 0.35], [0.75, 0.65, 0.55]],
            water: [0.02, 0.04, 0.12],
            atmosphere: [0.2, 0.3, 0.5],
            cliff: [0.3, 0.3, 0.3],
            beach: [0.7, 0.6, 0.4],
        };
        let detail = crate::texture::Pixels {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        };
        let plain = texture(&palette, 0);
        let detailed = texture_with_detail(&palette, 0, Some(&detail));
        assert_eq!(plain.width(), 512);
        assert_ne!(plain.data, detailed.data);
    }

    #[test]
    #[ignore = "requires installed original Spore resources"]
    fn native_planet_visuals_keep_source_keys_and_cache_by_world_id() {
        let install = crate::spore_install::discover().expect("installed Spore assets");
        let index = ResourceIndex::load(&install.packages);
        let mut cache = PlanetVisualCache::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let first = cached_surface(
            &index,
            0,
            2.2,
            &mut cache,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .expect("planet surface sources");
        assert_eq!(first.surface.planet_id, 0);
        assert_eq!(first.surface.terrain_definition.instance_id, 0x14E9_8A17);
        assert_eq!(
            first.surface.terrain_texture,
            Some(ResourceKey {
                group_id: 0,
                instance_id: 0x2538_D7D9,
                type_id: crate::asset_catalog::RW4,
            })
        );
        let cloud = first.cloud.as_ref().expect("original cloud shell");
        assert_eq!(cloud.source.key, crate::world_presentation::CLOUD_RASTER);
        assert_eq!(
            materials.get(&cloud.material).unwrap().base_color_texture,
            Some(cloud.source.image.clone())
        );

        let second = cached_surface(
            &index,
            0,
            2.2,
            &mut cache,
            &mut meshes,
            &mut images,
            &mut materials,
        )
        .expect("cached planet visual");
        assert_eq!(first.material, second.material);
        assert_eq!(
            first.cloud.unwrap().material,
            second.cloud.unwrap().material
        );
        assert_eq!(cache.planets.len(), 1);
    }

    #[test]
    #[ignore = "reads installed original Spore raster resources and writes ignored previews"]
    fn installed_nebula_rasters_decode_and_export_for_visual_inspection() {
        let install = crate::spore_install::discover().expect("installed Spore assets");
        let index = ResourceIndex::load(&install.packages);
        let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/space-rasters-066");
        std::fs::create_dir_all(&output).expect("create ignored preview directory");
        let mut rows = Vec::new();
        for (name, instance_id) in [
            ("nebula1", 0xBEBC_2889),
            ("nebula2", 0xBEBC_288A),
            ("nebula3", 0xBEBC_288B),
        ] {
            let key = ResourceKey {
                group_id: 0,
                instance_id,
                type_id: crate::asset_catalog::RW4,
            };
            let bytes = index
                .read(key)
                .unwrap_or_else(|e| panic!("{name} {key:?}: {e}"));
            let pixels = crate::texture::decode_rw4(&bytes)
                .unwrap_or_else(|| panic!("{name} did not decode as an RW4 raster"));
            let path = output.join(format!("{name}.png"));
            write_png(&path, &pixels);
            let (zero_alpha, partial_alpha, opaque_alpha) = pixels.rgba.chunks_exact(4).fold(
                (0_u64, 0_u64, 0_u64),
                |(zero, partial, opaque), pixel| match pixel[3] {
                    0 => (zero + 1, partial, opaque),
                    255 => (zero, partial, opaque + 1),
                    _ => (zero, partial + 1, opaque),
                },
            );
            rows.push(serde_json::json!({
                "name": name,
                "key": {"group": "00000000", "instance": format!("{instance_id:08X}"), "type": format!("{:08X}", crate::asset_catalog::RW4)},
                "width": pixels.width,
                "height": pixels.height,
                "zero_alpha_pixels": zero_alpha,
                "partial_alpha_pixels": partial_alpha,
                "opaque_pixels": opaque_alpha,
                "preview": path.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap().to_string_lossy(),
            }));
        }
        let manifest = output.join("manifest.json");
        std::fs::write(&manifest, serde_json::to_vec_pretty(&rows).unwrap())
            .expect("write ignored raster manifest");
        println!("Original nebula previews: {}", output.display());
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn terraforming_visual_updates_reflect_climate_and_t_score() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<Image>>();
        app.init_resource::<Assets<StandardMaterial>>();
        app.init_resource::<Time>();

        let mut run = crate::game::RunProgress::default();
        let world_id = 42;
        let idx = run.space_campaign.ensure_ecology(world_id);
        run.space_campaign.ecology[idx].temperature = 0.50;
        run.space_campaign.ecology[idx].atmosphere = 0.78;
        run.space_campaign.ecology[idx].flora = 3;
        run.space_campaign.ecology[idx].herbivores = 2;
        run.space_campaign.ecology[idx].carnivores = 1;
        assert_eq!(run.space_campaign.ecology[idx].t_score(), 1);

        app.insert_resource(run);

        // Spawn planet entity with WorldId and Planet
        let planet = app
            .world_mut()
            .spawn((
                crate::game::Planet {
                    radius: 2.0,
                    colonized: false,
                    name: "Test Planet".into(),
                    surveyed: true,
                },
                crate::persistence::WorldId(world_id),
            ))
            .id();

        // Spawn surface
        let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
        let surf_mat = materials.add(StandardMaterial::default());
        let atmo_mat = materials.add(StandardMaterial::default());
        let cloud_mat = materials.add(StandardMaterial::default());

        app.world_mut().spawn((
            MeshMaterial3d(surf_mat.clone()),
            PlanetSurface {
                planet_id: world_id,
                theme: ResourceKey {
                    group_id: 0,
                    instance_id: 0,
                    type_id: 0,
                },
                terrain_definition: ResourceKey {
                    group_id: 0,
                    instance_id: 0,
                    type_id: 0,
                },
                terrain_texture: None,
                cloud_texture: None,
            },
        ));

        // Spawn atmosphere halo child
        app.world_mut().spawn((
            PlanetAtmosphereHalo,
            MeshMaterial3d(atmo_mat.clone()),
            ChildOf(planet),
        ));

        // Spawn cloud layer child
        app.world_mut().spawn((
            PlanetCloudLayer,
            MeshMaterial3d(cloud_mat.clone()),
            ChildOf(planet),
        ));

        // Run update system
        app.add_systems(Update, update);
        app.update();

        // Verify atmosphere material updated with density & temperature
        let mats = app.world().resource::<Assets<StandardMaterial>>();
        let updated_atmo = mats.get(&atmo_mat).unwrap();
        let atmo_color = updated_atmo.base_color.to_srgba();
        assert!(atmo_color.alpha > 0.08); // Atmosphere (0.62) produces alpha ~0.094

        let updated_cloud = mats.get(&cloud_mat).unwrap();
        let cloud_color = updated_cloud.base_color.to_srgba();
        // SG_solar_planet_clouds_*Atmo~ are empty effects: no orbit clouds.
        assert_eq!(cloud_color.alpha, 0.0);

        let updated_surf = mats.get(&surf_mat).unwrap();
        let surf_color = updated_surf.base_color.to_srgba();
        assert!(surf_color.green > surf_color.red); // T1 produces green biome tint
    }
}
