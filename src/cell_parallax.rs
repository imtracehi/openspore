//! Layered depth for the Cell tide pool. Spore's Cell stage is seen from
//! above through deep blue water: blurred radiolarian crystals, leaves, huge
//! out-of-focus creatures and drifting specks sit at several depths below the
//! swimming plane and slide past with perspective parallax as the cell moves.
//!
//! Every sprite and model is installed original art (`cell_background_*`
//! rasters, `cg_rock_*` radiolarians and the Cell species models). Layer
//! depths, densities, tints and the wrap-around placement are OpenSpore
//! adaptations of that look, not a port of the native Cell compositor.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::{NotShadowCaster, NotShadowReceiver},
    math::Affine2,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::{
    asset_catalog::RW4, cell_visuals::CellVisualAssets, dbpf::ResourceKey, game::*,
    stages::StageOwned, Originals,
};

/// World-fixed sprite or model below (positive depth) or above (negative)
/// the swimming plane. Depth, size and wrap tile are given at growth scale 1
/// and scale with the cell, so each zoom level keeps the same composition.
#[derive(Component, Clone, Copy, Debug)]
pub struct CellParallax {
    pub depth: f32,
    pub size: f32,
    pub tile: f32,
    pub spin: f32,
    pub drift: Vec2,
    pub yaw: f32,
}

/// Camera-following floor of the pool whose texture is counter-scrolled so it
/// reads as fixed in the world at its depth.
#[derive(Component)]
pub struct CellBackdrop {
    pub depth: f32,
    pub extent: f32,
    pub repeat: f32,
}

/// Imported atlas raster: `cols` x `rows` equally sized cells.
#[derive(Clone)]
pub struct CellAtlas {
    pub image: Handle<Image>,
    pub cols: u32,
    pub rows: u32,
}

/// Original background rasters and radiolarian models for the parallax layers.
#[derive(Resource, Clone, Default)]
pub struct CellParallaxAssets {
    pub crystals: Option<CellAtlas>,
    pub crystals_blur: Option<CellAtlas>,
    pub crystals_small: Option<CellAtlas>,
    pub leaves: Option<CellAtlas>,
    pub specks: Option<CellAtlas>,
    pub radiolarians: Vec<(crate::OriginalModel, Transform)>,
}

const BACKDROP_DEPTH: f32 = 140.0;
const BACKDROP_EXTENT: f32 = 900.0;

fn raster(originals: &Originals, images: &mut Assets<Image>, name: &str) -> Option<Handle<Image>> {
    let key = ResourceKey {
        type_id: RW4,
        group_id: 0,
        instance_id: crate::world_objects::name_id(name),
    };
    let bytes = originals.resource_index.read(key).ok()?;
    let pixels = crate::texture::decode_rw4(&bytes)?;
    Some(images.add(Image::new(
        Extent3d {
            width: pixels.width,
            height: pixels.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels.rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )))
}

pub fn prepare(
    originals: &mut Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> CellParallaxAssets {
    if originals.resource_index.records.is_empty() {
        return CellParallaxAssets::default();
    }
    let atlas = |images: &mut Assets<Image>, name: &str, cols, rows| {
        raster(originals, images, name).map(|image| CellAtlas { image, cols, rows })
    };
    let mut out = CellParallaxAssets {
        crystals: atlas(images, "cell_background_crystals_4x2a", 4, 2),
        crystals_blur: atlas(images, "cell_background_crystals_newblur_4x2", 4, 2),
        crystals_small: atlas(images, "cell_background_crystals_haloed_small_b", 4, 2),
        leaves: atlas(images, "cell_background_leaf_4pack", 2, 2),
        specks: atlas(images, "cell_backgrounds_tracer_l4_particles", 1, 1),
        radiolarians: Vec::new(),
    };
    for name in [
        "cg_rock_l2_radiolarian_1",
        "cg_rock_l2_radiolarian_2",
        "cg_rock_l2_radiolarian_3",
        "cg_rock_l3_mellita_1",
    ] {
        match crate::cell_visuals::static_model(originals, name, 1.0, meshes, images, materials) {
            Ok(model) => out.radiolarians.push(model),
            Err(error) => warn!("original Cell radiolarian {name}: {error}"),
        }
    }
    out
}

/// Unit quad in the X/Z plane facing +Y, the camera side.
pub fn sprite_quad() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5, 0.0, -0.5],
            [0.5, 0.0, -0.5],
            [-0.5, 0.0, 0.5],
            [0.5, 0.0, 0.5],
        ],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4]);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
    );
    mesh.insert_indices(Indices::U32(vec![0, 2, 1, 1, 2, 3]));
    mesh
}

