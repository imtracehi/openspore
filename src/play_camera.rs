//! Orbit-camera state for the land stages.
//!
//! Creature: the retail rig (`CreatureRig`). The values are those of
//! `CreatureGame!CreatureCamera` (PROP 0xAD56080C!0xD03A25CF) and, since the
//! creature-cam2 pass, the way the camera class (0xD20000-0xD2B000: update
//! 0xD2A630, focus 0xD27300, follow 0xD22300, keys 0xD2A980, easing 0xD26790,
//! breathing 0xD20C70, shake 0xD22F30) applies them; see
//! `creature-cam2\CHANGES.md`. What could not be read from the code is marked
//! `ADAPTED` / `INFERRED`.
//!
//! Tribal and Civilization keep OpenSpore's own orbit presets (adapted, not
//! native camera data): their offsets mirror the earlier camera-follow defaults.

use crate::{editor::EditorState, stages::GameStage};
use bevy::{input::mouse::*, prelude::*};

const TRIBAL_OFFSET: Vec3 = Vec3::new(0.0, 17.0, 14.0);
const CIVILIZATION_OFFSET: Vec3 = Vec3::new(0.0, 34.0, 25.0);

/// Adapted input tuning of the Tribal and Civilization orbit presets.
const MOUSE_RADIANS_PER_PIXEL: f32 = 0.006;
const ZOOM_LOG_SCALE_PER_LINE: f32 = 0.12;
const MIN_PITCH: f32 = 8.0_f32.to_radians();
const MAX_PITCH: f32 = 78.0_f32.to_radians();
const MIN_DISTANCE_FACTOR: f32 = 0.55;
const MAX_DISTANCE_FACTOR: f32 = 2.75;
const MAX_MOUSE_DELTA_PIXELS: f32 = 4096.0;
const MAX_SCROLL_LINES_PER_FRAME: f32 = 64.0;
const TAU: f32 = std::f32::consts::TAU;

/// Original Creature camera values (`CreatureGame!CreatureCamera`), property
/// names in the comments. Some values are data the retail code never reads
/// (see their notes); they stay documented here.
#[allow(dead_code)]
pub mod tuning {
    /// cameraInitialDistance / cameraMinDistance / cameraMaxDistance
    pub const INITIAL_DISTANCE: f32 = 15.0;
    pub const MIN_DISTANCE: f32 = 5.0;
    pub const MAX_DISTANCE: f32 = 25.0;
    /// cameraMouseSensitivityX / Y (middle or right button drag) and Z (per wheel unit)
    pub const MOUSE_SENSITIVITY_X: f32 = 0.7;
    pub const MOUSE_SENSITIVITY_Y: f32 = 0.4;
    pub const MOUSE_SENSITIVITY_Z: f32 = 0.0006;
    /// The property loader (0xD24D3A / 0xD24D7A) multiplies X and Y by
    /// 0.003490658 rad (0.2 degrees): one retail unit is 0.2 degrees a pixel.
    pub const MOUSE_RADIANS_PER_UNIT: f32 = 0.003_490_658;
    /// Free-pitch limits (cameraAutoPitchCloseMin/Max, FarMin/Max, degrees).
    /// With cameraEnableFreePitch 1 (the shipped data) the camera applies none
    /// of them to the player's pitch (0xD21810 passes the pitch through); the
    /// zoom-dependent clamp `D20700` only runs when the camera snaps to the
    /// avatar (0xD221DA). Kept for that snap.
    pub const PITCH_CLOSE_MIN_DEG: f32 = -15.0;
    pub const PITCH_CLOSE_MAX_DEG: f32 = 15.0;
    pub const PITCH_FAR_MIN_DEG: f32 = -30.0;
    pub const PITCH_FAR_MAX_DEG: f32 = 85.0;
    /// cameraAutoPitchCloseStd / FarStd: the pitch the `/` key returns to
    /// (0xD20620: smoothstep between them by zoom).
    pub const PITCH_CLOSE_STD_DEG: f32 = 20.0;
    pub const PITCH_FAR_STD_DEG: f32 = 30.0;
    /// The camera's absolute pitch range (object +0x208 / +0x20c, set by the
    /// constructor 0xD28F6F from .data 0x1582868 / 0x158286C; the camera prop
    /// has no override): the mouse drag and the PageUp / PageDown keys stop
    /// here, -25 .. 50 degrees.
    pub const PITCH_MIN_DEG: f32 = -25.0;
    pub const PITCH_MAX_DEG: f32 = 50.0;
    /// openingMoment_StartingCameraAngle (CreatureGameTuning): the view the
    /// stage starts in.
    pub const START_PITCH_DEG: f32 = 12.0;
    /// cameraApproachTime / cameraVelocityDecayTime / cameraMaxAcceleration:
    /// loaded (0x1582760-68) but read by no code of the image (VERIFIED: no
    /// reference to those addresses besides the loader).
    pub const APPROACH_TIME: f32 = 0.4;
    /// cameraDeadZone and cameraDeadZoneBuffer, cameraMaxDistFromAnchor,
    /// cameraMinLerp / MaxLerp: only read by the dead-zone filter 0xD209B0,
    /// whose result is overwritten by `smoothed = focus` at its end (0xD20C3B),
    /// so the focus point follows the avatar rigidly. Kept as data.
    pub const DEAD_ZONE: f32 = 0.4;
    pub const DEAD_ZONE_BUFFER: f32 = 0.4;
    pub const MAX_DIST_FROM_ANCHOR: f32 = 2.5;
    /// minAvatarVelocityForAutoCamRot: the camera swings toward the travel
    /// direction only above this ground speed (0xD22DC6 compares the squared
    /// speed with its square).
    pub const MIN_SPEED_FOR_AUTO_ROTATION: f32 = 0.65;
    /// cameraAboveTerrainBuffer
    pub const ABOVE_TERRAIN: f32 = 0.5;
    /// cameraInterpolateDistanceTime: the current distance closes on the goal
    /// as `1 - 2^(-0.4343 dt / T)` = `1 - exp(-0.30103 dt / T)` (0xD26790; the
    /// azimuth and pitch have no easing in the follow mode: current = goal).
    pub const INTERPOLATE_TIME: f32 = 0.05;
    /// Rate (1/s) of that exponential.
    pub const DISTANCE_EASE_RATE: f32 = 0.301_03 / INTERPOLATE_TIME;
    /// Key rates (0xD2A980): `,` `.` turn the goal azimuth by pi/2 a second
    /// (.data 0x158280C); `=` `-` scale the goal distance by `1 -/+ dt`;
    /// PageUp / PageDown close on the pitch limit with `1 - exp(-0.30103 dt / 0.3)`
    /// (.data 0x15827EC = 0.3).
    pub const KEY_ROTATE_RATE: f32 = std::f32::consts::FRAC_PI_2;
    pub const KEY_PITCH_TIME: f32 = 0.3;
    /// cameraWASDRotateSpeed: only read by the first-person mode's heading
    /// follow (0xD2A79D), not by the `,` `.` keys.
    pub const WASD_ROTATE_SPEED: f32 = 5.0;
    /// rampOffsetStartDistance and avatarCenterCloseUpDistFromCenter (focus
    /// height, 0xD27300).
    pub const RAMP_OFFSET_START_DISTANCE: f32 = 1.3;
    pub const CLOSE_UP_DIST_FROM_CENTER: f32 = 0.5;
    /// avatarMovingAutoPitchClose / Far (degrees, 0xD22E07) and its time.
    pub const MOVING_PITCH_CLOSE_DEG: (f32, f32) = (4.0, 10.0);
    pub const MOVING_PITCH_FAR_DEG: (f32, f32) = (4.0, 20.0);
    pub const MOVING_PITCH_TIME: f32 = 0.9;
    /// cameraToMouseMinAngle / MaxAngle (degrees): the swing toward the travel
    /// direction only runs while the angle to it is inside this window.
    pub const TO_MOUSE_MIN_DEG: f32 = 10.0;
    pub const TO_MOUSE_MAX_DEG: f32 = 150.0;
    /// cameraFollowMouseRotateSpeed (x dt, at most 1) and the ramp limits
    /// per frame (.data 0x15827DC / 0x15827E0) and the per-frame step cap
    /// (.data 0x15827AC).
    pub const FOLLOW_ROTATE_SPEED: f32 = 45.0;
    pub const FOLLOW_RAMP_UP: f32 = 0.0012;
    pub const FOLLOW_RAMP_DOWN: f32 = 0.009;
    pub const FOLLOW_STEP_CAP: f32 = 0.1;
    /// Breathing (0xD20C70): speed, frequency and amplitude ranges.
    pub const BREATH_SPEED: (f32, f32) = (1.0, 4.0);
    pub const BREATH_FREQUENCY: (f32, f32) = (0.5, 0.7);
    pub const BREATH_AMPLITUDE: (f32, f32) = (0.0, 0.005);
    /// Shaking (0xD22F30): speed, frequency, amplitude and roll factor.
    pub const SHAKE_SPEED: (f32, f32) = (1.0, 7.5);
    pub const SHAKE_FREQUENCY: f32 = 20.0;
    pub const SHAKE_AMPLITUDE: (f32, f32) = (0.0, 0.03);
    pub const SHAKE_ROLL: f32 = 0.03;
}

