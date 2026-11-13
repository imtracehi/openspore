//! Creature look-at: the external target of the animation system and how the
//! clips' secondary frames use it (`notes/creature-look-notes.md`).
//!
//! ORIGINAL (march2017 addresses):
//! * every animated creature has one external target (AnimatedCreature
//!   +0x154..+0x16C, copied each frame into its anim cid +0x274..+0x2A4 by
//!   UpdateCreatures 0xA0C600): a point in the creature's local frame (mode
//!   0), a world point (mode 1) or a block of another creature (mode 2, the
//!   block chosen by a selector; FUN_00A02B30 / FUN_00A06190 / FUN_00A061E0
//!   set them). With no valid target the cid points at (0, -10, 1) local;
//!   FUN_009CD4B0 resolves the target to a world point;
//! * a clip channel with the secondary movement flag (1) uses a secondary
//!   frame point: the selected block of its secondary context, or the external
//!   target when that context selects nothing (flags 2; FUN_009B05F0).
//!   Positions (FUN_009B0D60) are keyed in a frame from the block's rest
//!   position toward that point (x toward it, z up), scaled by the distance
//!   (or, with the look-at flag 8, by the component's scale mode); the
//!   ground-relative flag (0x10) keys height against the ground. Rotations with
//!   the directional-only flag (2) aim the block (its -Y) at the point
//!   (FUN_009B06A0): directions more than ~45 degrees off the block's rest
//!   forward are pulled back toward it (smoothstep ramps at dot 0.7 / -0.3 /
//!   -0.5), the block's rest up stays up (FUN_009B0210), and the key turns
//!   relative to that look frame when the component flag 0x40 is set;
//! * game side (cCreatureBase FUN_00C17F40, run every frame from 0xC05970):
//!   a creature with a combat target looks at the target's head block
//!   (FUN_00C176E0(2, 0), the first head/mouth block; its spine when the
//!   target is dead); a moving NPC looks at its path goal or 10 units ahead;
//!   the player's avatar looks at the cursor's ground point (cGameViewManager
//!   pick), else 10 ahead; otherwise the target is cleared to
//!   (0, -max(10 x scale, radius), 0) local (FUN_00C0D3A0). The point moves
//!   toward a new target exponentially, factor e^(-3.0103 dt) (FUN_00C10630);
//! * the Creature Creator / Test Drive creature (EditorCreatureController)
//!   looks 1.5 ahead and 0.5 up by default (FUN_0059B390).
//!
//! ADAPTED: the secondary frame of a channel whose secondary context selects
//! a block uses that block's rest point (retail: its posed point); the look
//! frame is built in the creature's rest frame (retail composes the current
//! body frame); smoothing skips FUN_00C10630's turn-rate clamp. The cursor
//! pick is the terrain pick (`track_cursor`); only a headless stage without a
//! terrain surface uses the plane at the avatar's height.

use bevy::math::{Mat3, Quat, Vec3};

/// FUN_00C10630: the look point approaches its target by e^(-rate dt).
pub const LOOK_SMOOTH_RATE: f32 = 3.0103;
/// FUN_00C0D3A0 / FUN_00C17F40: default look distance ahead (creature units).
pub const DEFAULT_AHEAD: f32 = 10.0;
/// FUN_0059B390: the editor creature's default look point (forward, up).
pub const EDITOR_AHEAD: f32 = 1.5;
pub const EDITOR_UP: f32 = 0.5;

/// FUN_009B00B0: smoothstep of `x` between `a` and `b`.
pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    if (b - a).abs() >= f32::MIN_POSITIVE {
        let t = (x.max(a).min(b) - a) / (b - a);
        3.0 * t * t - 2.0 * t * t * t
    } else if x <= a {
        0.0
    } else if b <= x {
        1.0
    } else {
        0.5
    }
}

