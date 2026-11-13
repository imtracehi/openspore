//! The cinematic camera controller (`startCamera`), notes section 3.1.
//!
//! Original behaviour (VERIFIED in the notes unless marked):
//! * angles are in turns (x 2pi, constant 0x167A5FC), FOV in radians;
//! * the target point is the bound object's position (+ head / terrain
//!   height), plus `-targetOffset` rotated into the object's frame, plus
//!   `-extraTargetOffset`, re-evaluated every frame;
//! * orbit mode (default, `-ballistic`): eye = target + R(heading, pitch,
//!   roll) (0, -offset, 0) in the Spore frame (x right, y forward, z up),
//!   pushed above terrain/water + 0.1 (0xEB43F0 case 1); heading relative to
//!   the target's facing with `-orient`;
//! * `-anchored`: the eye stays where it is and turns toward the target;
//!   `-offset d` dollies it d along the view direction (negative pulls back);
//! * every animated scalar is `start + (end - start) e(t)`, angles the short
//!   way round (0xEB3310), t = elapsed / duration clamped to 0..1 (0xEB6C20),
//!   e(t) = t or, with `-ease`, the trapezoid curve of 0xEB3220;
//! * a blocking move is finished when |d angle| <= 0.15 for heading, pitch and
//!   roll, |d distance| <= 0.0225 and |d target|^2 <= 0.0225 (0xEB4010).
//!
//! INFERRED: the order of the heading/pitch rotations and the sign of heading
//! (the code builds the frame from -heading, 0xEB2... `^0x80000000`; with the
//! creature front at Spore +y, heading 0.5 puts the camera in front of the
//! face, which the brain-upgrade shots need). `-sphere`, `-satellite` and
//! `pathCamera` are not traced: they fall back to the orbit behaviour.

use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

/// Spore vector (x right, y forward, z up) to the Bevy frame (x right, -z
/// forward, y up), the same basis change the part importer uses.
pub fn spore_to_bevy(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[2], -v[1])
}

pub fn bevy_to_spore(v: Vec3) -> [f32; 3] {
    [v.x, -v.z, v.y]
}

/// Retail ease (0xEB3220): trapezoidal velocity, 1/4 accelerate, 1/2
/// constant, 1/4 decelerate.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.25 {
        t * t * 2.666_666_7
    } else if t < 0.75 {
        (t - 0.25) * 1.333_333_4 + 0.166_666_67
    } else {
        (1.333_333_4 - (t - 0.75) * 2.666_666_7) * (t - 0.75) + 0.833_333_3
    }
}

/// `start + (end - start) e`.
pub fn lerp(start: f32, end: f32, e: f32) -> f32 {
    start + (end - start) * e
}

/// Wrap an angle difference to [-pi, pi).
pub fn wrap(mut d: f32) -> f32 {
    while d < -PI {
        d += TAU;
    }
    while d >= PI {
        d -= TAU;
    }
    d
}

/// Angle interpolation the short way round (0xEB3310).
pub fn lerp_angle(start: f32, end: f32, e: f32) -> f32 {
    start + wrap(end - start) * e
}

/// Orbit eye offset (Bevy frame) for heading/pitch in radians and distance:
/// Spore `Rz(h) Rx(p) (0, -d, 0)` = d (cos p sin h, -cos p cos h, -sin p).
pub fn orbit_offset(heading: f32, pitch: f32, distance: f32) -> Vec3 {
    let (sh, ch) = heading.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    spore_to_bevy([distance * cp * sh, -distance * cp * ch, -distance * sp])
}

/// Inverse of [`orbit_offset`]: (heading, pitch, distance).
pub fn orbit_angles(offset: Vec3) -> (f32, f32, f32) {
    let d = offset.length();
    if d < 1e-5 {
        return (0.0, 0.0, 0.0);
    }
    let [x, y, z] = bevy_to_spore(offset);
    let pitch = (-z / d).clamp(-1.0, 1.0).asin();
    let heading = x.atan2(-y);
    (heading, pitch, d)
}

