//! Stats trays, the minimum playable size, the UFO's centre of mass and Test
//! Drive of the vehicle, building and UFO editors (helper `editors-paint`,
//! round 2; data in `notes/rigid-paint-notes.md` section 5).
//!
//! VERIFIED (installed PROPs):
//! * the vehicle setups' `verbTrayCollection` (`VHE_VerbTrayCollection`,
//!   `verbCollectionIsVehicleStats 1`; Military / Economic / Cultural classes
//!   `F5C8BC4D` / `9214283B` / `14DEBA81`): a header tray and three trays of
//!   one verb each: `vehicle_defense` ("Health", `stats_health_icon`), the
//!   class's power (`vehicle_militarypower` / `vehicle_economicpower` /
//!   `Vehicle_ReligiousPower`, the template's `vehicle_power`) and
//!   `vehicle_speed`; icons in `VehicleEditorVerbIcons~` (0xE4F27FFB);
//!   `UFOEditorSetup` has `editorShowAbilityIcons 0`; the building setups name
//!   no tray collection;
//! * a tray's level is the capability total of the parts
//!   (`modelCapabilitySpeed` / `Defense` / the four powers), the numbers
//!   `civ_combat` turns into the vehicle's stats (a tenth of each total,
//!   totals capped at `MaxVehicleStatLimit` 20);
//! * `editorMinPlayableWidth / Height / Depth` (vehicles 0.1, buildings 0.5 /
//!   1 / 0.5): ADAPTED, Accept refuses a smaller design (the retail message
//!   was not located);
//! * `editorMoveModelToCenterOfMass` (only the UFO): ADAPTED, the accepted
//!   design is moved so the centre of its blocks' volume is on the vertical
//!   axis.
//!
//! Test Drive (vehicles; retail has none for buildings): ADAPTED, the vehicle
//! drives on the studio floor with W / S (or Up / Down) and turns with A / D
//! (Left / Right) at its standard speed (`civ_combat::combat`, converted to
//! editor units by the stage scale) on a test ground around the pad; the
//! editor camera follows it (`follow_camera`: the orbit keeps its angles; the
//! retail Test Drive camera of the vehicle editors was not traced). The blocks' holder moves, not the preview root (the root
//! carries the `Player` marker, which stage systems keep in place).

use bevy::prelude::*;

use crate::{
    civ_combat,
    creature_editor::{CreatureEditor, Mode},
    creature_editor_data::{CreatureEditorData, SetupKind},
    game::{PartInstance, RunProgress},
    rigid_editor::{preview_rotation, RigidEditor, RigidHolder, VEHICLE_STAGE_SCALE},
    world_objects::name_id,
    Originals,
};

/// `VehicleEditorVerbIcons~`.
pub const VEHICLE_ICON_GROUP: u32 = 0xE4F2_7FFB;

/// Verb categories of the vehicle trays.
pub const TRAY_VERBS: [&str; 6] =
    ["vehicle_speed", "vehicle_defense", "vehicle_power", "vehicle_militarypower", "vehicle_religiouspower", "vehicle_economicpower"];

/// Setup property ids (reg_property).
pub mod setup_prop {
    pub const MIN_PLAYABLE_HEIGHT: u32 = 0x0538_B031;
    pub const MIN_PLAYABLE_WIDTH: u32 = 0x0538_B032;
    pub const MIN_PLAYABLE_DEPTH: u32 = 0x0538_B033;
}

/// Capability totals of the session's blocks: (speed, defense, power), the
/// power as `civ_combat` counts it (all four power capabilities).
pub fn totals(data: &CreatureEditorData, parts: &[PartInstance]) -> (i32, i32, i32) {
    let (mut speed, mut defense, mut power) = (0, 0, 0);
    for p in parts {
        let Some(info) = data.info_of(p) else { continue };
        for (verb, level) in &info.abilities {
            match *verb {
                "vehicle_speed" => speed += level,
                "vehicle_defense" => defense += level,
                "vehicle_power" | "vehicle_militarypower" | "vehicle_religiouspower" | "vehicle_economicpower" => power += level,
                _ => {}
            }
        }
    }
    (speed, defense, power)
}