/// FUN_009A4980: two unit vectors perpendicular to `a` (and to each other).
fn perpendicular(a: Vec3) -> (Vec3, Vec3) {
    let mut v = [1.0f32; 3];
    let mut big = 0.0f32;
    let mut index = 0;
    for (i, c) in a.to_array().iter().enumerate() {
        if c.abs() > big {
            big = c.abs();
            index = i;
        }
    }
    v[index] = 0.0;
    let x = Vec3::from_array(v).cross(a).normalize_or_zero();
    let z = a.cross(x).normalize_or_zero();
    (x, z)
}

/// FUN_009B0210: a frame whose column `along` is the unit vector from `from`
/// to `to` and whose column `third` is `up` made perpendicular to it; returns
/// the frame and the distance.
pub fn frame(from: Vec3, to: Vec3, along: usize, third: usize, up: Vec3) -> (Mat3, f32) {
    let d = to - from;
    let dist = d.length();
    let mut cols = [Vec3::ZERO; 3];
    let (a, mut x, z);
    if dist <= 1e-6 {
        a = Vec3::X;
        x = Vec3::Y;
        z = Vec3::Z;
    } else {
        a = d / dist;
        let cx = up.cross(a);
        if cx.length() <= 1e-6 {
            let (px, pz) = perpendicular(a);
            x = px;
            z = pz;
        } else {
            x = cx.normalize();
            z = a.cross(x);
        }
    }
    cols[along] = a;
    cols[third] = z;
    let mut other = (along + 1) % 3;
    if other == third {
        x = -x;
        other = (along + 2) % 3;
    }
    cols[other] = x;
    (Mat3::from_cols(cols[0], cols[1], cols[2]), dist)
}

/// FUN_009B06A0: the orientation that turns a block (rest orientation `rest`
/// in the creature frame, at `pos`) to face `target` with its -Y, pulled back
/// toward its rest forward when the target is too far to the side or behind;
/// its rest up axis stays up.
pub fn look_frame(rest: Quat, target: Vec3, pos: Vec3) -> Quat {
    let forward = -(rest * Vec3::Y);
    let mut dir = target - pos;
    if dir.length() != 0.0 {
        dir = dir.normalize();
    }
    let d = forward.dot(dir);
    if d < 0.7 {
        let k = if d < -0.3 {
            if d < -0.5 {
                (1.0 - smoothstep(-1.0, -0.5, d) * 0.8) - d
            } else {
                0.2 - d
            }
        } else {
            (smoothstep(-0.3, 0.7, d) * 0.5 + 0.2) - d
        };
        dir = (forward * k + dir).normalize_or_zero();
    }
    // DAT_01550A6C = 1: aim at pos + the clamped direction, rest up axis up.
    let aim = pos + dir;
    let up = rest * Vec3::Z;
    let (m, _) = frame(aim, pos, 1, 2, up);
    let q = Quat::from_mat3(&m);
    if q.is_finite() && q.length_squared() > 1e-8 {
        q.normalize()
    } else {
        rest
    }
}

/// FUN_009B0D60 (secondary branch): the goal of a keyed position `key` of a
/// block at `rest` in the secondary frame toward `point`. `measure(axis)` is
/// the component's scale measure along an axis (FUN_009B0B80: 1, bounds or
/// limb length); `ground_z` the ground height in the same frame.
pub fn secondary_position(rest: Vec3, point: Vec3, key: Vec3, movement: u32, measure: &dyn Fn(Vec3) -> f32, ground_z: f32) -> Vec3 {
    if movement & 0x10 == 0 {
        let (f, dist) = frame(rest, point, 0, 2, Vec3::Z);
        let k = if movement & 8 == 0 {
            key * dist
        } else {
            Vec3::new(key.x * measure(f.x_axis), key.y * measure(f.y_axis), key.z * measure(f.z_axis))
        };
        return rest + f.x_axis * k.x + f.y_axis * k.y + f.z_axis * k.z;
    }
    // Ground relative: horizontal frame toward the point, z keyed against
    // the ground (1 = on the ground).
    let mut d = point - rest;
    let (mut toward, mut length) = (1.0, 1.0);
    if movement & 8 != 0 {
        length = d.length();
        if length != 0.0 {
            d /= length;
        }
        toward = measure(d);
    }
    let side = Vec3::new(d.y, -d.x, 0.0).normalize_or_zero();
    let across = measure(side);
    let fx = key.x * toward;
    let fy = key.y * across;
    let along = if movement & 8 != 0 && length != 0.0 { fx / length } else { fx };
    let lift = ((point.z - rest.z) * along + (rest.z - ground_z)) * key.z;
    Vec3::new(rest.x + d.x * fx + side.x * fy, rest.y + d.y * fx + side.y * fy, rest.z + d.z * fx - lift)
}