/// Radial deep-water floor: bright center fading to the dark rim, tinted by
/// the native Cell background palette when present.
fn backdrop_mesh(shallow: [f32; 3], deep: [f32; 3]) -> Mesh {
    const N: usize = 48;
    let side = N + 1;
    let mut positions = Vec::with_capacity(side * side);
    let mut colors = Vec::with_capacity(side * side);
    let mut uvs = Vec::with_capacity(side * side);
    for z in 0..side {
        for x in 0..side {
            let u = x as f32 / N as f32;
            let v = z as f32 / N as f32;
            let p = Vec2::new(u - 0.5, v - 0.5);
            // The visible window is the central ~15% of the floor.
            let r = (p.length() / 0.11).clamp(0.0, 1.0);
            let t = r * r * (3.0 - 2.0 * r);
            let c: [f32; 3] = std::array::from_fn(|i| shallow[i] * (1.0 - t) + deep[i] * t);
            positions.push([p.x, 0.0, p.y]);
            colors.push(Color::srgb(c[0], c[1], c[2]).to_linear().to_f32_array());
            uvs.push([u, v]);
        }
    }
    let mut indices = Vec::with_capacity(N * N * 6);
    for z in 0..N {
        for x in 0..N {
            let a = (z * side + x) as u32;
            let b = a + side as u32;
            indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; side * side]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Spawn the deep floor. Returns its entity so the caller can attach source
/// provenance for the original screen raster.
pub fn spawn_backdrop(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    texture: Option<Handle<Image>>,
    palette: &[[f32; 3]],
) -> Entity {
    // The floor tiles its texture; mirrored repeats hide the raster's seams.
    if let Some(mut image) = texture.as_ref().and_then(|h| images.get_mut(h)) {
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::MirrorRepeat,
            address_mode_v: ImageAddressMode::MirrorRepeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..default()
        });
    }
    // Spore's pool is a saturated ultramarine with a lighter column of light
    // in the middle of the view. The native table's blue entries tint it.
    let native = |i: usize, fallback: [f32; 3]| palette.get(i).copied().unwrap_or(fallback);
    let shallow = mix3(native(2, [0.16, 0.42, 0.78]), [0.20, 0.42, 0.86], 0.55);
    let deep = mix3(native(6, [0.03, 0.10, 0.30]), [0.04, 0.10, 0.36], 0.5);
    let repeat = 5.0;
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: texture,
        uv_transform: Affine2::from_scale(Vec2::splat(repeat)),
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    commands
        .spawn((
            StageOwned,
            CellBackdrop {
                depth: BACKDROP_DEPTH,
                extent: BACKDROP_EXTENT,
                repeat,
            },
            Mesh3d(meshes.add(backdrop_mesh(shallow, deep))),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, -BACKDROP_DEPTH, 0.0).with_scale(Vec3::new(
                BACKDROP_EXTENT,
                1.0,
                BACKDROP_EXTENT,
            )),
            NotShadowCaster,
            NotShadowReceiver,
        ))
        .id()
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t)
}

/// Wrap a layer position to the nearest tile around `center`. Invalid or
/// degenerate inputs keep the entity centered instead of feeding NaNs into
/// its transform.
fn wrap_position(center: Vec2, at: Vec2, tile: f32) -> Vec2 {
    if !center.is_finite() {
        return Vec2::ZERO;
    }
    if !at.is_finite() || !tile.is_finite() || tile <= 0.0 {
        return center;
    }
    let rel = at - center;
    let wrapped =
        (rel + Vec2::splat(tile * 0.5)).rem_euclid(Vec2::splat(tile)) - Vec2::splat(tile * 0.5);
    let result = center + wrapped;
    if result.is_finite() {
        result
    } else {
        center
    }
}

