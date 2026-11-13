//! The portrait of the player's creature in the bottom right corner of the HUD.
//!
//! The retail game stores a PNG thumbnail of the player's creature (taken in
//! the creator) and the layout shows it in window `0x03D99151` of
//! `GlobalUICreature3-7`. Here the portrait is a live picture of the creature
//! itself: a second camera looks at the player's meshes from the front into a
//! small image, which a plain UI node shows over the window (the HUD surface
//! composites raw gamma-space texels, a render target is sRGB).
//!
//! ADAPTED: the retail thumbnail is a still taken in the creator; the portrait
//! camera sees the player's meshes through an extra render layer added to
//! them (the world camera ignores it), is lit by its own light, and the
//! backdrop is a flat neutral colour.

use bevy::{
    camera::{primitives::Aabb, visibility::RenderLayers, RenderTarget},
    prelude::*,
    render::render_resource::TextureFormat,
};

use super::{id, CreatureHudActive, CreatureHudRoot};
use crate::{cell_editor_scene as scene, game::Player};

/// Render layer of the portrait (cell portrait 27, editor 28/29, mechanical
/// preview 30, part thumbnails 31, cell background 1).
const LAYER: usize = 26;
/// Size of the rendered image in pixels (the window is about 67 px).
const SIZE: u32 = 128;
/// ADAPTED: flat backdrop.
const BACKDROP: (u8, u8, u8) = (58, 70, 80);
/// Vertical field of view of the portrait camera.
const FOV_DEGREES: f32 = 28.0;
/// Frames before the first picture is shown (the render pipeline and the
/// texture are prepared asynchronously).
const SETTLE_FRAMES: u32 = 4;

/// Plain UI node showing the portrait.
#[derive(Component)]
pub struct PortraitNode;

/// Marks the portrait camera and light.
#[derive(Component)]
pub struct PortraitCamera;

#[derive(Component)]
pub struct PortraitLight;

#[derive(Resource, Default)]
pub struct PortraitState {
    image: Option<Handle<Image>>,
    camera: Option<Entity>,
    light: Option<Entity>,
    node: Option<Entity>,
    /// Frames the camera has seen the creature.
    seen: u32,
}

impl PortraitState {
    /// The rendered portrait image (once the camera has seen the creature).
    pub fn image(&self) -> Option<Handle<Image>> {
        self.image.clone().filter(|_| self.seen > 0)
    }
}

/// World-space box of the transformed local boxes `(centre, half extents,
/// transform)`; `None` without any.
pub fn union_box(boxes: impl IntoIterator<Item = (Vec3, Vec3, Mat4)>) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for (centre, half, transform) in boxes {
        for corner in 0..8u32 {
            let sign = Vec3::new(
                if corner & 1 == 0 { -1.0 } else { 1.0 },
                if corner & 2 == 0 { -1.0 } else { 1.0 },
                if corner & 4 == 0 { -1.0 } else { 1.0 },
            );
            let at = transform.transform_point3(centre + half * sign);
            min = min.min(at);
            max = max.max(at);
        }
    }
    (min.is_finite() && max.is_finite()).then_some((min, max))
}

/// Camera and light placement for a creature occupying `min..max` and facing
/// `forward` (on the ground plane): the camera stands in front, a little
/// above, far enough to see the whole box; the light comes from above left.
/// Returns `(camera position, look-at target, light position)`.
pub fn portrait_pose(min: Vec3, max: Vec3, forward: Vec3) -> (Vec3, Vec3, Vec3) {
    let size = max - min;
    let radius = (size.length() * 0.5).max(0.25);
    let forward = Vec3::new(forward.x, 0.0, forward.z).try_normalize().unwrap_or(Vec3::NEG_Z);
    let distance = radius / (FOV_DEGREES.to_radians() * 0.5).tan() * 0.95;
    let target = (min + max) * 0.5 + Vec3::Y * size.y * 0.12;
    let position = target + forward * distance + Vec3::Y * distance * 0.18;
    let right = forward.cross(Vec3::Y);
    let light = target + (forward - right * 0.7 + Vec3::Y * 1.1).normalize() * distance * 2.0;
    (position, target, light)
}

