//! Camera cutaway for original flora. This is an adapted visibility policy,
//! not Spore's native occlusion shader. Shared imported materials stay intact.
use crate::{baked_model, dbpf::ResourceKey, game::Player, stages::GameStage, Originals};
use bevy::{mesh::VertexAttributeValues, prelude::*};
use std::collections::HashMap;

#[derive(Resource, Default)]
pub struct FloraBounds(HashMap<ResourceKey, Option<(Vec3, Vec3)>>);
#[derive(Component, Clone)]
pub struct FoliageFade {
    pub original: Handle<StandardMaterial>,
    pub owned: Handle<StandardMaterial>,
    pub factor: f32,
    pub source: ResourceKey,
}

/// Finite segment/AABB intersection. Expand the box by the followed body's
/// width, so vegetation covering its outline also fades.
fn obstructs(camera: Vec3, target: Vec3, min: Vec3, max: Vec3, padding: f32) -> bool {
    if !camera.is_finite()
        || !target.is_finite()
        || !min.is_finite()
        || !max.is_finite()
        || min.cmpgt(max).any()
        || !padding.is_finite()
        || padding < 0.
    {
        return false;
    }
    let delta = target - camera;
    if delta.length_squared() < 1e-6 {
        return false;
    }
    let min = min - Vec3::splat(padding);
    let max = max + Vec3::splat(padding);
    let mut near = 0f32;
    let mut far = 0.95f32;
    for axis in 0..3 {
        if delta[axis].abs() < 1e-6 {
            if camera[axis] < min[axis] || camera[axis] > max[axis] {
                return false;
            }
        } else {
            let a = (min[axis] - camera[axis]) / delta[axis];
            let b = (max[axis] - camera[axis]) / delta[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return false;
            }
        }
    }
    true
}

fn transformed_bounds(min: Vec3, max: Vec3, transform: GlobalTransform) -> (Vec3, Vec3) {
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for x in [min.x, max.x] {
        for y in [min.y, max.y] {
            for z in [min.z, max.z] {
                let p = transform.transform_point(Vec3::new(x, y, z));
                low = low.min(p);
                high = high.max(p);
            }
        }
    }
    (low, high)
}

/// CreatureGameTuning obstacleOcclusionMinAlpha.
pub const CREATURE_OCCLUSION_MIN_ALPHA: f32 = 0.25;

