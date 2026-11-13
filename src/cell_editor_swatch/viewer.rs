//! The hovered palette item's turning part model (`Palettes::AdvancedItemViewer`).
//!
//! Retail draws the item's part model into a viewport of its window's size x
//! zoom centred on the window (0x5F2410, shifted to stay inside the screen) with
//! a camera framed on the model's box (`ItemViewer::InitializeViewerCamera`
//! 0x5F3D80: the model's `modelBoundingBox` 0x1CBB2F62, `modelThumbnailZoom`
//! 0x05CD5DE9 = 1.2 from the rigblock template, `IThumbnailManager::
//! FrameBoundingBoxPalette`). `RotateModel` 0x5F5670 centres the model on the
//! middle of its box (`-0.5 (min + max)`) and turns it about that point; the
//! spin of `func54h` 0x5F3230: while the item is hovered (rotation allowed)
//! the angle grows by 0.0015 rad per millisecond (1.5 rad/s) and wraps with
//! `fmod(x, 2 pi)` then `- 2 pi` from values >= pi; when not hovered the
//! target is 0 and the base update 0x5F6FD0 closes the (shorter way round)
//! distance by `min(ms x 0.012, 1)` of itself per frame, snapping within 0.01.
//!
//! The palette icons (`cellparticons`) are the same model seen from straight
//! above with the part's outward axis up the picture, so the preview camera
//! looks straight down with the outward axis (+Z of the imported model) up.
//! ADAPTED: one preview at a time (retail has a viewer per item, each easing
//! back on its own); the framing is the half diagonal of the model's floor box
//! x [`FRAME_MARGIN`] (the retail box fit through the thumbnail manager is not
//! decoded) so the turning model never leaves the picture; the preview is a
//! render target drawn by a plain UI node over the item window, the flat icon
//! is hidden while it shows; only unlocked items show a model (a locked item's
//! window shows the shield).

use std::collections::HashMap;

use bevy::{
    asset::AssetId,
    camera::{visibility::RenderLayers, RenderTarget, ScalingMode},
    prelude::*,
    render::render_resource::TextureFormat,
};

use crate::{cell_editor_scene as scene, game::PartInstance, spui::model::WinId, Originals};

/// Render layer of the preview scene (24 thumbnails, 25..27 portraits, 28 / 29
/// the Creator, 30 mechanical previews, 31 part thumbnails).
pub const LAYER: usize = 23;
/// Size of the render target in pixels (an item window is about 60 px, 2 x
/// zoom while hovered).
const SIZE: u32 = 192;
/// Spin of a hovered item: 0.0015 rad/ms (0x5F3230).
pub const SPIN_PER_MS: f32 = 0.0015;
/// ADAPTED: the view is this much larger than the model's floor half diagonal.
pub const FRAME_MARGIN: f32 = 1.12;
/// Frames the camera renders before the preview replaces the icon (pipeline
/// and texture preparation).
const SETTLE_FRAMES: u32 = 3;

/// The item being previewed: the palette part, the item window's button, and
/// the viewport rectangle (screen pixels) in the SPUI root's screen size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub part: u32,
    pub button: WinId,
}

#[derive(Component)]
pub struct ViewerScene;

#[derive(Component)]
pub struct ViewerNode;

#[derive(Resource, Default)]
pub struct ItemViewer {
    /// The item currently viewed (hovered, or easing back to rest).
    pub current: Option<Shown>,
    /// Viewport of the preview in screen pixels and the screen it is in.
    pub rect: [f32; 4],
    pub screen: [f32; 2],
    /// The model's angle (`mRotation`).
    pub rotation: f32,
    /// The preview image is on screen for `current` (the icon is hidden).
    pub live: bool,
    image: Option<Handle<Image>>,
    camera: Option<Entity>,
    light: Option<Entity>,
    pivot: Option<Entity>,
    node: Option<Entity>,
    /// Part the scene was built for.
    built: Option<u32>,
    frames: u32,
    warned: bool,
    gamma: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
}

