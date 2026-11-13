//! Camera shakes: `ShakeEffect` components (`cell_bomb`, the egg hatch...)
//! sum into [`SwarmShake::offset`] each frame (sim.rs); this module moves the
//! GameCamera by that amount for the frame and puts it back before the stage's
//! camera controller runs again.
//!
//! NATIVE: life, fade time, the strength and frequency curves, the aspect, the
//! random / up-down tables as named by the effect data. ADAPTED: one strength
//! unit moves the camera by [`SHAKE_UNIT`] of its height (the native camera
//! shake code is not traced), the random table is a pair of incommensurate
//! sines, the shake follows the camera's screen axes.

use bevy::prelude::*;

/// Fraction of the camera height one unit of shake strength displaces it.
pub const SHAKE_UNIT: f32 = 0.012;

#[derive(Resource, Default)]
pub struct SwarmShake {
    /// Sum of this frame's shake amplitudes (x right, y up, in strength units).
    pub offset: Vec3,
    /// The camera translation before and after the shake was applied.
    applied: Option<(Vec3, Vec3)>,
}

/// Before the controllers: restore the camera if nothing moved it since.
pub fn undo_shake(mut shake: ResMut<SwarmShake>, mut cameras: Query<&mut Transform, With<crate::GameCamera>>) {
    if let Some((before, after)) = shake.applied.take() {
        for mut transform in &mut cameras {
            if transform.translation == after {
                transform.translation = before;
            }
        }
    }
}

/// After the controllers, before transform propagation.
pub fn apply_shake(mut shake: ResMut<SwarmShake>, mut cameras: Query<&mut Transform, With<crate::GameCamera>>) {
    if shake.offset == Vec3::ZERO {
        return;
    }
    let offset = shake.offset;
    for mut transform in &mut cameras {
        let height = transform.translation.y.abs().max(1.0);
        let before = transform.translation;
        let shifted = before + (transform.right() * offset.x + transform.up() * offset.y) * SHAKE_UNIT * height;
        transform.translation = shifted;
        shake.applied = Some((before, shifted));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn the_camera_moves_by_the_offset_and_comes_back() {
        let mut world = World::new();
        world.init_resource::<SwarmShake>();
        let camera = world.spawn((crate::GameCamera, Transform::from_xyz(0.0, 24.0, 0.0))).id();
        world.resource_mut::<SwarmShake>().offset = Vec3::new(1.0, 0.0, 0.0);
        world.run_system_once(apply_shake).unwrap();
        let moved = world.get::<Transform>(camera).unwrap().translation;
        assert!((moved.x - SHAKE_UNIT * 24.0).abs() < 1e-5, "{moved}");
        world.run_system_once(undo_shake).unwrap();
        assert_eq!(world.get::<Transform>(camera).unwrap().translation, Vec3::new(0.0, 24.0, 0.0));
        // A controller that moved the camera meanwhile is left alone.
        world.run_system_once(apply_shake).unwrap();
        world.get_mut::<Transform>(camera).unwrap().translation = Vec3::new(5.0, 24.0, 5.0);
        world.run_system_once(undo_shake).unwrap();
        assert_eq!(world.get::<Transform>(camera).unwrap().translation, Vec3::new(5.0, 24.0, 5.0));
    }
}