/// Raw wheel units per notch (Windows WHEEL_DELTA) for cameraMouseSensitivityZ.
const RAW_WHEEL_UNITS_PER_LINE: f32 = 120.0;
/// ADAPTED: the standing body height assumed for the focus point when the
/// drawn body is not measured yet (0xD27300 uses the locomotive's bounding box
/// height; 1.3 per unit of avatar scale reproduces the earlier 0.65 anchor).
const DEFAULT_BODY_HEIGHT: f32 = 1.3;
/// The focus height factor of the avatar state flag (0xC0B8D0: 1.0 when the
/// avatar's field +0xB34 is 1, else the .data 0x15827BC default 0.5). ADAPTED:
/// the field is not mapped, 0.5 is used.
const FOCUS_STATE_FACTOR: f32 = 0.5;
/// Seconds of continuous travel before the moving auto-pitch starts (0xD22DDF
/// compares the held-move timer with 2.0). ADAPTED: retail counts the held
/// mouse steering or an avatar goal; here the ground speed above the swing
/// threshold counts.
const MOVING_PITCH_DELAY: f32 = 2.0;
/// Per-frame values of the swing are given for 60 Hz; scaled by `dt * 60`.
const FRAME_RATE: f32 = 60.0;
/// Per-frame displacement treated as a teleport/respawn, not travel.
const FOLLOW_MAX_STEP: f32 = 2.0;
/// First-person eye height above the avatar origin and forward offset, per
/// unit of avatar scale. ADAPTED.
const EYE_UP: f32 = 0.45;
const EYE_FORWARD: f32 = 0.35;
const FIRST_PERSON_PITCH_LIMIT: f32 = 60.0_f32.to_radians();
const FIRST_PERSON_YAW_LIMIT: f32 = 120.0_f32.to_radians();

/// The retail Creature camera: orbit angles and distance, a focus point that
/// follows the avatar rigidly, and the retail swing / auto-pitch / breathing /
/// shake dynamics.
///
/// Angles follow the offset convention of [`PlayCamera::offset`]: the camera
/// sits at `anchor + (sin(az) cos(p), sin(p), cos(az) cos(p)) * distance`, so
/// azimuth 0 is behind an avatar facing -Z and an avatar of yaw `y` has the
/// camera behind it at azimuth `y`.
#[derive(Clone, Debug)]
pub struct CreatureRig {
    azimuth: f32,
    pitch: f32,
    distance: f32,
    goal_azimuth: f32,
    goal_pitch: f32,
    goal_distance: f32,
    anchor: Vec3,
    anchor_ready: bool,
    last_avatar: Option<Vec3>,
    avatar_speed: f32,
    avatar_yaw: f32,
    first_person: bool,
    look_yaw: f32,
    look_pitch: f32,
    /// The local up at the avatar (the planet's radial direction; +Y on the
    /// island): the orbit is built around it.
    up: Vec3,
    /// Measured standing height of the drawn body (the focus height).
    body_height: Option<f32>,
    /// Retail flag 0x169DE85: a key rotated or pitched the camera; cleared
    /// when the avatar's position changes.
    manual: bool,
    /// A middle / right button drag happened this frame (retail +0x314).
    dragging: bool,
    /// Seconds the avatar has been travelling above the swing speed.
    moving_time: f32,
    /// Retail +0x23C: the moving auto-pitch is pulling the goal pitch in.
    pitch_auto: bool,
    /// Retail 0x169DE8C: the swing's ramped fraction of the remaining angle.
    swing_ramp: f32,
    /// `/` is easing the camera back behind the avatar.
    resetting: bool,
    /// Camera time (+0x20), breathing offset (+0x24), shake offset in camera
    /// (right, up) axes, roll (+0x34).
    time: f32,
    breath: f32,
    shake: Vec2,
    roll: f32,
}

impl Default for CreatureRig {
    fn default() -> Self {
        Self::new()
    }
}

/// `smoothstep` of `t` clamped to 0..1.
fn smooth01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Zoom position of `distance`: 0 at the closest, 1 at the farthest.
pub fn zoom_fraction(distance: f32) -> f32 {
    ((distance - tuning::MIN_DISTANCE) / (tuning::MAX_DISTANCE - tuning::MIN_DISTANCE))
        .clamp(0.0, 1.0)
}

/// Pitch limits (radians) at a camera distance, smoothstep between the close
/// (-15..15) and far (-30..85) ranges of the original (0xD20700). Only the
/// camera snap applies them (see `tuning::PITCH_CLOSE_MIN_DEG`).
#[allow(dead_code)]
pub fn pitch_limits(distance: f32) -> (f32, f32) {
    let t = smooth01(zoom_fraction(distance));
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    (
        lerp(tuning::PITCH_CLOSE_MIN_DEG, tuning::PITCH_FAR_MIN_DEG).to_radians(),
        lerp(tuning::PITCH_CLOSE_MAX_DEG, tuning::PITCH_FAR_MAX_DEG).to_radians(),
    )
}

/// The standard pitch (radians) at a camera distance (0xD20620): 20 degrees
/// at the closest zoom, 30 at the farthest, smoothstep between. The `/` key
/// returns the pitch to it.
pub fn standard_pitch(distance: f32) -> f32 {
    let t = smooth01(zoom_fraction(distance));
    (tuning::PITCH_CLOSE_STD_DEG + (tuning::PITCH_FAR_STD_DEG - tuning::PITCH_CLOSE_STD_DEG) * t)
        .to_radians()
}

/// The moving auto-pitch window (radians) at a camera distance (0xD22E24):
/// `Far + (Close - Far) * f`, `f = (max - distance) / max`.
pub fn moving_pitch_window(distance: f32) -> (f32, f32) {
    let f = ((tuning::MAX_DISTANCE - distance) / tuning::MAX_DISTANCE).clamp(0.0, 1.0);
    let (close, far) = (tuning::MOVING_PITCH_CLOSE_DEG, tuning::MOVING_PITCH_FAR_DEG);
    (
        (far.0 + (close.0 - far.0) * f).to_radians(),
        (far.1 + (close.1 - far.1) * f).to_radians(),
    )
}

/// The absolute pitch range of the camera (radians).
fn pitch_range() -> (f32, f32) {
    (tuning::PITCH_MIN_DEG.to_radians(), tuning::PITCH_MAX_DEG.to_radians())
}

/// Speed -> 0..1 smoothstep weight used by the breathing and the shaking.
fn speed_weight(speed: f32, range: (f32, f32)) -> f32 {
    let s = speed.clamp(range.0, range.1);
    smooth01((s - range.0) / (range.1 - range.0))
}

// Some accessors are only read by the tests and by later HUD or capture code.
#[allow(dead_code)]
impl CreatureRig {
    pub fn new() -> Self {
        let pitch = tuning::START_PITCH_DEG.to_radians();
        Self {
            azimuth: 0.0,
            pitch,
            distance: tuning::INITIAL_DISTANCE,
            goal_azimuth: 0.0,
            goal_pitch: pitch,
            goal_distance: tuning::INITIAL_DISTANCE,
            anchor: Vec3::ZERO,
            anchor_ready: false,
            last_avatar: None,
            avatar_speed: 0.0,
            avatar_yaw: 0.0,
            first_person: false,
            look_yaw: 0.0,
            look_pitch: 0.0,
            up: Vec3::Y,
            body_height: None,
            manual: false,
            dragging: false,
            moving_time: 0.0,
            pitch_auto: false,
            swing_ramp: 0.0,
            resetting: false,
            time: 0.0,
            breath: 0.0,
            shake: Vec2::ZERO,
            roll: 0.0,
        }
    }

    /// The local up the orbit is built around.
    pub fn up(&self) -> Vec3 {
        self.up
    }

    /// Set the local up at the avatar (`creature_planet::reanchor`, every
    /// frame on the planet). The orbit offset and the look-at up turn from +Y
    /// to it, so the camera keeps its pitch over the curved ground.
    pub fn set_up(&mut self, up: Vec3) {
        if up.is_finite() && up.length_squared() > 0.5 {
            self.up = up.normalize();
        }
    }

    /// The avatar's measured standing height (the focus point's height).
    pub fn set_body_height(&mut self, height: Option<f32>) {
        self.body_height = height.filter(|h| h.is_finite() && *h > 0.0);
    }

    /// Move the rig's world positions with the planet (floating origin).
    pub fn shift(&mut self, map: impl Fn(Vec3) -> Vec3) {
        self.anchor = map(self.anchor);
        self.last_avatar = self.last_avatar.map(&map);
    }

    /// Back to the stage-start view at once, forgetting the anchor.
    pub fn restart(&mut self) {
        *self = Self::new();
    }

    /// `/` (key action 2, 0xD2ABD0): the azimuth turns back behind the avatar
    /// (pi/2 a second) and the pitch to the standard pitch of the zoom (at
    /// most |min| down / max up a second); the distance stays.
    pub fn reset_view(&mut self) {
        self.resetting = true;
        self.look_yaw = 0.0;
        self.look_pitch = 0.0;
    }

    pub fn azimuth(&self) -> f32 {
        self.azimuth
    }
    pub fn pitch(&self) -> f32 {
        self.pitch
    }
    pub fn distance(&self) -> f32 {
        self.distance
    }
    pub fn goal_distance(&self) -> f32 {
        self.goal_distance
    }
    pub fn goal_pitch(&self) -> f32 {
        self.goal_pitch
    }
    pub fn goal_azimuth(&self) -> f32 {
        self.goal_azimuth
    }
    pub fn anchor(&self) -> Vec3 {
        self.anchor
    }
    pub fn avatar_speed(&self) -> f32 {
        self.avatar_speed
    }
    pub fn is_first_person(&self) -> bool {
        self.first_person
    }
    /// Breathing offset along the view axis, shake offset (right, up) and
    /// roll (radians), for tests and captures.
    pub fn motion(&self) -> (f32, Vec2, f32) {
        (self.breath, self.shake, self.roll)
    }