/// Wrap an angle as 0x5F3230 does: `fmod(x, 2 pi)`, then `- 2 pi` from
/// values >= pi.
pub fn wrap_angle(x: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let r = x % tau;
    if r >= std::f32::consts::PI {
        r - tau
    } else {
        r
    }
}

/// One frame of the model's angle: while hovered it grows by 0.0015 rad per
/// millisecond; otherwise it closes `min(ms x 0.012, 1)` of the distance to 0
/// and snaps within 0.01 (0x5F6FD0).
pub fn spin_step(rotation: f32, hovered: bool, dt: f32) -> f32 {
    let ms = dt * 1000.0;
    if hovered {
        return wrap_angle(rotation + SPIN_PER_MS * ms);
    }
    let k = (ms * 0.012).min(1.0);
    let r = rotation - rotation * k;
    if r.abs() < 0.01 {
        0.0
    } else {
        r
    }
}

/// The viewport of an item window scaled by its zoom about the window centre
/// and shifted to stay inside the screen (0x5F2410).
pub fn viewport(window: [f32; 4], zoom: f32, screen: [f32; 2]) -> [f32; 4] {
    let (cx, cy) = ((window[0] + window[2]) * 0.5, (window[1] + window[3]) * 0.5);
    let (hw, hh) = ((window[2] - window[0]) * zoom * 0.5, (window[3] - window[1]) * zoom * 0.5);
    let mut r = [cx - hw, cy - hh, cx + hw, cy + hh];
    if r[0] < 0.0 {
        r[2] -= r[0];
        r[0] = 0.0;
    }
    if r[2] > screen[0] {
        r[0] -= r[2] - screen[0];
        r[2] = screen[0];
    }
    if r[1] < 0.0 {
        r[3] -= r[1];
        r[1] = 0.0;
    }
    if r[3] > screen[1] {
        r[1] -= r[3] - screen[1];
        r[3] = screen[1];
    }
    r
}

/// Model instance of a palette definition (the RW4 the in-game mount draws).
fn model_instance(originals: &Originals, definition: u32) -> u32 {
    originals
        .part_catalog
        .by_definition_in(crate::asset_catalog::CELL_PARTS, definition)
        .map(|d| d.model.instance_id)
        .unwrap_or(definition)
}

/// Floor box (x, z) of a mesh seen through a transform.
fn floor_box(meshes: &Assets<Mesh>, mesh: &Handle<Mesh>, transform: &Transform) -> Option<(Vec2, Vec2)> {
    crate::cell_hud::portrait::mesh_box(meshes, mesh, transform)
}

/// Log a failure to build a preview once (the flat icon stays).
fn warn_once(warned: &mut bool, message: std::fmt::Arguments) {
    if !*warned {
        *warned = true;
        warn!("{message}");
    }
}

