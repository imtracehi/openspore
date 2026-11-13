//! Isolated presentation of the actual later-stage assembly, using its shared
//! source mesh/material handles. The studio camera and lighting are adapted.
use crate::{
    material_binding::{OriginalSurfaceChild, OriginalSurfaceRoot},
    mechanical_creator::{CreatorKind, MechanicalCreator},
};
use bevy::{
    camera::{visibility::RenderLayers, RenderTarget},
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    mesh::VertexAttributeValues,
    prelude::*,
};

const PREVIEW_LAYER: usize = 30; // Layer 31 belongs to part thumbnails.

#[derive(Component)]
pub struct PreviewDraw {
    pub source: Entity,
}
#[derive(Component)]
pub struct PreviewCamera;
#[derive(Component)]
struct PreviewOwned;

#[derive(Clone, PartialEq)]
struct SourceDraw {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
    section: usize,
    states: Vec<usize>,
}

#[derive(Resource)]
pub struct PreviewScene {
    camera: Option<Entity>,
    saved_cameras: Vec<(Entity, bool)>,
    sources: Vec<SourceDraw>,
    kind: Option<CreatorKind>,
    revision: u64,
    yaw: f32,
    pitch: f32,
    distance: f32,
    pub draw_count: usize,
}
impl Default for PreviewScene {
    fn default() -> Self {
        Self {
            camera: None,
            saved_cameras: vec![],
            sources: vec![],
            kind: None,
            revision: 0,
            yaw: 0.55,
            pitch: 0.28,
            distance: 11.0,
            draw_count: 0,
        }
    }
}

pub fn orbit(
    creator: Res<MechanicalCreator>,
    mut scene: ResMut<PreviewScene>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
) {
    let delta = motion.read().map(|m| m.delta).sum::<Vec2>();
    let lines = wheel
        .read()
        .map(|w| match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 32.0,
        })
        .sum::<f32>();
    if !creator.open {
        return;
    }
    if windows
        .iter()
        .next()
        .and_then(|w| w.cursor_position().map(|p| p.x < w.width() * 0.58))
        .unwrap_or(false)
    {
        return;
    }
    if mouse.pressed(MouseButton::Right) && delta.is_finite() {
        scene.yaw =
            (scene.yaw - delta.x.clamp(-2048., 2048.) * 0.006).rem_euclid(std::f32::consts::TAU);
        scene.pitch = (scene.pitch + delta.y.clamp(-2048., 2048.) * 0.006).clamp(-0.25, 1.3);
    }
    if lines.is_finite() {
        scene.distance = (scene.distance * (-lines.clamp(-16., 16.) * 0.1).exp()).clamp(7.0, 18.0);
    }
}

fn subject(world: &mut World, kind: CreatorKind) -> Option<Entity> {
    match kind {
        CreatorKind::Vehicle => world
            .query_filtered::<Entity, With<crate::civ_rts::Flagship>>()
            .iter(world)
            .next(),
        CreatorKind::Ship => world
            .query_filtered::<Entity, With<crate::game::Player>>()
            .iter(world)
            .next(),
        CreatorKind::Building => world
            .query::<(Entity, &crate::civ::CityMarker)>()
            .iter(world)
            .find_map(|(e, marker)| (marker.0 == crate::civ::HOME_INDEX).then_some(e)),
    }
}

fn belongs(world: &World, entity: Entity, root: Entity, kind: CreatorKind) -> bool {
    let mut cursor = entity;
    let mut native = false;
    let mut building_subject = false;
    for _ in 0..64 {
        if world.get::<Visibility>(cursor) == Some(&Visibility::Hidden) {
            return false;
        }
        native |= world.get::<OriginalSurfaceChild>(cursor).is_some()
            || world.get::<OriginalSurfaceRoot>(cursor).is_some()
            || world
                .get::<crate::creation_assets::OriginalCreationPart>(cursor)
                .is_some()
            || world
                .get::<crate::civ::NativeCityHallSurface>(cursor)
                .is_some();
        building_subject |= world
            .get::<crate::civ::NativeCityHallSurface>(cursor)
            .is_some()
            || world.get::<crate::civ::CityPartRef>(cursor).is_some()
            || world
                .get::<crate::creation_assets::OriginalCreation>(cursor)
                .is_some();
        if cursor == root {
            return native && (kind != CreatorKind::Building || building_subject);
        }
        let Some(parent) = world.get::<ChildOf>(cursor) else {
            return false;
        };
        cursor = parent.parent();
    }
    false
}

