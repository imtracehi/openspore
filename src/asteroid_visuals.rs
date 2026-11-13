//! Installed solar asteroid art on batched camera-facing quads. The source
//! image is original; belt distribution, sizes and camera facing are adapted.
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

pub const ASTEROID_ATLAS: crate::dbpf::ResourceKey = crate::dbpf::ResourceKey {
    group_id: 0,
    instance_id: 0x9481_22bc,
    type_id: crate::asset_catalog::RW4,
};
const COUNT: usize = 420;
const GRID: usize = 4;
const ATLAS_LEVELS: usize = 9;
const TILE_LEVELS: usize = 7;

/// Marks the belt root after its placeholder mesh has been replaced by
/// frame-specific children. The root keeps `AsteroidBelt` for rotation.
#[derive(Component)]
pub struct NativeAsteroidRoot;

#[derive(Clone, Debug)]
struct Sprite {
    center: Vec3,
    half_size: f32,
    angle: f32,
    frame: usize,
}

#[derive(Component)]
pub struct NativeAsteroidBelt {
    pub source: crate::dbpf::ResourceKey,
    pub instances: usize,
    pub frame: usize,
    sprites: Vec<Sprite>,
}

/// Crop one atlas cell from every authored level that still isolates all 16
/// source frames. The final two atlas levels merge neighboring cells and are
/// intentionally excluded rather than replaced with generated mip data.
pub fn tile_mips(
    mips: &[crate::texture::Pixels],
    frame: usize,
) -> Result<Vec<crate::texture::Pixels>, String> {
    if frame >= GRID * GRID {
        return Err(format!("asteroid atlas frame {frame} is out of range"));
    }
    if mips.len() != ATLAS_LEVELS {
        return Err(format!(
            "expected {ATLAS_LEVELS} authored asteroid mip levels, got {}",
            mips.len()
        ));
    }
    let mut result = Vec::with_capacity(TILE_LEVELS);
    for (level, source) in mips.iter().take(TILE_LEVELS).enumerate() {
        let expected = 256u32 >> level;
        if source.width != expected
            || source.height != expected
            || source.rgba.len()
                != (expected as usize)
                    .checked_mul(expected as usize)
                    .and_then(|n| n.checked_mul(4))
                    .ok_or("asteroid mip dimensions overflow")?
        {
            return Err(format!(
                "invalid asteroid atlas mip {level}: {}x{} with {} RGBA bytes",
                source.width,
                source.height,
                source.rgba.len()
            ));
        }
        let tile_size = expected / GRID as u32;
        let tile_x = frame as u32 % GRID as u32 * tile_size;
        let tile_y = frame as u32 / GRID as u32 * tile_size;
        let mut rgba = Vec::with_capacity((tile_size * tile_size * 4) as usize);
        for y in tile_y..tile_y + tile_size {
            let start = ((y * expected + tile_x) * 4) as usize;
            let end = start + (tile_size * 4) as usize;
            rgba.extend_from_slice(&source.rgba[start..end]);
        }
        result.push(crate::texture::Pixels {
            width: tile_size,
            height: tile_size,
            rgba,
        });
    }
    Ok(result)
}

fn placements(radius: f32, seed: u32) -> Result<Vec<Sprite>, String> {
    if !radius.is_finite() || radius <= 0.0 {
        return Err("invalid asteroid belt radius".into());
    }
    let mut rng = ChaCha8Rng::seed_from_u64(seed as u64 ^ 0xa57e_072);
    Ok((0..COUNT)
        .map(|_| {
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            let distance =
                radius + rng.random_range(-2.6_f32..2.6) * rng.random_range(0.3_f32..1.0);
            Sprite {
                center: Vec3::new(
                    angle.cos() * distance,
                    rng.random_range(-0.6..0.6),
                    angle.sin() * distance,
                ),
                half_size: rng.random_range(0.08_f32..0.42).powf(1.4) + 0.06,
                angle: rng.random_range(0.0..std::f32::consts::TAU),
                frame: rng.random_range(0..GRID * GRID),
            }
        })
        .collect())
}