/// The trays of a rigid setup: per tray (verb, level). Every vehicle tray is
/// always shown (`statsAlwaysDisplay`), the UFO and the buildings have none.
pub fn tray_levels(data: &CreatureEditorData, kind: SetupKind, run: &RunProgress) -> Vec<Vec<(&'static str, i32)>> {
    if !kind.is_vehicle() {
        return vec![];
    }
    let limit = civ_combat::CAPABILITY_LIMIT as i32;
    let (speed, defense, power) = totals(data, &run.parts);
    data.trays_of(kind)
        .iter()
        .map(|tray| {
            tray.iter()
                .filter_map(|category| {
                    let verb = *TRAY_VERBS.iter().find(|v| name_id(v) == *category)?;
                    let level = match verb {
                        "vehicle_speed" => speed,
                        "vehicle_defense" => defense,
                        _ => power,
                    };
                    Some((verb, level.clamp(0, limit)))
                })
                .collect()
        })
        .collect()
}

/// The Civ locomotion of a vehicle class.
pub fn domain_of(kind: SetupKind) -> crate::civ::VehicleDomain {
    let name = kind.setup_name();
    if name.ends_with("Water") {
        crate::civ::VehicleDomain::Sea
    } else if name.ends_with("Air") {
        crate::civ::VehicleDomain::Air
    } else {
        crate::civ::VehicleDomain::Land
    }
}

/// The session's vehicle stats as the Civ stage will read them.
pub fn session_stats(originals: &Originals, run: &RunProgress) -> civ_combat::VehicleStats {
    let records: Vec<_> = run.parts.iter().map(crate::rigid_editor::to_record).collect();
    civ_combat::design_stats(&originals.resource_index, &records).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Minimum playable size, centre of mass
// ---------------------------------------------------------------------------

/// ADAPTED refusal text (the retail string was not located).
pub const TOO_SMALL: &str = "Your creation is too small";

/// Whether a design of extent `size` (x width, y height, z depth, editor
/// metres) meets the setup's minimum playable dimensions.
pub fn playable(size: Vec3, min: [f32; 3]) -> bool {
    size.x >= min[0] && size.y >= min[1] && size.z >= min[2]
}

/// Keep `CreatureEditor::accept_refusal` up to date: Accept refuses a design
/// under the setup's minimum playable size.
#[allow(clippy::too_many_arguments)]
pub fn check_playable(
    mut editor: ResMut<CreatureEditor>,
    mut rigid: ResMut<RigidEditor>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    meshes: Res<Assets<Mesh>>,
    mut mins: Local<std::collections::HashMap<SetupKind, [f32; 3]>>,
) {
    if !rigid.active || !editor.kind.is_rigid() {
        if editor.accept_refusal.is_some() {
            editor.accept_refusal = None;
        }
        return;
    }
    let kind = rigid.kind;
    let min = *mins.entry(kind).or_insert_with(|| {
        let p = originals
            .resource_index
            .properties(crate::cell_editor_data::prop_key(crate::cell_editor_data::EDITOR_SETUP_GROUP, name_id(kind.setup_name())))
            .ok();
        let n = |id: u32| p.as_ref().and_then(|p| crate::cell_editor_data::number(p, id)).unwrap_or(0.0);
        [n(setup_prop::MIN_PLAYABLE_WIDTH), n(setup_prop::MIN_PLAYABLE_HEIGHT), n(setup_prop::MIN_PLAYABLE_DEPTH)]
    });
    let poses = crate::rigid_editor_edit::block_poses(&mut rigid, &run, &originals, &meshes, kind.part_group());
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for (t, hull) in &poses {
        let Some(h) = hull else { continue };
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { h.min.x } else { h.max.x },
                if i & 2 == 0 { h.min.y } else { h.max.y },
                if i & 4 == 0 { h.min.z } else { h.max.z },
            );
            let w = t.transform_point(c);
            lo = lo.min(w);
            hi = hi.max(w);
        }
    }
    // An empty session closes as before (nothing to refuse).
    let refusal = (lo.is_finite() && hi.is_finite() && !playable(hi - lo, min)).then(|| TOO_SMALL.to_string());
    if editor.accept_refusal != refusal {
        editor.accept_refusal = refusal;
    }
}