/// Build, frame, turn and place the preview for the viewed item.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut viewer: ResMut<ItemViewer>,
    data: Res<crate::cell_editor_data::CellEditorData>,
    originals: Res<Originals>,
    meshes: Res<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cameras: Query<&mut Camera, With<ViewerScene>>,
    mut nodes: Query<(&mut Node, &mut Visibility), With<ViewerNode>>,
    mut pivots: Query<&mut Transform, (With<ViewerScene>, Without<Camera>, Without<DirectionalLight>)>,
) {
    let v = &mut *viewer;
    let Some(shown) = v.current else {
        v.live = false;
        if let Some(mut camera) = v.camera.and_then(|c| cameras.get_mut(c).ok()) {
            if camera.is_active {
                camera.is_active = false;
            }
        }
        if let Some((_, mut visibility)) = v.node.and_then(|n| nodes.get_mut(n).ok()) {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
        }
        return;
    };
    // The target, camera and light once.
    let image = v
        .image
        .get_or_insert_with(|| images.add(Image::new_target_texture(SIZE, SIZE, TextureFormat::Rgba8UnormSrgb, None)))
        .clone();
    if v.node.is_none() {
        v.node = Some(
            commands
                .spawn((
                    ViewerNode,
                    Node { position_type: PositionType::Absolute, ..default() },
                    ImageNode::new(image.clone()),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    GlobalZIndex(100),
                    crate::stages::StageOwned,
                ))
                .id(),
        );
    }
    if v.camera.is_none() {
        let layer = RenderLayers::layer(LAYER);
        v.camera = Some(
            commands
                .spawn((
                    ViewerScene,
                    Camera3d::default(),
                    bevy::core_pipeline::tonemapping::Tonemapping::None,
                    Camera { order: -4, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
                    RenderTarget::from(image.clone()),
                    Projection::Orthographic(OrthographicProjection {
                        scaling_mode: ScalingMode::Fixed { width: 2.0, height: 2.0 },
                        ..OrthographicProjection::default_3d()
                    }),
                    // Straight down, the part's outward axis (+Z) up the picture.
                    Transform::from_xyz(0.0, 20.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
                    layer.clone(),
                    AmbientLight { color: Color::WHITE, brightness: scene::AMBIENT_BRIGHTNESS, ..default() },
                    crate::stages::StageOwned,
                ))
                .id(),
        );
        v.light = Some(
            commands
                .spawn((
                    ViewerScene,
                    DirectionalLight {
                        color: Color::WHITE,
                        illuminance: scene::SUN_ILLUMINANCE,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(-0.75, 1.0, 0.1).normalize() * 20.0).looking_at(Vec3::ZERO, Vec3::Y),
                    layer,
                    crate::stages::StageOwned,
                ))
                .id(),
        );
    }
    // Rebuild the model for another part.
    if v.built != Some(shown.part) {
        if let Some(old) = v.pivot.take() {
            if let Ok(mut e) = commands.get_entity(old) {
                e.despawn();
            }
        }
        v.built = None;
        v.live = false;
        let file = data.family_file(shown.part, crate::cell_editor_data::Side::Center);
        let mut part = PartInstance::new(model_instance(&originals, file), 0).with_definition(file);
        part.offset = scene::offset_for(Vec3::ZERO);
        let Some(model) = crate::editor::model_for_part(&originals, &part) else {
            warn_once(&mut v.warned, format_args!("item viewer: no model for part {:08X} (file {file:08X})", shown.part));
            return;
        };
        let Some(material) = model.material.as_ref() else {
            warn_once(&mut v.warned, format_args!("item viewer: no material for part {:08X}", shown.part));
            return;
        };
        let transform = scene::part_transform(&part, model.scale);
        let Some((min, max)) = floor_box(&meshes, &model.draw_mesh(), &transform) else {
            warn_once(&mut v.warned, format_args!("item viewer: no mesh box for part {:08X}", shown.part));
            return;
        };
        // RotateModel: the model is moved by minus the middle of its box and
        // turned about that point (the floor middle; the height is not seen).
        let middle = (min + max) * 0.5;
        let half = (max - min) * 0.5;
        let radius = half.length().max(1e-3);
        let layer = RenderLayers::layer(LAYER);
        let gamma = scene::gamma_material(&mut v.gamma, material, &mut materials, &mut images);
        let pivot = commands
            .spawn((ViewerScene, Transform::default(), Visibility::default(), layer.clone(), crate::stages::StageOwned))
            .with_children(|p| {
                let mut t = transform;
                t.translation -= Vec3::new(middle.x, 0.0, middle.y);
                p.spawn((Mesh3d(model.draw_mesh()), MeshMaterial3d(gamma), t, layer.clone()));
            })
            .id();
        v.pivot = Some(pivot);
        if let Some(camera) = v.camera {
            let side = 2.0 * radius * FRAME_MARGIN;
            commands.entity(camera).insert(Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed { width: side, height: side },
                ..OrthographicProjection::default_3d()
            }));
        }
        v.built = Some(shown.part);
        v.frames = 0;
    }
    // Turn the model.
    if let Some(pivot) = v.pivot {
        if let Ok(mut t) = pivots.get_mut(pivot) {
            t.rotation = Quat::from_rotation_y(v.rotation);
        }
    }
    if let Some(mut camera) = v.camera.and_then(|c| cameras.get_mut(c).ok()) {
        camera.is_active = v.built.is_some();
    }
    v.frames = v.frames.saturating_add(1);
    v.live = v.built == Some(shown.part) && v.frames > SETTLE_FRAMES;
    // The node over the item window (percentages of the SPUI root's screen).
    if let Some((mut n, mut visibility)) = v.node.and_then(|n| nodes.get_mut(n).ok()) {
        let want = if v.live { Visibility::Inherited } else { Visibility::Hidden };
        if v.live {
            let pct = |value: f32, total: f32| Val::Percent(100.0 * value / total.max(1.0));
            n.left = pct(v.rect[0], v.screen[0]);
            n.top = pct(v.rect[1], v.screen[1]);
            n.width = pct(v.rect[2] - v.rect[0], v.screen[0]);
            n.height = pct(v.rect[3] - v.rect[1], v.screen[1]);
        }
        if *visibility != want {
            *visibility = want;
        }
    }
}

