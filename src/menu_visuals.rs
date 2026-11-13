//! Main-menu galaxy. Spore opens on a slowly turning spiral galaxy floating
//! in deep space. This scene layers the installed galaxy rasters
//! (`SG_galaxy_arms`, `SG_galaxy_arms_colored2`, `sg_galaxy_nebulaes_topdown`)
//! on a tilted disc, surrounds it with star sprites from `SG_star_16pack`
//! and faint original nebula plates. The disc projection, tilt, spin and
//! star placement are adaptations, not native galaxy generation. The
//! unrelated window texture in ui_spore_galaxy is excluded.
use crate::{
    dbpf::ResourceKey,
    stages::{GameStage, StageOwned},
    Originals,
};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
pub const GALAXY_ARMS: ResourceKey = raster(0x63B755D0);
/// `SG_galaxy_arms_colored2`: the warm core and blue arms.
pub const GALAXY_COLOR: ResourceKey = raster(0x3FA16323);
/// `sg_galaxy_nebulaes_topdown`: dust and nebula clumps along the arms.
pub const GALAXY_NEBULAE: ResourceKey = raster(0x7F813388);
/// `SG_star_16pack`: a 4x4 atlas of star glows.
pub const STAR_ATLAS: ResourceKey = raster(0x13EFD597);
/// `nebula1`..`nebula5` plates.
const NEBULAE: [u32; 5] = [0xBEBC2889, 0xBEBC288A, 0xBEBC288B, 0xBEBC288C, 0xBEBC288D];

const fn raster(instance_id: u32) -> ResourceKey {
    ResourceKey {
        group_id: 0,
        instance_id,
        type_id: crate::asset_catalog::RW4,
    }
}

#[derive(Component)]
pub struct MenuVisual;
#[derive(Component)]
pub(crate) struct MenuStars;
/// A nebula plate placed relative to the view.
#[derive(Component)]
pub(crate) struct MenuNebula {
    offset: Vec3,
    roll: f32,
    size: f32,
}
/// A galaxy disc layer; `spin` is its angular speed around the disc axis.
#[derive(Component)]
pub(crate) struct MenuGalaxyLayer {
    spin: f32,
    lift: f32,
}
#[derive(Component, Clone)]
pub struct NativeMenuGalaxy {
    pub key: ResourceKey,
    pub image: Handle<Image>,
    pub dimensions: UVec2,
}
#[derive(Resource, Default)]
pub struct MenuVisualCache {
    galaxy: Option<NativeMenuGalaxy>,
    layers: Vec<NativeMenuGalaxy>,
    stars: Option<Handle<Image>>,
    nebulae: Vec<Handle<Image>>,
}