/// One layer of atlas sprites scattered over a wrap tile.
pub struct SpriteLayer {
    pub depth: [f32; 2],
    pub size: [f32; 2],
    pub tile: f32,
    pub count: usize,
    pub tint: [f32; 3],
    pub alpha: [f32; 2],
    pub additive: bool,
    pub spin: f32,
    pub drift: f32,
}

/// Spawn a sprite layer and return the spawned entities (in spawn order).
pub fn spawn_layer(
    commands: &mut Commands,
    quad: &Handle<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    rng: &mut ChaCha8Rng,
    atlas: &CellAtlas,
    layer: &SpriteLayer,
) -> Vec<Entity> {
    let mut spawned = Vec::with_capacity(layer.count);
    for _ in 0..layer.count {
        let cell = rng.random_range(0..atlas.cols * atlas.rows);
        let (col, row) = (cell % atlas.cols, cell / atlas.cols);
        let scale = Vec2::new(1.0 / atlas.cols as f32, 1.0 / atlas.rows as f32);
        let alpha = rng.random_range(layer.alpha[0]..=layer.alpha[1]);
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(layer.tint[0], layer.tint[1], layer.tint[2], alpha),
            base_color_texture: Some(atlas.image.clone()),
            uv_transform: Affine2::from_scale_angle_translation(
                scale,
                0.0,
                Vec2::new(col as f32, row as f32) * scale,
            ),
            alpha_mode: if layer.additive {
                AlphaMode::Add
            } else {
                AlphaMode::Blend
            },
            unlit: true,
            fog_enabled: false,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        let parallax = CellParallax {
            depth: rng.random_range(layer.depth[0]..=layer.depth[1]),
            size: rng.random_range(layer.size[0]..=layer.size[1]),
            tile: layer.tile,
            spin: rng.random_range(-layer.spin..=layer.spin),
            drift: Vec2::new(
                rng.random_range(-layer.drift..=layer.drift),
                rng.random_range(-layer.drift..=layer.drift),
            ),
            yaw: rng.random_range(0.0..std::f32::consts::TAU),
        };
        let at = Vec3::new(
            rng.random_range(-0.5..0.5) * layer.tile,
            -parallax.depth,
            rng.random_range(-0.5..0.5) * layer.tile,
        );
        spawned.push(
            commands
                .spawn((
                    StageOwned,
                    parallax,
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(material),
                    Transform::from_translation(at).with_scale(Vec3::splat(parallax.size)),
                    NotShadowCaster,
                    NotShadowReceiver,
                ))
                .id(),
        );
    }
    spawned
}

/// Ghostly tint for out-of-focus background models: the authored texture is
/// kept, washed toward the water colour and made translucent.
fn ghost_material(
    materials: &mut Assets<StandardMaterial>,
    source: &Handle<StandardMaterial>,
    tint: Color,
) -> Handle<StandardMaterial> {
    let mut ghost = materials.get(source).cloned().unwrap_or_default();
    ghost.base_color = tint;
    ghost.alpha_mode = AlphaMode::Blend;
    ghost.unlit = true;
    ghost.fog_enabled = false;
    ghost.emissive = LinearRgba::BLACK;
    materials.add(ghost)
}

