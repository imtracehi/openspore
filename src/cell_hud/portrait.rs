//! The portrait of the player's cell in the bottom right corner of the HUD.
//!
//! The retail game stores a PNG thumbnail of the player's creature (taken in
//! the creator) and the HUD layout shows it in window `0x03D99151` of
//! `GlobalUICell-3`. Here the thumbnail is rendered live: the cell body and its
//! mounted parts (`RunProgress.cell_parts`, rest pose) are drawn straight down
//! with the creator's lighting and gamma-space materials, the front to the left
//! like the creator view, into a small image that is rebuilt whenever the parts
//! change (for example after the creator closes).
//!
//! Every mounted part draws the texture of its own instance (`cell_paint::CellPaintAssets::instance_material`,
//! the same material the stage and the Creator show for `run.cell_parts[i]`: retail keeps one atlas rectangle
//! per rigblock instance, `FUN_004CB890`), so two parts of one kind carry their own brush stamps; the
//! portrait is redrawn when the paint pass repaints an instance (`CellPaintAssets::revision`).
//!
//! The retail thumbnail is taken in the creator, so the scene is the Creator's:
//! the Cell level's installed pedestal model (`cl_editor_base_level0..4`: the
//! microscope backdrop raster and the ring platform, imported and bound as the
//! Creator does, `cell_editor::import_pedestal`) with the cell at the Creator's
//! subject scale, framed on the cell. ADAPTED: the view beyond the backdrop
//! plane (never reached at these zooms) is a flat blue, the creator backdrop's
//! average colour; the framing margin; the lighting is the plain editor
//! fallback light (the Creator's SH light is not applied to the portrait). The
//! image is shown by a plain UI node placed over the window, above the HUD
//! surface (the HUD surface composites raw gamma-space texels, a render target
//! is sRGB).

use std::collections::HashMap;

use bevy::{
    asset::AssetId,
    camera::{visibility::RenderLayers, RenderTarget, ScalingMode},
    prelude::*,
    render::render_resource::TextureFormat,
};

use super::{CellHudActive, CellHudRoot};
use crate::{
    cell_contract::CellHudState,
    cell_editor_scene as scene,
    cell_visuals::CellVisualAssets,
    game::{PartInstance, RunProgress},
    Originals,
};

/// The portrait window of `GlobalUICell-3`.
pub const PORTRAIT_WINDOW: u32 = 0x03D9_9151;
/// Layer of the portrait scene (28/29 editor, 30 mechanical preview, 31 part
/// thumbnails, 1 cell background).
const LAYER: usize = 27;
/// Size of the rendered image in pixels (the window is about 62 px).
const SIZE: u32 = 128;
/// ADAPTED: clear colour beyond the pedestal's backdrop, the creator backdrop's
/// average colour.
const BACKDROP: (u8, u8, u8) = (57, 64, 127);
/// Frames the camera renders after a change (asynchronous pipeline and
/// texture preparation, like the part thumbnails).
const RENDER_FRAMES: u32 = 45;
/// Frames after a (re)build before the image is shown.
const SETTLE_FRAMES: u32 = 3;
/// The view is this much larger than the cell's box.
const FRAME_MARGIN: f32 = 1.12;
/// Initial view (the body alone), replaced as soon as the cell is built.
const MIN_RADIUS: f32 = 1.05;

/// Plain UI node showing the portrait.
#[derive(Component)]
pub struct PortraitNode;

/// Marks the portrait camera, light and cell.
#[derive(Component)]
pub struct PortraitScene;

#[derive(Resource, Default)]
pub struct PortraitState {
    image: Option<Handle<Image>>,
    camera: Option<Entity>,
    scene: Option<Entity>,
    light: Option<Entity>,
    /// The Creator's pedestal (ring platform and backdrop) behind the cell.
    pedestal: Option<Entity>,
    node: Option<Entity>,
    /// What the scene was built from: parts, imported part models, body, paint, and the paint pass
    /// (`CellPaintAssets::revision`: the instances' own textures are repainted in place).
    built: Option<(Vec<PartInstance>, usize, bool, crate::cell_paint::CellPaint, u32)>,
    frames: u32,
    gamma: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
}