fn source_draws(world: &mut World, kind: CreatorKind) -> Vec<SourceDraw> {
    let Some(root) = subject(world, kind) else {
        return vec![];
    };
    let Some(root_global) = world.get::<GlobalTransform>(root) else {
        return vec![];
    };
    let inverse = root_global.affine().inverse();
    let candidates = world
        .query_filtered::<(
            Entity,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            &GlobalTransform,
        ), Without<PreviewDraw>>()
        .iter(world)
        .map(|(e, mesh, material, global)| (e, mesh.0.clone(), material.0.clone(), global.affine()))
        .collect::<Vec<_>>();
    let mut draws = Vec::new();
    for (entity, mesh, material, global) in candidates {
        if !belongs(world, entity, root, kind) {
            continue;
        }
        let transform = Transform::from_matrix(Mat4::from(inverse * global));
        if !transform.translation.is_finite()
            || !transform.scale.is_finite()
            || !transform.rotation.is_finite()
        {
            continue;
        }
        let (section, states) = world
            .get::<OriginalSurfaceChild>(entity)
            .map(|s| (s.source_section, s.state_sections.clone()))
            .or_else(|| {
                world
                    .get::<OriginalSurfaceRoot>(entity)
                    .map(|s| (s.source_section, s.state_sections.clone()))
            })
            .unwrap_or_default();
        draws.push(SourceDraw {
            entity,
            mesh,
            material,
            transform,
            section,
            states,
        });
    }
    draws.sort_by_key(|d| d.entity.to_bits());
    draws
}

fn bounds(world: &World, sources: &[SourceDraw]) -> Option<(Vec3, f32, f32)> {
    let meshes = world.get_resource::<Assets<Mesh>>()?;
    let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for draw in sources {
        let Some(mesh) = meshes.get(&draw.mesh) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        for position in positions {
            let p = draw.transform.transform_point(Vec3::from_array(*position));
            if p.is_finite() {
                min = min.min(p);
                max = max.max(p);
            }
        }
    }
    let extent = (max - min).max_element();
    (extent.is_finite() && extent > 0.).then_some((
        (min + max) * 0.5,
        4.0 / extent,
        -(max.y - min.y) * 2.0 / extent,
    ))
}