/// Every parallax layer of the pool. Deterministic per stage entry.
pub fn spawn_layers(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &CellParallaxAssets,
    cells: &CellVisualAssets,
) {
    let mut rng = ChaCha8Rng::seed_from_u64(0x5EA_CE11);
    let quad = meshes.add(sprite_quad());
    if let Some(atlas) = &assets.crystals_blur {
        // Far, softly blurred radiolarian crystals.
        spawn_layer(
            commands,
            &quad,
            materials,
            &mut rng,
            atlas,
            &SpriteLayer {
                depth: [70.0, 95.0],
                size: [14.0, 30.0],
                tile: 300.0,
                count: 26,
                tint: [0.55, 0.72, 1.0],
                alpha: [0.25, 0.5],
                additive: false,
                spin: 0.03,
                drift: 0.25,
            },
        );
    }
    if let Some(atlas) = &assets.crystals {
        spawn_layer(
            commands,
            &quad,
            materials,
            &mut rng,
            atlas,
            &SpriteLayer {
                depth: [26.0, 40.0],
                size: [4.0, 10.0],
                tile: 150.0,
                count: 26,
                tint: [0.72, 0.86, 1.0],
                alpha: [0.45, 0.75],
                additive: false,
                spin: 0.06,
                drift: 0.2,
            },
        );
    }
    if let Some(atlas) = &assets.leaves {
        spawn_layer(
            commands,
            &quad,
            materials,
            &mut rng,
            atlas,
            &SpriteLayer {
                depth: [16.0, 30.0],
                size: [5.0, 11.0],
                tile: 130.0,
                count: 10,
                tint: [0.75, 0.95, 0.75],
                alpha: [0.45, 0.7],
                additive: false,
                spin: 0.05,
                drift: 0.25,
            },
        );
    }
    if let Some(atlas) = &assets.crystals_small {
        spawn_layer(
            commands,
            &quad,
            materials,
            &mut rng,
            atlas,
            &SpriteLayer {
                depth: [6.0, 14.0],
                size: [1.0, 2.6],
                tile: 80.0,
                count: 30,
                tint: [0.8, 0.9, 1.0],
                alpha: [0.45, 0.8],
                additive: false,
                spin: 0.2,
                drift: 0.3,
            },
        );
        // A few large, faint shapes drifting between the camera and the cell.
        spawn_layer(
            commands,
            &quad,
            materials,
            &mut rng,
            atlas,
            &SpriteLayer {
                depth: [-12.0, -8.0],
                size: [5.0, 9.0],
                tile: 70.0,
                count: 4,
                tint: [0.75, 0.88, 1.0],
                alpha: [0.08, 0.16],
                additive: false,
                spin: 0.05,
                drift: 0.4,
            },
        );
    }
    if let Some(atlas) = &assets.specks {
        // Suspended specks just under the surface the cell swims in.
        spawn_layer(
            commands,
            &quad,
            materials,
            &mut rng,
            atlas,
            &SpriteLayer {
                depth: [0.8, 9.0],
                size: [0.25, 0.7],
                tile: 64.0,
                count: 90,
                tint: [0.75, 0.9, 1.0],
                alpha: [0.35, 0.8],
                additive: true,
                spin: 0.6,
                drift: 0.35,
            },
        );
    }
    // Radiolarian skeletons resting at depth.
    for i in 0..10 {
        let Some((model, local)) = assets
            .radiolarians
            .get(i % assets.radiolarians.len().max(1))
        else {
            break;
        };
        let parallax = CellParallax {
            depth: rng.random_range(10.0..34.0),
            size: rng.random_range(2.5..6.0),
            tile: 140.0,
            spin: rng.random_range(-0.08..0.08),
            drift: Vec2::ZERO,
            yaw: rng.random_range(0.0..std::f32::consts::TAU),
        };
        let tint = Color::srgba(0.62, 0.78, 1.0, 0.7);
        let Some(source_material) = model.material.as_ref() else {
            warn!(
                "Cell radiolarian {} has no material; skipping backdrop model",
                i
            );
            continue;
        };
        let material = ghost_material(materials, source_material, tint);
        let root = commands
            .spawn((
                StageOwned,
                parallax,
                Transform::from_xyz(
                    rng.random_range(-70.0..70.0),
                    -parallax.depth,
                    rng.random_range(-70.0..70.0),
                ),
                Visibility::default(),
            ))
            .id();
        commands.entity(root).with_children(|p| {
            p.spawn((
                StageOwned,
                Mesh3d(model.draw_mesh()),
                MeshMaterial3d(material),
                *local,
                NotShadowCaster,
                NotShadowReceiver,
            ));
        });
    }
    // Huge, out-of-focus creatures of a larger world below.
    for i in 0..5 {
        if cells.species.is_empty() {
            break;
        }
        let species = &cells.species[(i * 11 + 3) % cells.species.len()];
        let parallax = CellParallax {
            depth: rng.random_range(45.0..75.0),
            size: rng.random_range(9.0..16.0),
            tile: 260.0,
            spin: rng.random_range(-0.04..0.04),
            drift: Vec2::new(rng.random_range(-0.8..0.8), rng.random_range(-0.8..0.8)),
            yaw: rng.random_range(0.0..std::f32::consts::TAU),
        };
        let tint = Color::srgba(0.30, 0.45, 0.85, 0.42);
        let Some(source_material) = species.model.material.as_ref() else {
            warn!(
                "Cell backdrop species {} has no material; skipping model",
                species.rule.name
            );
            continue;
        };
        let material = ghost_material(materials, source_material, tint);
        let root = commands
            .spawn((
                StageOwned,
                parallax,
                Transform::from_xyz(
                    rng.random_range(-130.0..130.0),
                    -parallax.depth,
                    rng.random_range(-130.0..130.0),
                ),
                Visibility::default(),
            ))
            .id();
        commands.entity(root).with_children(|p| {
            p.spawn((
                StageOwned,
                Mesh3d(species.model.draw_mesh()),
                MeshMaterial3d(material),
                species.local,
                NotShadowCaster,
                NotShadowReceiver,
            ));
        });
    }
}

