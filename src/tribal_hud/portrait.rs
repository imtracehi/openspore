//! The portraits of the tribe list: the picture in window `0x03D99151` of every
//! `PosseItemTribe`.
//!
//! The retail game renders a thumbnail of each member. Here every list row has
//! its own live picture (tribal-audit-b; before, all members shared the first
//! one's): one camera, light and small image per row, each camera seeing only
//! its member (the member's meshes join the row's render layer), so a member's
//! own tool outfit shows in its picture. A plain UI node per list item shows the
//! matching image over the item's portrait window (the HUD surface composites
//! raw gamma-space texels, a render target is sRGB, see `creature_hud::portrait`).
//!
//! ADAPTED: the backdrop is a flat neutral colour and the subject is lit by the
//! portrait's own light.

use bevy::{
    camera::{primitives::Aabb, visibility::RenderLayers, RenderTarget},
    prelude::*,
    render::render_resource::TextureFormat,
};

use super::{world, TribalHudActive, TribalHudRoot, TribalHudState};
use crate::{
    cell_editor_scene as scene,
    creature_hud::portrait::{portrait_pose, union_box},
};

/// First render layer of the row portraits: row `n` uses `FIRST_LAYER + n`
/// (the other stages use layers up to 31).
const FIRST_LAYER: usize = 32;
/// Size of the rendered images in pixels (the window is about 42 px).
const SIZE: u32 = 96;
/// ADAPTED: flat backdrops (the chieftain's is warmer).
const CHIEF_BACKDROP: (u8, u8, u8) = (88, 72, 52);
const MEMBER_BACKDROP: (u8, u8, u8) = (52, 74, 82);
/// Frames before the first picture is shown (the render pipeline and the
/// texture are prepared asynchronously).
const SETTLE_FRAMES: u32 = 4;

/// Plain UI node showing a portrait over one list item.
#[derive(Component)]
pub struct PortraitNode(pub u8);

/// Marks a portrait camera and light.
#[derive(Component)]
pub struct PortraitCamera;

#[derive(Component)]
pub struct PortraitLight;

/// Which kind of portrait a list item shows (its backdrop).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subject {
    Chieftain,
    Member,
}

impl Subject {
    fn backdrop(self) -> (u8, u8, u8) {
        match self {
            Subject::Chieftain => CHIEF_BACKDROP,
            Subject::Member => MEMBER_BACKDROP,
        }
    }
}

/// The render layer of list row `row`.
fn layer_of(row: usize) -> usize {
    FIRST_LAYER + row
}

/// The camera order of list row `row` (before the world camera).
fn order_of(row: usize) -> isize {
    -10 - row as isize
}

/// The portrait a list row shows.
pub fn subject_of(chieftain: bool) -> Subject {
    if chieftain {
        Subject::Chieftain
    } else {
        Subject::Member
    }
}

#[derive(Default)]
struct Rig {
    image: Option<Handle<Image>>,
    camera: Option<Entity>,
    light: Option<Entity>,
    /// The member the camera is pointed at.
    subject: Option<Entity>,
    /// Frames the camera has seen its subject.
    seen: u32,
}

#[derive(Resource, Default)]
pub struct PortraitState {
    /// One rig per list row.
    rigs: Vec<Rig>,
    /// One node per list item, by item index.
    nodes: Vec<Entity>,
}

/// Put every mesh under `entity` on `layers`.
fn set_layers(
    commands: &mut Commands,
    entity: Entity,
    layers: RenderLayers,
    children: &Query<&Children>,
    meshes: &Query<(Option<&RenderLayers>, Option<&GlobalTransform>, Option<&Aabb>), With<Mesh3d>>,
) {
    let mut stack = vec![entity];
    while let Some(entity) = stack.pop() {
        if let Ok(kids) = children.get(entity) {
            stack.extend(kids.iter());
        }
        if let Ok((current, ..)) = meshes.get(entity) {
            if current.is_none_or(|c| *c != layers) {
                commands.entity(entity).insert(layers.clone());
            }
        }
    }
}