/// Move a remembered look point toward `target` (FUN_00C10630's exponential
/// factor; the first target is taken at once).
pub fn smooth(current: &mut Option<Vec3>, target: Vec3, dt: f32) -> Vec3 {
    let next = match *current {
        Some(c) if c.is_finite() => {
            let keep = (-LOOK_SMOOTH_RATE * dt.max(0.0)).exp();
            target + (c - target) * keep
        }
        _ => target,
    };
    *current = Some(next);
    next
}

/// The world position of a posed creature's first head block: FUN_00C176E0(2, 0)
/// is the first block of the head list (the `mout` blocks, FUN_004D9090), which
/// the look target of another creature aims at (FUN_00C17F40). Written by the
/// NPC posing (`locomotion_preview`) from the posed mouth joint of the rig.
#[derive(bevy::prelude::Component, Clone, Copy, Debug, PartialEq)]
pub struct PosedHead(pub Vec3);

/// ADAPTED (only for a creature that is not posed by clips, so has no
/// [`PosedHead`]): its head point is taken one creature unit above its origin
/// along its up axis (world units x its scale).
pub const HEAD_HEIGHT: f32 = 1.0;

pub fn head_point(gt: &bevy::prelude::GlobalTransform) -> Vec3 {
    let t = gt.compute_transform();
    t.translation + t.rotation * Vec3::Y * (HEAD_HEIGHT * t.scale.y.abs())
}

/// The point another creature looks at: the posed head block when the creature
/// has one, else [`head_point`].
pub fn head_of(gt: &bevy::prelude::GlobalTransform, posed: Option<&PosedHead>) -> Vec3 {
    posed.filter(|h| h.0.is_finite()).map_or_else(|| head_point(gt), |h| h.0)
}

/// The player's cursor on the ground (Creature stage), for the avatar's look.
#[derive(bevy::prelude::Resource, Default, Debug, Clone, Copy)]
pub struct CursorGround(pub Option<Vec3>);

/// The terrain point under the cursor (FUN_00C17F40 asks cGameViewManager for
/// it): the cursor ray against the stage terrain / sea, the same pick the click
/// to walk uses (`civ_rts::ground_hit`). ADAPTED only where no terrain exists
/// (headless): the horizontal plane at the avatar's height.
pub fn track_cursor(
    mut out: bevy::prelude::ResMut<CursorGround>,
    stage: Option<bevy::prelude::Res<bevy::prelude::State<crate::stages::GameStage>>>,
    windows: bevy::prelude::Query<&bevy::window::Window, bevy::prelude::With<bevy::window::PrimaryWindow>>,
    cameras: bevy::prelude::Query<(&bevy::prelude::Camera, &bevy::prelude::GlobalTransform), bevy::prelude::With<crate::GameCamera>>,
    players: bevy::prelude::Query<&bevy::prelude::GlobalTransform, bevy::prelude::With<crate::game::Player>>,
    surface: Option<bevy::prelude::Res<crate::terrain::LandSurface>>,
) {
    out.0 = None;
    if !stage.is_some_and(|s| *s.get() == crate::stages::GameStage::Creature) {
        return;
    }
    let (Ok(window), Ok((camera, eye)), Ok(player)) = (windows.single(), cameras.single(), players.single()) else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok(ray) = camera.viewport_to_world(eye, cursor) else { return };
    let dir = ray.direction.as_vec3();
    if surface.is_some() {
        out.0 = crate::civ_rts::ground_hit(ray.origin, dir, surface.as_deref());
        if out.0.is_some() {
            return;
        }
    }
    let height = player.translation().y;
    if dir.y.abs() < 1e-4 {
        return;
    }
    let t = (height - ray.origin.y) / dir.y;
    if t > 0.0 && t.is_finite() {
        out.0 = Some(ray.origin + dir * t);
    }
}