fn decode(
    originals: &Originals,
    images: &mut Assets<Image>,
    key: ResourceKey,
) -> Option<NativeMenuGalaxy> {
    let bytes = originals.resource_index.read(key).ok()?;
    let Some(p) = crate::texture::decode_rw4(&bytes) else {
        warn!("native menu raster {key:?} did not decode");
        return None;
    };
    let dimensions = UVec2::new(p.width, p.height);
    let image = images.add(Image::new(
        Extent3d {
            width: p.width,
            height: p.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        p.rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    Some(NativeMenuGalaxy {
        key,
        image,
        dimensions,
    })
}

/// Flat disc in X/Z facing +Y, UV-mapped onto a square raster. Vertex colours
/// warm the core and cool the rim so the arms read blue like Spore's galaxy.
fn galaxy_disc(core: [f32; 3], rim: [f32; 3]) -> Mesh {
    const RINGS: usize = 24;
    const SEGMENTS: usize = 72;
    let mut positions = vec![[0.0, 0.0, 0.0]];
    let mut uvs = vec![[0.5, 0.5]];
    let mut colors = vec![Color::srgb(core[0], core[1], core[2])
        .to_linear()
        .to_f32_array()];
    for ring in 1..=RINGS {
        let r = ring as f32 / RINGS as f32 * 0.5;
        let t = (r / 0.5).powf(0.7);
        let c: [f32; 3] = std::array::from_fn(|i| core[i] * (1.0 - t) + rim[i] * t);
        for segment in 0..SEGMENTS {
            let a = segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            positions.push([a.cos() * r, 0.0, a.sin() * r]);
            uvs.push([0.5 + a.cos() * r, 0.5 + a.sin() * r]);
            colors.push(Color::srgb(c[0], c[1], c[2]).to_linear().to_f32_array());
        }
    }
    let mut indices = Vec::new();
    for segment in 0..SEGMENTS {
        let a = 1 + segment as u32;
        let b = 1 + ((segment + 1) % SEGMENTS) as u32;
        indices.extend([0, b, a]);
    }
    for ring in 1..RINGS {
        let inner = 1 + (ring - 1) * SEGMENTS;
        let outer = 1 + ring * SEGMENTS;
        for segment in 0..SEGMENTS {
            let next = (segment + 1) % SEGMENTS;
            let (a, b) = ((inner + segment) as u32, (inner + next) as u32);
            let (c, d) = ((outer + segment) as u32, (outer + next) as u32);
            indices.extend([a, b, c, b, d, c]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn additive(
    materials: &mut Assets<StandardMaterial>,
    image: Option<Handle<Image>>,
    color: Color,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color_texture: image,
        base_color: color,
        alpha_mode: AlphaMode::Add,
        unlit: true,
        fog_enabled: false,
        cull_mode: None,
        ..default()
    })
}

pub fn spawn_menu_galaxy_art(
    mut commands: Commands,
    existing: Query<Entity, With<MenuVisual>>,
    originals: Res<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<MenuVisualCache>,
) {
    if !existing.is_empty() {
        return;
    }
    if cache.galaxy.is_none() {
        let Some(arms) = decode(&originals, &mut images, GALAXY_ARMS) else {
            return;
        };
        cache.galaxy = Some(arms);
        cache.layers = [GALAXY_COLOR, GALAXY_NEBULAE]
            .into_iter()
            .filter_map(|key| decode(&originals, &mut images, key))
            .collect();
        cache.stars = decode(&originals, &mut images, STAR_ATLAS).map(|s| s.image);
        cache.nebulae = NEBULAE
            .into_iter()
            .filter_map(|id| decode(&originals, &mut images, raster(id)).map(|n| n.image))
            .collect();
    }
    let galaxy = cache.galaxy.as_ref().unwrap().clone();
    // The structural arms raster stays the recorded native galaxy source.
    let disc = meshes.add(galaxy_disc([1.0, 0.86, 0.66], [0.55, 0.70, 1.0]));
    let arms_material = additive(&mut materials, Some(galaxy.image.clone()), Color::WHITE);
    commands.spawn((
        StageOwned,
        MenuVisual,
        MenuGalaxyLayer {
            spin: 0.035,
            lift: 0.0,
        },
        galaxy,
        Mesh3d(disc.clone()),
        MeshMaterial3d(arms_material),
        Transform::default(),
    ));
    for (index, layer) in cache.layers.iter().enumerate() {
        let tint = if index == 0 {
            Color::srgb(1.0, 1.0, 1.0)
        } else {
            Color::srgb(0.85, 0.75, 0.95)
        };
        commands.spawn((
            StageOwned,
            MenuVisual,
            MenuGalaxyLayer {
                spin: 0.035 + 0.006 * (index as f32 + 1.0),
                lift: 0.02 * (index as f32 + 1.0),
            },
            Mesh3d(disc.clone()),
            MeshMaterial3d(additive(&mut materials, Some(layer.image.clone()), tint)),
            Transform::default(),
        ));
    }

    // Star glows from the original 4x4 sprite atlas, scattered on a sphere
    // around the viewer: one mesh, sixteen sprite variants by UV.
    let mut seed = 0x42639u32;
    let mut random = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16777216.
    };
    let mut positions = vec![];
    let mut uvs = vec![];
    let mut colors = vec![];
    let mut indices = vec![];
    for _ in 0..900 {
        let z = random() * 2.0 - 1.0;
        let a = random() * std::f32::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        let center = Vec3::new(r * a.cos(), z, r * a.sin()) * 60.0;
        let size = 0.12 + random().powf(6.0) * 1.4;
        let tangent = center.cross(Vec3::Y).normalize_or(Vec3::X);
        let bitangent = center.cross(tangent).normalize() * size;
        let tangent = tangent * size;
        let cell = (random() * 16.0) as u32 % 16;
        let (u, v) = ((cell % 4) as f32 * 0.25, (cell / 4) as f32 * 0.25);
        let warm = random();
        let brightness = 0.35 + random() * 0.65;
        let color = Color::srgb(
            brightness * (0.75 + 0.25 * warm),
            brightness * 0.85,
            brightness * (1.0 - 0.25 * warm),
        )
        .to_linear()
        .to_f32_array();
        let start = positions.len() as u32;
        for (corner, uv) in [
            (-tangent - bitangent, [u, v]),
            (tangent - bitangent, [u + 0.25, v]),
            (tangent + bitangent, [u + 0.25, v + 0.25]),
            (-tangent + bitangent, [u, v + 0.25]),
        ] {
            positions.push((center + corner).to_array());
            uvs.push(uv);
            colors.push(color);
        }
        indices.extend([start, start + 1, start + 2, start, start + 2, start + 3]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    let stars = cache.stars.clone();
    commands.spawn((
        StageOwned,
        MenuVisual,
        MenuStars,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(additive(&mut materials, stars, Color::WHITE)),
        Transform::default(),
    ));

    // A few faint nebula plates far behind the galaxy.
    let quad = meshes.add(Rectangle::new(1., 1.));
    for (i, image) in cache.nebulae.iter().enumerate() {
        let a = i as f32 * 1.9 + 0.4;
        commands.spawn((
            StageOwned,
            MenuVisual,
            MenuNebula {
                offset: Vec3::new(a.cos() * 30.0, a.sin() * 14.0, -45.0),
                roll: a,
                size: 14.0 + 5.0 * (i % 3) as f32,
            },
            Mesh3d(quad.clone()),
            MeshMaterial3d(additive(
                &mut materials,
                Some(image.clone()),
                Color::srgb(0.32, 0.30, 0.36),
            )),
            Transform::from_scale(Vec3::splat(14.0 + 5.0 * (i % 3) as f32)),
        ));
    }
}

/// World units per galaxy unit: the galaxy disc (radius 1000 galaxy units,
/// `notes\front-end-notes.md` section 1) is 11 world units across its radius.
pub const GALAXY_SCALE: f32 = 0.011;
/// The galaxy centre in the world (the `galaxy_center_ref` the GGE
/// cinematics aim at). The disc lies in the world XZ plane, as the Spore
/// galaxy lies in its XY plane (`cinematics::camera::spore_to_bevy`).
pub const GALAXY_CENTRE: Vec3 = Vec3::ZERO;
/// The resting galaxy-screen camera: the end of the retail `GGE_intro` path,
/// (0, -1000, 500) from the galaxy centre looking at centre + (0, 35, -115)
/// (GGEIntro.trigger), in world units.
pub const REST_EYE: Vec3 = Vec3::new(0.0, 500.0 * GALAXY_SCALE, 1000.0 * GALAXY_SCALE);
pub const REST_LOOK: Vec3 = Vec3::new(0.0, -115.0 * GALAXY_SCALE, -35.0 * GALAXY_SCALE);

/// The galaxy screen's camera at rest.
pub fn rest_camera() -> Transform {
    Transform::from_translation(GALAXY_CENTRE + REST_EYE).looking_at(GALAXY_CENTRE + REST_LOOK, Vec3::Y)
}

pub fn menu_visual_presentation(
    time: Res<Time>,
    state: Res<State<GameStage>>,
    options: Res<crate::HudOptions>,
    editor: Res<crate::editor::EditorState>,
    mut clear: ResMut<ClearColor>,
    cameras: Query<&GlobalTransform, With<crate::GameCamera>>,
    mut visuals: Query<
        (
            &mut Transform,
            &mut Visibility,
            Option<&MenuStars>,
            Option<&MenuGalaxyLayer>,
            Option<&MenuNebula>,
        ),
        With<MenuVisual>,
    >,
) {
    let show = *state.get() == GameStage::MainMenu && !editor.open && !options.diagnostics;
    let Ok(camera) = cameras.single() else {
        return;
    };
    if show {
        clear.0 = Color::srgb(0.0, 0.0, 0.012);
    }
    let elapsed = time.elapsed_secs();
    // The backdrop (star sphere, nebula plates) grows with the distance to
    // the galaxy so it stays behind it during the `GGE_intro` fly-in (the
    // camera starts ~20000 galaxy units out); 1 at the resting camera.
    let reach = (camera.translation().distance(GALAXY_CENTRE) / 50.0).clamp(1.0, 4.5);
    for (mut transform, mut visibility, stars, layer, nebula) in &mut visuals {
        *visibility = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if let Some(layer) = layer {
            // The galaxy lies in its own frame (GALAXY_CENTRE, the world XZ
            // plane, GALAXY_SCALE): the retail `GGE_intro` path and the resting
            // galaxy-screen camera are given in it (REST_EYE / REST_LOOK: the
            // view axis 30.7 degrees below the galaxy plane, 1118 galaxy units
            // from the centre). The galaxy turns slowly around its own axis
            // (ADAPTED).
            transform.translation = GALAXY_CENTRE + Vec3::Y * layer.lift;
            transform.rotation = Quat::from_rotation_y(elapsed * layer.spin);
            transform.scale = Vec3::new(2000.0 * GALAXY_SCALE, 1.0, 2000.0 * GALAXY_SCALE);
        } else if let Some(nebula) = nebula {
            transform.translation = camera.translation() + camera.rotation() * (nebula.offset * reach);
            transform.rotation = camera.rotation() * Quat::from_rotation_z(nebula.roll);
            transform.scale = Vec3::splat(nebula.size * reach);
        } else if stars.is_some() {
            // The star sphere stays centred on the viewer.
            transform.translation = camera.translation();
            transform.scale = Vec3::splat(reach);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn galaxy_disc_maps_the_raster_square_and_covers_the_core() {
        let mesh = galaxy_disc([1.0; 3], [0.5; 3]);
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("disc has UVs");
        };
        assert_eq!(uvs[0], [0.5, 0.5], "the centre samples the galaxy core");
        assert!(uvs
            .iter()
            .all(|uv| uv.iter().all(|c| (0.0..=1.0).contains(c))));
    }
    #[test]
    #[ignore = "requires original Spore installation"]
    fn installed_menu_galaxy_has_native_alpha_and_visible_core() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        assert_eq!(
            GALAXY_ARMS.instance_id,
            crate::world_objects::name_id("SG_galaxy_arms")
        );
        assert_eq!(
            GALAXY_COLOR.instance_id,
            crate::world_objects::name_id("SG_galaxy_arms_colored2")
        );
        assert_eq!(
            STAR_ATLAS.instance_id,
            crate::world_objects::name_id("SG_star_16pack")
        );
        let p = crate::texture::decode_rw4(&index.read(GALAXY_ARMS).unwrap()).unwrap();
        assert_eq!((p.width, p.height), (1024, 1024));
        assert_eq!(p.rgba[3], 0, "transparent corner");
        assert!(
            p.rgba[((p.height / 2 * p.width + p.width / 2) * 4 + 3) as usize] > 200,
            "visible galaxy core"
        );
    }
}