/// `editorMoveModelToCenterOfMass` (UFO): move the blocks so the centre of
/// their volume (scale^3 x stretch) lies on the vertical axis (x = z = 0);
/// the height is kept. Returns the horizontal shift applied.
pub fn center_of_mass(parts: &mut [PartInstance]) -> Vec3 {
    let mut sum = Vec3::ZERO;
    let mut mass = 0.0;
    for p in parts.iter() {
        let Some(b) = &p.rigid else { continue };
        let s = b.stretch();
        let m = p.scale.max(0.01).powi(3) * s.x * s.y * s.z;
        sum += b.position() * m;
        mass += m;
    }
    if mass <= 0.0 {
        return Vec3::ZERO;
    }
    let c = sum / mass;
    let shift = Vec3::new(-c.x, 0.0, -c.z);
    if shift.length() < 1e-5 {
        return Vec3::ZERO;
    }
    for p in parts.iter_mut() {
        if let Some(b) = p.rigid.as_mut() {
            b.position = (b.position() + shift).to_array();
        }
    }
    shift
}

// ---------------------------------------------------------------------------
// Test Drive
// ---------------------------------------------------------------------------

/// ADAPTED test ground: the vehicle stays within this radius of the pad.
pub const TEST_GROUND_RADIUS: f32 = 40.0;
/// ADAPTED: seconds to reach the top speed; turn rate (rad/s).
const ACCELERATION_TIME: f32 = 1.0;
const TURN_RATE: f32 = 1.6;

#[derive(Resource, Default)]
pub struct TestDrive {
    pub active: bool,
    /// Pose on the test ground (preview space x / z) and heading.
    pub position: Vec2,
    pub heading: f32,
    pub speed: f32,
    /// Top speed (editor metres per second) and the stats it came from.
    pub top_speed: f32,
    pub stats: civ_combat::VehicleStats,
    /// Metres driven this Test Drive.
    pub distance: f32,
    /// Captures and the GPU test: drive forward on a wide circle.
    pub auto: bool,
}

/// Drive the vehicle in Test Drive (the preview root carries the blocks; the
/// Creature Creator's play code puts it back when the mode ends).
#[allow(clippy::too_many_arguments)]
pub fn test_drive(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    editor: Res<CreatureEditor>,
    rigid: Res<RigidEditor>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    mut drive: ResMut<TestDrive>,
    mut holders: Query<&mut Transform, With<RigidHolder>>,
) {
    let on = rigid.active && editor.mode == Mode::Play && rigid.kind.is_vehicle();
    let Ok(mut t) = holders.single_mut() else {
        drive.active = false;
        return;
    };
    let rest = Transform::from_rotation(preview_rotation());
    if !on {
        if drive.active {
            *t = rest;
            let auto = drive.auto;
            *drive = TestDrive { auto, ..Default::default() };
        }
        return;
    }
    if !drive.active {
        let stats = session_stats(&originals, &run);
        let speed = civ_combat::combat(domain_of(rigid.kind), stats).speed / VEHICLE_STAGE_SCALE;
        let auto = drive.auto;
        *drive = TestDrive { active: true, top_speed: speed, stats, auto, ..Default::default() };
        info!("rigid test drive: {:?} stats {stats:?}, top speed {speed:.1} m/s", rigid.kind);
    }
    let dt = time.delta_secs().min(0.1);
    let typing = editor.naming || editor.options_open;
    let held = |a: KeyCode, b: KeyCode| !typing && keys.any_pressed([a, b]);
    let (mut throttle, mut steer) = (0.0f32, 0.0f32);
    if held(KeyCode::KeyW, KeyCode::ArrowUp) {
        throttle += 1.0;
    }
    if held(KeyCode::KeyS, KeyCode::ArrowDown) {
        throttle -= 0.5;
    }
    if held(KeyCode::KeyA, KeyCode::ArrowLeft) {
        steer += 1.0;
    }
    if held(KeyCode::KeyD, KeyCode::ArrowRight) {
        steer -= 1.0;
    }
    if drive.auto {
        throttle = 1.0;
        steer = 0.8;
    }
    let target = throttle * drive.top_speed;
    let accel = drive.top_speed / ACCELERATION_TIME;
    drive.speed += (target - drive.speed).clamp(-accel * dt, accel * dt);
    if drive.speed.abs() > 0.01 {
        drive.heading += steer * TURN_RATE * dt * drive.speed.signum();
    }
    // The blocks' front (+Z) faces -Z of the preview root (the holder's half turn).
    let forward = Quat::from_rotation_y(drive.heading) * Vec3::NEG_Z;
    let step = forward * drive.speed * dt;
    let next = (drive.position + Vec2::new(step.x, step.z)).clamp_length_max(TEST_GROUND_RADIUS);
    drive.distance += (next - drive.position).length();
    drive.position = next;
    t.translation = Vec3::new(drive.position.x, 0.0, drive.position.y);
    t.rotation = Quat::from_rotation_y(drive.heading) * rest.rotation;
}