    pub fn set_first_person(&mut self, on: bool) {
        if self.first_person != on {
            self.first_person = on;
            self.look_yaw = 0.0;
            self.look_pitch = 0.0;
        }
    }

    /// Offset of the camera from the anchor (eased values).
    pub fn orbit_offset(&self) -> Vec3 {
        let horizontal = self.distance * self.pitch.cos();
        Vec3::new(
            self.azimuth.sin() * horizontal,
            self.pitch.sin() * self.distance,
            self.azimuth.cos() * horizontal,
        )
    }

    /// Mouse drag with the middle or right button, in pixels (0xD25B72: the
    /// azimuth moves by `-dx * X`, the pitch by `dy * Y`, X and Y in radians a
    /// pixel; the pitch stops at the absolute range).
    pub fn orbit_by_pixels(&mut self, delta: Vec2) {
        let x = tuning::MOUSE_SENSITIVITY_X * tuning::MOUSE_RADIANS_PER_UNIT;
        let y = tuning::MOUSE_SENSITIVITY_Y * tuning::MOUSE_RADIANS_PER_UNIT;
        if self.first_person {
            self.look_yaw = (self.look_yaw - delta.x * x)
                .clamp(-FIRST_PERSON_YAW_LIMIT, FIRST_PERSON_YAW_LIMIT);
            self.look_pitch = (self.look_pitch - delta.y * y)
                .clamp(-FIRST_PERSON_PITCH_LIMIT, FIRST_PERSON_PITCH_LIMIT);
            return;
        }
        self.dragging = true;
        self.resetting = false;
        self.goal_azimuth = wrap_angle(self.goal_azimuth - delta.x * x);
        self.tilt_by(delta.y * y);
    }

    /// Raise (positive) or lower the camera pitch inside the absolute range.
    fn tilt_by(&mut self, radians: f32) {
        let (low, high) = pitch_range();
        self.goal_pitch = (self.goal_pitch + radians).clamp(low, high);
    }

    /// Wheel motion in raw wheel units (0xD20571: the goal distance is
    /// multiplied by `1 - delta * cameraMouseSensitivityZ`, then clamped to
    /// the distance range); positive zooms in.
    pub fn zoom_by_raw(&mut self, raw: f32) {
        if !raw.is_finite() {
            return;
        }
        let factor = (1.0 - raw * tuning::MOUSE_SENSITIVITY_Z).max(0.05);
        self.goal_distance =
            (self.goal_distance * factor).clamp(tuning::MIN_DISTANCE, tuning::MAX_DISTANCE);
    }

    /// Wheel notches (1 line = 120 raw units); positive zooms in.
    pub fn zoom_by_lines(&mut self, lines: f32) {
        self.zoom_by_raw(lines * RAW_WHEEL_UNITS_PER_LINE);
    }

    /// `=`/`+` (positive) and `-` held for `dt` seconds (0xD2AA7A / 0xD2AA9B:
    /// the goal distance times `1 - dt` zooming in, `1 + dt` out).
    pub fn zoom_keys(&mut self, direction: f32, dt: f32) {
        let dt = dt.max(0.0);
        let factor = if direction > 0.0 { 1.0 - dt } else { 1.0 + dt };
        self.goal_distance = (self.goal_distance * factor.max(0.05))
            .clamp(tuning::MIN_DISTANCE, tuning::MAX_DISTANCE);
    }

    /// `.` (positive) rotates the camera right around the avatar, `,` left
    /// (0xD2A9CA / 0xD2AA59: pi/2 a second on the goal azimuth).
    pub fn rotate_keys(&mut self, direction: f32, dt: f32) {
        let step = tuning::KEY_ROTATE_RATE * dt * direction;
        if self.first_person {
            self.look_yaw =
                (self.look_yaw - step).clamp(-FIRST_PERSON_YAW_LIMIT, FIRST_PERSON_YAW_LIMIT);
            return;
        }
        self.goal_azimuth = wrap_angle(self.goal_azimuth + step);
        self.manual = true;
        self.resetting = false;
    }

    /// Turn the camera to `azimuth` (the minimap's click: look toward the
    /// point). The azimuth applies at once, as every azimuth change does.
    pub fn turn_to(&mut self, azimuth: f32) {
        if self.first_person || !azimuth.is_finite() {
            return;
        }
        self.goal_azimuth = wrap_angle(azimuth);
        self.manual = true;
        self.resetting = false;
    }

    /// PageUp (positive) / PageDown held for `dt` seconds (0xD2AAB8 /
    /// 0xD2AB44: the goal pitch closes on the pitch limit by
    /// `1 - exp(-0.30103 dt / 0.3)` of the gap).
    pub fn tilt_keys(&mut self, direction: f32, dt: f32) {
        if self.first_person {
            let step = tuning::KEY_ROTATE_RATE * dt * direction;
            self.look_pitch =
                (self.look_pitch + step).clamp(-FIRST_PERSON_PITCH_LIMIT, FIRST_PERSON_PITCH_LIMIT);
            return;
        }
        let (low, high) = pitch_range();
        let limit = if direction > 0.0 { high } else { low };
        let k = 1.0 - (-0.301_03 * dt.max(0.0) / tuning::KEY_PITCH_TIME).exp();
        self.goal_pitch = (self.goal_pitch + (limit - self.goal_pitch) * k).clamp(low, high);
        self.manual = true;
        self.resetting = false;
    }

    /// Apply the goals (0xD26790): the distance closes on its goal
    /// exponentially, the azimuth and the pitch are the goals themselves; the
    /// `/` reset moves the goals.
    pub fn ease(&mut self, dt: f32) {
        let dt = dt.max(0.0);
        if self.resetting {
            self.reset_step(dt);
        }
        let (low, high) = pitch_range();
        self.goal_distance = self
            .goal_distance
            .clamp(tuning::MIN_DISTANCE, tuning::MAX_DISTANCE);
        self.goal_pitch = self.goal_pitch.clamp(low, high);
        let k = 1.0 - (-tuning::DISTANCE_EASE_RATE * dt).exp();
        self.distance += (self.goal_distance - self.distance) * k;
        self.distance = self
            .distance
            .clamp(tuning::MIN_DISTANCE, tuning::MAX_DISTANCE);
        self.pitch = self.goal_pitch;
        self.azimuth = self.goal_azimuth;
    }

    /// One frame of the `/` reset (0xD2ABD0): the azimuth offset decays at
    /// pi/2 a second toward behind the avatar (retail snaps the heading to the
    /// avatar's and decays the offset; ADAPTED as one turn of the world
    /// azimuth), the pitch steps toward the standard pitch of the current zoom
    /// by at most `-|min| dt .. max dt`.
    fn reset_step(&mut self, dt: f32) {
        const EPS: f32 = 1.525_879e-5;
        let mut done = true;
        let cap = tuning::KEY_ROTATE_RATE * dt;
        let step = wrap_angle(self.avatar_yaw - self.goal_azimuth).clamp(-cap, cap);
        if step.abs() > EPS {
            self.goal_azimuth = wrap_angle(self.goal_azimuth + step);
            done = false;
        } else {
            self.goal_azimuth = self.avatar_yaw;
        }
        let (low, high) = pitch_range();
        let standard = standard_pitch(self.distance);
        let step = (standard - self.goal_pitch).clamp(low.abs() * -dt, high * dt);
        if step.abs() > EPS {
            self.goal_pitch += step;
            done = false;
        } else {
            self.goal_pitch = standard;
        }
        if done {
            self.resetting = false;
        }
    }

    /// The focus height above the avatar (0xD27300): `h` blends from the
    /// close value `0.5 H f + 0.5` to the far `0.5 H` by
    /// `clamp(distance / min - 1.3, 0, 1)`; `H` is the avatar's body height.
    fn focus_height(&self, scale: f32) -> f32 {
        let height = self
            .body_height
            .unwrap_or(DEFAULT_BODY_HEIGHT * scale.max(0.1));
        let far = 0.5 * height;
        let close = far * FOCUS_STATE_FACTOR + tuning::CLOSE_UP_DIST_FROM_CENTER;
        let t = (self.distance / tuning::MIN_DISTANCE - tuning::RAMP_OFFSET_START_DISTANCE)
            .clamp(0.0, 1.0);
        close + (far - close) * t
    }