impl PortraitState {
    /// The rendered portrait image (once the HUD has shown it).
    pub fn image(&self) -> Option<Handle<Image>> {
        self.image.clone().filter(|_| self.built.is_some())
    }
}

/// Floor and axis aligned box (x, z) of a mesh seen through a transform.
pub fn mesh_box(meshes: &Assets<Mesh>, mesh: &Handle<Mesh>, transform: &Transform) -> Option<(Vec2, Vec2)> {
    let bevy::mesh::VertexAttributeValues::Float32x3(positions) = meshes.get(mesh)?.attribute(Mesh::ATTRIBUTE_POSITION)?
    else {
        return None;
    };
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for p in positions {
        let at = transform.transform_point(Vec3::from_array(*p));
        min = min.min(Vec2::new(at.x, at.z));
        max = max.max(Vec2::new(at.x, at.z));
    }
    (min.is_finite() && max.is_finite()).then_some((min, max))
}

/// Camera framing of the cell: the box is centred and the view is the larger
/// side plus a margin, never tighter than the body.
pub fn framing(parts: &[(Vec2, Vec2)]) -> (Vec2, f32) {
    let body = (Vec2::splat(-crate::cell::CELL_BODY_RADIUS), Vec2::splat(crate::cell::CELL_BODY_RADIUS));
    let (min, max) = parts
        .iter()
        .fold(body, |(min, max), (a, b)| (min.min(*a), max.max(*b)));
    let size = (max - min).max_element().max(2.0 * crate::cell::CELL_BODY_RADIUS) * FRAME_MARGIN;
    ((min + max) * 0.5, size)
}

