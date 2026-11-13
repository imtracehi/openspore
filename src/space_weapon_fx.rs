//! UFO laser visuals from the retail Swarm effects (helper `space-visuals2`).
//!
//! Retail draws a UFO laser shot with effects, not with a mesh: the beam
//! `SG_laser_lvl_N_beam` (static ribbons along the effect's path parameter, a
//! screen glow and the laser sound) from the ship to the target, and
//! `SG_laser_lvl_N_beam_impact` (three particle systems) where it lands
//! (`SG_laser_lvl_N_scorch` / `_water` are the ground decals and are not
//! played here). The Space rules (`space::space_fire`) keep their projectile for
//! the hit test and damage; this module hides its green sphere and plays the
//! effects instead. Level N is the best owned laser (`laserlevel1..3`).
//!
//! The laser runs while F is held (VERIFIED: the hold handler 0x1050500
//! uses the tool again every `AutoFireRate` 0.1 s and each use re-aims the
//! beam, 0x1059EF0): the shots `space_fire` makes every 0.1 s refresh ONE beam
//! and ONE impact effect instead of starting new ones; when the shots stop
//! the beam and the impact end after [`BEAM_SECONDS`].
//!
//! ADAPTED: retail beams are hit-scan between the ship and the target the
//! weapon code names; the attachment to the ship hardpoint was not traced.
//! The beam follows the ship, ends at the aim point stored on the latest
//! shot, and the impact sits on that point (the damage still lands when the
//! shot's projectile arrives). The effects are authored in retail units; one
//! world unit is `RETAIL_UNITS_PER_WORLD_UNIT` of them in the solar system
//! view (the planet view scales by its own unit), so they are played at that
//! scale.

use bevy::prelude::*;

use crate::game::Player;
use crate::space::Projectile;
use crate::space_rules::RETAIL_UNITS_PER_WORLD_UNIT;
use crate::space_view::SpaceView;
use crate::stages::{GameStage, StageOwned};
use crate::swarm_fx::{SwarmEffect, SwarmLibrary};

/// Seconds a laser beam effect outlives the latest shot (the shots come every
/// 0.1 s while F is held).
pub const BEAM_SECONDS: f32 = 0.25;

/// Marks a [`Projectile`] fired by a UFO laser (inserted by `space_fire`).
#[derive(Component, Clone, Copy, Debug)]
pub struct LaserShot {
    /// Laser level 1..=3 (`laserlevel1..3`).
    pub level: u8,
    /// The aim point (world), where the beam ends and the impact plays.
    pub to: Vec3,
}

/// The beam effect of a held laser.
#[derive(Component)]
struct LaserBeam {
    age: f32,
    to: Vec3,
}

/// The impact effect of a held laser (it sits where the beam ends).
#[derive(Component)]
struct LaserImpact {
    age: f32,
}

/// Effect names of a laser level (`SG_laser_lvl_1..3_*`).
pub fn beam_effect(level: u8) -> String {
    format!("SG_laser_lvl_{}_beam", level.clamp(1, 3))
}

pub fn impact_effect(level: u8) -> String {
    format!("SG_laser_lvl_{}_beam_impact", level.clamp(1, 3))
}

/// The beam's path: the ship to the aim point (two points; the retail ribbons
/// are static and follow the path parameter).
pub fn beam_path(from: Vec3, to: Vec3) -> Vec<Vec3> {
    vec![from, to]
}

/// Scene units per retail unit: the planet view has its own (`PlanetView::unit`),
/// the solar system view one world unit per `RETAIL_UNITS_PER_WORLD_UNIT`.
fn scale(view: Option<&SpaceView>) -> f32 {
    view.and_then(|v| v.planet.as_ref()).map_or(1.0 / RETAIL_UNITS_PER_WORLD_UNIT, |p| p.unit())
}