/// Run after source transform propagation and before render visibility checks.
/// The main camera is never repositioned; closing simply reactivates it.
pub fn sync(world: &mut World) {
    let Some(creator) = world.get_resource::<MechanicalCreator>() else {
        return;
    };
    let (open, kind, revision) = (creator.open, creator.kind, creator.revision);
    let mut scene = world.remove_resource::<PreviewScene>().unwrap_or_default();
    if !open {
        if scene.camera.is_some() {
            for (camera, active) in scene.saved_cameras.drain(..) {
                if let Some(mut component) = world.get_mut::<Camera>(camera) {
                    component.is_active = active;
                }
            }
            let owned = world
                .query_filtered::<Entity, With<PreviewOwned>>()
                .iter(world)
                .collect::<Vec<_>>();
            for entity in owned {
                if let Ok(entity) = world.get_entity_mut(entity) {
                    entity.despawn();
                }
            }
            scene.camera = None;
            scene.sources.clear();
            scene.kind = None;
            scene.draw_count = 0;
        }
        world.insert_resource(scene);
        return;
    }
    if scene.camera.is_none() {
        let cameras = world
            .query_filtered::<Entity, With<crate::GameCamera>>()
            .iter(world)
            .collect::<Vec<_>>();
        let Some(main) = cameras.first().copied() else {
            world.insert_resource(scene);
            return;
        };
        let target = world.get::<RenderTarget>(main).cloned().unwrap_or_default();
        for camera in cameras {
            if let Some(mut component) = world.get_mut::<Camera>(camera) {
                scene.saved_cameras.push((camera, component.is_active));
                component.is_active = false;
            }
        }
        let camera = world
            .spawn((
                PreviewOwned,
                PreviewCamera,
                Camera3d::default(),
                Camera {
                    order: 1,
                    clear_color: Color::srgb(0.025, 0.055, 0.095).into(),
                    ..default()
                },
                target,
                Projection::Perspective(PerspectiveProjection {
                    fov: 45_f32.to_radians(),
                    near: 0.05,
                    far: 1000.,
                    ..default()
                }),
                RenderLayers::layer(PREVIEW_LAYER),
                bevy::camera::Exposure {
                    ev100: bevy::camera::Exposure::EV100_BLENDER,
                },
                AmbientLight {
                    color: Color::srgb(0.82, 0.9, 1.),
                    brightness: 280.,
                    ..default()
                },
                Transform::default(),
                GlobalTransform::default(),
            ))
            .id();
        scene.camera = Some(camera);
        for (position, brightness, color) in [
            (Vec3::new(-3., 5., -5.), 7500., Color::srgb(1., 0.94, 0.84)),
            (Vec3::new(4., 2., 3.), 1800., Color::srgb(0.7, 0.82, 1.)),
        ] {
            let transform = Transform::from_translation(position).looking_at(Vec3::ZERO, Vec3::Y);
            world.spawn((
                PreviewOwned,
                DirectionalLight {
                    color,
                    illuminance: brightness,
                    shadow_maps_enabled: false,
                    ..default()
                },
                transform,
                GlobalTransform::from(transform),
                RenderLayers::layer(PREVIEW_LAYER),
            ));
        }
    }
    let sources = source_draws(world, kind);
    // Keep live aircraft bobbing out of the studio. Committed edits and
    // newly mounted source entities/materials still refresh the preview.
    let source_identity_changed = sources.len() != scene.sources.len()
        || sources
            .iter()
            .zip(&scene.sources)
            .any(|(a, b)| a.entity != b.entity || a.mesh != b.mesh || a.material != b.material);
    if source_identity_changed || scene.kind != Some(kind) || scene.revision != revision {
        let old = world
            .query_filtered::<Entity, With<PreviewDraw>>()
            .iter(world)
            .collect::<Vec<_>>();
        for entity in old {
            world.entity_mut(entity).despawn();
        }
        scene.draw_count = 0;
        if let Some((center, fit, floor_y)) = bounds(world, &sources) {
            world.insert_resource(crate::mechanical_environment::MechanicalEnvironmentSubject(
                crate::mechanical_environment::SubjectFrame {
                    center: Vec3::ZERO,
                    extent: 4.0,
                    floor_y: floor_y - 0.05,
                },
            ));
            for draw in &sources {
                let mut transform = draw.transform;
                transform.translation = (transform.translation - center) * fit;
                transform.scale *= fit;
                world.spawn((
                    PreviewOwned,
                    PreviewDraw {
                        source: draw.entity,
                    },
                    Mesh3d(draw.mesh.clone()),
                    MeshMaterial3d(draw.material.clone()),
                    OriginalSurfaceChild {
                        source_section: draw.section,
                        state_sections: draw.states.clone(),
                    },
                    transform,
                    GlobalTransform::from(transform),
                    Visibility::Inherited,
                    RenderLayers::layer(PREVIEW_LAYER),
                ));
                scene.draw_count += 1;
            }
        }
        scene.sources = sources;
        scene.kind = Some(kind);
        scene.revision = revision;
    }
    if let Some(camera) = scene.camera {
        let offset = Vec3::new(
            scene.yaw.sin() * scene.pitch.cos(),
            scene.pitch.sin(),
            -scene.yaw.cos() * scene.pitch.cos(),
        ) * scene.distance;
        let right = (-offset).cross(Vec3::Y).normalize_or(Vec3::X);
        let look = -right * 3.4;
        let transform = Transform::from_translation(look + offset).looking_at(look, Vec3::Y);
        world
            .entity_mut(camera)
            .insert((transform, GlobalTransform::from(transform)));
        let roots = world
            .query_filtered::<Entity, With<crate::mechanical_creator::CreatorRoot>>()
            .iter(world)
            .collect::<Vec<_>>();
        for root in roots {
            world.entity_mut(root).insert(UiTargetCamera(camera));
        }
    }
    world.insert_resource(scene);
}

#[cfg(test)]
#[path = "mechanical_preview_tests.rs"]
mod tests;