pub fn update(world: &mut World) {
    let Some(state) = world.get_resource::<State<GameStage>>() else {
        return;
    };
    let enabled = matches!(
        state.get(),
        GameStage::Creature | GameStage::Tribal | GameStage::Civilization
    ) && !world.resource::<crate::editor::EditorState>().open;
    // Creature: CreatureGameTuning obstacleOcclusionMinAlpha 0.25 (notes C6A);
    // the other stages keep the former adapted 0.08.
    let min_alpha = if *state.get() == GameStage::Creature { CREATURE_OCCLUSION_MIN_ALPHA } else { 0.08 };
    let camera = world
        .query_filtered::<&Transform, With<crate::GameCamera>>()
        .iter(world)
        .next()
        .map(|t| t.translation);
    let target = world
        .query_filtered::<&Transform, With<Player>>()
        .iter(world)
        .next()
        .map(|t| t.translation + Vec3::Y * 0.3);
    let view = camera.zip(target);
    let dt = world.resource::<Time>().delta_secs().clamp(0., 0.05);
    let draws: Vec<_> = world
        .query::<(
            Entity,
            &crate::world_objects::OriginalWorldObject,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
            Option<&FoliageFade>,
        )>()
        .iter(world)
        .filter(|(_, s, _, _, _)| s.model.group_id == baked_model::FLORA_GROUP)
        .map(|(e, s, t, m, f)| (e, s.model, *t, m.0.clone(), f.cloned()))
        .collect();
    for (entity, source, transform, material, fade) in draws {
        let bounds = if let Some(b) = world.resource::<FloraBounds>().0.get(&source) {
            *b
        } else {
            let handle = world
                .resource::<Originals>()
                .baked_models
                .get(&source)
                .map(|m| m.handle.clone());
            let bounds = handle.and_then(|handle| {
                let meshes = world.resource::<Assets<Mesh>>();
                let mesh = meshes.get(&handle)?;
                let VertexAttributeValues::Float32x3(positions) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)?
                else {
                    return None;
                };
                let mut min = Vec3::splat(f32::INFINITY);
                let mut max = Vec3::splat(f32::NEG_INFINITY);
                for p in positions {
                    let p = Vec3::from_array(*p);
                    if !p.is_finite() {
                        return None;
                    }
                    min = min.min(p);
                    max = max.max(p);
                }
                (!positions.is_empty()).then_some((min, max))
            });
            world.resource_mut::<FloraBounds>().0.insert(source, bounds);
            bounds
        };
        let blocked = enabled
            && view.is_some_and(|(camera, target)| {
                bounds.is_some_and(|(min, max)| {
                    let (min, max) = transformed_bounds(min, max, transform);
                    obstructs(camera, target, min, max, 0.8)
                })
            });
        if !blocked && fade.is_none() {
            continue;
        }
        let mut fade = if let Some(f) = fade {
            f
        } else {
            let Some(original) = world
                .resource::<Assets<StandardMaterial>>()
                .get(&material)
                .cloned()
            else {
                continue;
            };
            let owned = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(original);
            world
                .entity_mut(entity)
                .insert(MeshMaterial3d(owned.clone()));
            FoliageFade {
                original: material,
                owned,
                factor: 1.,
                source,
            }
        };
        let goal = if blocked { min_alpha } else { 1. };
        let speed = if blocked { 14. } else { 7. };
        fade.factor += (goal - fade.factor) * (dt * speed).min(1.);
        if (fade.factor - goal).abs() < 0.003 {
            fade.factor = goal;
        }
        if !blocked && fade.factor == 1. {
            world
                .entity_mut(entity)
                .insert(MeshMaterial3d(fade.original));
            world.entity_mut(entity).remove::<FoliageFade>();
            world
                .resource_mut::<Assets<StandardMaterial>>()
                .remove(fade.owned.id());
            continue;
        }
        let original = world
            .resource::<Assets<StandardMaterial>>()
            .get(&fade.original)
            .cloned();
        if let Some(mut original) = original {
            original.base_color = original
                .base_color
                .with_alpha(original.base_color.alpha() * fade.factor);
            original.alpha_mode = AlphaMode::Blend;
            if let Some(mut live) = world
                .resource_mut::<Assets<StandardMaterial>>()
                .get_mut(&fade.owned)
            {
                *live = original;
            }
        }
        world.entity_mut(entity).insert(fade);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OriginalModel;
    #[test]
    fn camera_cutaway_uses_segment_and_full_transformed_bounds() {
        let camera = Vec3::new(0., 1., 10.);
        let target = Vec3::Y;
        assert!(obstructs(
            camera,
            target,
            Vec3::new(-1., 0., 3.),
            Vec3::new(1., 4., 5.),
            0.
        ));
        assert!(!obstructs(
            camera,
            target,
            Vec3::new(-1., 0., -5.),
            Vec3::new(1., 4., -3.),
            0.
        ));
        assert!(!obstructs(
            camera,
            target,
            Vec3::new(-1., 0., 12.),
            Vec3::new(1., 4., 15.),
            0.
        ));
        assert!(!obstructs(
            camera,
            target,
            Vec3::new(3., 0., 3.),
            Vec3::new(5., 4., 5.),
            0.8
        ));
        assert!(!obstructs(camera, camera, Vec3::ZERO, Vec3::ONE, 1.));
        assert!(!obstructs(
            camera,
            target,
            Vec3::splat(f32::NAN),
            Vec3::ONE,
            1.
        ));
        let transform = GlobalTransform::from(
            Transform::from_xyz(4., 0., 0.)
                .with_scale(Vec3::splat(2.))
                .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
        );
        let (min, max) =
            transformed_bounds(Vec3::new(-1., 0., -2.), Vec3::new(1., 3., 2.), transform);
        assert!(min.abs_diff_eq(Vec3::new(0., 0., -2.), 1e-5));
        assert!(max.abs_diff_eq(Vec3::new(8., 6., 2.), 1e-5));
    }
    #[test]
    fn cutaway_clones_only_the_obstruction_and_restores_source_material() {
        let mut app = crate::runtime::headless_app(default());
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        app.update();
        app.world_mut().spawn((
            Camera3d::default(),
            crate::GameCamera,
            Transform::from_xyz(0., 1., 10.),
        ));
        let world = app.world_mut();
        let actor = world
            .query_filtered::<Entity, With<Player>>()
            .single(world)
            .unwrap();
        world
            .entity_mut(actor)
            .insert(Transform::from_translation(Vec3::Y * 0.7));
        let key = ResourceKey {
            group_id: baked_model::FLORA_GROUP,
            instance_id: 42,
            type_id: crate::decode::TYPE_GMDL,
        };
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(2., 4., 2.));
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::srgb(0.3, 0.7, 0.2),
                alpha_mode: AlphaMode::Mask(0.4),
                ..default()
            });
        world.resource_mut::<Originals>().baked_models.insert(
            key,
            OriginalModel {
                handle: mesh.clone(),
                ..default()
            },
        );
        let a = world
            .spawn((
                baked_model::source(key),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                GlobalTransform::from_translation(Vec3::new(0., 2., 5.)),
            ))
            .id();
        let b = world
            .spawn((
                baked_model::source(key),
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                GlobalTransform::from_translation(Vec3::new(8., 2., 5.)),
            ))
            .id();
        for _ in 0..40 {
            update(world);
        }
        assert_ne!(
            world.get::<MeshMaterial3d<StandardMaterial>>(a).unwrap().0,
            material
        );
        assert_eq!(
            world.get::<MeshMaterial3d<StandardMaterial>>(b).unwrap().0,
            material
        );
        let original = world
            .resource::<Assets<StandardMaterial>>()
            .get(&material)
            .unwrap();
        assert_eq!(original.base_color.alpha(), 1.);
        assert_eq!(original.alpha_mode, AlphaMode::Mask(0.4));
        world.resource_mut::<crate::editor::EditorState>().open = true;
        for _ in 0..80 {
            update(world);
        }
        assert_eq!(
            world.get::<MeshMaterial3d<StandardMaterial>>(a).unwrap().0,
            material
        );
        assert!(world.get::<FoliageFade>(a).is_none());
        world.resource_mut::<crate::editor::EditorState>().open = false;
        for _ in 0..40 {
            update(world);
        }
        assert!(world.get::<FoliageFade>(a).is_some());
        let camera = world
            .query_filtered::<Entity, With<crate::GameCamera>>()
            .single(world)
            .unwrap();
        world.despawn(camera);
        for _ in 0..80 {
            update(world);
        }
        assert!(
            world.get::<FoliageFade>(a).is_none(),
            "missing camera must restore existing fades"
        );
        assert_eq!(
            world.get::<MeshMaterial3d<StandardMaterial>>(a).unwrap().0,
            material
        );
    }
}