/// Bevy yaw (rotation about +Y) of a transform's facing, with the creature
/// front at local -Z.
pub fn yaw_of(rotation: Quat) -> f32 {
    let f = rotation * Vec3::NEG_Z;
    (-f.x).atan2(-f.z)
}

/// Camera mode (`-anchored` 0, `-ballistic` 1, `-satellite` 2, default 4
/// orbit, `pathCamera` 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CameraMode {
    Anchored,
    Ballistic,
    Satellite,
    Path,
    #[default]
    Orbit,
}

/// Where on the target the camera aims (`-targetHead` 1, `-targetTerrain` 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Aim {
    #[default]
    Origin,
    Head,
    Terrain,
}

/// A parsed `startCamera` / `pathCamera` line (job struct of 0xAD99B0, the
/// defaults are the parser's).
#[derive(Clone, Debug, PartialEq)]
pub struct CameraSpec {
    pub target: Option<u32>,
    pub offset: f32,
    pub target_offset: [f32; 3],
    pub extra_target_offset: [f32; 3],
    pub follow: bool,
    pub orient: bool,
    pub orient_flat: bool,
    pub use_footprint: bool,
    pub use_radius: bool,
    pub use_height: bool,
    pub radius_target: Option<u32>,
    pub aim: Aim,
    /// Blocking unless `-noWait`.
    pub blocking: bool,
    pub snap: bool,
    pub ease: bool,
    pub mode: CameraMode,
    pub back_to_start: bool,
    /// Turns.
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,
    /// Radians; 0 keeps the current FOV.
    pub fov: f32,
    /// Seconds (default 1).
    pub duration: f32,
    pub sphere: bool,
    /// `-nearClip` / `-farClip` when written in the script.
    pub near_clip: Option<f32>,
    pub far_clip: Option<f32>,
    pub target_pos: Option<[f32; 3]>,
    /// `pathCamera -path (...)` / `-pathEndStart (...)` points.
    pub path: Vec<[f32; 3]>,
    /// The points came as `-pathEndStart` (0xADE150 sets job flag +0x44).
    pub path_end_start: bool,
}

impl CameraSpec {
    /// The spec with every length mapped through `f` (a stage whose world is
    /// smaller than the retail one: Civ, see `host::stage_length`). Angles and
    /// FOV stay.
    pub fn scaled(mut self, f: impl Fn(f32) -> f32) -> Self {
        self.offset = f(self.offset);
        for v in self.target_offset.iter_mut().chain(self.extra_target_offset.iter_mut()) {
            *v = f(*v);
        }
        if let Some(p) = self.target_pos.as_mut() {
            for v in p.iter_mut() {
                *v = f(*v);
            }
        }
        for p in self.path.iter_mut() {
            for v in p.iter_mut() {
                *v = f(*v);
            }
        }
        self.near_clip = self.near_clip.map(&f);
        self.far_clip = self.far_clip.map(&f);
        self
    }
}

impl Default for CameraSpec {
    fn default() -> Self {
        Self {
            target: None,
            offset: 0.0,
            target_offset: [0.0; 3],
            extra_target_offset: [0.0; 3],
            follow: false,
            orient: false,
            orient_flat: false,
            use_footprint: false,
            use_radius: false,
            use_height: false,
            radius_target: None,
            aim: Aim::Origin,
            blocking: true,
            snap: false,
            ease: false,
            mode: CameraMode::Orbit,
            back_to_start: false,
            heading: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov: 0.0,
            duration: 1.0,
            sphere: false,
            near_clip: None,
            far_clip: None,
            target_pos: None,
            path: Vec::new(),
            path_end_start: false,
        }
    }
}