/// The editor camera follows the driving vehicle: the Creature Creator's
/// camera is placed on the preview root every frame, this moves it by the
/// holder's offset (PostUpdate, before transforms propagate).
pub fn follow_camera(
    drive: Res<TestDrive>,
    holders: Query<&Transform, (With<RigidHolder>, Without<crate::GameCamera>)>,
    roots: Query<&Transform, (With<crate::rigid_editor::RigidPreview>, Without<crate::GameCamera>)>,
    mut cameras: Query<&mut Transform, (With<crate::GameCamera>, Without<RigidHolder>, Without<crate::rigid_editor::RigidPreview>)>,
) {
    if !drive.active {
        return;
    }
    let (Ok(holder), Ok(root), Ok(mut camera)) = (holders.single(), roots.single(), cameras.single_mut()) else {
        return;
    };
    camera.translation += root.rotation * (holder.translation * root.scale);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_size_and_centre_of_mass() {
        assert!(playable(Vec3::new(0.2, 0.2, 0.2), [0.1, 0.1, 0.1]));
        assert!(!playable(Vec3::new(0.6, 0.9, 0.6), [0.5, 1.0, 0.5]));
        let mut a = PartInstance::new(1, 0);
        a.rigid = Some(crate::rigid_editor::RigidBlock::at(Vec3::new(2.0, 1.0, 0.0)));
        let mut b = a.clone();
        b.rigid = Some(crate::rigid_editor::RigidBlock::at(Vec3::new(0.0, 3.0, 4.0)));
        let mut parts = vec![a, b];
        let shift = center_of_mass(&mut parts);
        assert!((shift - Vec3::new(-1.0, 0.0, -2.0)).length() < 1e-5, "{shift:?}");
        let p: Vec<Vec3> = parts.iter().map(|p| p.rigid.as_ref().unwrap().position()).collect();
        assert_eq!(p, vec![Vec3::new(1.0, 1.0, -2.0), Vec3::new(-1.0, 3.0, 2.0)]);
        assert_eq!(center_of_mass(&mut parts), Vec3::ZERO);
    }

    #[test]
    fn vehicle_classes_have_their_civ_locomotion() {
        use crate::civ::VehicleDomain as D;
        assert_eq!(domain_of(SetupKind::VehicleMilitaryLand), D::Land);
        assert_eq!(domain_of(SetupKind::VehicleEconomicWater), D::Sea);
        assert_eq!(domain_of(SetupKind::VehicleCulturalAir), D::Air);
    }
}