    /// Follow the avatar: ground speed from its movement, the swing toward
    /// its travel direction (0xD22300), the moving auto-pitch (0xD22E07) and
    /// the rigid focus point.
    pub fn track(&mut self, avatar: &Transform, dt: f32) {
        let here = avatar.translation;
        if !here.is_finite() {
            return;
        }
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.time += dt;
        let previous = self.last_avatar.replace(here);
        let mut moved = 0.0;
        self.avatar_speed = match previous {
            Some(previous) if dt > 0.0 => {
                moved = (here - previous).length();
                let ground = Vec3::new(here.x - previous.x, 0.0, here.z - previous.z).length();
                if ground > FOLLOW_MAX_STEP {
                    0.0
                } else {
                    ground / dt
                }
            }
            _ => 0.0,
        };
        self.avatar_yaw = avatar.rotation.to_euler(EulerRot::YXZ).0;
        // 0xD223EB: any movement of the avatar clears the "rotated by hand" flag.
        if moved > 1.0e-6 {
            self.manual = false;
        }
        let travelling = self.avatar_speed > tuning::MIN_SPEED_FOR_AUTO_ROTATION;
        self.moving_time = if travelling {
            self.moving_time + dt
        } else {
            0.0
        };
        let hands_off = !self.manual && !self.dragging && !self.first_person && !self.resetting;

        // Swing toward the travel direction (0xD22B1E-0xD22D7D). The retail
        // target is the avatar's goal direction (INFERRED); its heading is used
        // here. The step is a ramped fraction of the remaining angle, per frame.
        if hands_off && travelling {
            let error = wrap_angle(self.avatar_yaw - self.goal_azimuth);
            let angle = error.abs();
            if angle >= tuning::TO_MOUSE_MIN_DEG.to_radians()
                && angle <= tuning::TO_MOUSE_MAX_DEG.to_radians()
            {
                let frames = dt * FRAME_RATE;
                let wanted = (tuning::FOLLOW_ROTATE_SPEED * dt).min(1.0);
                let low = (self.swing_ramp - tuning::FOLLOW_RAMP_DOWN * frames).max(0.0);
                let high = self.swing_ramp + tuning::FOLLOW_RAMP_UP * frames;
                self.swing_ramp = wanted.clamp(low, high);
                let fraction = 1.0 - (1.0 - self.swing_ramp).max(0.0).powf(frames);
                let cap = tuning::FOLLOW_STEP_CAP * frames;
                self.goal_azimuth =
                    wrap_angle(self.goal_azimuth + (error * fraction).clamp(-cap, cap));
            } else {
                self.swing_ramp = 0.0;
            }
        } else {
            self.swing_ramp = 0.0;
        }

        // Moving auto-pitch (0xD22D92-0xD22F15): once the avatar has travelled
        // for a while the goal pitch is pulled into the moving window; a key
        // rotation or pitch switches it off again.
        if self.manual {
            self.pitch_auto = false;
        } else if !self.dragging
            && !self.first_person
            && travelling
            && self.moving_time > MOVING_PITCH_DELAY
        {
            self.pitch_auto = true;
        }
        if self.pitch_auto && !self.resetting {
            let (low, high) = moving_pitch_window(self.distance);
            let target = self.goal_pitch.clamp(low, high);
            let k = 1.0 - (-0.301_03 * dt / tuning::MOVING_PITCH_TIME).exp();
            self.goal_pitch += (target - self.goal_pitch) * k;
        }
        self.dragging = false;

        // 0xD209B0 leaves the smoothed focus equal to the focus (VERIFIED), so
        // the anchor is the focus point itself.
        self.anchor = here + self.up * self.focus_height(avatar.scale.x);
        self.anchor_ready = true;
    }

    /// Breathing (0xD20C70): a small sine along the view, stronger when the
    /// avatar stands still.
    fn breathe(&mut self, dt: f32) {
        let k = speed_weight(self.avatar_speed, tuning::BREATH_SPEED);
        let frequency = tuning::BREATH_FREQUENCY.0
            + (tuning::BREATH_FREQUENCY.1 - tuning::BREATH_FREQUENCY.0) * k;
        let amplitude = tuning::BREATH_AMPLITUDE.0
            + (tuning::BREATH_AMPLITUDE.1 - tuning::BREATH_AMPLITUDE.0) * (1.0 - k);
        let step = frequency * (self.time * frequency).cos() * dt * amplitude;
        self.breath = (self.breath + step).clamp(-amplitude, amplitude);
    }

    /// Shaking (0xD22F30): three cosine terms per axis, weighted by the avatar
    /// speed (none under 1 unit a second, full at 7.5), integrated into an
    /// offset clamped to twice the amplitude, and a roll.
    fn shake_step(&mut self, dt: f32) {
        let k = speed_weight(self.avatar_speed, tuning::SHAKE_SPEED);
        let f = tuning::SHAKE_FREQUENCY;
        let a = tuning::SHAKE_AMPLITUDE.0
            + (tuning::SHAKE_AMPLITUDE.1 - tuning::SHAKE_AMPLITUDE.0) * k;
        let t = self.time;
        let x = a * f * dt * (0.95 * (1.9 * f * t + 1.0).cos() + (f * t).cos());
        let y = 0.7 * 1.3 * f * (1.3 * f * t + 0.3).cos() * dt * a;
        let z = 0.35 * 2.1 * f * a * dt * (2.1 * f * t + 1.2).cos();
        self.shake += Vec2::new(x, y + z);
        let limit = 2.0 * a;
        if self.shake.length() > limit {
            self.shake = self.shake.normalize_or_zero() * limit;
        }
        let roll_limit = tuning::SHAKE_ROLL * a;
        let roll = self.roll + 1.7 * f * roll_limit * dt * (1.7 * f * t + 0.8).cos();
        self.roll = roll.clamp(-roll_limit, roll_limit);
    }

    /// The camera transform for this frame. `terrain` returns the ground
    /// height under a point, when there is a land surface.
    pub fn pose(
        &mut self,
        avatar: &Transform,
        dt: f32,
        terrain: impl Fn(f32, f32) -> Option<f32>,
    ) -> Transform {
        self.track(avatar, dt);
        self.ease(dt);
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.breathe(dt);
        self.shake_step(dt);
        let up = self.up;
        // cameraAboveTerrainBuffer: keep the camera 0.5 above the ground,
        // lifting along the local up.
        let above = |position: &mut Vec3| {
            if let Some(ground) = terrain(position.x, position.z) {
                let need = ground + tuning::ABOVE_TERRAIN - position.y;
                if need > 0.0 {
                    *position += up * (need / up.y.max(0.25));
                }
            }
        };
        if self.first_person {
            let heading = avatar.rotation.to_euler(EulerRot::YXZ).0;
            let scale = avatar.scale.x.max(0.1);
            let forward = Vec3::new(-heading.sin(), 0.0, -heading.cos());
            let mut position =
                avatar.translation + Vec3::Y * EYE_UP * scale + forward * EYE_FORWARD * scale;
            above(&mut position);
            return Transform::from_translation(position).with_rotation(Quat::from_euler(
                EulerRot::YXZ,
                heading + self.look_yaw,
                self.look_pitch,
                0.0,
            ));
        }
        let tilt = Quat::from_rotation_arc(Vec3::Y, up);
        let mut position = self.anchor + tilt * self.orbit_offset();
        above(&mut position);
        let base = Transform::from_translation(position).looking_at(self.anchor, up);
        // Breathing moves the camera along the view (BreathAffectsAim 0); the
        // shake moves the aim point (ShakeAffectsAim 1) and rolls the view.
        let view = (self.anchor - position).normalize_or(Vec3::NEG_Z);
        let position = position + view * self.breath;
        let aim = self.anchor + base.right() * self.shake.x + base.up() * self.shake.y;
        let mut pose = Transform::from_translation(position).looking_at(aim, up);
        if self.roll != 0.0 {
            pose.rotate_local_z(self.roll);
        }
        pose
    }
}

/// Orbit camera controls shared by the land stages.
///
/// `offset()` is relative to the followed target. Its untouched value is the
/// exact stage preset; after input it becomes a spherical orbit around that
/// target. Call [`set_stage`](Self::set_stage) when the active land stage
/// changes, or [`reset_to_preset`](Self::reset_to_preset) to restore its exact
/// default. The Creature stage uses the retail [`CreatureRig`] instead (see
/// [`creature_pose`](Self::creature_pose)).
#[derive(Resource, Clone, Debug)]
pub struct PlayCamera {
    stage: GameStage,
    preset_offset: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    changed: bool,
    rig: CreatureRig,
}

impl Default for PlayCamera {
    fn default() -> Self {
        Self::new(GameStage::Creature)
    }
}

impl PlayCamera {
    pub fn new(stage: GameStage) -> Self {
        let (stage, preset_offset) = land_preset(stage);
        let distance = preset_offset.length();
        Self {
            stage,
            preset_offset,
            yaw: preset_offset.x.atan2(preset_offset.z),
            pitch: (preset_offset.y / distance).asin(),
            distance,
            changed: false,
            rig: CreatureRig::new(),
        }
    }

    /// Switch to a land-stage preset. Returns true only when the land stage
    /// changed; a successful change resets all orbit input to the new preset.
    pub fn set_stage(&mut self, stage: GameStage) -> bool {
        if !matches!(
            stage,
            GameStage::Creature | GameStage::Tribal | GameStage::Civilization
        ) {
            return false;
        }
        let (stage, preset_offset) = land_preset(stage);
        if self.stage == stage {
            return false;
        }
        self.stage = stage;
        self.preset_offset = preset_offset;
        self.reset_to_preset();
        true
    }

    /// Restore the current stage's exact default offset and clear orbit input.
    pub fn reset_to_preset(&mut self) {
        self.distance = self.preset_offset.length();
        self.yaw = self.preset_offset.x.atan2(self.preset_offset.z);
        self.pitch = (self.preset_offset.y / self.distance).asin();
        self.changed = false;
        self.rig.restart();
    }

    pub fn stage(&self) -> &GameStage {
        &self.stage
    }

    /// The Creature rig (also valid before the Creature stage is entered).
    pub fn rig(&self) -> &CreatureRig {
        &self.rig
    }

    pub fn rig_mut(&mut self) -> &mut CreatureRig {
        &mut self.rig
    }