/// What the host resolved for the job's target this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetFrame {
    /// Object origin (or fixed point), Bevy world.
    pub position: Vec3,
    /// Object facing yaw (Bevy, rotation about +Y).
    pub yaw: f32,
    /// Offset multiplier from `-useRadius` / `-useFootprint` / `-useHeight`
    /// (`max(value, 1)`), 1 without those flags.
    pub scale: f32,
    /// Height added for `-targetHead` (ADAPTED: OpenSpore has no head bone).
    pub head: f32,
    /// Terrain/water height under the object for `-targetTerrain`.
    pub ground: f32,
}

impl TargetFrame {
    pub fn at(position: Vec3) -> Self {
        Self { position, yaw: 0.0, scale: 1.0, head: 0.0, ground: position.y }
    }
}

/// The current camera, captured when a job starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraState {
    pub eye: Vec3,
    pub look: Vec3,
    pub fov: f32,
}

/// The pose a job asks for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraPose {
    pub eye: Vec3,
    pub look: Vec3,
    /// Radians about the view axis.
    pub roll: f32,
    pub fov: f32,
}

impl CameraPose {
    pub fn transform(&self) -> Transform {
        let mut t = Transform::from_translation(self.eye);
        let dir = (self.look - self.eye).normalize_or(Vec3::NEG_Z);
        let up = if dir.cross(Vec3::Y).length_squared() < 1e-6 { Vec3::Z } else { Vec3::Y };
        t.look_to(dir, up);
        if self.roll != 0.0 {
            t.rotate_local_z(self.roll);
        }
        t
    }
}

/// Interpolated scalars of one job (0xEB31A0 / lerp structs).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Scalars {
    heading: f32,
    pitch: f32,
    roll: f32,
    distance: f32,
    fov: f32,
}

/// One running camera job.
#[derive(Clone, Debug)]
pub struct CameraJob {
    pub spec: CameraSpec,
    pub elapsed: f32,
    /// Blocks the state until done (cleared when the state advances).
    pub blocking: bool,
    start: Scalars,
    start_target: Vec3,
    /// Eye at the job start (anchor of `-anchored`).
    anchor: Vec3,
    /// `-backToStart`: the camera at cinematic start.
    back_to: Option<CameraState>,
    /// Last evaluated values (done check, follow-up jobs).
    current: Scalars,
    current_target: Vec3,
    desired: Scalars,
    desired_target: Vec3,
    /// Target point frozen after the move unless `-follow`.
    frozen_target: Option<Vec3>,
}

impl CameraJob {
    /// Start a job from the current camera. `frame` is the target resolved
    /// now; `cinematic_start` is the camera captured when the cinematic began.
    pub fn new(spec: CameraSpec, now: CameraState, frame: TargetFrame, cinematic_start: Option<CameraState>) -> Self {
        let goal = target_point(&spec, &frame);
        let (h, p, d) = orbit_angles(now.eye - now.look);
        let base_heading = if spec.orient || spec.orient_flat { frame.yaw } else { 0.0 };
        let start = match spec.mode {
            CameraMode::Anchored => Scalars { heading: 0.0, pitch: 0.0, roll: 0.0, distance: 0.0, fov: now.fov },
            _ => Scalars { heading: h - base_heading, pitch: p, roll: 0.0, distance: d, fov: now.fov },
        };
        let mut job = CameraJob {
            blocking: spec.blocking,
            back_to: if spec.back_to_start { cinematic_start } else { None },
            spec,
            elapsed: 0.0,
            start,
            start_target: now.look,
            anchor: now.eye,
            current: start,
            current_target: now.look,
            desired: start,
            desired_target: goal,
            frozen_target: None,
        };
        job.desired = job.desired_scalars(&frame);
        job
    }