fn start_beams(
    mut commands: Commands,
    library: Res<SwarmLibrary>,
    view: Option<Res<SpaceView>>,
    shots: Query<(Entity, &LaserShot), Added<LaserShot>>,
    player: Query<&Transform, With<Player>>,
    mut beams: Query<(&mut LaserBeam, &mut SwarmEffect), Without<LaserImpact>>,
    mut impacts: Query<(&mut LaserImpact, &mut SwarmEffect), Without<LaserBeam>>,
) {
    let Ok(ship) = player.single() else { return };
    let scale = scale(view.as_deref());
    for (shot, laser) in &shots {
        // The sphere is only the hit test; the effect is what the player sees.
        commands.entity(shot).insert(Visibility::Hidden);
        // A beam already up (the laser is held) is kept and re-aimed.
        let mut kept = false;
        for (mut beam, _) in &mut beams {
            beam.age = 0.0;
            beam.to = laser.to;
            kept = true;
        }
        for (mut impact, mut effect) in &mut impacts {
            impact.age = 0.0;
            effect.position = laser.to;
        }
        if kept {
            continue;
        }
        let beam = crate::world_objects::name_id(&beam_effect(laser.level));
        if library.loaded && library.find(beam).is_none() {
            continue;
        }
        let mut effect = SwarmEffect::new(beam);
        effect.position = ship.translation;
        effect.scale = scale;
        effect.path = beam_path(ship.translation, laser.to);
        commands.spawn((effect, LaserBeam { age: 0.0, to: laser.to }, StageOwned));
        let mut impact = SwarmEffect::new(crate::world_objects::name_id(&impact_effect(laser.level)));
        impact.position = laser.to;
        impact.scale = scale;
        commands.spawn((impact, LaserImpact { age: 0.0 }, StageOwned));
    }
}

fn update_beams(
    mut commands: Commands,
    time: Res<Time>,
    player: Query<&Transform, With<Player>>,
    mut beams: Query<(Entity, &mut SwarmEffect, &mut LaserBeam)>,
) {
    for (entity, mut effect, mut beam) in &mut beams {
        beam.age += time.delta_secs();
        if let Ok(ship) = player.single() {
            effect.position = ship.translation;
            effect.path = beam_path(ship.translation, beam.to);
        }
        if beam.age >= BEAM_SECONDS {
            effect.stop = true;
            commands.entity(entity).remove::<LaserBeam>();
        }
    }
}

fn update_impacts(
    mut commands: Commands,
    time: Res<Time>,
    mut impacts: Query<(Entity, &mut SwarmEffect, &mut LaserImpact)>,
) {
    for (entity, mut effect, mut impact) in &mut impacts {
        impact.age += time.delta_secs();
        if impact.age >= BEAM_SECONDS {
            effect.stop = true;
            commands.entity(entity).remove::<LaserImpact>();
        }
    }
}

/// `--space-fire-demo` (captures): a laser shot every 0.1 s from frame 90
/// to the aim point 12 units ahead of the ship, so the held beam and the
/// impact can be looked at without a pirate in range.
fn fire_demo(
    mut commands: Commands,
    mut frame: Local<u32>,
    mut on: Local<Option<bool>>,
    player: Query<&Transform, With<Player>>,
) {
    let on = *on.get_or_insert_with(|| std::env::args().any(|a| a == "--space-fire-demo"));
    *frame += 1;
    if !on || *frame < 90 || (*frame - 90) % 6 != 0 {
        return;
    }
    let Ok(ship) = player.single() else { return };
    let dir = ship.rotation * Vec3::NEG_Z;
    let to = ship.translation + dir * 12.0;
    commands.spawn((
        StageOwned,
        Projectile { vel: dir * 40.0, damage: 0.0, life: 0.3, area: 0.0 },
        LaserShot { level: 1, to },
        Transform::from_translation(ship.translation + dir * 1.2),
    ));
}

pub struct SpaceWeaponFxPlugin;

impl Plugin for SpaceWeaponFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (fire_demo, start_beams, update_beams, update_impacts)
                .chain()
                .run_if(in_state(GameStage::Space)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_names_follow_the_laser_level() {
        assert_eq!(beam_effect(1), "SG_laser_lvl_1_beam");
        assert_eq!(beam_effect(3), "SG_laser_lvl_3_beam");
        assert_eq!(impact_effect(2), "SG_laser_lvl_2_beam_impact");
        // Out-of-range levels clamp to an installed one.
        assert_eq!(beam_effect(0), "SG_laser_lvl_1_beam");
        assert_eq!(impact_effect(9), "SG_laser_lvl_3_beam_impact");
    }

    #[test]
    fn the_beam_runs_from_the_ship_to_the_aim_point() {
        let path = beam_path(Vec3::ZERO, Vec3::new(0.0, 0.0, -5.0));
        assert_eq!(path.len(), 2);
        assert_eq!(path[1], Vec3::new(0.0, 0.0, -5.0));
        assert!((scale(None) * RETAIL_UNITS_PER_WORLD_UNIT - 1.0).abs() < 1e-6);
    }
}