/// A world point a creature looks at instead of the game's choice (the
/// look-demo capture sets it; a traced cinematic `-target` could use it).
#[derive(bevy::prelude::Component, Clone, Copy, Debug)]
pub struct LookAt(pub Vec3);

/// Capture aid `--look-demo player|npc`: the camera frames one creature's
/// face, the creature plays a clip then the look idle, and its look target
/// sweeps from side to side (`quick_capture_demo`).
#[derive(bevy::prelude::Resource, Clone, Copy, Debug)]
pub struct LookDemo {
    pub subject: bevy::prelude::Entity,
    pub t: f32,
    pub distance: f32,
    pub height: f32,
    pub side: f32,
    /// Hold the subject at this world transform (an NPC brought before the
    /// camera; its own movement is overridden every frame).
    pub anchor: Option<bevy::prelude::Transform>,
}

/// The creature's facing in the Bevy frame: Spore's front (-Y) maps to -Z.
fn front_up(gt: &bevy::prelude::GlobalTransform) -> (Vec3, Vec3, Vec3) {
    let t = gt.compute_transform();
    (t.translation, (t.rotation * Vec3::NEG_Z).normalize_or_zero(), (t.rotation * Vec3::Y).normalize_or_zero())
}

pub fn demo_update(
    time: bevy::prelude::Res<bevy::prelude::Time>,
    mut commands: bevy::prelude::Commands,
    demo: Option<bevy::prelude::ResMut<LookDemo>>,
    subjects: bevy::prelude::Query<&bevy::prelude::GlobalTransform>,
    mut held: bevy::prelude::Query<&mut bevy::prelude::Transform, bevy::prelude::Without<crate::GameCamera>>,
) {
    let Some(mut demo) = demo else { return };
    demo.t += time.delta_secs();
    if let (Some(anchor), Ok(mut t)) = (demo.anchor, held.get_mut(demo.subject)) {
        t.translation = anchor.translation;
        t.rotation = anchor.rotation;
    }
    let Ok(gt) = subjects.get(demo.subject) else { return };
    let (pos, front, up) = front_up(gt);
    let side = front.cross(up).normalize_or_zero();
    let head = pos + up * demo.height;
    let target = head + front * 4.0 + side * (3.5 * (0.9 * demo.t).sin()) + up * (0.6 * (0.37 * demo.t).sin());
    commands.entity(demo.subject).insert(LookAt(target));
}

pub fn demo_camera(
    demo: Option<bevy::prelude::Res<LookDemo>>,
    subjects: bevy::prelude::Query<&bevy::prelude::GlobalTransform, bevy::prelude::Without<crate::GameCamera>>,
    mut cameras: bevy::prelude::Query<(&mut bevy::prelude::Transform, &mut bevy::prelude::GlobalTransform), bevy::prelude::With<crate::GameCamera>>,
) {
    let Some(demo) = demo else { return };
    let Ok(gt) = subjects.get(demo.subject) else { return };
    let (pos, front, up) = front_up(gt);
    let side = front.cross(up).normalize_or_zero();
    let eye = pos + front * demo.distance + up * demo.height + side * demo.side;
    // Last: after every camera system and the transform propagation (the
    // camera is a root entity), so the render uses this view.
    for (mut t, mut g) in &mut cameras {
        *t = bevy::prelude::Transform::from_translation(eye).looking_at(pos + up * (demo.height * 0.75), up);
        *g = bevy::prelude::GlobalTransform::from(*t);
    }
}