    fn desired_scalars(&self, frame: &TargetFrame) -> Scalars {
        let s = &self.spec;
        let fov = if s.fov > 0.0 { s.fov } else { self.start.fov };
        match s.mode {
            CameraMode::Anchored => Scalars {
                heading: 0.0,
                pitch: 0.0,
                roll: 0.0,
                distance: s.offset * frame.scale,
                fov,
            },
            _ => {
                let mut distance = s.offset * frame.scale;
                if distance == 0.0 && s.target.is_some() {
                    // 0xEB43F0 case 4: no distance with a target -> 10.
                    distance = 10.0;
                }
                Scalars { heading: s.heading * TAU, pitch: s.pitch * TAU, roll: s.roll * TAU, distance, fov }
            }
        }
    }

    /// Normalised time with the job's easing (`-snap` jumps to the end).
    pub fn progress(&self) -> f32 {
        if self.spec.snap || self.spec.duration <= 0.0 {
            return 1.0;
        }
        // Duration in whole milliseconds (0xEB31A0).
        let ms = (self.spec.duration * 1000.0).round().max(1.0);
        let t = (self.elapsed * 1000.0 / ms).clamp(0.0, 1.0);
        if self.spec.ease {
            ease(t)
        } else {
            t
        }
    }

    pub fn step(&mut self, dt: f32) {
        self.elapsed += dt.max(0.0);
    }

    /// Evaluate the pose for this frame. `ground(x, z)` is the terrain/water
    /// height for the eye clamp.
    pub fn pose(&mut self, frame: &TargetFrame, ground: impl Fn(f32, f32) -> f32) -> CameraPose {
        let e = self.progress();
        let live_goal = target_point(&self.spec, frame);
        let goal = match self.frozen_target {
            Some(p) => p,
            None => live_goal,
        };
        self.desired = self.desired_scalars(frame);
        self.desired_target = goal;
        if let Some(back) = self.back_to {
            // `-backToStart`: return to the camera of the cinematic start.
            let eye = self.anchor.lerp(back.eye, e);
            let look = self.start_target.lerp(back.look, e);
            let fov = lerp(self.start.fov, back.fov, e);
            self.current = Scalars { heading: 0.0, pitch: 0.0, roll: 0.0, distance: 0.0, fov };
            self.desired = self.current;
            self.current_target = look;
            self.desired_target = look;
            if e >= 1.0 {
                self.desired_target = back.look;
            }
            return CameraPose { eye, look, roll: 0.0, fov };
        }
        if self.spec.mode == CameraMode::Path && !self.spec.path.is_empty() {
            // `pathCamera`: the eye runs along the spline through the points
            // (offsets from the target object), the look point is the target.
            let fov = lerp(self.start.fov, self.desired.fov, e);
            self.current = Scalars { heading: 0.0, pitch: 0.0, roll: 0.0, distance: 0.0, fov };
            self.desired = self.current;
            self.current_target = goal;
            let eye = frame.position + path_point(&self.spec.path, e);
            return CameraPose { eye, look: goal, roll: 0.0, fov };
        }
        let target = self.start_target.lerp(goal, e);
        let current = Scalars {
            heading: lerp_angle(self.start.heading, self.desired.heading, e),
            pitch: lerp_angle(self.start.pitch, self.desired.pitch, e),
            roll: lerp_angle(self.start.roll, self.desired.roll, e),
            distance: lerp(self.start.distance, self.desired.distance, e),
            fov: lerp(self.start.fov, self.desired.fov, e),
        };
        self.current = current;
        self.current_target = target;
        if e >= 1.0 && !self.spec.follow && self.frozen_target.is_none() {
            self.frozen_target = Some(goal);
        }
        match self.spec.mode {
            CameraMode::Anchored => {
                let dir = (target - self.anchor).normalize_or(Vec3::NEG_Z);
                let eye = self.anchor + dir * current.distance;
                CameraPose { eye, look: target, roll: 0.0, fov: current.fov }
            }
            _ => {
                let base = if self.spec.orient || self.spec.orient_flat { frame.yaw } else { 0.0 };
                let mut eye = target + orbit_offset(current.heading + base, current.pitch, current.distance);
                // Keep the eye above the terrain / water (0xEB43F0 case 1).
                let floor = ground(eye.x, eye.z) + 0.1;
                if eye.y < floor {
                    eye.y = floor;
                }
                CameraPose { eye, look: target, roll: current.roll, fov: current.fov }
            }
        }
    }