fn positions(sprites: &[Sprite], right: Vec3, up: Vec3) -> Vec<[f32; 3]> {
    let mut output = Vec::with_capacity(sprites.len() * 4);
    for sprite in sprites {
        let (sin, cos) = sprite.angle.sin_cos();
        let r = (right * cos + up * sin) * sprite.half_size;
        let u = (-right * sin + up * cos) * sprite.half_size;
        for corner in [-r - u, r - u, r + u, -r + u] {
            output.push((sprite.center + corner).to_array());
        }
    }
    output
}

fn mesh(sprites: &[Sprite]) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        positions(sprites, Vec3::X, Vec3::Y),
    );
    let mut uvs = Vec::with_capacity(sprites.len() * 4);
    let mut indices = Vec::with_capacity(sprites.len() * 6);
    for (index, _) in sprites.iter().enumerate() {
        uvs.extend_from_slice(&[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        let base = index as u32 * 4;
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

pub fn replace(
    mut commands: Commands,
    originals: Option<Res<crate::Originals>>,
    mut meshes: Option<ResMut<Assets<Mesh>>>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    belts: Query<(Entity, &crate::space_scene::AsteroidBelt), Without<NativeAsteroidRoot>>,
    mut cached: Local<Option<Result<Vec<Handle<StandardMaterial>>, String>>>,
) {
    let (Some(originals), Some(mut meshes), Some(mut images), Some(mut materials)) =
        (originals, meshes, images, materials)
    else {
        return;
    };
    if belts.is_empty() || originals.resource_index.records.is_empty() {
        return;
    }
    if cached.is_none() {
        *cached = Some((|| {
            let data = originals.resource_index.read(ASTEROID_ATLAS)?;
            let mips = crate::texture::decode_rw4_mips(&data)
                .ok_or("original asteroid atlas is not a raster")?;
            if mips
                .first()
                .map_or(true, |base| base.width != 256 || base.height != 256)
            {
                return Err("unexpected original asteroid atlas dimensions".into());
            }
            let mut tile_materials = Vec::with_capacity(GRID * GRID);
            for frame in 0..GRID * GRID {
                let tile = tile_mips(&mips, frame)?;
                let base = &tile[0];
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
                image.texture_descriptor.mip_level_count = tile.len() as u32;
                image.data = Some(tile.into_iter().flat_map(|mip| mip.rgba).collect());
                image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: bevy::image::ImageAddressMode::ClampToEdge,
                    address_mode_v: bevy::image::ImageAddressMode::ClampToEdge,
                    mag_filter: ImageFilterMode::Linear,
                    min_filter: ImageFilterMode::Linear,
                    mipmap_filter: ImageFilterMode::Linear,
                    ..default()
                });
                let image = images.add(image);
                tile_materials.push(materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    base_color_texture: Some(image),
                    alpha_mode: AlphaMode::Blend,
                    unlit: true,
                    cull_mode: None,
                    fog_enabled: false,
                    ..default()
                }));
            }
            Ok(tile_materials)
        })());
        if let Some(Err(error)) = cached.as_ref() {
            warn!("original solar asteroid art: {error}");
        }
    }
    let Some(Ok(tile_materials)) = cached.as_ref() else {
        return;
    };
    for (entity, belt) in &belts {
        let Ok(sprites) = placements(belt.radius, belt.seed) else {
            continue;
        };
        commands
            .entity(entity)
            .insert(NativeAsteroidRoot)
            .remove::<(Mesh3d, MeshMaterial3d<StandardMaterial>)>();
        let mut by_frame: Vec<Vec<Sprite>> = (0..GRID * GRID).map(|_| Vec::new()).collect();
        for sprite in sprites {
            let frame = sprite.frame;
            by_frame[frame].push(sprite);
        }
        for (frame, sprites) in by_frame.into_iter().enumerate() {
            if sprites.is_empty() {
                continue;
            }
            let count = sprites.len();
            let mesh = meshes.add(mesh(&sprites));
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    crate::stages::StageOwned,
                    NativeAsteroidBelt {
                        source: ASTEROID_ATLAS,
                        instances: count,
                        frame,
                        sprites,
                    },
                    Mesh3d(mesh),
                    MeshMaterial3d(tile_materials[frame].clone()),
                    Name::new(format!("original solar asteroid atlas frame {frame}")),
                    RenderLayers::default(),
                ));
            });
        }
    }
}