/// The Creator's pedestal in the portrait scene: a root and one entity per
/// surface, all on the portrait layer. Each surface is marked as a drawn
/// surface ([`crate::material_binding::OriginalSurfaceChild`]): without it
/// `material_binding::attach_remaining` took the first surface for a whole
/// model and spawned the backdrop again under it with no render layer, which
/// the main camera drew as a translucent square at the world origin (the
/// Cell stage showed it over the water for the whole level).
fn spawn_pedestal(
    commands: &mut Commands,
    model: &crate::OriginalModel,
    gamma: &mut HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Entity {
    let layer = RenderLayers::layer(LAYER);
    let root = commands
        .spawn((PortraitScene, Transform::default(), Visibility::default(), layer.clone(), crate::stages::StageOwned))
        .id();
    commands.entity(root).with_children(|p| {
        for surface in &model.surfaces {
            p.spawn((
                crate::material_binding::OriginalSurfaceChild::of(surface),
                Mesh3d(surface.handle.clone()),
                MeshMaterial3d(scene::gamma_material(gamma, &surface.material, materials, images)),
                // As in the Creator: the platform's pointer faces the cell's front.
                Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
                layer.clone(),
            ));
        }
    });
    root
}

/// `--cell-hud-parts` (captures): mount a flagellum, a jaw and two eyes on the
/// player's cell so that the portrait can be checked with parts. Imports the
/// part models like the creator does.
#[allow(clippy::too_many_arguments)]
pub fn demo_parts(
    stage: Res<State<crate::stages::GameStage>>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut done: Local<Option<bool>>,
) {
    let on = *done.get_or_insert_with(|| std::env::args().any(|a| a == "--cell-hud-parts"));
    if !on || *stage.get() != crate::stages::GameStage::Cell || originals.resource_index.records.is_empty() {
        return;
    }
    *done = Some(false);
    // (part file name, attachment point, direction it points, scale)
    let wanted = [
        ("cl_movement_flagella_03", Vec3::new(0.0, 0.0, 0.85), Vec3::Z, 1.0),
        ("cl_mouth_jaw_04", Vec3::new(0.0, 0.0, -0.85), -Vec3::Z, 1.0),
        ("cl_sense_eye_01", Vec3::new(-0.35, 0.0, -0.55), -Vec3::X, 1.0),
        ("cl_sense_eye_01", Vec3::new(0.35, 0.0, -0.55), Vec3::X, 1.0),
    ];
    let mut parts = Vec::new();
    for (name, point, dir, scale) in wanted {
        let Some(definition) = originals
            .part_catalog
            .parts
            .iter()
            .find(|p| {
                p.key.group_id == crate::asset_catalog::CELL_PARTS
                    && p.key.instance_id == crate::world_objects::name_id(name)
            })
            .cloned()
        else {
            continue;
        };
        if crate::editor::ensure_model(&mut originals, &definition, &mut meshes, &mut images, &mut materials).is_err() {
            continue;
        }
        let mut part = PartInstance::new(definition.model.instance_id, 0).with_definition(definition.key.instance_id);
        part.offset = scene::offset_for(point);
        part.yaw = scene::yaw_for(dir);
        part.scale = scale;
        parts.push(part);
    }
    run.cell_parts = parts;
}

/// Keep the portrait scene, camera, image and UI node in step with the HUD.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut state: ResMut<PortraitState>,
    active: Res<CellHudActive>,
    run: Res<RunProgress>,
    mut originals: ResMut<Originals>,
    visuals: Option<Res<CellVisualAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut paint: ResMut<crate::cell_paint::CellPaintAssets>,
    mut cameras: Query<&mut Camera, With<PortraitScene>>,
) {
    let body = visuals.as_deref().and_then(|v| v.body.as_ref());
    if !active.0 || body.is_none() {
        // Leave the HUD: drop the scene and the node (the image is kept).
        // (the entities may already be gone with the stage)
        for e in [state.camera.take(), state.scene.take(), state.light.take(), state.pedestal.take(), state.node.take()]
            .into_iter()
            .flatten()
        {
            if let Ok(mut entity) = commands.get_entity(e) {
                entity.despawn();
            }
        }
        state.built = None;
        return;
    }
    let image = state
        .image
        .get_or_insert_with(|| {
            images.add(Image::new_target_texture(SIZE, SIZE, TextureFormat::Rgba8UnormSrgb, None))
        })
        .clone();
    if state.node.is_none() {
        let node = commands
            .spawn((
                PortraitNode,
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                ImageNode::new(image.clone()),
                Visibility::Hidden,
                Pickable::IGNORE,
                GlobalZIndex(101),
                crate::stages::StageOwned,
            ))
            .id();
        state.node = Some(node);
    }
    if state.camera.is_none() {
        let layer = RenderLayers::layer(LAYER);
        let (backdrop_r, backdrop_g, backdrop_b) = BACKDROP;
        let camera = commands
            .spawn((
                PortraitScene,
                Camera3d::default(),
                // As the creator: plain LDR, the textures keep their colours.
                bevy::core_pipeline::tonemapping::Tonemapping::None,
                Camera {
                    order: -3,
                    clear_color: ClearColorConfig::Custom(Color::srgb_u8(backdrop_r, backdrop_g, backdrop_b)),
                    ..default()
                },
                RenderTarget::from(image.clone()),
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed {
                        width: 2.0 * MIN_RADIUS,
                        height: 2.0 * MIN_RADIUS,
                    },
                    ..OrthographicProjection::default_3d()
                }),
                // Straight down; world +X is screen up, so the cell's front
                // (-Z) is on the left like in the creator.
                Transform::from_xyz(0.0, 20.0, 0.0).looking_at(Vec3::ZERO, Vec3::X),
                layer.clone(),
                AmbientLight {
                    color: Color::WHITE,
                    brightness: scene::AMBIENT_BRIGHTNESS,
                    ..default()
                },
                crate::stages::StageOwned,
            ))
            .id();
        let light = commands
            .spawn((
                PortraitScene,
                DirectionalLight {
                    color: Color::WHITE,
                    illuminance: scene::SUN_ILLUMINANCE,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_translation(Vec3::new(-0.75, 1.0, 0.1).normalize() * 20.0)
                    .looking_at(Vec3::ZERO, Vec3::Y),
                layer,
                crate::stages::StageOwned,
            ))
            .id();
        state.camera = Some(camera);
        state.light = Some(light);
        state.built = None;
        // The Creator's pedestal of the cell's level behind the cell.
        let level = crate::cell_editor::editor_level(run.cell.food_progression);
        if let Some(key) = crate::cell_editor_data::level_pedestal(&originals.resource_index, level) {
            match crate::cell_editor::import_pedestal(&mut originals, key, &mut meshes, &mut images, &mut materials) {
                Ok((model, _)) => {
                    let mut gamma = std::mem::take(&mut state.gamma);
                    state.pedestal = Some(spawn_pedestal(&mut commands, &model, &mut gamma, &mut materials, &mut images));
                    state.gamma = gamma;
                }
                Err(e) => warn!("cell portrait pedestal {key:?}: {e}"),
            }
        }
    }
    // Rebuild the cell when its parts (or their imported models) changed.
    let signature = (
        run.cell_parts.clone(),
        originals.models.len(),
        body.is_some(),
        run.cell_creation.paint.clone(),
        paint.revision,
    );
    if state.built.as_ref() != Some(&signature) {
        if let Some(old) = state.scene.take() {
            if let Ok(mut entity) = commands.get_entity(old) {
                entity.despawn();
            }
        }
        let layer = RenderLayers::layer(LAYER);
        // The cell at the Creator's subject scale next to its pedestal.
        let root = commands
            .spawn((
                PortraitScene,
                Transform::from_scale(Vec3::splat(scene::SUBJECT_SCALE)),
                Visibility::default(),
                layer.clone(),
                crate::stages::StageOwned,
            ))
            .id();
        // The gamma copies snapshot their source's texture: a repaint (the paint pass above) needs fresh ones.
        state.gamma.clear();
        let mut gamma = std::mem::take(&mut state.gamma);
        let mut boxes = Vec::new();
        commands.entity(root).with_children(|p| {
            if let Some(body) = body {
                if let Some(material) = body.material.as_ref() {
                    boxes.extend(mesh_box(&meshes, &body.draw_mesh(), &Transform::default()));
                    p.spawn((
                        Mesh3d(body.draw_mesh()),
                        MeshMaterial3d(scene::gamma_material(&mut gamma, material, &mut materials, &mut images)),
                        Transform::default(),
                        layer.clone(),
                    ));
                }
            }
            for (index, part) in run.cell_parts.iter().enumerate() {
                let Some(model) = crate::editor::model_for_part(&originals, part) else {
                    continue;
                };
                let Some(material) = model.material.as_ref() else {
                    continue;
                };
                // Retail keeps one atlas rectangle per rigblock instance (`FUN_004CB890`): the part
                // draws the texture of its own instance (`cell_paint`), so two parts of one kind differ.
                let material = paint
                    .instance_material(index, model, &originals, &mut materials, &mut images)
                    .unwrap_or_else(|| material.clone());
                let transform = scene::part_transform(part, model.scale);
                boxes.extend(mesh_box(&meshes, &model.draw_mesh(), &transform));
                p.spawn((
                    Mesh3d(model.draw_mesh()),
                    MeshMaterial3d(scene::gamma_material(&mut gamma, &material, &mut materials, &mut images)),
                    transform,
                    layer.clone(),
                ));
            }
        });
        state.gamma = gamma;
        // Frame the whole cell: centred, square, a little larger than its box.
        if let Some(camera) = state.camera {
            let (centre, size) = framing(&boxes);
            // The boxes are in the cell's own space; the subject root scales it.
            let (centre, size) = (centre * scene::SUBJECT_SCALE, size * scene::SUBJECT_SCALE);
            commands.entity(camera).insert((
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed {
                        width: size,
                        height: size,
                    },
                    ..OrthographicProjection::default_3d()
                }),
                Transform::from_xyz(centre.x, 20.0, centre.y).looking_at(Vec3::new(centre.x, 0.0, centre.y), Vec3::X),
            ));
        }
        state.scene = Some(root);
        state.built = Some(signature);
        state.frames = RENDER_FRAMES;
    }
    // Render a few frames after every change, then rest.
    if let Some(camera) = state.camera {
        if let Ok(mut camera) = cameras.get_mut(camera) {
            let render = state.frames > 0;
            if camera.is_active != render {
                camera.is_active = render;
            }
        }
    }
    state.frames = state.frames.saturating_sub(1);
}