    /// Still moving (0xEB4010).
    pub fn transitioning(&self) -> bool {
        if self.progress() >= 1.0 {
            return false;
        }
        if self.spec.mode == CameraMode::Path && !self.spec.path.is_empty() {
            return true;
        }
        let (c, d) = (&self.current, &self.desired);
        wrap(c.heading - d.heading).abs() > 0.15
            || wrap(c.pitch - d.pitch).abs() > 0.15
            || wrap(c.roll - d.roll).abs() > 0.15
            || (c.distance - d.distance).abs() > 0.0225
            || self.current_target.distance_squared(self.desired_target) > 0.0225
    }

    /// The look point now (a follow-up job starts from it).
    pub fn look(&self) -> Vec3 {
        self.current_target
    }
}

/// A point of a `pathCamera` path at `t` (0..1), Bevy frame. The spline
/// set-up (0xEB6FD0) gives point i the knot i / (n - 1); INFERRED: a uniform
/// Catmull-Rom curve through the points (the ease is in the point spacing,
/// GGEIntro.trigger "ease in").
pub fn path_point(points: &[[f32; 3]], t: f32) -> Vec3 {
    let p: Vec<Vec3> = points.iter().map(|v| spore_to_bevy(*v)).collect();
    match p.len() {
        0 => Vec3::ZERO,
        1 => p[0],
        n => {
            let u = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let i = (u.floor() as usize).min(n - 2);
            let f = u - i as f32;
            let p0 = p[i.saturating_sub(1)];
            let (p1, p2) = (p[i], p[i + 1]);
            let p3 = p[(i + 2).min(n - 1)];
            let (f2, f3) = (f * f, f * f * f);
            0.5 * (2.0 * p1 + (p2 - p0) * f + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f2 + (3.0 * p1 - p0 - 3.0 * p2 + p3) * f3)
        }
    }
}