    /// Offset from the camera's followed entity. For the Creature stage this
    /// is the orbit offset from the rig's anchor.
    pub fn offset(&self) -> Vec3 {
        if self.stage == GameStage::Creature {
            return self.rig.orbit_offset();
        }
        if !self.changed {
            return self.preset_offset;
        }
        let horizontal = self.distance * self.pitch.cos();
        Vec3::new(
            self.yaw.sin() * horizontal,
            self.pitch.sin() * self.distance,
            self.yaw.cos() * horizontal,
        )
    }

    /// Apply per-frame input. Positive wheel lines zoom in. Mouse motion only
    /// orbits while the orbit button is held (right for every land stage, plus
    /// the middle button in the Creature stage); editor mode suppresses both.
    pub fn input_delta(
        &mut self,
        mouse_delta: Vec2,
        scroll_lines: f32,
        orbit_button_held: bool,
        editor_open: bool,
    ) {
        if editor_open {
            return;
        }
        let drag_ok = orbit_button_held
            && mouse_delta.is_finite()
            && mouse_delta.x.abs() <= MAX_MOUSE_DELTA_PIXELS
            && mouse_delta.y.abs() <= MAX_MOUSE_DELTA_PIXELS;
        let scroll_ok = scroll_lines.is_finite()
            && scroll_lines.abs() <= MAX_SCROLL_LINES_PER_FRAME
            && scroll_lines != 0.0;
        if self.stage == GameStage::Creature {
            if drag_ok {
                self.rig.orbit_by_pixels(mouse_delta);
            }
            if scroll_ok {
                self.rig.zoom_by_lines(scroll_lines);
            }
            return;
        }

        if drag_ok {
            self.yaw = (self.yaw - mouse_delta.x * MOUSE_RADIANS_PER_PIXEL).rem_euclid(TAU);
            self.pitch =
                (self.pitch + mouse_delta.y * MOUSE_RADIANS_PER_PIXEL).clamp(MIN_PITCH, MAX_PITCH);
            self.changed = true;
        }
        if scroll_ok {
            self.distance *= (-scroll_lines * ZOOM_LOG_SCALE_PER_LINE).exp();
            let preset_distance = self.preset_offset.length();
            self.distance = self.distance.clamp(
                preset_distance * MIN_DISTANCE_FACTOR,
                preset_distance * MAX_DISTANCE_FACTOR,
            );
            self.changed = true;
        }
    }

    /// Creature keyboard camera: `,` `.` rotate, `=` `+` numpad+ zoom in, `-`
    /// numpad- zoom out, PageUp/Numpad9 and PageDown/Numpad3 pitch, `/` reset,
    /// Ctrl+Z first-person view (ADAPTED key: the original binds plain Z, which
    /// the manual documents as the sneak toggle).
    pub fn creature_keys(&mut self, keys: &mut ButtonInput<KeyCode>, dt: f32) {
        if self.stage != GameStage::Creature || !dt.is_finite() || dt < 0.0 {
            return;
        }
        let held = |keys: &ButtonInput<KeyCode>, a: KeyCode, b: KeyCode| {
            keys.pressed(a) || keys.pressed(b)
        };
        let axis = |positive: bool, negative: bool| match (positive, negative) {
            (true, false) => 1.0,
            (false, true) => -1.0,
            _ => 0.0,
        };
        let rotate = axis(keys.pressed(KeyCode::Period), keys.pressed(KeyCode::Comma));
        let zoom = axis(
            held(keys, KeyCode::Equal, KeyCode::NumpadAdd),
            held(keys, KeyCode::Minus, KeyCode::NumpadSubtract),
        );
        let tilt = axis(
            held(keys, KeyCode::PageUp, KeyCode::Numpad9),
            held(keys, KeyCode::PageDown, KeyCode::Numpad3),
        );
        if rotate != 0.0 {
            self.rig.rotate_keys(rotate, dt);
        }
        if zoom != 0.0 {
            self.rig.zoom_keys(zoom, dt);
        }
        if tilt != 0.0 {
            self.rig.tilt_keys(tilt, dt);
        }
        if keys.just_pressed(KeyCode::Slash) {
            self.rig.reset_view();
        }
        let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        if ctrl && keys.just_pressed(KeyCode::KeyZ) {
            let on = !self.rig.is_first_person();
            self.rig.set_first_person(on);
            // Z also toggles sneak; Ctrl+Z is the camera's.
            keys.clear_just_pressed(KeyCode::KeyZ);
        }
    }

    /// Horizontal camera-relative forward and right vectors for movement.
    /// Forward points from the camera toward its target. At the default yaw,
    /// this is exactly -Z forward and +X right.
    pub fn horizontal_move_basis(&self) -> (Vec3, Vec3) {
        let yaw = if self.stage == GameStage::Creature {
            self.rig.azimuth()
        } else {
            self.yaw
        };
        let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos()).normalize();
        let right = forward.cross(Vec3::Y).normalize();
        (forward, right)
    }

    /// The Creature camera transform for this frame (called by `camera_follow`
    /// after the avatar has moved and been grounded).
    pub fn creature_pose(
        &mut self,
        avatar: &Transform,
        dt: f32,
        terrain: impl Fn(f32, f32) -> Option<f32>,
    ) -> Transform {
        self.rig.set_body_height(crate::creature_support::body_height());
        self.rig.pose(avatar, dt, terrain)
    }
}

/// Wrap an angle difference into [-PI, PI).
fn wrap_angle(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI
}

/// Stage preset: the exact untouched offset. Anything that is not a land
/// stage keeps the Creature preset.
fn land_preset(stage: GameStage) -> (GameStage, Vec3) {
    match stage {
        GameStage::Tribal => (GameStage::Tribal, TRIBAL_OFFSET),
        GameStage::Civilization => (GameStage::Civilization, CIVILIZATION_OFFSET),
        _ => (GameStage::Creature, CreatureRig::new().orbit_offset()),
    }
}