/// Keep the rig of list row `row` (camera, light, image) pointed at `subject`,
/// or take it down when there is none.
#[allow(clippy::too_many_arguments)]
fn drive_rig(
    commands: &mut Commands,
    rig: &mut Rig,
    row: usize,
    kind: Subject,
    subject: Option<(Entity, GlobalTransform)>,
    images: &mut Assets<Image>,
    children: &Query<&Children>,
    meshes: &Query<(Option<&RenderLayers>, Option<&GlobalTransform>, Option<&Aabb>), With<Mesh3d>>,
    poses: &mut Query<&mut Transform, Or<(With<PortraitCamera>, With<PortraitLight>)>>,
    cameras: &Query<(), With<PortraitCamera>>,
) {
    // A member that left the row goes back to the world layer only.
    if let Some(previous) = rig.subject {
        if Some(previous) != subject.map(|(e, _)| e) {
            set_layers(commands, previous, RenderLayers::layer(0), children, meshes);
            rig.subject = None;
            rig.seen = 0;
        }
    }
    let Some((subject, pose)) = subject else {
        for e in [rig.camera.take(), rig.light.take()].into_iter().flatten() {
            if let Ok(mut entity) = commands.get_entity(e) {
                entity.despawn();
            }
        }
        rig.seen = 0;
        return;
    };
    rig.subject = Some(subject);
    let layer = layer_of(row);
    let image = rig
        .image
        .get_or_insert_with(|| images.add(Image::new_target_texture(SIZE, SIZE, TextureFormat::Rgba8UnormSrgb, None)))
        .clone();
    if rig.camera.is_none() || rig.camera.is_some_and(|c| cameras.get(c).is_err()) {
        let (r, g, b) = kind.backdrop();
        rig.camera = Some(
            commands
                .spawn((
                    PortraitCamera,
                    Camera3d::default(),
                    bevy::core_pipeline::tonemapping::Tonemapping::None,
                    Camera { order: order_of(row), clear_color: ClearColorConfig::Custom(Color::srgb_u8(r, g, b)), ..default() },
                    RenderTarget::from(image.clone()),
                    Projection::Perspective(PerspectiveProjection {
                        fov: 28.0_f32.to_radians(),
                        near: 0.05,
                        far: 400.0,
                        ..default()
                    }),
                    Transform::from_xyz(0.0, 2.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
                    RenderLayers::layer(layer),
                    AmbientLight { color: Color::WHITE, brightness: scene::AMBIENT_BRIGHTNESS, ..default() },
                    crate::stages::StageOwned,
                ))
                .id(),
        );
        rig.light = Some(
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
                    RenderLayers::layer(layer),
                    crate::stages::StageOwned,
                ))
                .id(),
        );
    }
    // The subject's meshes join the portrait layer (the world camera keeps
    // seeing them on layer 0); their boxes give the framing.
    let layers = RenderLayers::from_layers(&[0, layer]);
    let mut stack = vec![subject];
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
    for e in [rig.camera, rig.light].into_iter().flatten() {
        if let Ok(mut t) = poses.get_mut(e) {
            *t = if Some(e) == rig.camera {
                Transform::from_translation(position).looking_at(target, Vec3::Y)
            } else {
                Transform::from_translation(light).looking_at(target, Vec3::Y)
            };
        }
    }
    rig.seen = rig.seen.saturating_add(1);
}