/// Runs after both the belt and camera transforms are propagated. One small
/// vertex buffer holds every sprite; source texels/UVs and material stay shared.
pub fn face_camera(
    cameras: Query<&GlobalTransform, With<crate::GameCamera>>,
    belts: Query<(&NativeAsteroidBelt, &Mesh3d, &GlobalTransform), Without<crate::GameCamera>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
) {
    let (Ok(camera), Some(mut meshes)) = (cameras.single(), meshes) else {
        return;
    };
    let (_, rotation, _) = camera.to_scale_rotation_translation();
    for (belt, handle, global) in &belts {
        let inverse = global.affine().inverse();
        let right = inverse
            .transform_vector3(rotation * Vec3::X)
            .normalize_or(Vec3::X);
        let up = inverse
            .transform_vector3(rotation * Vec3::Y)
            .normalize_or(Vec3::Y);
        let Some(mut mesh) = meshes.get_mut(&handle.0) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(current)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let fresh = positions(&belt.sprites, right, up);
        if current.len() == fresh.len() {
            current.copy_from_slice(&fresh);
        }
    }
}

#[cfg(test)]
#[path = "asteroid_visuals_tests.rs"]
mod runtime_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_belt_stays_bounded_and_all_atlas_tiles_are_used() {
        let first = placements(50.5, 0).unwrap();
        let second = placements(50.5, 0).unwrap();
        assert_eq!(first.len(), COUNT);
        let mut frames = std::collections::HashSet::new();
        for (a, b) in first.iter().zip(second) {
            assert_eq!(a.center, b.center);
            assert!((47.9..=53.1).contains(&a.center.xz().length()));
            assert!(a.center.y.abs() <= 0.6);
            assert!(a.frame < 16);
            frames.insert(a.frame);
        }
        assert_eq!(frames.len(), 16);
        assert!(placements(f32::NAN, 0).is_err());
    }
    #[test]
    fn camera_facing_preserves_centers_sizes_and_frame_uv_bounds() {
        let sprites = vec![Sprite {
            center: Vec3::new(4., 5., 6.),
            half_size: 2.,
            angle: 0.,
            frame: 5,
        }];
        let vertices = positions(&sprites, Vec3::Z, Vec3::Y);
        let center = vertices.iter().map(|p| Vec3::from_array(*p)).sum::<Vec3>() / 4.;
        assert_eq!(center, sprites[0].center);
        assert!(vertices.iter().all(|p| p[0] == 4.));
        assert_eq!(vertices[0], [4., 3., 4.]);
        let mesh = mesh(&sprites);
        let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("UVs");
        };
        assert_eq!(uvs, &vec![[0., 1.], [1., 1.], [1., 0.], [0., 0.]]);
    }

    #[test]
    fn tile_mips_crop_exact_source_pixels_and_omit_merged_levels() {
        let mips: Vec<_> = (0..ATLAS_LEVELS)
            .map(|level| {
                let size = 256u32 >> level;
                let mut rgba = Vec::with_capacity((size * size * 4) as usize);
                for y in 0..size {
                    for x in 0..size {
                        rgba.extend_from_slice(&[x as u8, y as u8, level as u8, 255]);
                    }
                }
                crate::texture::Pixels {
                    width: size,
                    height: size,
                    rgba,
                }
            })
            .collect();
        let tile = tile_mips(&mips, 5).unwrap();
        assert_eq!(
            tile.iter()
                .map(|level| (level.width, level.height))
                .collect::<Vec<_>>(),
            vec![(64, 64), (32, 32), (16, 16), (8, 8), (4, 4), (2, 2), (1, 1)]
        );
        for (level, pixels) in tile.iter().enumerate() {
            let side = 256u32 >> level;
            let tile_side = side / 4;
            let start_x = (5 % 4) as u32 * tile_side;
            let start_y = (5 / 4) as u32 * tile_side;
            assert_eq!(
                &pixels.rgba[..4],
                &[start_x as u8, start_y as u8, level as u8, 255]
            );
        }
        assert!(tile_mips(&mips, 16).is_err());
        assert!(tile_mips(&mips[..8], 0).is_err());
    }
}