/// Input resource updater. All input and editor resources are optional so the
/// system is harmless in headless/minimal apps that only install this plugin.
pub(crate) fn update_play_camera_input(
    mut camera: ResMut<PlayCamera>,
    stage: Option<Res<State<GameStage>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    pixels_per_line: Option<Res<MouseScrollPixelsPerLine>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<ResMut<ButtonInput<KeyCode>>>,
    editor: Option<Res<EditorState>>,
    pause: Option<Res<crate::pause_menu::PauseMenu>>,
    time: Option<Res<Time>>,
    mut right: Local<crate::control_options::RightGesture>,
) {
    let Some(stage) = stage else {
        return;
    };
    let active_stage = stage.get();
    // The Tribal camera is `tribal_controls::TribalCamera` (free pan, rotate,
    // pitch and zoom); this input system serves Creature and Civilization.
    if !matches!(active_stage, GameStage::Creature | GameStage::Civilization) {
        return;
    }
    camera.set_stage(active_stage.clone());

    let editor_open = editor.is_some_and(|state| state.open);
    let creature = *active_stage == GameStage::Creature;
    let mouse_delta = motion.map_or(Vec2::ZERO, |input| input.delta);
    // The Creature camera orbits with the middle or the right button
    // (`UIMachineCreature`: from the moment the button goes down). With the
    // Creature control option (`UIMachineCreatureTwoButton`) the right button
    // acts on a click and turns the camera only once the cursor travelled 16
    // pixels or the left button joined it (`control_options::RightGesture`; the
    // Creature controls track the same gesture to know it was no click).
    let two_button = creature && crate::control_options::creature_two_button();
    let orbit_held = buttons.is_some_and(|input| {
        if two_button {
            let frame = right.update(
                input.pressed(MouseButton::Left),
                input.pressed(MouseButton::Right),
                input.just_pressed(MouseButton::Right),
                false,
                mouse_delta,
            );
            frame.rotating || input.pressed(MouseButton::Middle)
        } else {
            *right = Default::default();
            input.pressed(MouseButton::Right) || (creature && input.pressed(MouseButton::Middle))
        }
    });

    let scroll_lines = scroll.map_or(0.0, |input| {
        let default_ratio = MouseScrollPixelsPerLine::default();
        let ratio = pixels_per_line.as_deref().unwrap_or(&default_ratio);
        input.to_lines(ratio).delta.y
    });
    camera.input_delta(mouse_delta, scroll_lines, orbit_held, editor_open);
    if creature && !editor_open && !pause.is_some_and(|pause| pause.open) {
        if let (Some(mut keys), Some(time)) = (keys, time) {
            camera.creature_keys(&mut keys, time.delta_secs());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close_vec3(actual: Vec3, expected: Vec3) {
        assert!(
            actual.distance(expected) < 1.0e-4,
            "{actual:?} != {expected:?}"
        );
    }

    fn avatar(at: Vec3, yaw: f32) -> Transform {
        Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw))
    }

    fn flat(_: f32, _: f32) -> Option<f32> {
        None
    }

    /// Run the follow for `seconds` at 60 Hz with the avatar walking along
    /// `direction` at `speed`, facing `yaw`.
    fn walk(
        rig: &mut CreatureRig,
        from: Vec3,
        direction: Vec3,
        speed: f32,
        yaw: f32,
        seconds: f32,
    ) -> Vec3 {
        let dt = 1.0 / 60.0;
        let mut at = from;
        for _ in 0..(seconds / dt) as usize {
            at += direction * speed * dt;
            rig.pose(&avatar(at, yaw), dt, flat);
        }
        at
    }

    #[test]
    fn creature_preset_is_the_retail_initial_view() {
        let rig = CreatureRig::new();
        assert_eq!(rig.distance(), 15.0);
        assert!(
            (rig.pitch().to_degrees() - 12.0).abs() < 1e-4,
            "opening angle 12"
        );
        assert_eq!(rig.azimuth(), 0.0);
        let offset = PlayCamera::new(GameStage::Creature).offset();
        assert!((offset.length() - 15.0).abs() < 1e-4);
        assert!(
            offset.x.abs() < 1e-6 && offset.z > 0.0 && offset.y > 0.0,
            "behind a -Z facing avatar"
        );
        assert!(offset.y / offset.length() < 12.0_f32.to_radians().sin() + 1e-5);
    }

    #[test]
    fn untouched_and_reset_offsets_match_stage_presets_exactly() {
        for (stage, preset) in [
            (GameStage::Tribal, TRIBAL_OFFSET),
            (GameStage::Civilization, CIVILIZATION_OFFSET),
        ] {
            let mut camera = PlayCamera::new(stage);
            assert_eq!(camera.offset(), preset);
            camera.input_delta(Vec2::new(30.0, -12.0), 2.0, true, false);
            camera.reset_to_preset();
            assert_eq!(camera.offset(), preset);
        }
        let creature = PlayCamera::new(GameStage::Creature).offset();
        let mut camera = PlayCamera::default();
        camera.input_delta(Vec2::new(30.0, -12.0), 2.0, true, false);
        camera.reset_to_preset();
        assert_eq!(camera.offset(), creature);
    }

    #[test]
    fn stage_change_resets_orbit_and_unsupported_stages_do_not_replace_preset() {
        let mut camera = PlayCamera::default();
        camera.input_delta(Vec2::new(80.0, 45.0), -3.0, true, false);
        assert!(camera.set_stage(GameStage::Tribal));
        assert_eq!(camera.offset(), TRIBAL_OFFSET);
        assert!(!camera.set_stage(GameStage::Cell));
        assert_eq!(camera.offset(), TRIBAL_OFFSET);
        assert!(camera.set_stage(GameStage::Creature));
        assert_eq!(camera.rig().goal_distance(), tuning::INITIAL_DISTANCE);
    }

    #[test]
    fn yaw_rotates_horizontal_basis_without_vertical_component() {
        let mut camera = PlayCamera::new(GameStage::Tribal);
        let (forward, right) = camera.horizontal_move_basis();
        close_vec3(forward, Vec3::NEG_Z);
        close_vec3(right, Vec3::X);

        camera.input_delta(Vec2::new(90.0, 0.0), 0.0, true, false);
        let (forward, right) = camera.horizontal_move_basis();
        assert!(forward.y.abs() < 1.0e-6 && right.y.abs() < 1.0e-6);
        assert!((forward.length() - 1.0).abs() < 1.0e-6);
        assert!((right.length() - 1.0).abs() < 1.0e-6);
        assert!(forward.dot(right).abs() < 1.0e-6);
        assert!(forward.x > 0.5);

        // The Creature basis follows the rig azimuth.
        let mut camera = PlayCamera::default();
        close_vec3(camera.horizontal_move_basis().0, Vec3::NEG_Z);
        camera.input_delta(Vec2::new(200.0, 0.0), 0.0, true, false);
        camera.rig_mut().ease(1.0);
        let (forward, right) = camera.horizontal_move_basis();
        assert!(forward.x > 0.2 && forward.dot(right).abs() < 1.0e-6);
    }

    #[test]
    fn tribal_pitch_and_zoom_remain_bounded_and_editor_suppresses_input() {
        let mut camera = PlayCamera::new(GameStage::Tribal);
        let initial = camera.offset();
        camera.input_delta(Vec2::new(0.0, 1.0e9), 1.0e9, true, false);
        let far = camera.offset();
        assert!(far.length() <= initial.length() * MAX_DISTANCE_FACTOR + 1.0e-4);
        assert!(far.length() >= initial.length() * MIN_DISTANCE_FACTOR - 1.0e-4);
        let elevation = (far.y / far.length()).asin();
        assert!(elevation >= MIN_PITCH - 1.0e-5);
        assert!(elevation <= MAX_PITCH + 1.0e-5);

        let before_editor = camera.offset();
        camera.input_delta(Vec2::new(500.0, -500.0), -100.0, true, true);
        assert_eq!(camera.offset(), before_editor);
    }

    #[test]
    fn tribal_yaw_wraps_and_malformed_extreme_input_is_rejected() {
        let mut camera = PlayCamera::new(GameStage::Tribal);
        camera.input_delta(Vec2::new(-MAX_MOUSE_DELTA_PIXELS, 0.0), 0.0, true, false);
        assert!((0.0..TAU).contains(&camera.yaw));

        let before = (camera.yaw, camera.pitch, camera.distance, camera.offset());
        camera.input_delta(Vec2::new(f32::INFINITY, 100.0), f32::NAN, true, false);
        assert_eq!(
            (camera.yaw, camera.pitch, camera.distance, camera.offset()),
            before
        );
        camera.input_delta(Vec2::new(100_000.0, 0.0), 100_000.0, true, false);
        assert_eq!(
            (camera.yaw, camera.pitch, camera.distance, camera.offset()),
            before
        );
    }

    #[test]
    fn creature_zoom_is_clamped_and_uses_the_wheel_sensitivity() {
        let mut rig = CreatureRig::new();
        // One notch is 120 raw units of 0.0006: the goal distance is
        // multiplied by `1 - delta * 0.0006` (0xD20571).
        rig.zoom_by_lines(1.0);
        let expected = 15.0 * (1.0 - 120.0 * 0.0006);
        assert!(
            (rig.goal_distance() - expected).abs() < 1e-4,
            "{}",
            rig.goal_distance()
        );
        rig.zoom_by_lines(-1.0);
        assert!((rig.goal_distance() - expected * (1.0 + 120.0 * 0.0006)).abs() < 1e-4);
        rig.zoom_by_lines(60.0);
        assert_eq!(rig.goal_distance(), tuning::MIN_DISTANCE);
        rig.zoom_by_lines(-120.0);
        assert_eq!(rig.goal_distance(), tuning::MAX_DISTANCE);
        // Non-finite and absurd frames are rejected by the shared entry point.
        let mut camera = PlayCamera::default();
        camera.input_delta(Vec2::ZERO, f32::NAN, false, false);
        camera.input_delta(Vec2::ZERO, 1.0e6, false, false);
        assert_eq!(camera.rig().goal_distance(), tuning::INITIAL_DISTANCE);
        camera.input_delta(Vec2::ZERO, 3.0, false, true);
        assert_eq!(
            camera.rig().goal_distance(),
            tuning::INITIAL_DISTANCE,
            "editor suppresses zoom"
        );
    }

    #[test]
    fn creature_pitch_limits_and_standard_pitch_follow_the_retail_curves() {
        // 0xD20700: smoothstep between the close and far ranges.
        let (low, high) = pitch_limits(tuning::MIN_DISTANCE);
        assert!((low.to_degrees() + 15.0).abs() < 1e-3 && (high.to_degrees() - 15.0).abs() < 1e-3);
        let (low, high) = pitch_limits(tuning::MAX_DISTANCE);
        assert!((low.to_degrees() + 30.0).abs() < 1e-3 && (high.to_degrees() - 85.0).abs() < 1e-3);
        let (low, high) = pitch_limits(15.0);
        // zoom 0.5 -> smoothstep 0.5 (the same as linear at the middle)
        assert!((low.to_degrees() + 22.5).abs() < 1e-3 && (high.to_degrees() - 50.0).abs() < 1e-3);
        let (low, _) = pitch_limits(10.0);
        // zoom 0.25 -> smoothstep 0.15625, not 0.25
        assert!((low.to_degrees() - (-15.0 - 15.0 * 0.15625)).abs() < 1e-3, "{}", low.to_degrees());
        // 0xD20620: the standard pitch is 20 at the closest zoom, 30 at the farthest.
        assert!((standard_pitch(tuning::MIN_DISTANCE).to_degrees() - 20.0).abs() < 1e-3);
        assert!((standard_pitch(tuning::MAX_DISTANCE).to_degrees() - 30.0).abs() < 1e-3);
        assert!((standard_pitch(15.0).to_degrees() - 25.0).abs() < 1e-3);
        // 0xD22E24: the moving window, 4..10 at the closest zoom, 4..20 at the farthest.
        let (low, high) = moving_pitch_window(tuning::MIN_DISTANCE);
        // f = (25 - 5) / 25 = 0.8: 20 + (10 - 20) * 0.8
        assert!((low.to_degrees() - 4.0).abs() < 1e-3 && (high.to_degrees() - 12.0).abs() < 1e-3);
        let (low, high) = moving_pitch_window(tuning::MAX_DISTANCE);
        assert!((low.to_degrees() - 4.0).abs() < 1e-3 && (high.to_degrees() - 20.0).abs() < 1e-3);

        // With free pitch (the shipped data) only the absolute -25..50 range
        // limits the player's pitch, at any zoom.
        let mut rig = CreatureRig::new();
        rig.tilt_keys(1.0, 100.0);
        assert!((rig.goal_pitch().to_degrees() - 50.0).abs() < 1e-3);
        rig.zoom_by_lines(60.0);
        rig.ease(1.0);
        assert!((rig.pitch().to_degrees() - 50.0).abs() < 1e-3, "zoom does not clamp the pitch");
        rig.tilt_keys(-1.0, 100.0);
        rig.ease(1.0);
        assert!((rig.pitch().to_degrees() + 25.0).abs() < 1e-3);
    }

    #[test]
    fn creature_mouse_orbit_uses_the_retail_sensitivities() {
        let mut rig = CreatureRig::new();
        let start_az = rig.goal_azimuth();
        let start_pitch = rig.goal_pitch();
        // 0xD24D3A: X 0.7 and Y 0.4 times 0.2 degrees a pixel.
        rig.orbit_by_pixels(Vec2::new(100.0, 0.0));
        let turned = wrap_angle(rig.goal_azimuth() - start_az).to_degrees();
        assert!((turned + 100.0 * 0.7 * 0.2).abs() < 1e-3, "{turned}");
        rig.orbit_by_pixels(Vec2::new(0.0, 10.0));
        let tilted = (rig.goal_pitch() - start_pitch).to_degrees();
        assert!((tilted - 10.0 * 0.4 * 0.2).abs() < 1e-3, "{tilted}");
        // The pitch stops at the camera's absolute range.
        rig.orbit_by_pixels(Vec2::new(0.0, 100000.0));
        assert!((rig.goal_pitch().to_degrees() - 50.0).abs() < 1e-3);
        rig.orbit_by_pixels(Vec2::new(0.0, -100000.0));
        assert!((rig.goal_pitch().to_degrees() + 25.0).abs() < 1e-3);

        // Through the shared entry point only the orbit button orbits.
        let mut camera = PlayCamera::default();
        camera.input_delta(Vec2::new(50.0, 50.0), 0.0, false, false);
        assert_eq!(camera.rig().goal_azimuth(), 0.0);
        camera.input_delta(Vec2::new(50.0, 0.0), 0.0, true, false);
        assert!(camera.rig().goal_azimuth() < 0.0);
        camera.input_delta(Vec2::new(f32::INFINITY, 0.0), 0.0, true, false);
        assert!(camera.rig().goal_azimuth().is_finite());
    }

    #[test]
    fn creature_keys_rotate_pitch_zoom_reset_and_ignore_other_stages() {
        let mut camera = PlayCamera::default();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Period);
        camera.creature_keys(&mut keys, 0.5);
        // pi/2 a second (0xD2AA59).
        let right = camera.rig().goal_azimuth();
        assert!((right - std::f32::consts::FRAC_PI_4).abs() < 1e-5, "{right}");
        keys.release(KeyCode::Period);
        keys.press(KeyCode::Comma);
        camera.creature_keys(&mut keys, 0.5);
        assert!(
            camera.rig().goal_azimuth().abs() < 1e-5,
            "comma undoes period"
        );
        keys.release(KeyCode::Comma);

        // PageUp closes on 50 degrees by 1 - exp(-0.30103 dt / 0.3) of the gap.
        keys.press(KeyCode::PageUp);
        camera.creature_keys(&mut keys, 0.25);
        let k = 1.0 - (-0.30103_f32 * 0.25 / 0.3).exp();
        let expected = 12.0 + (50.0 - 12.0) * k;
        assert!(
            (camera.rig().goal_pitch().to_degrees() - expected).abs() < 1e-2,
            "{}",
            camera.rig().goal_pitch().to_degrees()
        );
        keys.release(KeyCode::PageUp);
        keys.press(KeyCode::Numpad3);
        camera.creature_keys(&mut keys, 0.5);
        assert!(camera.rig().goal_pitch().to_degrees() < 12.0);
        keys.release(KeyCode::Numpad3);

        // `=` multiplies the goal distance by 1 - dt, `-` by 1 + dt.
        keys.press(KeyCode::Equal);
        camera.creature_keys(&mut keys, 0.1);
        assert!((camera.rig().goal_distance() - 13.5).abs() < 1e-4);
        keys.release(KeyCode::Equal);
        keys.press(KeyCode::NumpadSubtract);
        camera.creature_keys(&mut keys, 0.5);
        assert!((camera.rig().goal_distance() - 20.25).abs() < 1e-4);
        keys.release(KeyCode::NumpadSubtract);

        // `/` turns back behind the avatar and returns to the standard pitch
        // of the zoom; the distance stays (0xD2ABD0).
        camera.rig_mut().rotate_keys(1.0, 1.0);
        keys.press(KeyCode::Slash);
        camera.creature_keys(&mut keys, 0.0);
        for _ in 0..300 {
            camera.rig_mut().ease(0.05);
        }
        let rig = camera.rig();
        assert!((rig.goal_distance() - 20.25).abs() < 1e-4);
        assert!(rig.goal_azimuth().abs() < 1e-4, "{}", rig.goal_azimuth());
        assert!((rig.goal_pitch() - standard_pitch(rig.distance())).abs() < 1e-4);

        // Other stages never consume the keys.
        let mut tribal = PlayCamera::new(GameStage::Tribal);
        keys.reset_all();
        keys.press(KeyCode::Period);
        tribal.creature_keys(&mut keys, 1.0);
        assert_eq!(tribal.rig().goal_azimuth(), 0.0);
    }

    #[test]
    fn ctrl_z_toggles_first_person_and_keeps_z_for_sneak() {
        let mut camera = PlayCamera::default();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyZ);
        camera.creature_keys(&mut keys, 0.016);
        assert!(
            !camera.rig().is_first_person(),
            "plain Z is the sneak toggle"
        );
        assert!(keys.just_pressed(KeyCode::KeyZ));
        keys.reset_all();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyZ);
        camera.creature_keys(&mut keys, 0.016);
        assert!(camera.rig().is_first_person());
        assert!(!keys.just_pressed(KeyCode::KeyZ), "the chord is consumed");
        keys.reset_all();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyZ);
        camera.creature_keys(&mut keys, 0.016);
        assert!(!camera.rig().is_first_person());
    }

    #[test]
    fn first_person_sits_at_the_head_looking_along_the_heading() {
        let mut rig = CreatureRig::new();
        rig.set_first_person(true);
        let body = Transform::from_xyz(2.0, 0.7, 3.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let pose = rig.pose(&body, 1.0 / 60.0, flat);
        // Facing -X: the eye is ahead (toward -X) and above the origin.
        assert!(pose.translation.x < 2.0 && pose.translation.y > 0.7);
        close_vec3(pose.rotation * Vec3::NEG_Z, Vec3::NEG_X);
        // Looking up and around is limited.
        rig.orbit_by_pixels(Vec2::new(0.0, -100000.0));
        let pose = rig.pose(&body, 1.0 / 60.0, flat);
        let up = (pose.rotation * Vec3::NEG_Z).y;
        assert!(up > 0.5 && up < 0.9, "pitch is clamped to 60 degrees: {up}");
        rig.set_first_person(false);
        let pose = rig.pose(&body, 1.0 / 60.0, flat);
        assert!(
            pose.translation.distance(body.translation) > 10.0,
            "third person again"
        );
    }

    #[test]
    fn focus_point_is_the_avatar_plus_the_retail_height_ramp() {
        // Far zoom: 0.5 H above the origin; the default H is 1.3 a unit of scale.
        let mut rig = CreatureRig::new();
        rig.pose(&avatar(Vec3::ZERO, 0.0), 1.0 / 60.0, flat);
        close_vec3(rig.anchor(), Vec3::Y * 0.65);
        // Close zoom: 0.5 * (0.5 H) + 0.5 (the .data default state factor).
        let mut rig = CreatureRig::new();
        rig.set_body_height(Some(3.0));
        rig.goal_distance = tuning::MIN_DISTANCE;
        rig.distance = tuning::MIN_DISTANCE;
        rig.pose(&avatar(Vec3::ZERO, 0.0), 1.0 / 60.0, flat);
        close_vec3(rig.anchor(), Vec3::Y * (0.5 * 1.5 + 0.5));
        // Between 1.3 x min and 2.3 x min the height blends from close to far.
        let mut rig = CreatureRig::new();
        rig.set_body_height(Some(3.0));
        rig.goal_distance = 9.0;
        rig.distance = 9.0;
        rig.pose(&avatar(Vec3::ZERO, 0.0), 1.0 / 60.0, flat);
        let t = (9.0 / 5.0 - 1.3_f32).clamp(0.0, 1.0);
        let expected = 1.25 + (1.5 - 1.25) * t;
        assert!((rig.anchor().y - expected).abs() < 0.02, "{}", rig.anchor().y);
        // A non-finite avatar leaves the camera where it was.
        let before = rig.anchor();
        rig.track(&avatar(Vec3::NAN, 0.0), 0.016);
        assert_eq!(rig.anchor(), before);
    }

    #[test]
    fn walking_avatar_is_followed_rigidly() {
        // 0xD209B0 ends with smoothed focus = focus: no lag at all.
        let mut rig = CreatureRig::new();
        rig.pose(&avatar(Vec3::ZERO, 0.0), 1.0 / 60.0, flat);
        let end = walk(&mut rig, Vec3::ZERO, Vec3::NEG_Z, 4.0, 0.0, 3.0);
        close_vec3(rig.anchor(), end + Vec3::Y * 0.65);
        // Standing still the anchor stays on the avatar.
        for _ in 0..60 {
            rig.pose(&avatar(end, 0.0), 1.0 / 60.0, flat);
        }
        close_vec3(rig.anchor(), end + Vec3::Y * 0.65);
    }

    #[test]
    fn camera_swings_toward_the_heading_only_above_the_speed_threshold() {
        // Facing +X (yaw -90 degrees) and walking slowly: the camera stays.
        let yaw = -std::f32::consts::FRAC_PI_2;
        let mut rig = CreatureRig::new();
        walk(&mut rig, Vec3::ZERO, Vec3::X, 0.6, yaw, 3.0);
        assert!(rig.goal_azimuth().abs() < 1e-5, "0.6 u/s is under 0.65");
        // Above the threshold it swings round until it is within 10 degrees
        // of the heading (cameraToMouseMinAngle).
        let mut rig = CreatureRig::new();
        walk(&mut rig, Vec3::ZERO, Vec3::X, 2.0, yaw, 12.0);
        let left = wrap_angle(rig.azimuth() - yaw).abs();
        assert!(left < tuning::TO_MOUSE_MIN_DEG.to_radians() + 0.02, "{left}");
        assert!(left > 0.0, "it stops inside the 10 degree window");

        // The swing starts slowly: the ramp grows 0.0012 a frame, so the
        // closed fraction after t seconds is 1 - exp(-0.072 * 60 t^2 / 2 ...).
        let mut rig = CreatureRig::new();
        walk(&mut rig, Vec3::ZERO, Vec3::X, 2.0, yaw, 0.1);
        let first = wrap_angle(rig.goal_azimuth()).abs();
        walk(&mut rig, Vec3::new(0.2, 0.0, 0.0), Vec3::X, 2.0, yaw, 0.4);
        let early = wrap_angle(rig.goal_azimuth()).abs();
        walk(&mut rig, Vec3::new(1.0, 0.0, 0.0), Vec3::X, 2.0, yaw, 1.5);
        let later = wrap_angle(rig.goal_azimuth()).abs();
        assert!(first < 0.1, "{first}");
        assert!(early > 0.3 && early < 0.9, "{early}");
        assert!(later > 1.2, "{later}");

        // A target more than 150 degrees away is left alone.
        let mut rig = CreatureRig::new();
        walk(&mut rig, Vec3::ZERO, Vec3::Z, 2.0, std::f32::consts::PI * 0.99, 6.0);
        assert!(rig.goal_azimuth().abs() < 1e-5);

        // A key rotation holds the swing until the avatar moves again.
        let mut rig = CreatureRig::new();
        rig.rotate_keys(1.0, 0.2);
        let held = rig.goal_azimuth();
        for _ in 0..120 {
            rig.pose(&avatar(Vec3::ZERO, yaw), 1.0 / 60.0, flat);
        }
        assert_eq!(rig.goal_azimuth(), held, "no movement, no swing");

        // A mouse drag stops the swing for that frame.
        let mut rig = CreatureRig::new();
        walk(&mut rig, Vec3::ZERO, Vec3::X, 2.0, yaw, 1.0);
        let before = rig.goal_azimuth();
        rig.orbit_by_pixels(Vec2::new(1.0, 0.0));
        let dragged = rig.goal_azimuth();
        rig.pose(&avatar(Vec3::new(3.0, 0.0, 0.0), yaw), 1.0 / 60.0, flat);
        assert_eq!(rig.goal_azimuth(), dragged);
        assert_ne!(before, dragged);

        // Teleports do not count as speed.
        let mut rig = CreatureRig::new();
        rig.pose(&avatar(Vec3::ZERO, yaw), 1.0 / 60.0, flat);
        rig.pose(&avatar(Vec3::new(30.0, 0.0, 0.0), yaw), 1.0 / 60.0, flat);
        assert_eq!(rig.avatar_speed(), 0.0);
        assert_eq!(rig.goal_azimuth(), 0.0);
    }

    #[test]
    fn moving_auto_pitch_pulls_a_high_camera_down_after_two_seconds() {
        let mut rig = CreatureRig::new();
        rig.tilt_keys(1.0, 100.0); // 50 degrees
        // The key flag clears on the first step of the avatar.
        walk(&mut rig, Vec3::ZERO, Vec3::NEG_Z, 3.0, 0.0, 1.5);
        assert!(rig.goal_pitch().to_degrees() > 49.0, "not before 2 s of travel");
        walk(&mut rig, Vec3::new(0.0, 0.0, -4.5), Vec3::NEG_Z, 3.0, 0.0, 8.0);
        let (_, high) = moving_pitch_window(rig.distance());
        assert!(
            rig.goal_pitch() < high + 0.05,
            "{} > {}",
            rig.goal_pitch().to_degrees(),
            high.to_degrees()
        );
        // A key pitch while standing switches it off (the flag a key sets is
        // cleared again by the next step of the avatar, as in retail).
        for _ in 0..10 {
            rig.pose(&avatar(Vec3::new(0.0, 0.0, -30.0), 0.0), 1.0 / 60.0, flat);
        }
        rig.tilt_keys(1.0, 100.0);
        rig.pose(&avatar(Vec3::new(0.0, 0.0, -30.0), 0.0), 1.0 / 60.0, flat);
        walk(&mut rig, Vec3::new(0.0, 0.0, -30.0), Vec3::NEG_Z, 3.0, 0.0, 1.0);
        assert!(rig.goal_pitch().to_degrees() > 49.0);
    }

    #[test]
    fn breathing_and_shaking_follow_the_avatar_speed() {
        // Standing still: breathing up to 0.005, no shake.
        let mut rig = CreatureRig::new();
        let mut peak = 0.0_f32;
        for _ in 0..600 {
            rig.pose(&avatar(Vec3::ZERO, 0.0), 1.0 / 60.0, flat);
            let (breath, shake, roll) = rig.motion();
            peak = peak.max(breath.abs());
            assert_eq!(shake, Vec2::ZERO);
            assert_eq!(roll, 0.0);
        }
        assert!(peak > 0.001 && peak <= 0.005 + 1e-6, "{peak}");
        // Running at 8 u/s: shake up to twice 0.03, roll up to 0.03 x 0.03.
        let mut rig = CreatureRig::new();
        let (mut peak, mut roll_peak) = (0.0_f32, 0.0_f32);
        let dt = 1.0 / 60.0;
        let mut at = Vec3::ZERO;
        for _ in 0..600 {
            at += Vec3::NEG_Z * 8.0 * dt;
            rig.pose(&avatar(at, 0.0), dt, flat);
            let (_, shake, roll) = rig.motion();
            peak = peak.max(shake.length());
            roll_peak = roll_peak.max(roll.abs());
        }
        assert!(peak > 0.005 && peak <= 0.06 + 1e-5, "{peak}");
        assert!(roll_peak <= 0.03 * 0.03 + 1e-6);
    }

    #[test]
    fn camera_stays_half_a_unit_above_the_terrain_and_frames_the_anchor() {
        let mut rig = CreatureRig::new();
        let body = avatar(Vec3::new(0.0, 0.7, 0.0), 0.0);
        let hill = |_x: f32, z: f32| Some(if z > 5.0 { 20.0 } else { 0.0 });
        let pose = rig.pose(&body, 1.0 / 60.0, hill);
        assert!(
            pose.translation.y >= 20.0 + tuning::ABOVE_TERRAIN - 1e-4,
            "{}",
            pose.translation.y
        );
        let toward = (rig.anchor() - pose.translation).normalize();
        assert!(
            ((pose.rotation * Vec3::NEG_Z) - toward).length() < 1e-4,
            "still looks at the anchor"
        );
        // On flat ground at the initial zoom the camera is not clamped.
        let mut rig = CreatureRig::new();
        let pose = rig.pose(&body, 1.0 / 60.0, |_, _| Some(0.0));
        let expected = rig.anchor() + rig.orbit_offset();
        close_vec3(pose.translation, expected);
    }

    #[test]
    fn reset_view_turns_behind_the_avatar_and_restart_is_immediate() {
        let mut rig = CreatureRig::new();
        rig.orbit_by_pixels(Vec2::new(300.0, 80.0));
        rig.zoom_by_lines(7.0);
        rig.ease(1.0);
        let distance = rig.goal_distance();
        assert!(rig.azimuth().abs() > 0.3 && rig.distance() < 15.0);
        rig.reset_view();
        rig.ease(0.1);
        assert!(rig.azimuth().abs() > 0.1, "turning at pi/2 a second, not a cut");
        for _ in 0..100 {
            rig.ease(0.1);
        }
        assert!(rig.azimuth().abs() < 1e-3);
        assert!((rig.goal_pitch() - standard_pitch(rig.distance())).abs() < 1e-4);
        assert_eq!(rig.goal_distance(), distance, "the reset leaves the zoom");
        rig.orbit_by_pixels(Vec2::new(100.0, 0.0));
        rig.restart();
        assert_eq!(rig.goal_azimuth(), 0.0);
        assert_eq!(rig.azimuth(), 0.0);
    }

    #[test]
    fn distance_eases_exponentially_and_angles_apply_at_once() {
        let mut rig = CreatureRig::new();
        rig.zoom_by_lines(5.0);
        let goal = rig.goal_distance();
        rig.ease(0.05);
        // 1 - exp(-0.30103) of the gap after one interpolation time.
        let gap = (15.0 - rig.distance()) / (15.0 - goal);
        assert!((gap - (1.0 - (-0.30103_f32).exp())).abs() < 1e-3, "{gap}");
        rig.orbit_by_pixels(Vec2::new(40.0, 20.0));
        rig.ease(0.0);
        assert_eq!(rig.azimuth(), rig.goal_azimuth());
        assert_eq!(rig.pitch(), rig.goal_pitch());
    }

    #[test]
    fn pixel_scroll_is_normalized_to_line_units() {
        let raw = AccumulatedMouseScroll {
            unit: MouseScrollUnit::Pixel,
            delta: Vec2::new(0.0, 200.0),
        };
        let lines = raw.to_lines(&MouseScrollPixelsPerLine::default());
        assert_eq!(lines.unit, MouseScrollUnit::Line);
        assert_eq!(lines.delta.y, 2.0);
    }
}