/// Keep the portrait cameras, lights, images and the UI nodes in step with the
/// HUD and the tribe.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut state: ResMut<PortraitState>,
    active: Res<TribalHudActive>,
    hud: Res<TribalHudState>,
    mut images: ResMut<Assets<Image>>,
    entities: Query<&GlobalTransform>,
    children: Query<&Children>,
    meshes: Query<(Option<&RenderLayers>, Option<&GlobalTransform>, Option<&Aabb>), With<Mesh3d>>,
    mut poses: Query<&mut Transform, Or<(With<PortraitCamera>, With<PortraitLight>)>>,
    cameras: Query<(), With<PortraitCamera>>,
    mut nodes: Query<(Entity, &PortraitNode)>,
) {
    let on = active.0;
    // One rig per list row, each on its own member.
    let wanted = if on { hud.list.len().min(world::MAX_ITEMS) } else { 0 };
    let state = &mut *state;
    state.rigs.resize_with(world::MAX_ITEMS, Rig::default);
    for row in 0..world::MAX_ITEMS {
        let item = hud.list.get(row).filter(|_| row < wanted);
        let subject = item.and_then(|m| entities.get(m.entity).ok().map(|t| (m.entity, *t)));
        let kind = subject_of(item.is_some_and(|m| m.chieftain));
        drive_rig(&mut commands, &mut state.rigs[row], row, kind, subject, &mut images, &children, &meshes, &mut poses, &cameras);
    }
    // One node per list item: spawn up to the list length, drop the surplus.
    state.nodes.retain(|e| nodes.get(*e).is_ok());
    for (entity, node) in &mut nodes {
        if node.0 as usize >= wanted {
            commands.entity(entity).despawn();
        }
    }
    state.nodes.retain(|e| nodes.get(*e).is_ok_and(|(_, n)| (n.0 as usize) < wanted));
    while state.nodes.len() < wanted {
        let n = state.nodes.len();
        let Some(image) = state.rigs.get(n).and_then(|r| r.image.clone()) else {
            break;
        };
        let e = commands
            .spawn((
                PortraitNode(n as u8),
                Node { position_type: PositionType::Absolute, ..default() },
                ImageNode::new(image),
                Visibility::Hidden,
                Pickable::IGNORE,
                GlobalZIndex(101),
                crate::stages::StageOwned,
            ))
            .id();
        state.nodes.push(e);
    }
}

/// Place each portrait node over its list item's portrait window and give it
/// the picture of its member.
pub fn place(
    state: Res<PortraitState>,
    hud: Res<TribalHudState>,
    roots: Query<(&crate::spui::view::SpuiState, &TribalHudRoot, Option<&Visibility>), Without<PortraitNode>>,
    mut nodes: Query<(&PortraitNode, &mut Node, &mut Visibility, &mut ImageNode)>,
) {
    for (node, mut n, mut visibility, mut image) in &mut nodes {
        let index = node.0 as usize;
        let (handle, seen) = state.rigs.get(index).map_or((None, 0), |r| (r.image.clone(), r.seen));
        if let Some(handle) = handle {
            if image.image != handle {
                image.image = handle;
            }
        }
        let window = roots
            .iter()
            .find(|(_, kind, _)| **kind == TribalHudRoot::PosseItem(node.0))
            // The portrait goes with its layout while the options menu hides the HUD layers
            // (`front_end::pause::layers`).
            .filter(|(ui, _, shown)| ui.ready && shown.is_none_or(|v| *v != Visibility::Hidden))
            .and_then(|(ui, _, _)| {
                let root = ui.find(world::id::ITEM_ROOT)?;
                let portrait = ui.find(world::id::ITEM_PORTRAIT)?;
                ui.visible(root).then(|| (ui.rect(portrait), ui.screen))
            });
        // The planner drawer is drawn by the HUD surface, which a UI node
        // would otherwise cover (the options menu stands clear of the list).
        let covered = hud.planner_open;
        let want = match window {
            Some((rect, screen)) if seen >= SETTLE_FRAMES && !covered => {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_has_its_own_picture() {
        assert_eq!(subject_of(true), Subject::Chieftain);
        assert_eq!(subject_of(false), Subject::Member);
        assert_ne!(Subject::Chieftain.backdrop(), Subject::Member.backdrop());
        // Distinct layers and camera orders per row.
        for row in 1..world::MAX_ITEMS {
            assert_ne!(layer_of(row), layer_of(row - 1));
            assert_ne!(order_of(row), order_of(row - 1));
            assert!(order_of(row) < 0);
        }
    }

    #[test]
    fn portrait_layers_stay_clear_of_the_other_stages_layers() {
        // 1 cell background, 26 creature portrait, 27 cell portrait, 28/29
        // editor, 30 mechanical preview, 31 part thumbnails.
        for row in 0..world::MAX_ITEMS {
            assert!(layer_of(row) > 31);
        }
    }
}