/// Target point of a job: object origin (+ head / terrain height) + target
/// offset in the object's frame + extra offset, scaled by `frame.scale`.
pub fn target_point(spec: &CameraSpec, frame: &TargetFrame) -> Vec3 {
    let mut base = frame.position;
    match spec.aim {
        Aim::Origin => {}
        Aim::Head => base.y += frame.head,
        Aim::Terrain => base.y = frame.ground,
    }
    let local = spore_to_bevy(spec.target_offset) * frame.scale;
    let extra = spore_to_bevy(spec.extra_target_offset) * frame.scale;
    base + Quat::from_rotation_y(frame.yaw) * local + extra
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn path_camera_runs_through_its_points() {
        let pts = [[0.0, 0.0, 10.0], [0.0, -10.0, 5.0], [0.0, -20.0, 0.0]];
        assert!(path_point(&pts, 0.0).distance(spore_to_bevy(pts[0])) < 1e-4);
        assert!(path_point(&pts, 0.5).distance(spore_to_bevy(pts[1])) < 1e-4);
        assert!(path_point(&pts, 1.0).distance(spore_to_bevy(pts[2])) < 1e-4);
        // Two equal points hold the camera (GGE_intro's first state).
        let hold = [[1.0, 2.0, 3.0], [1.0, 2.0, 3.0]];
        assert!(path_point(&hold, 0.4).distance(spore_to_bevy(hold[0])) < 1e-4);
        // The job's eye is the target plus the path point; it stays busy
        // until its duration ends.
        let spec = CameraSpec { mode: CameraMode::Path, path: pts.to_vec(), duration: 2.0, ..default() };
        let now = CameraState { eye: Vec3::new(0.0, 5.0, 5.0), look: Vec3::ZERO, fov: 0.8 };
        let frame = TargetFrame::at(Vec3::new(1.0, 0.0, 0.0));
        let mut job = CameraJob::new(spec, now, frame, None);
        job.step(1.0);
        let pose = job.pose(&frame, |_, _| -1e6);
        assert!(pose.eye.distance(Vec3::new(1.0, 0.0, 0.0) + spore_to_bevy(pts[1])) < 1e-3);
        assert!(pose.look.distance(frame.position) < 1e-4);
        assert!(job.transitioning());
        job.step(1.0);
        let _ = job.pose(&frame, |_, _| -1e6);
        assert!(!job.transitioning());
    }

    #[test]
    fn ease_is_the_retail_trapezoid() {
        assert_eq!(ease(0.0), 0.0);
        assert!(close(ease(0.25), 1.0 / 6.0));
        assert!(close(ease(0.5), 0.5));
        assert!(close(ease(0.75), 5.0 / 6.0));
        assert!(close(ease(1.0), 1.0));
        assert!(close(ease(2.0), 1.0));
        // Continuous and monotonic.
        let mut last = 0.0;
        for i in 1..=100 {
            let v = ease(i as f32 / 100.0);
            assert!(v >= last);
            last = v;
        }
    }

    #[test]
    fn angles_take_the_short_way() {
        assert!(close(lerp_angle(0.1, TAU - 0.1, 0.5), 0.0));
        assert!(close(wrap(3.0 * PI).abs(), PI));
        assert!(close(wrap(-1.5 * PI), 0.5 * PI));
        assert!(close(lerp_angle(-3.0, 3.0, 1.0), -3.0 - (TAU - 6.0)));
    }

    #[test]
    fn orbit_heading_zero_is_behind_and_half_a_turn_in_front() {
        // Creature front is Bevy -Z: heading 0 puts the eye behind (+Z).
        let behind = orbit_offset(0.0, 0.0, 6.0);
        assert!(close(behind.z, 6.0) && close(behind.x, 0.0) && close(behind.y, 0.0));
        let front = orbit_offset(0.5 * TAU, 0.0, 6.0);
        assert!(close(front.z, -6.0));
        // Negative pitch looks down from above (-0.05 turn = -18 degrees).
        let above = orbit_offset(0.0, -0.05 * TAU, 6.0);
        assert!(above.y > 0.0 && close(above.y, 6.0 * (0.05 * TAU).sin()));
        for (h, p) in [(0.3, -0.2), (-2.0, 0.4), (1.0, 0.0)] {
            let (h2, p2, d) = orbit_angles(orbit_offset(h, p, 4.0));
            assert!(close(wrap(h - h2), 0.0) && close(p, p2) && close(d, 4.0), "{h} {p} -> {h2} {p2}");
        }
    }

    #[test]
    fn spore_vectors_map_to_bevy() {
        assert_eq!(spore_to_bevy([1.0, 2.0, 3.0]), Vec3::new(1.0, 3.0, -2.0));
        assert_eq!(bevy_to_spore(spore_to_bevy([1.0, 2.0, 3.0])), [1.0, 2.0, 3.0]);
        assert!(close(yaw_of(Quat::from_rotation_y(0.7)), 0.7));
    }

    #[test]
    fn target_offset_turns_with_the_object() {
        let spec = CameraSpec { target_offset: [0.0, 1.0, 0.5], ..default() };
        let frame = TargetFrame { yaw: 0.5 * PI, ..TargetFrame::at(Vec3::new(1.0, 0.0, 0.0)) };
        // Spore +y (forward) -> Bevy -Z, turned a quarter to the left -> -X.
        let p = target_point(&spec, &frame);
        assert!(close(p.x, 0.0) && close(p.y, 0.5) && close(p.z, 0.0), "{p}");
        let terrain = CameraSpec { aim: Aim::Terrain, ..default() };
        let f = TargetFrame { ground: -0.3, ..TargetFrame::at(Vec3::new(0.0, -2.0, 0.0)) };
        assert!(close(target_point(&terrain, &f).y, -0.3));
    }

    fn state() -> CameraState {
        CameraState { eye: Vec3::new(0.0, 10.0, 20.0), look: Vec3::ZERO, fov: 0.8 }
    }

    #[test]
    fn snap_jumps_to_the_orbit_pose() {
        let spec = CameraSpec {
            target: Some(1),
            offset: 6.0,
            heading: 0.5,
            pitch: -0.05,
            fov: 0.6,
            orient: true,
            snap: true,
            ..default()
        };
        let frame = TargetFrame { yaw: 0.0, scale: 1.0, ..TargetFrame::at(Vec3::new(5.0, 0.0, 5.0)) };
        let mut job = CameraJob::new(spec, state(), frame, None);
        let pose = job.pose(&frame, |_, _| -100.0);
        assert!(close(pose.fov, 0.6));
        assert!(close(pose.eye.distance(Vec3::new(5.0, 0.0, 5.0)), 6.0));
        // In front of a creature facing -Z: the eye is at smaller Z.
        assert!(pose.eye.z < 5.0 && pose.eye.y > 0.0);
        assert!(!job.transitioning());
    }

    #[test]
    fn eased_move_blocks_until_close_enough() {
        let spec = CameraSpec { target: Some(1), offset: 6.0, duration: 2.0, ease: true, ..default() };
        let frame = TargetFrame::at(Vec3::new(3.0, 0.0, 0.0));
        let mut job = CameraJob::new(spec, state(), frame, None);
        job.pose(&frame, |_, _| -100.0);
        assert!(job.transitioning());
        job.step(1.0);
        let mid = job.pose(&frame, |_, _| -100.0);
        assert!(job.transitioning());
        // Half way (ease 0.5): the target point is half way to (3, 0, 0).
        assert!(close(mid.look.x, 1.5), "{mid:?}");
        job.step(1.0);
        job.pose(&frame, |_, _| -100.0);
        assert!(!job.transitioning());
        // The eye never goes under the ground.
        let mut low = CameraJob::new(CameraSpec { target: Some(1), offset: 6.0, pitch: 0.2, snap: true, ..default() }, state(), frame, None);
        let pose = low.pose(&frame, |_, _| 1.0);
        assert!(pose.eye.y >= 1.1 - 1e-4);
    }

    #[test]
    fn anchored_turns_in_place_and_dollies() {
        let spec = CameraSpec { target: Some(1), mode: CameraMode::Anchored, offset: -15.0, snap: true, ..default() };
        let frame = TargetFrame::at(Vec3::new(10.0, 0.0, 20.0));
        let mut job = CameraJob::new(spec, state(), frame, None);
        let pose = job.pose(&frame, |_, _| -100.0);
        let dir = (Vec3::new(10.0, 0.0, 20.0) - Vec3::new(0.0, 10.0, 20.0)).normalize();
        assert!(pose.eye.distance(Vec3::new(0.0, 10.0, 20.0) - dir * 15.0) < 1e-3);
        assert_eq!(pose.look, Vec3::new(10.0, 0.0, 20.0));
    }

    #[test]
    fn back_to_start_returns_to_the_captured_camera() {
        let start = CameraState { eye: Vec3::new(1.0, 2.0, 3.0), look: Vec3::new(0.0, 0.0, 0.0), fov: 0.7 };
        let spec = CameraSpec { back_to_start: true, duration: 0.35, ..default() };
        let mut job = CameraJob::new(spec, state(), TargetFrame::at(Vec3::ZERO), Some(start));
        job.step(0.35);
        let pose = job.pose(&TargetFrame::at(Vec3::ZERO), |_, _| -100.0);
        assert!(pose.eye.distance(start.eye) < 1e-4 && close(pose.fov, 0.7));
    }
}