/// Place the portrait node over the HUD's portrait window.
pub fn place(
    state: Res<PortraitState>,
    hud: Res<CellHudState>,
    roots: Query<(&crate::spui::view::SpuiState, &CellHudRoot)>,
    mut nodes: Query<(&mut Node, &mut Visibility), With<PortraitNode>>,
) {
    let Some(node) = state.node else {
        return;
    };
    let Ok((mut n, mut visibility)) = nodes.get_mut(node) else {
        return;
    };
    let window = roots
        .iter()
        .find(|(_, kind)| **kind == CellHudRoot::Main)
        .filter(|(ui, _)| ui.ready)
        .and_then(|(ui, _)| ui.find(PORTRAIT_WINDOW).map(|w| (ui.rect(w), ui.screen)));
    let settled = state.built.is_some() && state.frames + SETTLE_FRAMES <= RENDER_FRAMES;
    let want = match window {
        Some((rect, screen)) if settled && !hud.hud_hidden => {
            // Percentages of the full-screen root keep the node on the window
            // at any resolution or UI scale.
            let pct = |v: f32, total: f32| Val::Percent(100.0 * v / total.max(1.0));
            n.left = pct(rect.x1, screen[0]);
            n.top = pct(rect.y1, screen[1]);
            n.width = pct(rect.width(), screen[0]);
            n.height = pct(rect.height(), screen[1]);
            Visibility::Inherited
        }
        _ => Visibility::Hidden,
    };
    if *visibility != want {
        *visibility = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_keeps_the_body_and_centres_longer_cells() {
        // A bare body: centred, a little larger than its diameter.
        let (centre, size) = framing(&[]);
        assert_eq!(centre, Vec2::ZERO);
        assert!((size - 1.8 * FRAME_MARGIN).abs() < 1e-4);
        // A tail reaching 2 units behind (+Z) shifts the view back and grows it.
        let tail = (Vec2::new(-0.1, 0.8), Vec2::new(0.1, 2.0));
        let (centre, size) = framing(&[tail]);
        assert!((centre.y - (2.0 - 0.9) * 0.5).abs() < 1e-4, "centre {centre:?}");
        assert!((size - 2.9 * FRAME_MARGIN).abs() < 1e-4, "size {size}");
    }

    /// The pedestal's surfaces are drawn individually on the portrait layer:
    /// `attach_remaining` must not add another copy of the model's other
    /// surfaces (it spawns them without a layer, so the main camera drew them
    /// at the world origin: the translucent square over the Cell water).
    #[test]
    fn the_pedestal_adds_no_surface_copies_outside_the_portrait_layer() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Originals>()
            .add_systems(Update, crate::material_binding::attach_remaining);
        let mut surfaces = vec![];
        for (section, size) in [(4usize, 1.0f32), (8, 2.0)] {
            let handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(Cuboid::new(size, 1.0, 1.0));
            let material = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
            surfaces.push(crate::OriginalSurface { handle, material, source_section: section, state_sections: vec![section + 1] });
        }
        let model = crate::OriginalModel { handle: surfaces[0].handle.clone(), surfaces, ..default() };
        // The installed pedestal model sits in the shared cache (`import_at`),
        // which is where `attach_remaining` looks a model up by its first mesh.
        app.world_mut().resource_mut::<Originals>().world_models.insert(
            crate::dbpf::ResourceKey { type_id: 1, group_id: 2, instance_id: 3 },
            model.clone(),
        );
        let root = app
            .world_mut()
            .run_system_once(move |mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>| {
                let mut gamma = HashMap::new();
                spawn_pedestal(&mut commands, &model, &mut gamma, &mut materials, &mut images)
            })
            .unwrap();
        for _ in 0..3 {
            app.update();
        }
        let world = app.world_mut();
        let drawn = world.query::<&Mesh3d>().iter(world).count();
        assert_eq!(drawn, 2, "one entity per surface, no copies");
        let unlayered = world.query_filtered::<(), (With<Mesh3d>, Without<RenderLayers>)>().iter(world).count();
        assert_eq!(unlayered, 0, "every pedestal mesh is on the portrait layer");
        assert_eq!(world.get::<Children>(root).map(|c| c.len()), Some(2));
    }

    #[test]
    fn mesh_box_follows_the_transform() {
        use bevy::asset::RenderAssetUsages;
        let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-1.0, 0.0, -1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 0.0]]);
        let mut meshes = Assets::<Mesh>::default();
        let handle = meshes.add(mesh);
        let at = Transform::from_translation(Vec3::new(2.0, 0.0, 3.0)).with_scale(Vec3::splat(2.0));
        let (min, max) = mesh_box(&meshes, &handle, &at).unwrap();
        assert_eq!((min, max), (Vec2::new(0.0, 1.0), Vec2::new(4.0, 5.0)));
    }
}