/// `--look-demo player|npc [--look-clip HEX] [--look-frames N] [--look-every K]
/// [--look-dist D] [--look-height H] [--look-side S]` after the Creature
/// warm-up: frames `look-<mode>-NN.png`.
pub fn quick_capture_demo(app: &mut bevy::prelude::App, args: &[String], output: &std::path::Path, target: &bevy::prelude::Handle<bevy::prelude::Image>, stage: &str) {
    use bevy::prelude::*;
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
    let Some(mode) = flag("--look-demo") else { return };
    if stage != "creature" {
        return;
    }
    let num = |name: &str, d: f32| flag(name).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
    if mode == "custom" {
        // A body edit makes the avatar the player's own skinned creature
        // (`player_appearance` shows the installed starter only while the
        // build is untouched).
        if let Some(p) = app.world_mut().resource_mut::<crate::game::RunProgress>().body.spine.first_mut() {
            p.radius += 0.03;
        }
        for _ in 0..30 {
            app.update();
            crate::runtime::gpu_flush(app);
        }
    }
    let world = app.world_mut();
    let Some((player, at)) = world.query_filtered::<(Entity, &GlobalTransform), With<crate::game::Player>>().iter(world).next().map(|(e, g)| (e, g.translation())) else { return };
    let subject = if mode == "npc" {
        let mut near: Vec<(f32, Entity, bool)> = world
            .query_filtered::<(Entity, &GlobalTransform, &InheritedVisibility), With<crate::baked_model::NativeCreature>>()
            .iter(world)
            .map(|(e, g, v)| (g.translation().distance(at), e, v.get()))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        info!("look demo: nearest native creatures {:?}", &near[..near.len().min(6)]);
        world
            .query_filtered::<(Entity, &GlobalTransform, &InheritedVisibility), (With<crate::baked_model::NativeCreature>, With<crate::locomotion_preview::GaitSkeleton>)>()
            .iter(world)
            .filter(|(_, _, v)| v.get())
            .min_by(|a, b| a.1.translation().distance(at).total_cmp(&b.1.translation().distance(at)))
            .map(|(e, _, _)| e)
    } else {
        Some(player)
    };
    let Some(subject) = subject else {
        warn!("--look-demo {mode}: no creature");
        return;
    };
    let clip = flag("--look-clip").and_then(|c| u32::from_str_radix(c.trim_start_matches("0x"), 16).ok()).unwrap_or(0x042A_1CAF);
    if let Some(gt) = world.get::<GlobalTransform>(subject) {
        let t = gt.compute_transform();
        info!("look demo: {mode} {subject:?} at {:.1} scale {:.2}, clip {clip:08X}", t.translation, t.scale.y);
    }
    world.entity_mut(subject).insert(crate::locomotion_preview::PoseClip { action: Some(clip), idle: LOOK_IDLE, serial: 1 });
    // An NPC is brought 8 units in front of the avatar, facing it (the
    // camera stands between them).
    let anchor = (subject != player).then(|| {
        let p = world.get::<Transform>(player).copied().unwrap_or_default();
        let front = (p.rotation * Vec3::NEG_Z).normalize_or_zero();
        let mut t = world.get::<Transform>(subject).copied().unwrap_or_default();
        t.translation = p.translation + front * num("--look-npc-dist", 8.0);
        t.rotation = p.rotation * Quat::from_rotation_y(std::f32::consts::PI);
        t
    });
    world.insert_resource(LookDemo { subject, t: 0.0, distance: num("--look-dist", 4.0), height: num("--look-height", 1.2), side: num("--look-side", 0.0), anchor });
    let frames = num("--look-frames", 240.0) as usize;
    let every = (num("--look-every", 20.0) as usize).max(1);
    for i in 0..frames {
        app.update();
        crate::runtime::gpu_flush(app);
        if i == 1 {
            let world = app.world_mut();
            let holder = world.get::<crate::locomotion_preview::GaitSkeleton>(subject).map(|s| s.holder());
            let at = |w: &World, e: Entity| w.get::<GlobalTransform>(e).map(|g| g.translation());
            let eye = world.query_filtered::<&GlobalTransform, With<crate::GameCamera>>().iter(world).next().map(|g| g.translation());
            info!("look demo: subject {:?} holder {:?} eye {:?} player {:?}", at(world, subject), holder.and_then(|h| at(world, h)), eye, at(world, player));
        }
        if i % every == every - 1 {
            screenshot(app, target, &output.join(format!("look-{mode}-{:02}.png", i / every)));
        }
    }
    app.world_mut().remove_resource::<LookDemo>();
}