/// Keep every layer wrapped around the player at the current zoom level and
/// counter-scroll the deep floor so it reads as fixed in the world.
pub fn update(
    time: Res<Time>,
    run: Res<RunProgress>,
    player: Query<&Transform, (With<Player>, Without<CellParallax>, Without<CellBackdrop>)>,
    mut layers: Query<(&mut Transform, &mut CellParallax), Without<CellBackdrop>>,
    mut backdrop: Query<
        (
            &mut Transform,
            &CellBackdrop,
            &MeshMaterial3d<StandardMaterial>,
        ),
        Without<CellParallax>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(player) = player.single() else {
        return;
    };
    let s = crate::cell::growth_scale(run.dna_this_stage);
    let center = Vec2::new(player.translation.x, player.translation.z);
    let dt = time.delta_secs();
    for (mut t, mut p) in &mut layers {
        p.yaw += p.spin * dt;
        let tile = p.tile * s;
        let mut at = Vec2::new(t.translation.x, t.translation.z) + p.drift * s * dt;
        at = wrap_position(center, at, tile);
        t.translation = Vec3::new(at.x, -p.depth * s, at.y);
        t.scale = Vec3::splat(p.size * s);
        t.rotation = Quat::from_rotation_y(p.yaw);
    }
    for (mut t, floor, material) in &mut backdrop {
        let extent = floor.extent * s;
        t.translation = Vec3::new(center.x, -floor.depth * s, center.y);
        t.scale = Vec3::new(extent, 1.0, extent);
        if let Some(mut material) = materials.get_mut(&material.0) {
            let offset = center / extent * floor.repeat;
            material.uv_transform =
                Affine2::from_scale_angle_translation(Vec2::splat(floor.repeat), 0.0, offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_wrap_keeps_positions_inside_the_nearest_tile() {
        let center = Vec2::new(10.0, -4.0);
        let wrapped = wrap_position(center, Vec2::new(101.0, -95.0), 20.0);
        assert!(wrapped.abs_diff_eq(Vec2::new(1.0, 5.0), 1e-5), "{wrapped}");
        let negative = wrap_position(center, Vec2::new(-51.0, 56.0), 20.0);
        assert!(
            negative.abs_diff_eq(Vec2::new(9.0, -4.0), 1e-5),
            "{negative}"
        );
    }

    #[test]
    fn invalid_wrap_inputs_fall_back_to_the_center() {
        let center = Vec2::new(2.0, 3.0);
        assert_eq!(wrap_position(center, Vec2::ZERO, 0.0), center);
        assert_eq!(wrap_position(center, Vec2::splat(f32::NAN), 5.0), center);
        assert_eq!(
            wrap_position(Vec2::splat(f32::NAN), Vec2::ZERO, 5.0),
            Vec2::ZERO
        );
    }
}