/// Keep the portrait camera, light, image and UI node in step with the HUD
/// and the player's creature.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut state: ResMut<PortraitState>,
    active: Res<CreatureHudActive>,
    mut images: ResMut<Assets<Image>>,
    player: Query<(Entity, &GlobalTransform), With<Player>>,
    children: Query<&Children>,
    meshes: Query<(Option<&RenderLayers>, Option<&GlobalTransform>, Option<&Aabb>), With<Mesh3d>>,
    mut rig: Query<&mut Transform, Or<(With<PortraitCamera>, With<PortraitLight>)>>,
    cameras: Query<(), With<PortraitCamera>>,
) {
    let player = player.single().ok().filter(|_| active.0);
    let Some((player, pose)) = player else {
        // Leave the HUD: drop the camera, light and node (the image is kept).
        for e in [state.camera.take(), state.light.take(), state.node.take()].into_iter().flatten() {
            if let Ok(mut entity) = commands.get_entity(e) {
                entity.despawn();
            }
        }
        state.seen = 0;
        return;
    };
    let image = state
        .image
        .get_or_insert_with(|| images.add(Image::new_target_texture(SIZE, SIZE, TextureFormat::Rgba8UnormSrgb, None)))
        .clone();
    if state.node.is_none() {
        state.node = Some(
            commands
                .spawn((
                    PortraitNode,
                    Node { position_type: PositionType::Absolute, ..default() },
                    ImageNode::new(image.clone()),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    GlobalZIndex(101),
                    crate::stages::StageOwned,
                ))
                .id(),
        );
    }
    if state.camera.is_none() || state.camera.is_some_and(|c| cameras.get(c).is_err()) {
        let (r, g, b) = BACKDROP;
        state.camera = Some(
            commands
                .spawn((
                    PortraitCamera,
                    Camera3d::default(),
                    bevy::core_pipeline::tonemapping::Tonemapping::None,
                    Camera { order: -4, clear_color: ClearColorConfig::Custom(Color::srgb_u8(r, g, b)), ..default() },
                    RenderTarget::from(image.clone()),
                    Projection::Perspective(PerspectiveProjection {
                        fov: FOV_DEGREES.to_radians(),
                        near: 0.05,
                        far: 400.0,
                        ..default()
                    }),
                    Transform::from_xyz(0.0, 2.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
                    RenderLayers::layer(LAYER),
                    AmbientLight { color: Color::WHITE, brightness: scene::AMBIENT_BRIGHTNESS, ..default() },
                    crate::stages::StageOwned,
                ))
                .id(),
        );
        state.light = Some(
            commands
                .spawn((
                    PortraitLight,
                    DirectionalLight {
                        color: Color::WHITE,
                        illuminance: scene::SUN_ILLUMINANCE,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(-3.0, 6.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
                    RenderLayers::layer(LAYER),
                    crate::stages::StageOwned,
                ))
                .id(),
        );
    }
    // The player's meshes join the portrait layer (the world camera keeps
    // seeing them on layer 0); their boxes give the framing.
    let layers = RenderLayers::from_layers(&[0, LAYER]);
    let mut stack = vec![player];
    let mut boxes = Vec::new();
    while let Some(entity) = stack.pop() {
        if let Ok(kids) = children.get(entity) {
            stack.extend(kids.iter());
        }
        let Ok((current, transform, aabb)) = meshes.get(entity) else {
            continue;
        };
        if current.is_none_or(|c| *c != layers) {
            commands.entity(entity).insert(layers.clone());
        }
        if let (Some(transform), Some(aabb)) = (transform, aabb) {
            boxes.push((Vec3::from(aabb.center), Vec3::from(aabb.half_extents), transform.to_matrix()));
        }
    }
    let Some((min, max)) = union_box(boxes) else {
        return;
    };
    let (position, target, light) = portrait_pose(min, max, pose.forward().as_vec3());
    for e in [state.camera, state.light].into_iter().flatten() {
        if let Ok(mut t) = rig.get_mut(e) {
            *t = if Some(e) == state.camera {
                Transform::from_translation(position).looking_at(target, Vec3::Y)
            } else {
                Transform::from_translation(light).looking_at(target, Vec3::Y)
            };
        }
    }
    state.seen = state.seen.saturating_add(1);
}

/// Place the portrait node over the HUD's portrait window.
pub fn place(
    state: Res<PortraitState>,
    roots: Query<(&crate::spui::view::SpuiState, &CreatureHudRoot, Option<&Visibility>), Without<PortraitNode>>,
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
        .find(|(_, kind, _)| **kind == CreatureHudRoot::Main)
        // The portrait goes with its layout while the options menu hides the HUD layers
        // (`front_end::pause::layers`).
        .filter(|(ui, _, shown)| ui.ready && shown.is_none_or(|v| *v != Visibility::Hidden))
        .and_then(|(ui, _, _)| ui.find(id::PORTRAIT).map(|w| (ui.rect(w), ui.screen)));
    let want = match window {
        Some((rect, screen)) if state.seen >= SETTLE_FRAMES => {
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
    fn the_union_of_boxes_follows_their_transforms() {
        assert!(union_box([]).is_none());
        let unit = (Vec3::ZERO, Vec3::splat(0.5), Mat4::IDENTITY);
        let moved = (
            Vec3::ZERO,
            Vec3::splat(0.5),
            Mat4::from_scale_rotation_translation(Vec3::splat(2.0), Quat::IDENTITY, Vec3::new(3.0, 0.0, 0.0)),
        );
        let (min, max) = union_box([unit, moved]).unwrap();
        assert_eq!(min, Vec3::new(-0.5, -1.0, -1.0));
        assert_eq!(max, Vec3::new(4.0, 1.0, 1.0));
        // A quarter turn about Y swaps the x and z extents of a flat box.
        let flat = (Vec3::ZERO, Vec3::new(2.0, 0.5, 0.5), Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let (min, max) = union_box([flat]).unwrap();
        assert!((max - Vec3::new(0.5, 0.5, 2.0)).length() < 1e-5 && (min + Vec3::new(0.5, 0.5, 2.0)).length() < 1e-5);
    }

    #[test]
    fn the_camera_stands_in_front_of_the_creature_and_sees_all_of_it() {
        let (min, max) = (Vec3::new(-1.0, 0.0, -1.5), Vec3::new(1.0, 2.0, 1.5));
        // Facing -Z: the camera is on the -Z side, looking back at the centre.
        let (camera, target, light) = portrait_pose(min, max, Vec3::NEG_Z);
        assert!(camera.z < min.z, "in front: {camera:?}");
        assert!(camera.y > target.y, "a little above");
        assert!((target.x).abs() < 1e-5 && target.y > 1.0 && target.y < 1.5);
        let radius = (max - min).length() * 0.5;
        let distance = (camera - target).length();
        // The sphere around the box fits the vertical field of view.
        let half_view = distance * (FOV_DEGREES.to_radians() * 0.5).tan();
        assert!(half_view >= radius * 0.9, "half view {half_view} radius {radius}");
        // The light comes from the front left, higher than the target.
        assert!(light.y > target.y && light.z < target.z);
        // Turning the creature turns the camera with it.
        let (side, _, _) = portrait_pose(min, max, Vec3::X);
        assert!(side.x > max.x);
        // A vertical or zero heading falls back to the default front.
        let (fallback, _, _) = portrait_pose(min, max, Vec3::ZERO);
        assert!(fallback.z < min.z);
    }
}