/// A plain screenshot of the capture target (no provenance files; as the
/// cinematic captures do).
fn screenshot(app: &mut bevy::prelude::App, target: &bevy::prelude::Handle<bevy::prelude::Image>, path: &std::path::Path) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    let _ = std::fs::remove_file(path);
    app.world_mut().spawn(Screenshot::image(target.clone())).observe(save_to_disk(path.to_path_buf()));
    for _ in 0..600 {
        app.update();
        crate::runtime::gpu_flush(app);
        if std::fs::metadata(path).is_ok_and(|m| m.len() > 1000) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// `csa_idle_walk_lookat_loop` (TLSA 0x0452B64A): the idle whose `Head lookat`
/// channel (movement 3, weight 0.7) turns the head to the look target; the
/// editor plays it after `csa_idle_walk_lookat_start` 0x0452B634 when a walk
/// stops (FUN_006388D0). FUN_00C0C790's creature versions (0x04866D8E /
/// 0x04866DAB) carry no look channel in the installed clips.
pub const LOOK_IDLE: u32 = 0x0452_B64A;

/// `OPENSPORE_LOOK_DEBUG`: log the look target / clip of `LookAt` subjects.
pub fn debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OPENSPORE_LOOK_DEBUG").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothstep_matches_the_ramp_helper() {
        assert_eq!(smoothstep(-0.3, 0.7, -1.0), 0.0);
        assert_eq!(smoothstep(-0.3, 0.7, 2.0), 1.0);
        assert!((smoothstep(0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
        assert_eq!(smoothstep(1.0, 1.0, 0.5), 0.0);
        assert_eq!(smoothstep(1.0, 1.0, 2.0), 1.0);
    }

    #[test]
    fn frames_are_rotations_with_the_requested_axes() {
        let (m, d) = frame(Vec3::ZERO, Vec3::new(0.0, -2.0, 0.0), 0, 2, Vec3::Z);
        assert!((d - 2.0).abs() < 1e-6);
        assert!(m.x_axis.distance(Vec3::NEG_Y) < 1e-6);
        assert!(m.z_axis.distance(Vec3::Z) < 1e-6);
        assert!((m.determinant() - 1.0).abs() < 1e-5);
        // Column 1 along: the third column is up, column 0 is the negated cross.
        let (m, _) = frame(Vec3::new(0.0, -1.0, 0.0), Vec3::ZERO, 1, 2, Vec3::Z);
        assert!(m.y_axis.distance(Vec3::Y) < 1e-6);
        assert!((m.determinant() - 1.0).abs() < 1e-5);
        // Up along the direction falls back to a perpendicular basis.
        let (m, _) = frame(Vec3::ZERO, Vec3::Z, 0, 2, Vec3::Z);
        assert!((m.determinant().abs() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn look_frame_faces_the_target_within_the_cone_and_holds_back_behind() {
        let pos = Vec3::new(0.0, -0.5, 1.0);
        // Straight ahead: the rest orientation.
        let q = look_frame(Quat::IDENTITY, pos + Vec3::new(0.0, -5.0, 0.0), pos);
        assert!(q.angle_between(Quat::IDENTITY) < 1e-4);
        // 30 degrees to the side (inside the dot 0.7 cone): faced exactly.
        let dir = Quat::from_rotation_z(0.5) * Vec3::NEG_Y;
        let q = look_frame(Quat::IDENTITY, pos + dir * 3.0, pos);
        assert!((q * Vec3::NEG_Y).distance(dir) < 1e-4, "{:?}", q * Vec3::NEG_Y);
        assert!((q * Vec3::Z).distance(Vec3::Z) < 1e-4);
        // 90 degrees to the side: pulled back toward forward.
        let side = Vec3::X;
        let q = look_frame(Quat::IDENTITY, pos + side * 3.0, pos);
        let f = q * Vec3::NEG_Y;
        assert!(f.dot(side) > 0.9 && f.dot(Vec3::NEG_Y) > 0.25, "{f:?}");
        // Straight behind: keeps looking forward.
        let q = look_frame(Quat::IDENTITY, pos + Vec3::Y * 3.0, pos);
        assert!((q * Vec3::NEG_Y).distance(Vec3::NEG_Y) < 1e-3);
        // A turned rest orientation keeps its own up axis.
        let rest = Quat::from_rotation_y(0.3);
        let q = look_frame(rest, pos + Vec3::new(0.0, -5.0, 0.0), pos);
        assert!((q * Vec3::Z).dot(rest * Vec3::Z) > 0.99);
    }

    #[test]
    fn secondary_positions_follow_the_frame_toward_the_point() {
        let rest = Vec3::new(0.0, -1.0, 1.0);
        let point = Vec3::new(0.0, -5.0, 1.0);
        let one = |_: Vec3| 1.0;
        // Unscaled keys (look-at flag): x moves toward the point.
        let g = secondary_position(rest, point, Vec3::new(0.25, 0.0, 0.0), 0xB, &one, 0.0);
        assert!(g.distance(Vec3::new(0.0, -1.25, 1.0)) < 1e-5, "{g}");
        // Without the look-at flag the key is in units of the distance.
        let g = secondary_position(rest, point, Vec3::new(0.5, 0.0, 0.0), 0x1, &one, 0.0);
        assert!(g.distance(Vec3::new(0.0, -3.0, 1.0)) < 1e-5, "{g}");
        // Ground relative: z = 1 is the ground.
        let g = secondary_position(rest, point, Vec3::new(0.0, 0.0, 1.0), 0x11, &one, 0.0);
        assert!(g.z.abs() < 1e-5, "{g}");
        let g = secondary_position(rest, point, Vec3::ZERO, 0x11, &one, 0.0);
        assert!(g.distance(rest) < 1e-5);
    }

    #[test]
    fn another_creatures_look_aims_at_its_posed_head_block() {
        let gt = bevy::prelude::GlobalTransform::from(
            bevy::prelude::Transform::from_xyz(2.0, 0.0, 3.0).with_scale(Vec3::splat(2.0)),
        );
        // No pose: one creature unit above the origin, times its scale.
        assert!(head_of(&gt, None).distance(Vec3::new(2.0, 2.0, 3.0)) < 1e-5);
        // A posed first head block wins; a broken one is ignored.
        let posed = PosedHead(Vec3::new(2.0, 1.4, 2.2));
        assert_eq!(head_of(&gt, Some(&posed)), Vec3::new(2.0, 1.4, 2.2));
        let broken = PosedHead(Vec3::new(f32::NAN, 0.0, 0.0));
        assert!(head_of(&gt, Some(&broken)).distance(Vec3::new(2.0, 2.0, 3.0)) < 1e-5);
    }

    #[test]
    fn look_points_approach_their_target() {
        let mut current = None;
        assert_eq!(smooth(&mut current, Vec3::X, 0.1), Vec3::X);
        let p = smooth(&mut current, Vec3::ZERO, 0.23);
        assert!((p.x - (-LOOK_SMOOTH_RATE * 0.23f32).exp()).abs() < 1e-5);
    }
}