/// Drop the preview scene when the Creator closes.
pub fn cleanup(mut commands: Commands, mut viewer: ResMut<ItemViewer>) {
    if viewer.camera.is_none() && viewer.node.is_none() {
        return;
    }
    for e in [viewer.camera.take(), viewer.light.take(), viewer.pivot.take(), viewer.node.take()].into_iter().flatten() {
        if let Ok(mut entity) = commands.get_entity(e) {
            entity.despawn();
        }
    }
    viewer.built = None;
    viewer.current = None;
    viewer.live = false;
    viewer.rotation = 0.0;
    viewer.image = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hovered_item_turns_one_and_a_half_radians_a_second() {
        let mut r = 0.0;
        for _ in 0..60 {
            r = spin_step(r, true, 1.0 / 60.0);
        }
        assert!((r - 1.5).abs() < 1e-4, "after a second the angle is {r}");
    }

    #[test]
    fn the_angle_wraps_through_pi() {
        let mut r = 3.0;
        for _ in 0..60 {
            r = spin_step(r, true, 1.0 / 60.0);
        }
        // 3.0 + 1.5 = 4.5 -> 4.5 - 2 pi
        assert!((r - (4.5 - std::f32::consts::TAU)).abs() < 1e-4, "{r}");
        assert!(r < std::f32::consts::PI && r > -std::f32::consts::PI);
    }

    #[test]
    fn it_eases_back_to_rest_and_snaps() {
        let mut r = 2.0;
        r = spin_step(r, false, 1.0 / 60.0);
        // k = 16.7 ms x 0.012 = 0.2: 2.0 -> 1.6
        assert!((r - 1.6).abs() < 1e-3, "{r}");
        for _ in 0..200 {
            r = spin_step(r, false, 1.0 / 60.0);
        }
        assert_eq!(r, 0.0, "snaps within 0.01");
        assert_eq!(spin_step(-1.0, false, 1.0), 0.0, "a long frame closes the whole gap");
    }

    #[test]
    fn the_viewport_scales_about_the_window_and_stays_on_screen() {
        let r = viewport([100.0, 100.0, 160.0, 160.0], 2.0, [1280.0, 900.0]);
        assert_eq!(r, [70.0, 70.0, 190.0, 190.0]);
        // Against the left / top edge it is shifted, not shrunk.
        let r = viewport([10.0, 5.0, 70.0, 65.0], 2.0, [1280.0, 900.0]);
        assert_eq!(r, [0.0, 0.0, 120.0, 120.0]);
        let r = viewport([1250.0, 880.0, 1280.0, 900.0], 2.0, [1280.0, 900.0]);
        assert_eq!(r, [1220.0, 860.0, 1280.0, 900.0]);
    }
}
