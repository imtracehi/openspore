//! Space stage camera and zoom levels (S6), after the retail `cSpaceCamera`
//! classes: the solar-system and galaxy cameras (one orbit camera class
//! driven by the `SpaceCamera` tuning prop) and the planet camera (a chase
//! camera whose distance, look-at offset and field of view come from the
//! `planetCameraDistanceFOV` table by the UFO's altitude).
//!
//! Everything below the `// --- state ---` line is pure maths in the retail
//! units (tests pin it to the decompiled routines, see
//! `notes\space-ctrl-notes.md`); the Bevy systems at the bottom scale it into
//! the scene's units (`SOLAR_SCALE`, `GALAXY_SCALE`, `PlanetView::unit`,
//! `// ADAPTED:`).
//!
//! * Wheel notches (and the held zoom keys, one notch per frame as in the
//!   original) feed a velocity ramp (`WheelRamp`, 0x101AD50); the orbit camera
//!   moves its target radius by `rate(radius) * velocity * dt` (0x101B010),
//!   stops at the zoom floor / ceiling until the push is repeated, and only
//!   crosses into the next view (planet / solar / galaxy) when the ship is at
//!   its destination.
//! * End / Home and Ctrl+ / Ctrl- step the discrete zoom levels
//!   (`galaxyDiscreteZoom`, `solarDiscreteZoom`, 0x101B3E0).
//! * Pitch follows the distance-based `phi` curve plus the player's offset;
//!   rotating is 1.5 rad/s on the keys and 0.005 rad per pixel on the mouse.
//! * Every view goes through the retail projection (cViewer 0x7C5400, VERIFIED
//!   by the Cell / editor helpers): the FOV of 60 degrees is the HORIZONTAL
//!   angle of a 4:3 view, the vertical half-angle tangent is
//!   `tan(fov / 2) x 0.75` on any non-square view (`retail_vertical_fov`).
//! * The planet camera's fourth table column is the camera's ELEVATION angle
//!   above the surface, not a field of view (0x101CFE0: it is the angle of the
//!   quaternion that swings the camera offset about the ship's right axis;
//!   the FOV passed to 0x7C5400 is the constant `+0x70` = spaceFOV): from
//!   straight overhead at height (80 degrees at 300-600) to a low chase view
//!   (20 degrees at the ground). See `notes\space-cam-notes.md`.

use bevy::input::mouse::{AccumulatedMouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};

use crate::game::Player;
use crate::space_controls::{SpaceControls, TriggerSet};
use crate::space_galaxy::{GALAXY_ORIGIN, GALAXY_RADIUS};

// ---------------------------------------------------------------------------
// Tuning (gametuning~!SpaceCamera 0x521D0174 and spaceufo~, original data)
// ---------------------------------------------------------------------------

/// spaceFOV (degrees; the data's solar / galaxy FOV start and end are 60 too).
pub const FOV_DEG: f32 = 60.0;
/// spaceNearClip / spaceFarClip. The solar camera hard-codes near 1.0 in the
/// code (0x101C0FF): the scene scales it with `SOLAR_SCALE`.
pub const NEAR_CLIP: f32 = 0.05;
pub const SOLAR_NEAR_CLIP: f32 = 1.0;
pub const FAR_CLIP: f32 = 10_000.0;
/// Camera far plane while in Space (spaceFarClip). The galaxy map sits 20,000
/// below the system scene, so neither is ever inside the other's far plane.
pub const SPACE_FAR: f32 = FAR_CLIP;

/// Limits and curves of one orbit camera context (retail units).
#[derive(Clone, Copy, Debug)]
pub struct OrbitLimits {
    pub min: f32,
    pub max: f32,
    /// Zoom stops here until the player pushes again, then (when allowed)
    /// the camera descends into the next view.
    pub floor: f32,
    pub ceiling: f32,
    /// Manual zoom rate (radius units / s) at `min` and at `max` radius.
    pub rate: (f32, f32),
    pub auto_in: (f32, f32),
    pub auto_out: (f32, f32),
    pub discrete: &'static [f32],
    /// (radius, polar angle in degrees) breakpoints of the pitch curve.
    pub phi: &'static [(f32, f32)],
    /// Largest polar angle (radians): a solar camera never goes below the
    /// plane, the galaxy camera may look from 135 degrees.
    pub phi_max: f32,
    /// (start, end) field of view in degrees across min..max radius.
    pub fov: (f32, f32),
}

/// solar* properties.
pub const SOLAR: OrbitLimits = OrbitLimits {
    min: 5.0,
    max: 1750.0,
    floor: 40.0,
    ceiling: 1200.0,
    rate: (250.0, 5000.0),
    auto_in: (20.0, 2000.0),
    auto_out: (10.0, 2000.0),
    discrete: &[500.0],
    phi: &[(5.0, 25.0), (40.0, 60.0), (100.0, 75.0), (900.0, 50.0)],
    phi_max: std::f32::consts::FRAC_PI_2,
    fov: (60.0, 60.0),
};
/// galaxy* properties (the galaxy has no zoom ceiling below its max radius).
pub const GALAXY: OrbitLimits = OrbitLimits {
    min: 0.4,
    max: 1200.0,
    floor: 1.0,
    ceiling: 1200.0,
    rate: (1.5, 3500.0),
    auto_in: (1.0, 1000.0),
    auto_out: (1.0, 1000.0),
    discrete: &[5.0, 40.0, 200.0, 1200.0],
    phi: &[(1.0, 50.0), (40.0, 45.0), (700.0, 30.0)],
    phi_max: std::f32::consts::PI * 0.75,
    fov: (60.0, 60.0),
};
/// The solar camera's zoom floor (0x101B010) and its minimum drawn radius
/// (0x101BF30) are 400 when the active star is not a star (black hole,
/// proto-planetary disk, galactic core).
pub const SOLAR_FLOOR_NON_STAR: f32 = 400.0;

/// solarZoomOutStartRadius / EndRadius: leaving a planet starts the solar
/// camera close and pulls it out.
pub const SOLAR_LEAVE_PLANET: (f32, f32) = (5.0, 200.0);
/// galaxyZoomOutStartRadius / EndRadius: leaving a system.
pub const GALAXY_LEAVE_SYSTEM: (f32, f32) = (0.7, 5.0);
/// Camera drift ("breathing", driftFrequency/Amplitude 0..2).
pub const DRIFT_FREQ: [f32; 3] = [0.017, 0.033, 0.02517];
pub const DRIFT_AMP: [f32; 3] = [0.05, 0.05, 0.1];

/// planetCameraDistanceFOV rows: (altitude, camera distance, look-at height
/// offset, camera ELEVATION degrees), highest altitude first. The property is
/// named "FOV" but 0x101CFE0 uses the fourth value as the angle that swings the
/// camera up above the ship's heading (see the module docs).
pub const PLANET_TABLE: &[[f32; 4]] = &[
    [3000.0, 200.0, 0.0, 60.0],
    [700.0, 160.0, 0.0, 70.0],
    [600.0, 150.0, 0.0, 80.0],
    [500.0, 200.0, 0.0, 80.0],
    [300.0, 300.0, 0.0, 80.0],
    [100.0, 200.0, -40.0, 50.0],
    [30.0, 80.0, -15.0, 30.0],
    [20.0, 70.0, -10.0, 20.0],
];
/// planetCameraCombatTuning (weapon tools armed).
pub const PLANET_COMBAT_TABLE: &[[f32; 4]] = &[
    [3000.0, 200.0, 0.0, 60.0],
    [700.0, 160.0, 0.0, 70.0],
    [600.0, 150.0, 0.0, 80.0],
    [500.0, 300.0, 0.0, 80.0],
    [100.0, 200.0, 0.0, 60.0],
    [20.0, 100.0, 0.0, 40.0],
];
/// planetCameraBusterTuning (planet buster).
pub const PLANET_BUSTER_TABLE: &[[f32; 4]] = &[
    [1500.0, 500.0, 0.0, 85.0],
    [600.0, 900.0, 0.0, 85.0],
    [100.0, 1400.0, 0.0, 85.0],
    [20.0, 1480.0, 0.0, 85.0],
];
/// UFO altitude limits (spaceufo ufoMinAltitude / ufoMaxAltitude).
pub const UFO_MIN_ALTITUDE: f32 = 20.0;
pub const UFO_MAX_ALTITUDE: f32 = 1600.0;
/// planetZoomCeiling, planetMaxHeight, planetZoomInStartHeight / EndHeight.
pub const PLANET_ZOOM_CEILING: f32 = 700.0;
pub const PLANET_MAX_HEIGHT: f32 = 1200.0;
pub const PLANET_ZOOM_IN: (f32, f32) = (1000.0, 275.0);
/// planetZoomRateMin / Max (altitude units / s per unit of zoom velocity).
pub const PLANET_ZOOM_RATE: (f32, f32) = (900.0, 3600.0);
/// planetAtmosphereStart / End (the atmosphere fades the stars by altitude).
pub const PLANET_ATMOSPHERE: (f32, f32) = (351.0, 1200.0);
/// planetCameraDistanceInterpolationRate / Terrain / FOV rates, ramp, tolerance.
pub const PLANET_INTERP: (f32, f32, f32) = (10.0, 1.0, 1.0);
pub const PLANET_INTERP_RAMP: f32 = 2.0;
pub const PLANET_INTERP_TOLERANCE: f32 = 0.01;
/// planetCameraRotationSpeed (degrees / s at the top, see `planet_pitch_rate`)
/// and planetCameraMouseRotationSpeed (degrees per pixel).
pub const PLANET_ROTATION_SPEED: f32 = 60.0;
pub const PLANET_MOUSE_ROTATION: f32 = 0.3;
/// Key rotation of every camera (0.3 x 0.005 rad x ms = 1.5 rad/s): the
/// retail rotate keys on the solar and galaxy cameras.
pub const KEY_ROTATE_RATE: f32 = 1.5;
/// The planet camera's `,` `.` keys: 0.3 x 0.3 degrees per millisecond.
pub const PLANET_KEY_ROTATE: f32 = 90.0;
/// Mouse drag on the solar / galaxy cameras (radians per pixel, 0x1003B7C).
pub const MOUSE_ROTATE: f32 = 0.005;

// --- the scene's scale (ADAPTED) ------------------------------------------

/// ADAPTED: scene units per retail solar-camera unit. The home system of the
/// scene (extent 104) is about a quarter of a retail system (orbits to ~400),
/// so the discrete zoom 500 shows all of it, as it does in the original.
pub const SOLAR_SCALE: f32 = 0.26;
/// The scene's galaxy map was drawn for the retail galaxy camera radii.
pub const GALAXY_SCALE: f32 = 1.0;
/// Retail planet radius (planet-re 1A). Over a world the planet view works
/// in retail proportion: a scene planet of radius r stands for the retail
/// planet of radius 500, so one retail unit is `PlanetView::unit` = r / 500
/// scene units for the UFO altitude, the camera table, the clip planes, the
/// UFO's speed and size (`space_planets` draws the real surface at that scale).
pub const RETAIL_PLANET_RADIUS: f32 = crate::planet_query::PLANET_RADIUS;
/// Planet camera near clip (retail 2.0; far 3000, space-ctrl notes 4).
pub const PLANET_NEAR_CLIP: f32 = 2.0;
/// Horizon angle (0x10186B0, 15 degrees): the planet camera never drops below
/// this elevation above the ship's tangent plane, so it does not look under
/// the horizon.
pub const HORIZON_ANGLE_DEG: f32 = 15.0;
/// ADAPTED: the camera's clearance over the terrain under the eye, retail
/// units. Retail has no such constant (decoded: 0x101C630 sets the eye to the
/// first terrain hit of the ray from the look-at point, 0xB18110; the free
/// camera 0x101EC30 does the same); the clearance keeps the hit point out of
/// the near clip (`PLANET_NEAR_CLIP`) and the ground.
pub const CAMERA_TERRAIN_CLEARANCE: f32 = 5.0;
/// The pitch offset's change per second while the camera's ray is blocked
/// (0x101C630: `+0x78 -= 0.3 x dt` at 0x101CF84, the constant at 0x15B6DBC).
pub const CAMERA_BLOCKED_PITCH_RATE: f32 = 0.3;
/// 0x101C630 only runs within this distance (retail units, `fadd [0x13F08A0]`
/// = 200) above the planet's radius.
pub const CAMERA_RAY_RANGE: f32 = 200.0;
/// planetCameraMaxAltitudeForRotation (0x46D60AD, used by 0xFF18F30): the
/// camera swings toward the ship's movement only below this height.
pub const PLANET_ROTATION_MAX_ALTITUDE: f32 = 200.0;
/// The player's pitch offset keeps the camera's total elevation between 1 and
/// 89 degrees (the 91 / 179 degree bounds of 0x10197F0 against the angle from
/// the up axis).
pub const ELEVATION_LIMITS_DEG: (f32, f32) = (1.0, 89.0);
/// Rate (1/s) at which the pitch offset returns to nothing while the player
/// is not pitching (`min(2 dt, 1)`, 0x10197F0).
pub const PITCH_RETURN_RATE: f32 = 2.0;
/// The eye counts as "near the ground" below this height above the surface
/// (0x101C630 only acts within planet radius + 200).
pub const NEAR_GROUND_HEIGHT: f32 = 200.0;
/// Planet discrete zoom in (End): the zoom altitude goes to the terrain plus
/// 2 x ufoMinAltitude (0x101B8C0).
pub const PLANET_DISCRETE_IN_ALTITUDE: f32 = 2.0 * UFO_MIN_ALTITUDE;
/// `galacticRevolutionMin/MaxDistance` and `RateMin/Max` (`SpaceGalacticConstants`
/// 0x1106D054, read by 0x10172B0): the galaxy camera's azimuth drifts by
/// `lerp(rateMin, rateMax, t) x 0.0001` radians per millisecond, t the radius
/// between the two distances.
pub const GALAXY_REVOLUTION_DISTANCE: (f32, f32) = (10.0, 1500.0);
pub const GALAXY_REVOLUTION_RATE: (f32, f32) = (0.0, -0.04);
/// ADAPTED: the UFO's size over a world in retail units (the player's ship
/// model is fitted to `SHIP_MODEL_SIZE`; the retail creation bounds were not
/// read): about a city hall (13 units, `space_planets`), a quarter of the
/// 20-degree view at altitude 20.
pub const UFO_RETAIL_SIZE: f32 = 12.0;
/// The player's ship model size in the scene (`creation_assets` fit 2.4).
pub const SHIP_MODEL_SIZE: f32 = 2.4;

/// The planet surface under a direction from the world's centre: the height
/// above the retail radius 500 in retail units (the water surface over the
/// sea), from the planet engine (`space_planets`). Without it the world is
/// its plain sphere.
#[derive(Clone)]
pub struct TerrainProbe(pub std::sync::Arc<dyn Fn(Vec3) -> f32 + Send + Sync>);

impl std::fmt::Debug for TerrainProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TerrainProbe")
    }
}

/// ADAPTED: `ufoSolarSpeed` 0.25 per ms = 250 retail units per second, scaled
/// into the scene (x `SOLAR_SCALE` = 65), divided by the typical `Mover` speed
/// of about 20 so `space_fly` keeps using `Mover::speed x flight_scale`.
pub const SOLAR_SPEED_SCALE: f32 = 0.25 * 1000.0 * SOLAR_SCALE / 20.0;

// --- legacy names ----------------------------------------------------------

/// Closest system-scene camera distance (the planet view) and the default
/// solar-system distance, in scene units.
pub const PLANET_DISTANCE: f32 = 13.0;
pub const SYSTEM_DISTANCE: f32 = 500.0 * SOLAR_SCALE;
pub const GALAXY_NEAR: f32 = 5.0;
pub const GALAXY_DEFAULT: f32 = 200.0;
/// Wheel notches multiply the distance by this factor (`zoom_by`, tests and
/// the legacy HUD only: the controls use the velocity ramp).
const ZOOM_STEP: f32 = 1.22;

// ---------------------------------------------------------------------------
// Pure maths
// ---------------------------------------------------------------------------

const EPS: f32 = 1.525_878_9e-5;

/// Piecewise-linear value of an ascending `(x, y)` table, clamped at its ends
/// (0x1017450 for the `phi` curves).
pub fn curve(points: &[(f32, f32)], x: f32) -> f32 {
    let Some(&(first_x, first_y)) = points.first() else {
        return 0.0;
    };
    if x <= first_x {
        return first_y;
    }
    for pair in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    points[points.len() - 1].1
}

impl OrbitLimits {
    /// Polar angle (radians from straight above the plane) of the pitch curve
    /// at `radius`.
    pub fn phi_curve(&self, radius: f32) -> f32 {
        curve(self.phi, radius).to_radians()
    }

    /// Radius units per second that a full zoom push moves the target at
    /// (0x1017630): linear in the radius between the min and max radius.
    pub fn zoom_rate(&self, radius: f32, auto: bool, zooming_in: bool) -> f32 {
        let (lo, hi) = if !auto {
            self.rate
        } else if zooming_in {
            self.auto_in
        } else {
            self.auto_out
        };
        let t = ((radius - self.min) / (self.max - self.min)).clamp(0.0, 1.0);
        lo + (hi - lo) * t
    }

    /// Field of view (degrees) across the radius range.
    pub fn fov_deg(&self, radius: f32) -> f32 {
        let t = (radius - self.min) / (self.max - self.min);
        self.fov.0 + (self.fov.1 - self.fov.0) * t
    }

    /// The next discrete zoom level in or out of `radius` (0x101B3E0): the
    /// largest listed radius below it (else the minimum) or the smallest above
    /// it (else the maximum).
    pub fn discrete_target(&self, radius: f32, zoom_out: bool) -> f32 {
        if zoom_out {
            self.discrete
                .iter()
                .copied()
                .find(|level| *level > radius)
                .unwrap_or(self.max)
        } else {
            self.discrete
                .iter()
                .rev()
                .copied()
                .find(|level| *level < radius)
                .unwrap_or(self.min)
        }
    }
}

/// Slide `x` toward `target` with a speed limit and an acceleration limit
/// (0x1042580). Units are per millisecond, `dt_ms` the frame in milliseconds:
/// the original calls it with `vmax = rate / 1000`, `amax = 0.01 * vmax`.
pub fn smooth_to(x: &mut f32, v: &mut f32, target: f32, vmax: f32, amax: f32, dt_ms: f32) {
    if dt_ms <= 0.0 {
        return;
    }
    let dist = (target - *x).abs();
    if dist < EPS {
        *x = target;
        *v = 0.0;
        return;
    }
    let arrive = dist / dt_ms;
    let brake = (dist * amax).sqrt();
    let speed = arrive.min(brake).min(vmax);
    let desired = if target <= *x { -speed } else { speed };
    let accel = ((desired - *v) / dt_ms).clamp(-amax, amax);
    *v = (accel * dt_ms + *v).clamp(-vmax, vmax);
    *x += *v * dt_ms;
}

/// The camera follows its focus with the same limiter in three dimensions
/// (0x10426B0, `vmax = 1.0` per ms, `amax = 0.001` per ms squared).
pub fn follow_focus(focus: &mut Vec3, velocity: &mut Vec3, target: Vec3, vmax: f32, amax: f32, dt_ms: f32) {
    if dt_ms <= 0.0 {
        return;
    }
    let offset = target - *focus;
    let dist = offset.length();
    if dist <= EPS && velocity.length() <= EPS {
        *focus = target;
        *velocity = Vec3::ZERO;
        return;
    }
    let speed = (dist / dt_ms).min((dist * amax * 2.0).sqrt()).min(vmax);
    let direction = if dist > EPS { offset / dist } else { velocity.normalize_or_zero() };
    let mut accel = (direction * speed - *velocity) / dt_ms;
    if accel.length() > amax {
        accel = accel.normalize_or_zero() * amax;
    }
    *velocity += accel * dt_ms;
    if velocity.length() > vmax {
        *velocity = velocity.normalize_or_zero() * vmax;
    }
    *focus += *velocity * dt_ms;
}

/// Mouse wheel (and held zoom key) velocity ramp (0x101B9C0 records each
/// notch, 0x101AD50 turns the recent notches into a velocity in -1..1).
#[derive(Clone, Debug, Default)]
pub struct WheelRamp {
    history: Vec<(f32, bool)>,
    /// 0 idle, +1 a zoom-in notch arrived this frame, -1 a zoom-out notch.
    pending: i8,
    /// Zoom velocity: +1 zooming in at full speed, -1 out.
    pub v: f32,
}

/// What the ramp's velocity change should sound like (`ui_camera_zoom_out`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomCue {
    None,
    OutStarted,
    InStarted,
}

/// Notches count for this long (seconds) when ramping up.
const WHEEL_WINDOW: f32 = 1.0;
/// Without a notch for this long the velocity starts to decay.
const WHEEL_IDLE: f32 = 0.2;
/// Seconds of the rise: the velocity approaches +-1 at `3 n` per second.
const WHEEL_RISE: f32 = 0.1;
/// The decay time constant: `v *= exp(-15 dt)`.
const WHEEL_DECAY: f32 = 0.02;

impl WheelRamp {
    /// A wheel notch (positive: zoom in).
    pub fn notch(&mut self, now: f32, zoom_in: bool) {
        self.history.push((now, zoom_in));
        self.pending = if zoom_in { 1 } else { -1 };
    }

    /// Forget the recent notches and stop (0x101AFA0): the zoom floor and the
    /// ceiling make the player push again.
    pub fn reset(&mut self) {
        self.history.clear();
        self.pending = 0;
        self.v = 0.0;
    }

    pub fn update(&mut self, now: f32, dt: f32) -> ZoomCue {
        let before = self.v;
        if let Some(&(_, last_dir)) = self.history.last() {
            // Keep the unbroken run of same-direction notches of the last second.
            let mut cut = None;
            for (i, &(time, dir)) in self.history.iter().enumerate() {
                if dir != last_dir || now - time > WHEEL_WINDOW {
                    cut = Some(i);
                }
            }
            if let Some(i) = cut {
                self.history.drain(..=i);
            }
        }
        if self.pending == 0 {
            let idle = self
                .history
                .last()
                .is_none_or(|&(time, _)| WHEEL_IDLE < now - time);
            if idle {
                self.v *= (-0.301_03 * dt / WHEEL_DECAY).exp();
            }
        } else {
            let goal = if self.pending > 0 { 1.0 } else { -1.0 };
            let n = self.history.len() as f32;
            let keep = (-0.301_03 * (n / WHEEL_RISE) * dt).exp();
            self.v += (goal - self.v) * (1.0 - keep);
        }
        self.pending = 0;
        if self.v.abs() < 1.0e-4 {
            self.v = 0.0;
        }
        if self.v < -EPS && -EPS <= before {
            ZoomCue::OutStarted
        } else if self.v > EPS && before < EPS && before != EPS {
            ZoomCue::InStarted
        } else {
            ZoomCue::None
        }
    }
}

/// What lets the orbit camera pass its zoom floor / ceiling (the three
/// `Can...` routines 0x10422F0, 0x10423E0, 0x1042460 all need the UFO at its
/// destination).
#[derive(Clone, Copy, Debug, Default)]
pub struct ZoomGate {
    /// Zooming in below the floor enters the next view down.
    pub can_in: bool,
    /// Zooming out above the ceiling leaves the system (solar only).
    pub can_out: bool,
    /// The ship is at its destination (idle).
    pub idle: bool,
}

/// A completed move into the next view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crossing {
    /// Solar -> planet, galaxy -> solar.
    In,
    /// Solar -> galaxy.
    Out,
}

/// The solar / galaxy camera state (retail units).
#[derive(Clone, Debug)]
pub struct OrbitCam {
    pub radius: f32,
    /// Where the radius is heading (0xB4); the input moves it, the limiter
    /// carries the radius there.
    pub target: f32,
    pub rvel: f32,
    prev_radius: f32,
    /// Azimuth (0x6C) and polar angle (0x70) in the camera's z-up frame.
    pub theta: f32,
    pub phi: f32,
    /// The player's pitch adjustment on top of the curve (0x80).
    pub phi_offset: f32,
    /// An automatic zoom (discrete step, entry pull-out) is running (0xD7).
    pub auto: bool,
    at_floor: bool,
    at_ceiling: bool,
    pending: Option<Crossing>,
    drift_t: f32,
}

impl OrbitCam {
    pub fn new(radius: f32, theta: f32, phi: f32) -> Self {
        Self {
            radius,
            target: radius,
            rvel: 0.0,
            prev_radius: radius,
            theta,
            phi,
            phi_offset: 0.0,
            auto: false,
            at_floor: false,
            at_ceiling: false,
            pending: None,
            drift_t: 0.0,
        }
    }

    /// Jump to `radius` (0x101B370) and forget any movement.
    pub fn place(&mut self, radius: f32) {
        self.radius = radius;
        self.target = radius;
        self.prev_radius = radius;
        self.rvel = 0.0;
        self.auto = false;
        self.pending = None;
        self.at_floor = false;
        self.at_ceiling = false;
    }

    /// Zoom automatically to `target` (entry pull-outs, discrete steps).
    pub fn auto_zoom(&mut self, target: f32) {
        self.auto = true;
        self.target = target;
    }

    /// One discrete step in (End, Ctrl+) or out (Home, Ctrl-), 0x101B3E0.
    pub fn discrete_step(&mut self, lim: &OrbitLimits, zoom_out: bool) {
        self.at_floor = false;
        self.at_ceiling = false;
        self.auto = true;
        self.target = lim.discrete_target(self.radius, zoom_out);
        self.pending = if self.target <= lim.min + EPS {
            Some(Crossing::In)
        } else if self.target >= lim.max - EPS {
            Some(Crossing::Out)
        } else {
            None
        };
    }

    /// The player's push on the zoom (`v`: the wheel ramp velocity), 0x101B010.
    pub fn push(&mut self, lim: &OrbitLimits, v: f32, dt: f32, gate: ZoomGate, solar: bool, wheel: &mut WheelRamp) {
        let r = self.radius;
        let mut movement = -(v * dt);
        let mut lower = lim.min;
        let mut upper = lim.max;
        // A push against a running automatic zoom cancels it.
        if self.auto
            && lim.floor <= r
            && r <= lim.ceiling
            && movement.abs() > EPS
            && (self.target - r) * movement < 0.0
        {
            self.auto = false;
            self.pending = None;
        }
        if !self.auto {
            if !gate.can_in || lim.floor < r {
                lower = lim.floor;
                self.at_floor = false;
            } else if r == lim.floor {
                if !self.at_floor {
                    self.at_floor = true;
                    wheel.reset();
                    movement = 0.0;
                }
            } else {
                self.auto = true;
                self.target = lim.min;
                self.pending = Some(Crossing::In);
            }
            if !solar || !gate.can_out || r < lim.ceiling {
                upper = lim.ceiling;
                self.at_ceiling = false;
            } else if r == lim.ceiling {
                if !self.at_ceiling {
                    self.at_ceiling = true;
                    wheel.reset();
                    movement = 0.0;
                }
            } else {
                self.auto = true;
                self.target = lim.max;
                self.pending = Some(Crossing::Out);
            }
        }
        if !self.auto {
            let rate = lim.zoom_rate(r, false, false);
            self.target = (rate * movement + r).clamp(lower, upper);
        }
    }

    /// Carry the radius toward its target, run the pitch rules and the drift
    /// (the tail of 0x101BF30 and 0x101A200). Returns a completed crossing.
    pub fn step(&mut self, lim: &OrbitLimits, dt: f32, gate: ZoomGate) -> Option<Crossing> {
        if (self.radius - self.target).abs() < EPS {
            if self.auto {
                self.auto = false;
            }
        }
        let zooming_in = self.target <= self.radius && self.radius != self.target;
        let rate = lim.zoom_rate(self.radius, self.auto, zooming_in);
        let dt_ms = dt * 1000.0;
        smooth_to(
            &mut self.radius,
            &mut self.rvel,
            self.target,
            rate * 0.001,
            0.01 * rate * 0.001,
            dt_ms,
        );
        if !self.auto {
            self.radius = self.radius.clamp(lim.min, lim.max);
        }
        // The pitch offset shrinks toward the end of the range being
        // approached, so the extremes always show the curve's angle.
        let edge = if self.radius <= self.prev_radius { lim.min } else { lim.max };
        if self.prev_radius != edge {
            self.phi_offset = (self.radius - edge) * self.phi_offset / (self.prev_radius - edge);
        }
        self.phi = (self.phi_offset + lim.phi_curve(self.radius)).clamp(0.1, lim.phi_max);
        self.theta = wrap_angle(self.theta);
        self.prev_radius = self.radius;
        self.drift_t += dt;
        // A finished automatic zoom to an extreme completes the crossing.
        if let Some(crossing) = self.pending {
            let arrived = (self.radius - self.target).abs() < EPS
                && (self.target <= lim.min + EPS || self.target >= lim.max - EPS);
            if arrived {
                self.pending = None;
                let allowed = match crossing {
                    Crossing::In => gate.can_in && gate.idle,
                    Crossing::Out => gate.can_out && gate.idle,
                };
                if allowed {
                    return Some(crossing);
                }
            }
        }
        None
    }

    /// Rotate by `d_theta` / `d_phi` radians (0x1003B00 on this camera): the
    /// pitch offset follows so the curve stays the reference.
    pub fn rotate(&mut self, lim: &OrbitLimits, d_theta: f32, d_phi: f32) {
        self.theta = wrap_angle(self.theta + d_theta);
        self.phi = (self.phi + d_phi).clamp(0.1, lim.phi_max);
        self.phi_offset = self.phi - lim.phi_curve(self.radius);
    }

    /// Slow camera drift: (radius, theta, phi) additions (driftAmplitude 0..2).
    pub fn drift(&self) -> (f32, f32, f32) {
        let t = self.drift_t;
        (
            (DRIFT_FREQ[0] * t).sin() * DRIFT_AMP[0],
            (DRIFT_FREQ[1] * t).sin() * DRIFT_AMP[1],
            (DRIFT_FREQ[2] * t).sin() * DRIFT_AMP[2],
        )
    }
}

pub fn wrap_angle(angle: f32) -> f32 {
    let mut a = angle.rem_euclid(std::f32::consts::TAU);
    if a >= std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    a
}

/// The retail spherical offset, z-up (0x10424D0).
pub fn spherical(radius: f32, theta: f32, phi: f32) -> Vec3 {
    Vec3::new(
        theta.cos() * phi.sin() * radius,
        theta.sin() * phi.sin() * radius,
        phi.cos() * radius,
    )
}

/// The scene's y-up frame: the retail z axis is up, the retail y axis is the
/// scene's -z (a proper rotation, so angles keep their sense).
pub fn to_scene(retail: Vec3) -> Vec3 {
    Vec3::new(retail.x, retail.z, -retail.y)
}

/// Interpolated `(distance, look-at offset, elevation degrees)` of a planet camera
/// table at an altitude (0x10433F0 + 0x1043320: piecewise linear between the
/// bracketing rows; outside the table the last row, as the original does).
pub fn planet_row(table: &[[f32; 4]], altitude: f32) -> (f32, f32, f32) {
    let last = table[table.len() - 1];
    for pair in table.windows(2) {
        let (high, low) = (pair[0], pair[1]);
        if altitude <= high[0] && low[0] < altitude {
            let t = (altitude - low[0]) / (high[0] - low[0]);
            let mix = |a: f32, b: f32| b + (a - b) * t;
            return (mix(high[1], low[1]), mix(high[2], low[2]), mix(high[3], low[3]));
        }
    }
    (last[1], last[2], last[3])
}

/// The planet camera's zoom speed for an altitude: `planetZoomRateMin..Max`
/// across the UFO's altitude range up to `planetMaxHeight`.
pub fn planet_zoom_rate(altitude: f32) -> f32 {
    let t = ((altitude - UFO_MIN_ALTITUDE) / (PLANET_MAX_HEIGHT - UFO_MIN_ALTITUDE)).clamp(0.0, 1.0);
    PLANET_ZOOM_RATE.0 + (PLANET_ZOOM_RATE.1 - PLANET_ZOOM_RATE.0) * t
}

/// Pitch speed of the planet camera's PageUp / PageDown keys (radians / s):
/// 60 x 3 degrees at height, 1 x 3 degrees at the ground (0xFFF05D).
pub fn planet_pitch_rate(altitude: f32) -> f32 {
    let t = ((altitude - UFO_MIN_ALTITUDE) / (PLANET_MAX_HEIGHT - UFO_MIN_ALTITUDE)).clamp(0.0, 1.0);
    let near = 1.0 - t;
    ((1.0 - PLANET_ROTATION_SPEED) * near + PLANET_ROTATION_SPEED) * 0.052_359_88
}

/// Atmosphere amount (0 in space, 1 at the ground) for an altitude
/// (planetAtmosphereStart / End, a smoothstep as `FUN_00B0EFC0`).
pub fn atmosphere_amount(altitude: f32) -> f32 {
    let t = ((PLANET_ATMOSPHERE.1 - altitude) / (PLANET_ATMOSPHERE.1 - PLANET_ATMOSPHERE.0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Azimuth drift of the galaxy camera in radians per millisecond (0x10172B0):
/// `lerp(rateMin, rateMax, t) x 0.0001`, `t` the radius' place between the two
/// revolution distances (`SpaceGalacticConstants`).
pub fn galaxy_revolution_rate(radius: f32) -> f32 {
    let (near, far) = GALAXY_REVOLUTION_DISTANCE;
    let t = ((radius - near) / (far - near)).clamp(0.0, 1.0);
    (GALAXY_REVOLUTION_RATE.0 + (GALAXY_REVOLUTION_RATE.1 - GALAXY_REVOLUTION_RATE.0) * t) * 0.0001
}

/// The vertical field of view (radians) of a camera FOV in degrees through the
/// retail projection (cViewer 0x7C5400): the half-angle tangent is
/// `tan(fov / 2)`, times 0.75 on a non-square view.
pub fn vertical_fov(fov_deg: f32, aspect: f32) -> f32 {
    crate::cell_editor_scene::retail_vertical_fov(fov_deg.to_radians(), aspect)
}

/// solarPullOutUp / solarPullOutRight (`SpaceCamera` prop, loaded into
/// DAT_016dd390 / DAT_016dd38c by 0x1020700): the half-angle tangents of the
/// wedge the focus must stay in at full zoom-out, along the screen's up and right.
pub const SOLAR_PULL_OUT_UP: f32 = 0.4;
pub const SOLAR_PULL_OUT_RIGHT: f32 = 1.2;

/// The solar camera's zoom-out fraction (`this+0xBC`, 0x101A200): the radius'
/// place between the solar minimum and maximum radius.
pub fn solar_zoom_fraction(radius: f32) -> f32 {
    ((radius - SOLAR.min) / (SOLAR.max - SOLAR.min)).clamp(0.0, 1.0)
}

/// The solar pull-out (0x1017F80, called by 0x101BF30 with `T` = `this+0x8C`
/// = the star (the zero vector set by the constructor 0x101A030), `F` = the
/// smoothed focus (`this+0x98`) and `O` = the eye's offset from the focus
/// (`spherical(radius, theta, phi)`)): the camera looks at
/// `T + (F - T) t` and sits `O` away from there, `t` between `1 - fraction`
/// and 1 (`fraction` = `solar_zoom_fraction`). So zooming out slides the
/// look-at point from the focus toward the star, as far as the focus stays
/// inside the camera's wedge: four planes through the eye with the normals
/// `k_up x fraction x D +- V` and `k_right x fraction x D +- H` (`D` the unit
/// offset, `H = z x D` the screen's right, `V = D x H` its up, the ecliptic
/// normal `z` = `up`); a plane the focus would cross at `t` below its ratio
/// raises `t` to `n.(F - T - O) / n.(F - T)` (the retail code, which reads like
/// the sun is the object kept in view but is really the focus: that ratio is
/// where `n.(F - E') = 0` for the pulled eye `E'`). `min_distance` is the
/// retail 1.0 under which nothing is pulled, in the caller's units.
pub fn solar_pull_out(target: Vec3, focus: Vec3, offset: Vec3, fraction: f32, up: Vec3, min_distance: f32) -> Vec3 {
    let d = focus - target;
    if d.length_squared() <= min_distance * min_distance {
        return focus;
    }
    let w = d - offset;
    let dir = offset.normalize_or_zero();
    let h = up.cross(dir);
    if dir == Vec3::ZERO || h.length_squared() < 1.0e-12 {
        return focus;
    }
    let h = h.normalize();
    let v = dir.cross(h);
    let up_k = fraction * SOLAR_PULL_OUT_UP;
    let right_k = fraction * SOLAR_PULL_OUT_RIGHT;
    let mut t = 1.0 - fraction;
    for normal in [dir * up_k + v, dir * up_k - v, dir * right_k + h, dir * right_k - h] {
        let n = normal.normalize_or_zero();
        let numerator = n.dot(w);
        if numerator > 0.0 {
            let ratio = numerator / n.dot(d);
            if ratio > t {
                t = ratio;
            }
        }
    }
    target + d * t.min(1.0)
}

// ---------------------------------------------------------------------------
// --- state ---
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceZoom {
    Planet,
    System,
    Galaxy,
}

impl SpaceZoom {
    pub fn label(self) -> &'static str {
        match self {
            SpaceZoom::Planet => "Planet view",
            SpaceZoom::System => "Solar system",
            SpaceZoom::Galaxy => "Galaxy",
        }
    }
}

/// An interstellar jump in progress (drawn on the galaxy map).
#[derive(Clone, Copy, Debug)]
pub struct Travel {
    pub from: u32,
    pub to: u32,
    /// 0..1 progress along the route.
    pub t: f32,
}

/// Seconds an interstellar jump takes on the map.
pub const TRAVEL_SECONDS: f32 = 1.6;

/// Starmap filter mode in Galactic view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GalaxyFilter {
    #[default]
    All,
    Colonies,
    Empires,
    Visited,
    Trade,
}

impl GalaxyFilter {
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Colonies,
        Self::Empires,
        Self::Visited,
        Self::Trade,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All Stars",
            Self::Colonies => "My Colonies",
            Self::Empires => "Empires",
            Self::Visited => "Visited",
            Self::Trade => "Trade Routes",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// The paused camera's distance limits in retail units (0x101EC30 clamps its
/// target distance `+0x88` to 50..500).
pub const FREE_DISTANCE: (f32, f32) = (50.0, 500.0);

/// The paused free camera (mode 2 of 0x1020D60, 0x101EC30): while the game
/// is paused the planet camera stops following the ship's altitude and the
/// table: the look-at point stays where it was, the wheel / zoom keys change
/// the distance (`target`, 50..500), the rotate keys and the camera drag turn
/// and pitch the view around it.
#[derive(Clone, Copy, Debug)]
pub struct FreeCam {
    /// Where the zoom input sent the camera distance (`this+0x88`).
    pub target: f32,
    ramp: f32,
    settling: bool,
}

/// The planet camera and flight state while the ship is over a world.
#[derive(Clone, Debug)]
pub struct PlanetView {
    /// Campaign identity of the world.
    pub world: u32,
    pub center: Vec3,
    pub radius: f32,
    /// The UFO's altitude above the surface in retail units: the zoom.
    pub altitude: f32,
    /// An automatic altitude change is running (the entry descent).
    pub auto: Option<f32>,
    pub rate_v: f32,
    pub at_ceiling: bool,
    /// Unit vector from the planet's centre to the ship, and the ship's heading
    /// (tangent to the surface).
    pub up: Vec3,
    pub heading: Vec3,
    /// Ship velocity in the tangent plane (scene units / s) and the turn ramp.
    pub velocity: Vec3,
    pub turn: f32,
    /// Ship tilt: bank, nose and vertical, radians (visual only).
    pub bank: f32,
    pub nose: f32,
    pub vtilt: f32,
    /// Camera heading relative to the ship's heading (radians, +0x94 in the
    /// original: the camera direction follows the UFO's heading unless the
    /// player rotated it, then it swings toward the movement while the ship
    /// flies low, 0x1018F30).
    pub cam_yaw: f32,
    /// The player rotated the camera (flag +0x198 bit 2); until the camera is
    /// aligned with the ship again it keeps its own heading.
    pub yaw_manual: bool,
    /// The player's pitch offset on top of the table elevation, radians,
    /// positive raises the camera (the original's +0x78 with the opposite
    /// sign); it returns to nothing at `PITCH_RETURN_RATE` unless the player
    /// pitched and the ship is not flying (flag +0x198 bit 1).
    pub pitch_offset: f32,
    pub pitch_manual: bool,
    /// Smoothed table values (retail units, elevation in radians) and the
    /// blend ramp.
    pub dist: f32,
    pub offset: f32,
    pub elevation: f32,
    ramp: f32,
    /// Weapon tools armed / planet buster selected (other tables).
    pub combat: bool,
    pub buster: bool,
    /// The generated surface of the world (`space_planets`), once ready.
    pub terrain: Option<TerrainProbe>,
    /// The paused free camera, while the game is paused (`set_paused`).
    pub free: Option<FreeCam>,
}

impl PlanetView {
    pub fn new(world: u32, center: Vec3, radius: f32, up: Vec3, heading: Vec3, altitude: f32) -> Self {
        let (dist, offset, elevation) = planet_row(PLANET_TABLE, altitude);
        Self {
            world,
            center,
            radius,
            altitude,
            auto: None,
            rate_v: 0.0,
            at_ceiling: false,
            up,
            heading,
            velocity: Vec3::ZERO,
            turn: 0.0,
            bank: 0.0,
            nose: 0.0,
            vtilt: 0.0,
            cam_yaw: 0.0,
            yaw_manual: false,
            pitch_offset: 0.0,
            pitch_manual: false,
            dist,
            offset,
            elevation: elevation.to_radians(),
            ramp: 0.0,
            combat: false,
            buster: false,
            terrain: None,
            free: None,
        }
    }

    /// Scene units per retail unit over this world (`RETAIL_PLANET_RADIUS`).
    pub fn unit(&self) -> f32 {
        self.radius / RETAIL_PLANET_RADIUS
    }

    /// The surface under `dir` above the retail radius, retail units (0 on
    /// the plain sphere).
    pub fn ground(&self, dir: Vec3) -> f32 {
        self.terrain.as_ref().map_or(0.0, |t| (t.0)(dir))
    }

    /// The ship's height above the planet's nominal radius in scene units: the
    /// UFO altitude is measured above the ground under it.
    pub fn height(&self) -> f32 {
        self.altitude * self.unit()
    }

    /// The ship's world position over the planet.
    pub fn ship_position(&self) -> Vec3 {
        self.center + self.up * (self.radius + self.ground(self.up) * self.unit() + self.height())
    }

    /// The planet camera's field of view, degrees: the constant `+0x70` =
    /// spaceFOV passed to cViewer 0x7C5400 (the table's fourth column is the
    /// elevation, see the module docs). Convert with `retail_vertical_fov`.
    pub fn fov_deg(&self) -> f32 {
        FOV_DEG
    }

    /// Camera (eye, look-at, spaceFOV radians) in the scene (0x101CFE0): the
    /// look-at point is the ship plus the table's offset along the up axis,
    /// the eye sits `dist` behind it along the camera heading, swung up about
    /// the ship's right axis by the table's elevation angle plus the player's
    /// pitch offset (the quaternion of the original). The camera stays inside
    /// 1..89 degrees of elevation (0x10197F0) and above the horizon angle
    /// while it is near the ground (0x101C630: its terrain ray, below), and
    /// the eye is the first terrain hit of the ray from the look-at point
    /// (`terrain_hit`; ADAPTED: a clearance over the terrain under the eye and
    /// no pitch bump yet, retail raises the pitch offset by 0.3 x dt while the
    /// ray is blocked).
    pub fn camera(&self) -> (Vec3, Vec3, f32) {
        let unit = self.unit();
        let (mut eye, look) = self.wanted_pose();
        // 0x101C630 (its end, within the planet's radius + 200 of the centre):
        // the ray from the look-at point to the eye is cast against the terrain
        // (0xB16D00 builds the ray, 0xB18110 returns the hit parameter, 0xB163D0
        // the point) and the eye becomes the first terrain hit.
        if (eye - self.center).length() <= self.radius + CAMERA_RAY_RANGE * unit {
            if let Some(hit) = self.terrain_hit(look, eye) {
                eye = hit;
            }
        }
        // Stay above the terrain under the eye (ADAPTED clearance: the retail
        // eye sits exactly on the hit, with no clearance of its own).
        let from_center = eye - self.center;
        let dir = from_center.normalize_or(self.up);
        let floor = self.radius + (self.ground(dir) + CAMERA_TERRAIN_CLEARANCE) * unit;
        if from_center.length() < floor {
            eye = self.center + dir * floor;
        }
        (eye, look, self.fov_deg().to_radians())
    }

    /// The eye and the look-at point the table, the heading and the pitch ask
    /// for (0x101CFE0 with the elevation limits and the horizon rule), before
    /// the terrain rays of 0x101C630.
    fn wanted_pose(&self) -> (Vec3, Vec3) {
        let unit = self.unit();
        let flat = Quat::from_axis_angle(self.up, self.cam_yaw) * self.heading;
        let look = self.ship_position() + self.up * (self.offset * unit);
        let place = |elevation: f32| -> Vec3 {
            // -D cos(e) + up sin(e): behind and above.
            look + (self.up * elevation.sin() - flat * elevation.cos()) * (self.dist * unit)
        };
        let (lo, hi) = (ELEVATION_LIMITS_DEG.0.to_radians(), ELEVATION_LIMITS_DEG.1.to_radians());
        // The paused camera applies the pitch offset only while the player's
        // pitch flag is set (0x101EC30 tests `+0x198` bit 0).
        let pitch = if self.free.is_some() && !self.pitch_manual { 0.0 } else { self.pitch_offset };
        let wanted = (self.elevation + pitch).clamp(lo, hi);
        let mut eye = place(wanted);
        let height = |at: Vec3| -> f32 {
            let from = at - self.center;
            let dir = from.normalize_or(self.up);
            (from.length() - self.radius) / unit - self.ground(dir)
        };
        let horizon = HORIZON_ANGLE_DEG.to_radians();
        if wanted < horizon && height(eye) < NEAR_GROUND_HEIGHT {
            eye = place(horizon);
        }
        (eye, look)
    }

    /// The first point of the segment `from` -> `to` that is under the terrain
    /// of the world (the last point above it, found by bisection after a
    /// coarse march), `None` when the segment stays above the ground or there
    /// is no terrain probe or `from` is already under it (0xB18110: the ray
    /// test of the planet camera's last step).
    pub fn terrain_hit(&self, from: Vec3, to: Vec3) -> Option<Vec3> {
        self.terrain.as_ref()?;
        let unit = self.unit();
        let under = |p: Vec3| -> bool {
            let v = p - self.center;
            v.length() < self.radius + self.ground(v.normalize_or(self.up)) * unit
        };
        if under(from) {
            return None;
        }
        const STEPS: usize = 24;
        let at = |t: f32| from + (to - from) * t;
        let mut above = 0.0;
        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            if under(at(t)) {
                let (mut lo, mut hi) = (above, t);
                for _ in 0..12 {
                    let mid = 0.5 * (lo + hi);
                    if under(at(mid)) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(at(lo));
            }
            above = t;
        }
        None
    }

    /// The last lines of 0x101C630: while the ray from the look-at point to
    /// the eye the camera wants is blocked by the terrain, the pitch offset
    /// (`+0x78`, `-= 0.3 x dt`: the retail sign is the opposite of
    /// `pitch_offset`'s) swings the camera up over the obstacle. Within the
    /// elevation limits (0x10197F0).
    pub fn lift_when_blocked(&mut self, dt: f32) {
        let (eye, look) = self.wanted_pose();
        if (eye - self.center).length() > self.radius + CAMERA_RAY_RANGE * self.unit() || self.terrain_hit(look, eye).is_none() {
            return;
        }
        let hi = ELEVATION_LIMITS_DEG.1.to_radians();
        self.pitch_offset = (self.pitch_offset + CAMERA_BLOCKED_PITCH_RATE * dt).min(hi - self.elevation);
    }

    /// The speed term of the table lookup (0x101CFE0): the camera looks up the
    /// table at `altitude + smoothstep(speed fraction) x ufoMinAltitude`, so it
    /// pulls back while the ship flies fast. ADAPTED: the speed fraction is the
    /// ship's speed over the keyboard top speed (the original divides by
    /// 0xC389A0, a UFO-state velocity limit that was not traced).
    pub fn table_altitude(&self) -> f32 {
        let unit = self.unit().max(1.0e-9);
        let top = crate::space_controls::speed_limits(self.altitude).0 * unit;
        let f = if top > 0.0 { (self.velocity.length() / top).clamp(0.0, 1.0) } else { 0.0 };
        self.altitude + (3.0 - 2.0 * f) * f * f * UFO_MIN_ALTITUDE
    }

    /// The altitude blend of the camera table values (0x101CFE0): the current
    /// values approach the table row of the altitude at the interpolation rates.
    /// Distance, look-at offset AND elevation share the distance rate (the
    /// original lerps +0x6c, +0x80 and +0x84 with the same factor).
    pub fn blend(&mut self, dt: f32) {
        let table = if self.buster {
            PLANET_BUSTER_TABLE
        } else if self.combat {
            PLANET_COMBAT_TABLE
        } else {
            PLANET_TABLE
        };
        let (dist, offset, elevation) = planet_row(table, self.table_altitude());
        let elevation = elevation.to_radians();
        let relative = |a: f32, b: f32| ((a - b) / b.abs().max(1.0e-3)).abs();
        let converged = relative(self.dist, dist) <= PLANET_INTERP_TOLERANCE
            && relative(self.elevation, elevation) <= PLANET_INTERP_TOLERANCE;
        self.ramp = if converged {
            (self.ramp - PLANET_INTERP_RAMP * dt).max(0.0)
        } else {
            (self.ramp + PLANET_INTERP_RAMP * dt).min(1.0)
        };
        let mix = (PLANET_INTERP.0 * self.ramp * dt).min(1.0);
        self.dist += (dist - self.dist) * mix;
        self.offset += (offset - self.offset) * mix;
        self.elevation += (elevation - self.elevation) * mix;
    }

    /// The camera's heading and pitch offset for the frame (0x1018F30,
    /// 0x10197F0): `yaw` turns the camera about the up axis (PageUp / `,` `.`
    /// and the camera drag set the manual flags), `pitch` (positive raises)
    /// changes the pitch offset inside the elevation limits. Without manual
    /// input the camera heading is the ship's; a rotated camera keeps its own
    /// until the ship flies below `PLANET_ROTATION_MAX_ALTITUDE`, when it swings
    /// toward the direction of movement at (speed fraction) per second; the
    /// pitch offset returns to nothing at `PITCH_RETURN_RATE` unless the player
    /// pitched and the ship stands still.
    pub fn steer_camera(&mut self, yaw: f32, pitch: f32, dt: f32, camera_held: bool) {
        // --- heading
        if yaw != 0.0 {
            self.yaw_manual = true;
            self.cam_yaw = wrap_angle(self.cam_yaw + yaw);
        }
        let unit = self.unit().max(1.0e-9);
        let top = crate::space_controls::speed_limits(self.altitude).0 * unit;
        let speed = self.velocity.length();
        let fraction = if top > 0.0 { (speed / top).clamp(0.0, 1.0) } else { 0.0 };
        if !self.yaw_manual {
            self.cam_yaw = 0.0;
        } else if speed > 1.0e-4 * top.max(1.0e-6) && self.altitude < PLANET_ROTATION_MAX_ALTITUDE {
            // The movement direction against the ship's heading.
            let dir = tangent(self.velocity, self.up);
            let target = self.heading.cross(dir).dot(self.up).atan2(self.heading.dot(dir));
            let delta = wrap_angle(target - self.cam_yaw);
            if delta.abs() < std::f32::consts::FRAC_PI_2 {
                self.cam_yaw = wrap_angle(self.cam_yaw + delta * (fraction * dt).min(1.0));
            }
        }
        if self.yaw_manual && self.cam_yaw.abs() < 0.02 && speed > 1.0e-4 * top.max(1.0e-6) {
            self.yaw_manual = false;
            self.cam_yaw = 0.0;
        }
        // --- pitch
        if pitch != 0.0 {
            self.pitch_manual = true;
            let (lo, hi) = (ELEVATION_LIMITS_DEG.0.to_radians(), ELEVATION_LIMITS_DEG.1.to_radians());
            self.pitch_offset = (self.pitch_offset + pitch).clamp(lo - self.elevation, hi - self.elevation);
        }
        // The flag clears while the ship flies and the camera button is up.
        if self.pitch_manual && pitch == 0.0 && !camera_held && speed > 1.0e-4 * top.max(1.0e-6) {
            self.pitch_manual = false;
        }
        if !self.pitch_manual {
            self.pitch_offset -= self.pitch_offset * (PITCH_RETURN_RATE * dt).min(1.0);
            if self.pitch_offset.abs() < 1.0e-4 {
                self.pitch_offset = 0.0;
            }
        }
    }

    /// One frame of the planet zoom (0x101B520): `v` is the zoom velocity
    /// (keys +-0.5, otherwise the wheel ramp). `gate` is the retail gate
    /// 0x1044930 (the planet camera is the active camera, the comm screen is
    /// closed, the game is not paused and no UFO state / object busies the
    /// ship; ADAPTED: the port reads "busy" as the ship flying somewhere on
    /// its own). With the gate the ceiling (`planetZoomCeiling`) stops the
    /// first push, a further push (or arriving above it) leaves; without it
    /// there is no ceiling and no automatic ascent: the altitude only meets
    /// the UFO's own limit (`ufoMaxAltitude` 1600), so a busy ship zooms out
    /// over its world but cannot leave. Returns true when the ship leaves
    /// for the solar view.
    pub fn zoom(&mut self, v: f32, dt: f32, gate: bool, wheel: &mut WheelRamp) -> bool {
        if let Some(goal) = self.auto {
            // The entry descent (planetZoomInStartHeight -> EndHeight).
            let rate = planet_zoom_rate(self.altitude);
            smooth_to(&mut self.altitude, &mut self.rate_v, goal, rate * 0.001, rate * 0.003, dt * 1000.0);
            if (self.altitude - goal).abs() < 0.5 || (v != 0.0 && self.altitude <= PLANET_ZOOM_CEILING) {
                self.auto = None;
                self.rate_v = 0.0;
            }
            return false;
        }
        let rate = planet_zoom_rate(self.altitude);
        if !gate {
            // No ceiling stop, no ascent (0x101B520 skips both when 0x1044930
            // is false; the clamp is the UFO's altitude limit).
            self.at_ceiling = false;
            self.altitude = (self.altitude - v * rate * dt).clamp(UFO_MIN_ALTITUDE, UFO_MAX_ALTITUDE);
            return false;
        }
        if !self.at_ceiling && self.altitude > PLANET_ZOOM_CEILING + 1.0 {
            // Above the ceiling with the gate open (a busy ship zoomed up and
            // arrived): the automatic ascent starts (the radius test
            // `!= ceiling` of 0x101B520).
            return true;
        }
        let mut leave = false;
        let upper = if self.at_ceiling { PLANET_MAX_HEIGHT } else { PLANET_ZOOM_CEILING };
        let next = (self.altitude - v * rate * dt).clamp(UFO_MIN_ALTITUDE, upper);
        if !self.at_ceiling && next >= PLANET_ZOOM_CEILING && self.altitude >= PLANET_ZOOM_CEILING - 1.0e-3 {
            // First arrival at the ceiling: stop and wait for another push.
            self.at_ceiling = true;
            wheel.reset();
            self.altitude = PLANET_ZOOM_CEILING;
            return false;
        }
        if self.at_ceiling && next > PLANET_ZOOM_CEILING + 1.0 {
            leave = true;
        }
        if next < PLANET_ZOOM_CEILING - 1.0 {
            self.at_ceiling = false;
        }
        self.altitude = next;
        leave
    }

    /// Enter or leave the paused free camera (0x1020D60: the game's pause
    /// flag switches the camera to mode 2 and back; entering keeps the current
    /// distance as the free camera's target, leaving returns to the table).
    pub fn set_paused(&mut self, paused: bool) {
        match (paused, self.free.is_some()) {
            (true, false) => {
                self.free = Some(FreeCam {
                    target: self.dist,
                    ramp: 0.0,
                    settling: false,
                })
            }
            (false, true) => {
                self.free = None;
                self.ramp = 0.0;
            }
            _ => {}
        }
    }

    /// One frame of the paused free camera (0x101EC30). `v` is the zoom
    /// velocity (the wheel ramp or +-0.5 for the keys): it moves the target
    /// distance by `planetZoomRateMax x v x dt` inside 50..500 and the
    /// distance settles on it with the planet camera's blend (ramp 2, rate
    /// 10, tolerance 1%). `yaw` / `pitch` (radians, positive raises) turn the
    /// view about the world's up axis and change the pitch offset; unlike the
    /// running camera nothing swings the heading back or returns the pitch.
    pub fn free_step(&mut self, v: f32, yaw: f32, pitch: f32, dt: f32) {
        let Some(mut free) = self.free else { return };
        if v.abs() > EPS {
            free.target = (free.target - PLANET_ZOOM_RATE.1 * v * dt).clamp(FREE_DISTANCE.0, FREE_DISTANCE.1);
            free.settling = true;
        }
        if free.settling {
            let mut ramp = 0.0;
            if ((self.dist - free.target) / free.target).abs() > PLANET_INTERP_TOLERANCE {
                ramp = (free.ramp + PLANET_INTERP_RAMP * dt).min(1.0);
            }
            free.ramp = ramp;
            if ramp <= 0.0 {
                free.settling = false;
            } else {
                self.dist += (free.target - self.dist) * (PLANET_INTERP.0 * ramp * dt).min(1.0);
            }
        }
        if yaw != 0.0 {
            self.yaw_manual = true;
            self.cam_yaw = wrap_angle(self.cam_yaw + yaw);
        }
        if pitch != 0.0 {
            self.pitch_manual = true;
            let (lo, hi) = (ELEVATION_LIMITS_DEG.0.to_radians(), ELEVATION_LIMITS_DEG.1.to_radians());
            self.pitch_offset = (self.pitch_offset + pitch).clamp(lo - self.elevation, hi - self.elevation);
        }
        self.free = Some(free);
    }
}

/// Camera and zoom state of the Space stage.
#[derive(Resource, Debug, Clone)]
pub struct SpaceView {
    /// True while the galaxy map is shown.
    pub galaxy: bool,
    /// Active Starmap filter on the galaxy map.
    pub filter: GalaxyFilter,
    /// Solar-system camera distance in scene units (mirrors `solar.radius`).
    pub system_distance: f32,
    pub system_target: f32,
    /// Galaxy-map camera distance in scene units (mirrors `gal.radius`).
    pub galaxy_distance: f32,
    pub galaxy_target: f32,
    /// Galaxy-map selection (star index).
    pub cursor: u32,
    /// Smoothed galaxy-map focus point in the galactic plane.
    pub galaxy_focus: Vec2,
    pub travel: Option<Travel>,
    /// Star system a finished jump arrived at, until the system is swapped.
    pub arrival: Option<u32>,
    /// Keyboard repeat for stepping the galaxy cursor.
    pub cursor_repeat: f32,
    /// The orbit cameras (retail units) and the wheel velocity ramp.
    pub solar: OrbitCam,
    pub gal: OrbitCam,
    pub wheel: WheelRamp,
    /// The planet view, while the ship is over a world.
    pub planet: Option<PlanetView>,
    /// A planet view was asked for before the ship was over a world
    /// (`--space-view planet`).
    pub want_planet: bool,
    /// Smoothed camera focus (scene units).
    pub focus: Vec3,
    focus_v: Vec3,
    /// Where the solar camera's focus is heading (scene units, 0x101BF30): the
    /// ship while it flies, the active planet (else the star) once it is at
    /// its destination. Set by `zoom_input`; `None` follows the ship.
    pub focus_target: Option<Vec3>,
    /// The world the ship last hovered at or was sent to (`mpActivePlanet` of
    /// `sSpacePlayerData`, 0x1021110): the solar focus parks there.
    pub active_planet: Option<u32>,
    /// After leaving a planet the solar camera follows the ship while it pulls
    /// out (state 2 of 0x101BF30, ADAPTED: until the pull-out has finished).
    follow_ship: bool,
    /// The sound of the last zoom push (consumed by `space_audio`).
    pub zoom_cue: ZoomCue,
    eye: Vec3,
    look: Vec3,
    fov: f32,
    placed: bool,
    /// The world's centre at the last planet-camera frame: the smoothed eye
    /// rides along with the orbiting world.
    anchor: Vec3,
}

/// The scene's default opening orientation: the camera on +z, as before.
const THETA_START: f32 = -std::f32::consts::FRAC_PI_2;

impl Default for SpaceView {
    fn default() -> Self {
        let solar = OrbitCam::new(500.0, THETA_START, SOLAR.phi_curve(500.0));
        let gal = OrbitCam::new(GALAXY_DEFAULT, THETA_START, GALAXY.phi_curve(GALAXY_DEFAULT));
        Self {
            galaxy: false,
            filter: GalaxyFilter::All,
            system_distance: solar.radius * SOLAR_SCALE,
            system_target: solar.radius * SOLAR_SCALE,
            galaxy_distance: gal.radius * GALAXY_SCALE,
            galaxy_target: gal.radius * GALAXY_SCALE,
            cursor: 0,
            galaxy_focus: Vec2::ZERO,
            travel: None,
            arrival: None,
            cursor_repeat: 0.0,
            solar,
            gal,
            wheel: WheelRamp::default(),
            planet: None,
            want_planet: false,
            focus: Vec3::ZERO,
            focus_v: Vec3::ZERO,
            focus_target: None,
            active_planet: None,
            follow_ship: false,
            zoom_cue: ZoomCue::None,
            eye: Vec3::ZERO,
            look: Vec3::ZERO,
            fov: FOV_DEG.to_radians(),
            placed: false,
            anchor: Vec3::ZERO,
        }
    }
}

/// Initial view requested by `--quick-capture ... --space-view <level>`.
#[derive(Resource, Clone, Copy, Debug)]
pub struct SpaceViewRequest(pub SpaceZoom);

impl SpaceViewRequest {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "planet" => Some(Self(SpaceZoom::Planet)),
            "system" => Some(Self(SpaceZoom::System)),
            "galaxy" => Some(Self(SpaceZoom::Galaxy)),
            _ => None,
        }
    }
}

impl SpaceView {
    /// A fresh view centred on `star`, honouring a capture request.
    pub fn entering(star: u32, focus: Vec2, request: Option<SpaceZoom>) -> Self {
        let mut view = Self {
            cursor: star,
            galaxy_focus: focus,
            ..default()
        };
        match request {
            Some(SpaceZoom::Planet) => view.want_planet = true,
            Some(SpaceZoom::Galaxy) => {
                // A capture of the map starts at the overview radius.
                view.open_galaxy();
                view.gal.place(GALAXY_DEFAULT);
                // Capture aid: OPENSPORE_GALAXY_RADIUS=<units> starts the map camera closer (the galactic disk
                // sprites only show inside 100 units of the camera).
                if let Some(radius) = std::env::var("OPENSPORE_GALAXY_RADIUS").ok().and_then(|v| v.parse::<f32>().ok()) {
                    view.gal.place(radius.clamp(GALAXY.min, GALAXY.max));
                }
            }
            _ => {}
        }
        view.sync_distances();
        view
    }

    pub fn zoom(&self) -> SpaceZoom {
        if self.galaxy {
            SpaceZoom::Galaxy
        } else if self.planet.is_some() {
            SpaceZoom::Planet
        } else {
            SpaceZoom::System
        }
    }

    /// Mirror the retail-unit radii into the scene-unit fields other modules
    /// read.
    fn sync_distances(&mut self) {
        self.system_distance = self.solar.radius * SOLAR_SCALE;
        self.system_target = self.solar.target * SOLAR_SCALE;
        self.galaxy_distance = self.gal.radius * GALAXY_SCALE;
        self.galaxy_target = self.gal.target * GALAXY_SCALE;
    }

    /// Ship top-speed multiplier in the solar system: the retail UFO flies at
    /// `ufoSolarSpeed` 0.25 units per millisecond whatever the zoom
    /// (`SOLAR_SPEED_SCALE`); over a planet `space_controls::planet_flight`
    /// has its own speeds.
    pub fn flight_scale(&self) -> f32 {
        SOLAR_SPEED_SCALE
    }

    /// One wheel notch or key step out (`steps > 0`) or in (`steps < 0`):
    /// jumps the target radius by the legacy factor (tests, scripts); the
    /// controls use `wheel` and `push`.
    pub fn zoom_by(&mut self, steps: f32) {
        if steps == 0.0 {
            return;
        }
        let factor = ZOOM_STEP.powf(steps);
        if self.galaxy {
            let next = self.gal.target * factor;
            if next < GALAXY.floor && steps < 0.0 {
                if self.travel.is_none() {
                    self.leave_galaxy();
                }
            } else {
                self.gal.target = next.clamp(GALAXY.min, GALAXY.max);
                self.gal.radius = self.gal.target;
            }
        } else if self.planet.is_some() {
            if steps > 0.0 {
                self.planet = None;
                self.solar.place(SOLAR_LEAVE_PLANET.1);
            }
        } else {
            let next = self.solar.target * factor;
            if next > SOLAR.ceiling && steps > 0.0 {
                self.open_galaxy();
            } else {
                self.solar.target = next.clamp(SOLAR.floor, SOLAR.ceiling);
                self.solar.radius = self.solar.target;
            }
        }
        self.sync_distances();
    }

    /// `Z` (and Home, Ctrl-): one discrete zoom level out.
    pub fn level_out(&mut self) {
        if self.galaxy {
            self.gal.discrete_step(&GALAXY, true);
        } else if self.planet.is_some() {
            self.planet = None;
            self.solar.place(SOLAR_LEAVE_PLANET.0);
            self.solar.auto_zoom(SOLAR_LEAVE_PLANET.1);
        } else {
            self.solar.discrete_step(&SOLAR, true);
        }
        self.sync_distances();
    }

    /// `X`'s old step, End and Ctrl+: one discrete zoom level in.
    pub fn level_in(&mut self) {
        if self.galaxy {
            if self.travel.is_none() {
                self.gal.discrete_step(&GALAXY, false);
            }
        } else if self.planet.is_none() {
            self.solar.discrete_step(&SOLAR, false);
        }
        self.sync_distances();
    }

    /// Leave the system for the galaxy map: the galaxy camera starts close
    /// (galaxyZoomOutStartRadius) and pulls out.
    pub fn open_galaxy(&mut self) {
        if !self.galaxy {
            self.galaxy = true;
            self.planet = None;
            self.gal.place(GALAXY_LEAVE_SYSTEM.0);
            self.gal.auto_zoom(GALAXY_LEAVE_SYSTEM.1);
            self.placed = false;
            self.sync_distances();
        }
    }

    /// Back to the solar system the ship is in: the solar camera starts at its
    /// ceiling and eases in to the system overview (the discrete level).
    pub fn leave_galaxy(&mut self) {
        if self.galaxy {
            self.galaxy = false;
            self.solar.place(SOLAR.ceiling);
            self.solar.auto_zoom(500.0);
            self.placed = false;
            self.sync_distances();
        }
    }

    /// Enter the planet view over `world` (the ship is over it): the camera
    /// starts at planetZoomInStartHeight and descends to the end height.
    pub fn enter_planet(&mut self, world: u32, center: Vec3, radius: f32, ship: Vec3, facing: Vec3) {
        let up = (ship - center).normalize_or(Vec3::Y);
        let heading = tangent(facing, up);
        let mut planet = PlanetView::new(world, center, radius, up, heading, PLANET_ZOOM_IN.0);
        planet.auto = Some(PLANET_ZOOM_IN.1);
        self.planet = Some(planet);
        self.active_planet = Some(world);
        self.wheel.reset();
    }

    /// Back to the solar camera after the planet: start close, pull out.
    pub fn leave_planet(&mut self) {
        if self.planet.take().is_some() {
            self.solar.place(SOLAR_LEAVE_PLANET.0);
            self.solar.auto_zoom(SOLAR_LEAVE_PLANET.1);
            self.follow_ship = true;
            self.wheel.reset();
            self.sync_distances();
        }
    }

    /// The solar camera's focus target for the frame (0x101BF30): the ship
    /// while it is not at its destination (or while it pulls out of a planet),
    /// otherwise the active planet's centre, and the star (the origin, the
    /// default of the retail code) when there is none. `planet_at` finds a
    /// world's centre.
    pub fn solar_focus_target(&mut self, ship: Vec3, idle: bool, planet_at: impl Fn(u32) -> Option<Vec3>) -> Vec3 {
        if !self.solar.auto {
            self.follow_ship = false;
        }
        let target = if !idle || self.follow_ship {
            ship
        } else {
            self.active_planet.and_then(planet_at).unwrap_or(Vec3::ZERO)
        };
        self.focus_target = Some(target);
        target
    }

    /// The camera's eye, look-at point and vertical FOV (scene units).
    pub fn pose(&self) -> (Vec3, Vec3, f32) {
        (self.eye, self.look, self.fov)
    }
}

/// `v` projected onto the tangent plane of `up`, or any tangent when `v` is
/// along it.
pub fn tangent(v: Vec3, up: Vec3) -> Vec3 {
    let flat = v - up * v.dot(up);
    if flat.length_squared() > 1.0e-8 {
        flat.normalize()
    } else {
        let any = if up.x.abs() < 0.9 { Vec3::X } else { Vec3::Z };
        (any - up * any.dot(up)).normalize()
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// The ship is flying somewhere on its own (a click, the right button).
fn ship_busy(controls: &SpaceControls, motion: Option<&crate::space::ShipMotion>) -> bool {
    controls.fly_to.is_some() || motion.is_some_and(|m| m.autopilot_target.is_some())
}

/// Reads the zoom inputs (wheel, zoom keys, discrete steps, the HUD's camera
/// buttons, rotate and pitch keys, camera drag) and advances the camera
/// state for the frame.
#[allow(clippy::too_many_arguments)]
pub fn zoom_input(
    time: Res<Time>,
    mut wheel_events: MessageReader<MouseWheel>,
    mut controls_res: Option<ResMut<SpaceControls>>,
    view: Option<ResMut<SpaceView>>,
    mut player: Query<(&mut Transform, Option<&crate::space::ShipMotion>), With<Player>>,
    planets: Query<(&Transform, &crate::game::Planet, &crate::persistence::WorldId), Without<Player>>,
    stars: Query<&crate::space::SystemStar>,
    comm: Option<Res<crate::space_comm::CommSession>>,
    mut trace_frame: Local<u32>,
) {
    let Some(mut view) = view else {
        wheel_events.clear();
        return;
    };
    let now = time.elapsed_secs();
    let dt = time.delta_secs().min(0.1);
    let mut notches = 0.0f32;
    for event in wheel_events.read() {
        notches += match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 60.0,
        };
    }
    let default_controls = SpaceControls::default();
    let controls = controls_res.as_deref().unwrap_or(&default_controls).clone();
    let controls = &controls;
    // The HUD's button presses are one-frame requests.
    if let Some(c) = controls_res.as_deref_mut() {
        c.buttons.zoom_in_edge = false;
        c.buttons.zoom_out_edge = false;
    }
    let triggers = &controls.triggers;
    let (ship_at, busy) = player
        .single()
        .map(|(t, m)| (t.translation, ship_busy(controls, m)))
        .unwrap_or((Vec3::ZERO, false));
    let idle = !busy && view.travel.is_none();
    // Wheel notches: one per notch of scroll, positive zooms in.
    let count = notches.abs().round().max(if notches != 0.0 { 1.0 } else { 0.0 }) as usize;
    for _ in 0..count.min(6) {
        let zoom_in = notches > 0.0;
        view.wheel.notch(now, zoom_in);
    }
    // The zoom keys act as a wheel notch each frame they are held (0xFFE700);
    // the planet camera takes +-0.5 directly.
    let key_zoom = triggers.zoom_in() as i32 - triggers.zoom_out() as i32;
    if view.planet.is_none() && key_zoom != 0 {
        view.wheel.notch(now, key_zoom > 0);
    }
    let cue = view.wheel.update(now, dt);
    if cue != ZoomCue::None {
        view.zoom_cue = cue;
    }
    let quick = controls.quick_zoom;
    let buttons = controls.buttons;
    let quick = if quick != 0 {
        quick
    } else if buttons.zoom_in_edge {
        1
    } else if buttons.zoom_out_edge {
        -1
    } else {
        0
    };

    if view.galaxy {
        // --- galaxy ---------------------------------------------------------
        let gate = ZoomGate {
            can_in: view.travel.is_none(),
            can_out: false,
            idle: view.travel.is_none(),
        };
        let v = view.wheel.v;
        if quick != 0 {
            view.gal.discrete_step(&GALAXY, quick < 0);
        }
        let mut wheel = std::mem::take(&mut view.wheel);
        view.gal.push(&GALAXY, v, dt, gate, false, &mut wheel);
        view.wheel = wheel;
        let (rot, pitch) = camera_rotation(controls, dt, None);
        if rot != 0.0 || pitch != 0.0 {
            view.gal.rotate(&GALAXY, rot, pitch);
        }
        // The galaxy slowly revolves (0x101A200 adds 0x10172B0 x dt in
        // milliseconds to the azimuth). ADAPTED: the star backdrop the
        // original turns with it (cSpaceGfx source transform) stays fixed.
        let revolution = galaxy_revolution_rate(view.gal.radius) * dt * 1000.0;
        view.gal.theta = wrap_angle(view.gal.theta + revolution);
        if let Some(Crossing::In) = view.gal.step(&GALAXY, dt, gate) {
            if view.travel.is_none() {
                view.leave_galaxy();
            }
        }
    } else if view.planet.is_some() {
        // --- planet ---------------------------------------------------------
        // The gate of 0x1044930 (see `PlanetView::zoom`): the comm screen is
        // closed and the ship is not busy (ADAPTED: busy = flying on its own).
        let can_leave = idle && !comm.as_deref().is_some_and(|c| c.open);
        let v = if key_zoom != 0 { 0.5 * key_zoom as f32 } else { view.wheel.v };
        let mut wheel = std::mem::take(&mut view.wheel);
        let mut leave = false;
        if let Some(planet) = view.planet.as_mut() {
            if quick > 0 {
                // End: down to the terrain plus 2 x ufoMinAltitude, only when
                // the ship is higher (0x101B8C0, VERIFIED).
                if planet.altitude > PLANET_DISCRETE_IN_ALTITUDE {
                    planet.auto = Some(PLANET_DISCRETE_IN_ALTITUDE);
                }
                wheel.reset();
            } else if quick < 0 {
                // Home: the original sends the zoom altitude out to
                // 2 x (radius + planetMaxHeight) (0x101B8C0), which leaves the
                // planet. ADAPTED: up to the zoom ceiling first, a second press
                // there leaves (like the wheel); without the gate (busy ship)
                // there is no ceiling, the altitude goes to the UFO's limit.
                if can_leave && planet.altitude >= PLANET_ZOOM_CEILING - 1.0 {
                    leave = true;
                } else {
                    planet.auto = Some(if can_leave { PLANET_ZOOM_CEILING } else { UFO_MAX_ALTITUDE });
                }
                wheel.reset();
            }
            planet.combat = controls.firing;
            leave |= planet.zoom(v, dt, can_leave, &mut wheel);
            let (yaw, pitch) = camera_rotation(controls, dt, Some(planet.altitude));
            planet.steer_camera(yaw, pitch, dt, controls.camera_held);
            planet.lift_when_blocked(dt);
            planet.blend(dt);
        }
        view.wheel = wheel;
        if leave {
            // The ship stays where it is over the world: out along its up
            // direction onto the ecliptic, beside the surface.
            if let (Some(p), Ok((mut t, _))) = (view.planet.as_ref(), player.single_mut()) {
                let flat = Vec3::new(p.up.x, 0.0, p.up.z).normalize_or(Vec3::X);
                t.translation = p.center + flat * (p.radius + 3.0);
            }
            view.leave_planet();
        }
    } else {
        // --- solar system -----------------------------------------------------
        // The nearest world the ship is hovering at (the planet gate).
        let enter = nearest_planet(ship_at, &planets, crate::space::action_range() * 2.0);
        // The active planet (`mpActivePlanet`): the world the ship was sent to
        // or last hovered at. The focus parks on it (0x101BF30).
        let destination = player.single().ok().and_then(|(_, m)| m.and_then(|m| m.autopilot_target));
        if let Some(world) = destination {
            view.active_planet = Some(world);
        } else if let (Some((world, ..)), true) = (enter, idle) {
            view.active_planet = Some(world);
        }
        let focus_target = view.solar_focus_target(ship_at, idle, |id| {
            planets.iter().find(|(_, _, w)| w.0 == id).map(|(t, ..)| t.translation)
        });
        *trace_frame += 1;
        if *trace_frame % 60 == 1 && std::env::var_os("OPENSPORE_SPACE_TRACE").is_some() {
            info!(
                "space solar focus: ship {ship_at:?} idle {idle} active {:?} (near {:?}) -> target {focus_target:?}, radius {:.1}",
                view.active_planet,
                enter.map(|e| e.0),
                view.solar.radius
            );
        }
        let gate = ZoomGate {
            can_in: enter.is_some(),
            can_out: true,
            idle,
        };
        let v = view.wheel.v;
        // A system without a star (black hole, proto-planetary disk, core)
        // stops the zoom at 400, not at the floor of 40 (0x101B010).
        let mut solar = SOLAR;
        if stars.single().is_ok_and(|star| !star.kind.has_planets()) {
            solar.floor = SOLAR_FLOOR_NON_STAR;
        }
        let solar = &solar;
        if quick != 0 {
            view.solar.discrete_step(solar, quick < 0);
        }
        let mut wheel = std::mem::take(&mut view.wheel);
        view.solar.push(solar, v, dt, gate, true, &mut wheel);
        view.wheel = wheel;
        let (rot, pitch) = camera_rotation(controls, dt, None);
        if rot != 0.0 || pitch != 0.0 {
            view.solar.rotate(solar, rot, pitch);
        }
        match view.solar.step(solar, dt, gate) {
            Some(Crossing::In) => {
                if let Some((world, center, radius)) = enter {
                    let facing = Vec3::new(-view.solar.theta.cos(), 0.0, view.solar.theta.sin());
                    view.enter_planet(world, center, radius, ship_at, facing);
                }
            }
            Some(Crossing::Out) => view.open_galaxy(),
            None => {}
        }
        // `--space-view planet` and the like: drop into the nearest world
        // (`OPENSPORE_SPACE_WORLD=<world id>`: that world of the system, for
        // development captures).
        if view.want_planet {
            let chosen = std::env::var("OPENSPORE_SPACE_WORLD")
                .ok()
                .and_then(|w| w.trim().parse::<u32>().ok())
                .and_then(|want| {
                    planets
                        .iter()
                        .find(|(_, _, id)| id.0 == want)
                        .map(|(t, p, id)| (id.0, t.translation, p.radius))
                });
            if let Some((world, center, radius)) = chosen.or_else(|| nearest_planet(ship_at, &planets, f32::MAX)) {
                view.want_planet = false;
                let facing = -(ship_at - center);
                view.enter_planet(world, center, radius, ship_at, facing);
            }
        }
    }
    view.sync_distances();
}

/// The paused camera (0x1020D60 + 0x101EC30): while the pause menu holds the
/// game the simulation systems (and `SpaceControls::gather`) do not run, so
/// this reads the wheel, the zoom / rotate keys and the camera drag itself and
/// drives `PlanetView::free_step`; when the game resumes it hands the planet
/// camera back to the table (`set_paused(false)`). The solar and galaxy
/// cameras keep running from real time while paused (0x1020D60 only freezes
/// the planet camera's modes; space-cam2-notes): the wheel, the zoom keys and
/// the rotate keys move them, the galaxy keeps revolving, and zooming out of
/// the solar system still opens the galaxy. The planet gate (entering a world,
/// jumping between stars) is a game action and waits for the pause to end.
#[allow(clippy::too_many_arguments)]
pub fn paused_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    mut wheel_events: MessageReader<MouseWheel>,
    pause: Option<Res<crate::pause_menu::PauseMenu>>,
    mut controls_res: Option<ResMut<SpaceControls>>,
    view: Option<ResMut<SpaceView>>,
    stars: Query<&crate::space::SystemStar>,
) {
    let Some(mut view) = view else {
        wheel_events.clear();
        return;
    };
    let paused = pause.as_deref().is_some_and(|p| p.open);
    if !paused {
        wheel_events.clear();
        if view.planet.as_ref().is_some_and(|p| p.free.is_some()) {
            if let Some(planet) = view.planet.as_mut() {
                planet.set_paused(false);
            }
        }
        return;
    }
    let now = time.elapsed_secs();
    let dt = time.delta_secs().min(0.1);
    let mut notches = 0.0f32;
    for event in wheel_events.read() {
        notches += match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 60.0,
        };
    }
    let mut controls = controls_res.as_deref().cloned().unwrap_or_default();
    controls.triggers = crate::space_controls::read_triggers(&keys);
    let scheme = controls.scheme;
    controls.camera_held = mouse.as_deref().is_some_and(|m| {
        [MouseButton::Middle, MouseButton::Right]
            .into_iter()
            .any(|b| scheme.camera_button(b) && m.pressed(b))
    });
    controls.drag = if controls.camera_held {
        motion.as_deref().map_or(Vec2::ZERO, |m| m.delta)
    } else {
        Vec2::ZERO
    };
    if let Some(c) = controls_res.as_deref_mut() {
        c.camera_held = controls.camera_held;
        c.drag = Vec2::ZERO;
    }
    let count = notches.abs().round().max(if notches != 0.0 { 1.0 } else { 0.0 }) as usize;
    for _ in 0..count.min(6) {
        view.wheel.notch(now, notches > 0.0);
    }
    let key_zoom = controls.triggers.zoom_in() as i32 - controls.triggers.zoom_out() as i32;
    // The zoom keys act as a wheel notch each frame they are held outside the
    // planet view (0xFFE700), as `zoom_input` does.
    if view.planet.is_none() && key_zoom != 0 {
        view.wheel.notch(now, key_zoom > 0);
    }
    let cue = view.wheel.update(now, dt);
    if cue != ZoomCue::None {
        view.zoom_cue = cue;
    }
    if view.galaxy {
        // The galaxy keeps zooming, turning and revolving; the star jump
        // (`Crossing::In`) is a game action and waits.
        let gate = ZoomGate { can_in: false, can_out: false, idle: view.travel.is_none() };
        let v = view.wheel.v;
        let mut wheel = std::mem::take(&mut view.wheel);
        view.gal.push(&GALAXY, v, dt, gate, false, &mut wheel);
        view.wheel = wheel;
        let (rot, pitch) = camera_rotation(&controls, dt, None);
        if rot != 0.0 || pitch != 0.0 {
            view.gal.rotate(&GALAXY, rot, pitch);
        }
        let revolution = galaxy_revolution_rate(view.gal.radius) * dt * 1000.0;
        view.gal.theta = wrap_angle(view.gal.theta + revolution);
        view.gal.step(&GALAXY, dt, gate);
        view.sync_distances();
        return;
    }
    if view.planet.is_none() {
        // The solar camera zooms and turns; leaving the system to the galaxy
        // map is camera-only, entering a world is not.
        let mut solar = SOLAR;
        if stars.single().is_ok_and(|star| !star.kind.has_planets()) {
            solar.floor = SOLAR_FLOOR_NON_STAR;
        }
        let gate = ZoomGate { can_in: false, can_out: true, idle: true };
        let v = view.wheel.v;
        let mut wheel = std::mem::take(&mut view.wheel);
        view.solar.push(&solar, v, dt, gate, true, &mut wheel);
        view.wheel = wheel;
        let (rot, pitch) = camera_rotation(&controls, dt, None);
        if rot != 0.0 || pitch != 0.0 {
            view.solar.rotate(&solar, rot, pitch);
        }
        if let Some(Crossing::Out) = view.solar.step(&solar, dt, gate) {
            view.open_galaxy();
        }
        view.sync_distances();
        return;
    }
    let v = if key_zoom != 0 { 0.5 * key_zoom as f32 } else { view.wheel.v };
    let Some(planet) = view.planet.as_mut() else { return };
    planet.set_paused(true);
    let (yaw, pitch) = camera_rotation(&controls, dt, Some(planet.altitude));
    planet.free_step(v, yaw, pitch, dt);
}

/// The camera drag hides the cursor (0xFDB320 calls `ShowCursor(false)` and
/// shows the drop-cursor icon 0x67F5C34 while the camera button is down;
/// ADAPTED: the icon is not drawn, the cursor only disappears).
pub fn drag_cursor(
    controls: Option<Res<SpaceControls>>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let hide = controls.as_deref().is_some_and(|c| c.camera_held);
    for mut cursor in &mut cursors {
        if cursor.visible == hide {
            cursor.visible = !hide;
        }
    }
}

/// Camera rotation of the frame in radians: (azimuth, pitch). Keys `,` `.`
/// and A / D (the interstellar views), PageUp / PageDown and W / S, the HUD's
/// rotate buttons and the camera-button drag. For the orbit cameras the pitch
/// is a change of the polar angle (PageUp raises the camera: it shrinks); for
/// the planet camera (`planet_altitude` given) it is a change of the camera's
/// elevation, positive raising it.
fn camera_rotation(controls: &SpaceControls, dt: f32, planet_altitude: Option<f32>) -> (f32, f32) {
    let t = &controls.triggers;
    let planet = planet_altitude.is_some();
    let mut azimuth = 0.0;
    let mut pitch = 0.0;
    // `,` rotates left, `.` right; in the interstellar views A / D too.
    let left = t.has(TriggerSet::ROTATE_LEFT)
        || controls.buttons.rotate_left
        || (!planet && t.has(TriggerSet::LEFT));
    let right = t.has(TriggerSet::ROTATE_RIGHT)
        || controls.buttons.rotate_right
        || (!planet && t.has(TriggerSet::RIGHT));
    let rate = if planet { PLANET_KEY_ROTATE.to_radians() } else { KEY_ROTATE_RATE };
    if left {
        azimuth -= rate * dt;
    }
    if right {
        azimuth += rate * dt;
    }
    let raise = t.has(TriggerSet::PITCH_UP) || (!planet && t.has(TriggerSet::FORWARD));
    let lower = t.has(TriggerSet::PITCH_DOWN) || (!planet && t.has(TriggerSet::BACK));
    let pitch_rate = planet_altitude.map_or(KEY_ROTATE_RATE, planet_pitch_rate);
    if raise {
        pitch += pitch_rate * dt;
    }
    if lower {
        pitch -= pitch_rate * dt;
    }
    if controls.camera_held && controls.drag != Vec2::ZERO {
        if planet {
            azimuth -= controls.drag.x * PLANET_MOUSE_ROTATION.to_radians();
            pitch += controls.drag.y * PLANET_MOUSE_ROTATION.to_radians();
        } else {
            azimuth -= controls.drag.x * MOUSE_ROTATE;
            pitch -= controls.drag.y * MOUSE_ROTATE;
        }
    }
    // The orbit cameras speak in polar angles: raising shrinks it.
    if !planet {
        pitch = -pitch;
    }
    (azimuth, pitch)
}

/// The world nearest `ship` within `reach` of its surface: (id, centre, radius).
pub fn nearest_planet(
    ship: Vec3,
    planets: &Query<(&Transform, &crate::game::Planet, &crate::persistence::WorldId), Without<Player>>,
    reach: f32,
) -> Option<(u32, Vec3, f32)> {
    planets
        .iter()
        .filter(|(t, p, _)| t.translation.distance(ship) < p.radius + reach)
        .min_by(|a, b| {
            a.0.translation
                .distance(ship)
                .total_cmp(&b.0.translation.distance(ship))
        })
        .map(|(t, p, id)| (id.0, t.translation, p.radius))
}

/// Places the Space camera after `camera_follow`, which keeps a fixed offset
/// for every stage.
#[allow(clippy::type_complexity)]
pub fn camera(
    time: Res<Time>,
    view: Option<ResMut<SpaceView>>,
    capture: Option<Res<crate::runtime::RenderSmokeCamera>>,
    editor: Res<crate::editor::EditorState>,
    stars: Query<&crate::space::SystemStar>,
    mut player: Query<&mut Transform, (With<Player>, Without<crate::GameCamera>)>,
    mut cameras: Query<
        (&mut Transform, &mut Projection),
        (With<crate::GameCamera>, Without<Player>),
    >,
) {
    let Some(mut view) = view else { return };
    if capture.is_some() || editor.open {
        return;
    }
    let Ok((mut camera, mut projection)) = cameras.single_mut() else {
        return;
    };
    let dt = time.delta_secs().min(0.1);
    let ease = |rate: f32| 1.0 - (-rate * dt).exp();
    let planet_pose = view.planet.as_ref().map(|p| p.camera());
    let galaxy = view.galaxy;
    let (look, eye, fov, near) = if galaxy {
        let focus = view.galaxy_focus;
        let look = GALAXY_ORIGIN + Vec3::new(focus.x, 0.0, focus.y);
        let (dr, dt_, dp) = view.gal.drift();
        let r = (view.gal.radius + dr) * GALAXY_SCALE;
        let theta = view.gal.theta + dt_;
        let phi = (view.gal.phi + dp).clamp(0.1, GALAXY.phi_max);
        let eye = look + to_scene(spherical(r, theta, phi));
        let fov = GALAXY.fov_deg(view.gal.radius).to_radians();
        (look, eye, fov, NEAR_CLIP)
    } else if let Some((eye, look, fov)) = planet_pose {
        let unit = view.planet.as_ref().map_or(1.0, |p| p.unit());
        (look, eye, fov, (PLANET_NEAR_CLIP * unit).max(1.0e-4))
    } else {
        let ship = player
            .single()
            .map(|t| t.translation)
            .unwrap_or(Vec3::ZERO);
        let dt_ms = dt * 1000.0;
        let mut focus = view.focus;
        let mut velocity = view.focus_v;
        // The ship while it flies, the active planet / the star when it is
        // parked (0x101BF30, `SpaceView::solar_focus_target`).
        let wanted = view.focus_target.unwrap_or(ship);
        if !view.placed {
            focus = wanted;
            velocity = Vec3::ZERO;
        } else {
            // 1.0 retail units per ms scaled into the scene.
            follow_focus(
                &mut focus,
                &mut velocity,
                wanted,
                1.0 * SOLAR_SCALE,
                0.001 * SOLAR_SCALE,
                dt_ms,
            );
        }
        view.focus = focus;
        view.focus_v = velocity;
        let (dr, dt_, dp) = view.solar.drift();
        // The drift never takes the radius under half of itself (0x101BF30);
        // a system without a star (black hole, proto-planetary disk, core)
        // keeps the camera at least `SOLAR_FLOOR_NON_STAR` out (0x15B6DB0).
        let mut r = (view.solar.radius + dr).max(view.solar.radius * 0.5);
        if stars.single().is_ok_and(|star| !star.kind.has_planets()) {
            r = r.max(SOLAR_FLOOR_NON_STAR);
        }
        let r = r * SOLAR_SCALE;
        let theta = view.solar.theta + dt_;
        let phi = (view.solar.phi + dp).clamp(0.01, SOLAR.phi_max);
        let offset = to_scene(spherical(r, theta, phi));
        // The pull-out (0x1017F80): zoomed out, the camera looks part of the
        // way from the focus toward the star (the origin) while the focus
        // stays in its wedge; the eye keeps the same offset from there.
        let look = solar_pull_out(
            Vec3::ZERO,
            focus,
            offset,
            solar_zoom_fraction(view.solar.radius),
            Vec3::Y,
            SOLAR_SCALE,
        );
        let eye = look + offset;
        let fov = SOLAR.fov_deg(view.solar.radius).to_radians();
        (look, eye, fov, SOLAR_NEAR_CLIP * SOLAR_SCALE)
    };
    // Every Space view's FOV goes through the retail projection (0x7C5400):
    // 60 degrees is the 4:3 horizontal angle (`vertical_fov`).
    let fov = match &*projection {
        Projection::Perspective(perspective) => vertical_fov(fov.to_degrees(), perspective.aspect_ratio),
        _ => fov,
    };
    if let Projection::Perspective(perspective) = &mut *projection {
        if (perspective.far - SPACE_FAR).abs() > 0.5 {
            perspective.far = SPACE_FAR;
        }
        if (perspective.near - near).abs() > 1.0e-4 * near.min(1.0).max(1.0e-3) {
            perspective.near = near;
        }
        if (perspective.fov - fov).abs() > 1.0e-4 {
            perspective.fov = fov;
        }
    }
    // The UFO is drawn smaller over a world and readable across the system.
    if let Ok(mut ship) = player.single_mut() {
        let scale = match view.planet.as_ref() {
            Some(p) => UFO_RETAIL_SIZE * p.unit() / SHIP_MODEL_SIZE,
            None => ship_scale(view.system_distance),
        };
        if (ship.scale.x - scale).abs() > 0.002 * scale {
            ship.scale = Vec3::splat(scale);
        }
    }
    // Over a world the smoothed camera rides with the world along its orbit.
    if let Some(center) = view.planet.as_ref().map(|p| p.center) {
        if view.placed {
            let shift = center - view.anchor;
            view.eye += shift;
            view.look += shift;
        }
        view.anchor = center;
    }
    if !view.placed {
        view.eye = eye;
        view.look = look;
        view.placed = true;
    } else if view.planet.is_some() || galaxy {
        // The planet camera blends its own values; the galaxy follows its
        // focus through `galaxy_controls`.
        let k = ease(18.0);
        view.eye = view.eye.lerp(eye, k);
        view.look = view.look.lerp(look, k);
    } else {
        view.eye = eye;
        view.look = look;
    }
    view.fov = fov;
    camera.translation = view.eye;
    // Over a world the camera's up is the world's local up (the horizon
    // stays level wherever the ship is on the sphere).
    let up = view.planet.as_ref().map_or(Vec3::Y, |p| p.up);
    camera.look_at(view.look, up);
    // Keep a galaxy-scale focus clamp so the map never drifts off the disc.
    view.galaxy_focus = view.galaxy_focus.clamp_length_max(GALAXY_RADIUS);
}

/// The UFO's display scale at a solar-system camera distance (scene units).
pub fn ship_scale(distance: f32) -> f32 {
    (distance / 55.0).clamp(0.4, 1.9)
}

/// Leaving Space restores the shared camera's default projection.
pub fn restore_camera(
    mut cameras: Query<&mut Projection, With<crate::GameCamera>>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    // A camera drag in progress must not leave the cursor hidden.
    for mut cursor in &mut cursors {
        cursor.visible = true;
    }
    for mut projection in &mut cameras {
        if let Projection::Perspective(perspective) = &mut *projection {
            let default = PerspectiveProjection::default();
            perspective.far = default.far;
            perspective.near = default.near;
            perspective.fov = default.fov;
        }
    }
}

/// Space is lit by its star, not the shared daylight: the stage's
/// directional sun goes dark and the ambient fill drops so planets show a
/// day side and a night side. `world_presentation` restores both on exit.
pub fn lighting(
    view: Option<Res<SpaceView>>,
    mut suns: Query<&mut DirectionalLight, With<crate::GameSun>>,
    mut ambient: Query<&mut AmbientLight, With<crate::GameCamera>>,
) {
    // ADAPTED: over a world its night side gets a dim blue fill, standing in
    // for the terrain's night light (kNightLightColor tinted by
    // kNightLightingTint 0.43 / 0.49 / 1.0); in space the fill stays low.
    let level = if view.as_ref().is_some_and(|v| v.planet.is_some()) { 220.0 } else { 55.0 };
    for mut sun in &mut suns {
        if sun.illuminance != 0.0 {
            sun.illuminance = 0.0;
        }
    }
    for mut light in &mut ambient {
        if light.brightness != level {
            light.brightness = level;
            light.color = Color::srgb(0.62, 0.70, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 1.0e-3, "{a} != {b}");
    }

    #[test]
    fn tuning_matches_the_space_camera_prop() {
        assert_eq!(FOV_DEG, 60.0);
        assert_eq!((NEAR_CLIP, FAR_CLIP), (0.05, 10_000.0));
        assert_eq!((SOLAR.min, SOLAR.max, SOLAR.floor, SOLAR.ceiling), (5.0, 1750.0, 40.0, 1200.0));
        assert_eq!((GALAXY.min, GALAXY.max, GALAXY.floor), (0.4, 1200.0, 1.0));
        assert_eq!(SOLAR.discrete, &[500.0]);
        assert_eq!(GALAXY.discrete, &[5.0, 40.0, 200.0, 1200.0]);
        assert_eq!(PLANET_TABLE.len(), 8);
        assert_eq!(PLANET_COMBAT_TABLE.len(), 6);
        assert_eq!(PLANET_BUSTER_TABLE.len(), 4);
        assert_eq!(PLANET_ZOOM_IN, (1000.0, 275.0));
        assert_eq!(SOLAR_LEAVE_PLANET, (5.0, 200.0));
        // 0.25 units per ms scaled into the scene over a typical ship speed of 20.
        close(SOLAR_SPEED_SCALE, 3.25);
        assert_eq!(GALAXY_LEAVE_SYSTEM, (0.7, 5.0));
    }

    #[test]
    fn pitch_curves_interpolate_by_radius() {
        // Solar: (5,25) (40,60) (100,75) (900,50) degrees.
        close(SOLAR.phi_curve(1.0).to_degrees(), 25.0);
        close(SOLAR.phi_curve(5.0).to_degrees(), 25.0);
        close(SOLAR.phi_curve(22.5).to_degrees(), 42.5);
        close(SOLAR.phi_curve(100.0).to_degrees(), 75.0);
        close(SOLAR.phi_curve(500.0).to_degrees(), 75.0 - 25.0 * 400.0 / 800.0);
        close(SOLAR.phi_curve(5000.0).to_degrees(), 50.0);
        // Galaxy: (1,50) (40,45) (700,30).
        close(GALAXY.phi_curve(0.4).to_degrees(), 50.0);
        close(GALAXY.phi_curve(700.0).to_degrees(), 30.0);
        assert!(GALAXY.phi_curve(100.0) < GALAXY.phi_curve(10.0));
    }

    #[test]
    fn zoom_rates_scale_linearly_with_the_radius() {
        close(SOLAR.zoom_rate(SOLAR.min, false, false), 250.0);
        close(SOLAR.zoom_rate(SOLAR.max, false, false), 5000.0);
        close(SOLAR.zoom_rate(SOLAR.min, true, true), 20.0);
        close(SOLAR.zoom_rate(SOLAR.max, true, false), 2000.0);
        close(GALAXY.zoom_rate(GALAXY.max, false, false), 3500.0);
        let mid = (SOLAR.min + SOLAR.max) / 2.0;
        close(SOLAR.zoom_rate(mid, false, false), (250.0 + 5000.0) / 2.0);
    }

    #[test]
    fn discrete_zoom_steps_the_listed_levels() {
        assert_eq!(GALAXY.discrete_target(1200.0, false), 200.0);
        assert_eq!(GALAXY.discrete_target(200.0, false), 40.0);
        assert_eq!(GALAXY.discrete_target(40.0, false), 5.0);
        assert_eq!(GALAXY.discrete_target(5.0, false), GALAXY.min);
        assert_eq!(GALAXY.discrete_target(5.0, true), 40.0);
        assert_eq!(GALAXY.discrete_target(1200.0, true), GALAXY.max);
        assert_eq!(GALAXY.discrete_target(100.0, true), 200.0);
        assert_eq!(SOLAR.discrete_target(1000.0, false), 500.0);
        assert_eq!(SOLAR.discrete_target(500.0, false), SOLAR.min);
        assert_eq!(SOLAR.discrete_target(200.0, true), 500.0);
        assert_eq!(SOLAR.discrete_target(500.0, true), SOLAR.max);
    }

    #[test]
    fn the_limiter_arrives_without_overshoot_and_respects_its_speed() {
        let (mut x, mut v) = (0.0f32, 0.0f32);
        let mut peak = 0.0f32;
        for _ in 0..2000 {
            smooth_to(&mut x, &mut v, 10.0, 0.01, 0.0001, 16.0);
            peak = peak.max(v.abs());
            assert!(x <= 10.0 + 1.0e-3, "overshoot {x}");
        }
        assert!(peak <= 0.01 + 1.0e-6);
        close(x, 10.0);
        assert_eq!(v, 0.0);
        // Moving down works the same way.
        for _ in 0..4000 {
            smooth_to(&mut x, &mut v, -3.0, 0.01, 0.0001, 16.0);
        }
        close(x, -3.0);
    }

    #[test]
    fn the_wheel_ramp_builds_with_notches_and_decays_when_idle() {
        let mut ramp = WheelRamp::default();
        let dt = 1.0 / 60.0;
        let mut now = 0.0;
        // One notch: a short, small push.
        ramp.notch(now, true);
        let cue = ramp.update(now, dt);
        assert_eq!(cue, ZoomCue::InStarted);
        let single = ramp.v;
        assert!(single > 0.0 && single < 0.1, "{single}");
        // Held (a notch per frame): ramps toward full speed.
        let mut held = WheelRamp::default();
        for _ in 0..60 {
            now += dt;
            held.notch(now, true);
            held.update(now, dt);
        }
        assert!(held.v > 0.99, "{}", held.v);
        // Idle: it decays to a stop within a second.
        for _ in 0..90 {
            now += dt;
            held.update(now, dt);
        }
        assert!(held.v.abs() < 1.0e-3, "{}", held.v);
        // Zoom out announces itself.
        let mut out = WheelRamp::default();
        assert_eq!(
            {
                out.notch(0.0, false);
                out.update(0.0, dt)
            },
            ZoomCue::OutStarted
        );
        assert!(out.v < 0.0);
    }

    #[test]
    fn a_push_moves_the_target_and_stops_at_the_floor_until_pushed_again() {
        let dt = 1.0 / 60.0;
        let mut cam = OrbitCam::new(100.0, THETA_START, 1.0);
        let mut wheel = WheelRamp::default();
        let gate = ZoomGate::default();
        // Zoom in at full speed with no world to enter: stops at the floor.
        for frame in 0..600 {
            cam.push(&SOLAR, 1.0, dt, gate, true, &mut wheel);
            cam.step(&SOLAR, dt, gate);
        }
        close(cam.radius, SOLAR.floor);
        assert!(!cam.auto);
        // Zoom out is limited by the ceiling the same way.
        for frame in 0..1200 {
            cam.push(&SOLAR, -1.0, dt, gate, true, &mut wheel);
            cam.step(&SOLAR, dt, gate);
        }
        close(cam.radius, SOLAR.ceiling);
    }

    #[test]
    fn pushing_through_the_floor_enters_the_next_view_only_when_allowed() {
        let dt = 1.0 / 60.0;
        let mut cam = OrbitCam::new(SOLAR.floor, THETA_START, 1.0);
        let mut wheel = WheelRamp::default();
        let gate = ZoomGate {
            can_in: true,
            can_out: true,
            idle: true,
        };
        // The first frame at the floor stops the push (and clears the ramp).
        wheel.v = 1.0;
        cam.push(&SOLAR, 1.0, dt, gate, true, &mut wheel);
        assert_eq!(wheel.v, 0.0, "the floor resets the ramp");
        close(cam.target, SOLAR.floor);
        // Pushing again descends automatically and finally crosses.
        let mut crossed = None;
        for frame in 0..2000 {
            cam.push(&SOLAR, 1.0, dt, gate, true, &mut wheel);
            if let Some(c) = cam.step(&SOLAR, dt, gate) {
                crossed = Some(c);
                break;
            }
        }
        assert_eq!(crossed, Some(Crossing::In));
        // A busy ship never crosses.
        let mut cam = OrbitCam::new(SOLAR.floor, THETA_START, 1.0);
        let busy = ZoomGate {
            idle: false,
            ..gate
        };
        for frame in 0..2000 {
            cam.push(&SOLAR, 1.0, dt, busy, true, &mut wheel);
            assert_eq!(cam.step(&SOLAR, dt, busy), None);
        }
    }

    #[test]
    fn discrete_steps_run_automatically_to_the_listed_radius() {
        let dt = 1.0 / 60.0;
        let mut cam = OrbitCam::new(1200.0, THETA_START, 1.0);
        let gate = ZoomGate::default();
        cam.discrete_step(&GALAXY, false);
        assert!(cam.auto);
        for frame in 0..1200 {
            cam.step(&GALAXY, dt, gate);
        }
        close(cam.radius, 200.0);
        assert!(!cam.auto);
        cam.discrete_step(&GALAXY, true);
        for frame in 0..1200 {
            cam.step(&GALAXY, dt, gate);
        }
        close(cam.radius, 1200.0);
    }

    #[test]
    fn the_pitch_follows_the_curve_and_the_offset_fades_at_the_ends() {
        let dt = 1.0 / 60.0;
        let mut cam = OrbitCam::new(500.0, THETA_START, SOLAR.phi_curve(500.0));
        let gate = ZoomGate::default();
        cam.step(&SOLAR, dt, gate);
        close(cam.phi, SOLAR.phi_curve(500.0));
        // Raising the camera (PageUp) makes the polar angle smaller...
        cam.rotate(&SOLAR, 0.0, -0.3);
        assert!(cam.phi < SOLAR.phi_curve(500.0) - 0.29);
        // ...and a long zoom in brings the offset to nothing at the minimum.
        cam.auto_zoom(SOLAR.min);
        for frame in 0..3000 {
            cam.step(&SOLAR, dt, gate);
        }
        assert!(cam.phi_offset.abs() < 0.02, "{}", cam.phi_offset);
        close(cam.phi, SOLAR.phi_curve(SOLAR.min));
        // The polar angle never leaves its limits.
        cam.rotate(&SOLAR, 0.0, -10.0);
        close(cam.phi, 0.1);
        cam.rotate(&SOLAR, 0.0, 10.0);
        close(cam.phi, SOLAR.phi_max);
        let mut galaxy = OrbitCam::new(100.0, 0.0, 1.0);
        galaxy.rotate(&GALAXY, 0.0, 10.0);
        close(galaxy.phi, GALAXY.phi_max);
        assert!(GALAXY.phi_max > std::f32::consts::FRAC_PI_2, "the galaxy can be seen from below");
    }

    #[test]
    fn the_spherical_offset_is_z_up_and_maps_into_the_scene() {
        let up = spherical(10.0, 0.0, 0.0);
        close(up.z, 10.0);
        let side = spherical(10.0, 0.0, std::f32::consts::FRAC_PI_2);
        close(side.x, 10.0);
        // z-up maps to the scene's +y; retail +y to scene -z.
        let scene = to_scene(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(scene, Vec3::new(1.0, 3.0, -2.0));
        // The opening angle puts the camera on the scene's +z side.
        let eye = to_scene(spherical(10.0, THETA_START, 1.0));
        assert!(eye.z > 5.0 && eye.y > 0.0);
    }

    #[test]
    fn the_planet_table_interpolates_distance_offset_and_fov() {
        let (d, o, f) = planet_row(PLANET_TABLE, 20.0);
        assert_eq!((d, o, f), (70.0, -10.0, 20.0));
        let (d, o, f) = planet_row(PLANET_TABLE, 3000.0);
        assert_eq!((d, o, f), (200.0, 0.0, 60.0));
        // Half way between 100 and 300: (250, -20, 65).
        let (d, o, f) = planet_row(PLANET_TABLE, 200.0);
        close(d, 250.0);
        close(o, -20.0);
        close(f, 65.0);
        // Between the first two rows (3000, 700): 1850 is the mid point.
        let (d, _, f) = planet_row(PLANET_TABLE, 1850.0);
        close(d, 180.0);
        close(f, 65.0);
        // The combat table keeps the look-at offset at 0.
        let (_, o, _) = planet_row(PLANET_COMBAT_TABLE, 50.0);
        assert_eq!(o, 0.0);
        // The planet buster pulls way out.
        let (d, _, f) = planet_row(PLANET_BUSTER_TABLE, 20.0);
        assert_eq!((d, f), (1480.0, 85.0));
    }

    #[test]
    fn the_planet_zoom_rates_and_atmosphere_follow_the_data() {
        close(planet_zoom_rate(UFO_MIN_ALTITUDE), 900.0);
        close(planet_zoom_rate(PLANET_MAX_HEIGHT), 3600.0);
        close(atmosphere_amount(351.0), 1.0);
        close(atmosphere_amount(1200.0), 0.0);
        assert!(atmosphere_amount(800.0) > 0.0 && atmosphere_amount(800.0) < 1.0);
        // The camera pitches slowly near the ground, quickly high up.
        assert!(planet_pitch_rate(20.0) < planet_pitch_rate(1000.0));
        close(planet_pitch_rate(1200.0), 60.0 * 0.052_359_88);
        // Retail proportion over a world: the altitude scales with the world.
        let p = PlanetView::new(1, Vec3::ZERO, 2.5, Vec3::Y, Vec3::Z, UFO_MIN_ALTITUDE);
        close(p.unit(), 0.005);
        close(p.height(), 20.0 * 0.005);
        // The ceiling stays inside the action range of the largest terrestrial
        // world (display radius 4.2).
        assert!(PLANET_ZOOM_CEILING * 4.2 / RETAIL_PLANET_RADIUS < crate::space::action_range());
    }

    fn planet() -> PlanetView {
        PlanetView::new(1, Vec3::new(10.0, 0.0, 0.0), 3.0, Vec3::Y, Vec3::Z, 275.0)
    }

    #[test]
    fn the_planet_zoom_stops_at_the_ceiling_and_leaves_on_a_second_push() {
        let dt = 1.0 / 60.0;
        let mut p = planet();
        let mut wheel = WheelRamp::default();
        // Zoom out: the first arrival stops at the ceiling.
        let mut left = false;
        for _ in 0..600 {
            left |= p.zoom(-1.0, dt, true, &mut wheel);
            if p.at_ceiling {
                break;
            }
        }
        assert!(p.at_ceiling && !left);
        close(p.altitude, PLANET_ZOOM_CEILING);
        // Pushing again leaves.
        for _ in 0..600 {
            if p.zoom(-1.0, dt, true, &mut wheel) {
                left = true;
                break;
            }
        }
        assert!(left, "a second push past the ceiling leaves the planet");
        // A busy ship (the gate of 0x1044930 is closed) has no ceiling: it
        // zooms out to the UFO's own limit but never leaves; zooming in goes
        // to the minimum.
        let mut p = planet();
        for _ in 0..1000 {
            assert!(!p.zoom(-1.0, dt, false, &mut wheel));
        }
        close(p.altitude, UFO_MAX_ALTITUDE);
        assert!(!p.at_ceiling);
        // Once the gate opens above the ceiling the automatic ascent starts.
        assert!(p.zoom(0.0, dt, true, &mut wheel), "arriving above the ceiling leaves");
        for _ in 0..1000 {
            p.zoom(1.0, dt, false, &mut wheel);
        }
        close(p.altitude, UFO_MIN_ALTITUDE);
    }

    #[test]
    fn the_pull_out_slides_the_look_at_point_toward_the_star_inside_its_wedge() {
        let up = Vec3::Y;
        // Eye 100 behind the focus on +z, looking along -z: H = x, V = y.
        let offset = Vec3::new(0.0, 0.0, 100.0);
        // No zoom-out fraction, or a focus on the star: nothing moves.
        let focus = Vec3::new(100.0, 0.0, 0.0);
        assert_eq!(solar_pull_out(Vec3::ZERO, focus, offset, 0.0, up, 1.0), focus);
        assert_eq!(solar_pull_out(Vec3::ZERO, Vec3::new(0.5, 0.0, 0.0), offset, 1.0, up, 1.0), Vec3::new(0.5, 0.0, 0.0));
        // Fully zoomed out the focus 45 degrees off the axis (tan 1 < 1.2)
        // fits the wedge: the camera looks straight at the star.
        let out = solar_pull_out(Vec3::ZERO, focus, offset, 1.0, up, 1.0);
        assert!(out.length() < 1.0e-4, "{out}");
        // Half way out the wedge is 0.6 wide: the pull stops at 1 - 0.4.
        let out = solar_pull_out(Vec3::ZERO, focus, offset, 0.5, up, 1.0);
        close(out.x, 100.0 * 0.5);
        // A focus at 63 degrees (200 across, 100 away) at full zoom-out: the
        // right plane (tan 1.2) allows 40 percent of the pull (checked by hand:
        // n = (1, 0, 1.2), n.(F - T - O) = 80, n.(F - T) = 200).
        let far = Vec3::new(200.0, 0.0, 0.0);
        let out = solar_pull_out(Vec3::ZERO, far, offset, 1.0, up, 1.0);
        close(out.x, 200.0 * 0.4);
        close(out.y, 0.0);
        // Whatever the geometry the result lies on the segment, the pull never
        // exceeds the zoom fraction and the focus stays inside the wedge of
        // the pulled camera.
        for (i, (f, o)) in [
            (0.3f32, Vec3::new(40.0, 220.0, 130.0)),
            (0.8, Vec3::new(-90.0, 300.0, 20.0)),
            (1.0, Vec3::new(10.0, 150.0, -200.0)),
            (0.6, Vec3::new(0.0, 500.0, 5.0)),
        ]
        .into_iter()
        .enumerate()
        {
            let focus = Vec3::new(120.0 - 30.0 * i as f32, 5.0, -80.0 + 40.0 * i as f32);
            let out = solar_pull_out(Vec3::ZERO, focus, o, f, up, 1.0);
            let t = out.dot(focus) / focus.length_squared();
            assert!((out - focus * t).length() < 1.0e-3, "on the segment {i}");
            assert!(t >= 1.0 - f - 1.0e-4 && t <= 1.0 + 1.0e-4, "t {t} for fraction {f}");
            let eye = out + o;
            let dir = o.normalize();
            let h = up.cross(dir).normalize();
            let v = dir.cross(h);
            for n in [
                dir * (f * SOLAR_PULL_OUT_UP) + v,
                dir * (f * SOLAR_PULL_OUT_UP) - v,
                dir * (f * SOLAR_PULL_OUT_RIGHT) + h,
                dir * (f * SOLAR_PULL_OUT_RIGHT) - h,
            ] {
                let n = n.normalize();
                assert!(n.dot(focus - eye) <= 1.0e-2, "the focus left the wedge ({i})");
            }
        }
        // The zoom fraction is the radius' place between the solar limits.
        close(solar_zoom_fraction(SOLAR.min), 0.0);
        close(solar_zoom_fraction(SOLAR.max), 1.0);
        close(solar_zoom_fraction(500.0), 495.0 / 1745.0);
    }

    #[test]
    fn the_solar_focus_parks_on_the_active_planet_or_the_star() {
        let planet_at = |id: u32| (id == 7).then_some(Vec3::new(30.0, 0.0, 10.0));
        let ship = Vec3::new(5.0, 0.0, 5.0);
        let mut view = SpaceView::default();
        // Flying: the ship, whatever the active planet.
        view.active_planet = Some(7);
        assert_eq!(view.solar_focus_target(ship, false, planet_at), ship);
        // Parked at the active planet: its centre.
        assert_eq!(view.solar_focus_target(ship, true, planet_at), Vec3::new(30.0, 0.0, 10.0));
        // Parked with no active planet: the star at the origin.
        view.active_planet = None;
        assert_eq!(view.solar_focus_target(ship, true, planet_at), Vec3::ZERO);
        view.active_planet = Some(99);
        assert_eq!(view.solar_focus_target(ship, true, planet_at), Vec3::ZERO, "an unknown world is no planet");
        // Leaving a planet: the camera follows the ship while it pulls out.
        let mut view = SpaceView::default();
        view.enter_planet(7, Vec3::new(30.0, 0.0, 10.0), 3.0, Vec3::new(30.0, 5.0, 10.0), Vec3::Z);
        assert_eq!(view.active_planet, Some(7));
        view.leave_planet();
        assert!(view.solar.auto);
        assert_eq!(view.solar_focus_target(ship, true, planet_at), ship);
        view.solar.auto = false;
        assert_eq!(view.solar_focus_target(ship, true, planet_at), Vec3::new(30.0, 0.0, 10.0));
    }

    #[test]
    fn the_paused_camera_zooms_its_distance_and_keeps_its_heading() {
        let dt = 1.0 / 60.0;
        let mut p = planet();
        p.altitude = 300.0;
        for _ in 0..600 {
            p.blend(dt);
        }
        let running_dist = p.dist;
        let running_elevation = p.elevation;
        // Pausing keeps the look-at point, distance and elevation.
        p.set_paused(true);
        assert!(p.free.is_some());
        let before = p.camera();
        p.free_step(0.0, 0.0, 0.0, dt);
        let after = p.camera();
        assert!(before.0.distance(after.0) < 1.0e-5 && before.1.distance(after.1) < 1.0e-5);
        // Zooming in is limited to 50, out to 500 (retail units).
        for _ in 0..600 {
            p.free_step(1.0, 0.0, 0.0, dt);
        }
        assert!((p.dist - FREE_DISTANCE.0).abs() < 0.01 * FREE_DISTANCE.0 + 0.5, "{}", p.dist);
        for _ in 0..600 {
            p.free_step(-1.0, 0.0, 0.0, dt);
        }
        assert!((p.dist - FREE_DISTANCE.1).abs() < 0.01 * FREE_DISTANCE.1 + 0.5, "{}", p.dist);
        // The altitude and the table do not move it; the heading stays turned.
        p.free_step(0.0, 0.7, 0.1, dt);
        assert!(p.yaw_manual && p.pitch_manual);
        for _ in 0..300 {
            p.free_step(0.0, 0.0, 0.0, dt);
        }
        close(p.cam_yaw, 0.7);
        close(p.pitch_offset, 0.1);
        close(p.elevation, running_elevation);
        // Resuming hands the camera back to the table's distance.
        p.set_paused(false);
        assert!(p.free.is_none());
        for _ in 0..1200 {
            p.blend(dt);
        }
        assert!((p.dist - running_dist).abs() <= 0.02 * running_dist, "{} {}", p.dist, running_dist);
        // Without the pitch flag the paused camera ignores a leftover offset.
        let mut q = planet();
        q.pitch_offset = 0.3;
        q.pitch_manual = false;
        let running = q.camera().0;
        q.set_paused(true);
        assert!(q.camera().0.distance(running) > 1.0e-4, "the offset is not applied while paused without the flag");
    }

    #[test]
    fn the_pause_menu_frees_the_planet_camera_and_a_camera_drag_hides_the_cursor() {
        use std::time::Duration;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)));
        app.add_message::<MouseWheel>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<ButtonInput<MouseButton>>();
        app.init_resource::<crate::pause_menu::PauseMenu>();
        app.init_resource::<SpaceControls>();
        let mut view = SpaceView::default();
        view.planet = Some(planet());
        let first = view.planet.as_ref().unwrap().dist;
        app.insert_resource(view);
        app.world_mut().spawn((Window::default(), PrimaryWindow, CursorOptions::default()));
        app.add_systems(Update, (paused_camera, drag_cursor).chain());
        let cursor_visible = |app: &mut App| {
            let world = app.world_mut();
            world.query::<&CursorOptions>().single(world).unwrap().visible
        };
        // Running: nothing is free.
        app.update();
        app.update();
        assert!(app.world().resource::<SpaceView>().planet.as_ref().unwrap().free.is_none());
        // The pause menu opens: the camera is free and Space zooms its distance out.
        app.world_mut().resource_mut::<crate::pause_menu::PauseMenu>().open = true;
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
        for _ in 0..40 {
            app.update();
        }
        {
            let view = app.world().resource::<SpaceView>();
            let planet = view.planet.as_ref().unwrap();
            let free = planet.free.expect("paused: the free camera");
            assert!(free.target > first && planet.dist > first, "{} {} {first}", free.target, planet.dist);
        }
        // A camera drag (middle button) hides the cursor and releasing shows it.
        assert!(cursor_visible(&mut app));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Middle);
        app.update();
        assert!(app.world().resource::<SpaceControls>().camera_held);
        assert!(!cursor_visible(&mut app), "the cursor is hidden while dragging");
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Middle);
        app.update();
        assert!(cursor_visible(&mut app), "the cursor returns after the drag");
        // Resuming hands the camera back.
        app.world_mut().resource_mut::<crate::pause_menu::PauseMenu>().open = false;
        app.update();
        assert!(app.world().resource::<SpaceView>().planet.as_ref().unwrap().free.is_none());
        // Leaving Space restores a hidden cursor.
        app.world_mut().resource_mut::<SpaceControls>().camera_held = true;
        app.update();
        assert!(!cursor_visible(&mut app));
        {
            use bevy::ecs::system::RunSystemOnce;
            app.world_mut().run_system_once(restore_camera).unwrap();
        }
        assert!(cursor_visible(&mut app));
    }

    #[test]
    fn the_solar_and_galaxy_cameras_keep_running_while_paused() {
        use std::time::Duration;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)));
        app.add_message::<MouseWheel>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<ButtonInput<MouseButton>>();
        app.init_resource::<crate::pause_menu::PauseMenu>();
        app.init_resource::<SpaceControls>();
        app.insert_resource(SpaceView::default());
        app.add_systems(Update, paused_camera);
        app.world_mut().resource_mut::<crate::pause_menu::PauseMenu>().open = true;
        // The solar camera zooms in with the held zoom key (X) while the pause menu is open.
        let target0 = app.world().resource::<SpaceView>().solar.target;
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyX);
        for _ in 0..30 {
            app.update();
        }
        let target1 = app.world().resource::<SpaceView>().solar.target;
        assert!(target1 < target0, "the solar zoom target moved in: {target0} -> {target1}");
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyX);
        // The galaxy keeps revolving (0x101A200) from real time.
        {
            let mut view = app.world_mut().resource_mut::<SpaceView>();
            view.open_galaxy();
            view.gal.radius = 800.0;
            view.gal.target = 800.0;
        }
        let theta0 = app.world().resource::<SpaceView>().gal.theta;
        for _ in 0..30 {
            app.update();
        }
        let theta1 = app.world().resource::<SpaceView>().gal.theta;
        assert!(theta1 != theta0, "the galaxy turned: {theta0} -> {theta1}");
    }

    #[test]
    fn entering_a_planet_descends_from_the_start_height() {
        let dt = 1.0 / 60.0;
        let mut view = SpaceView::default();
        view.enter_planet(2, Vec3::new(10.0, 0.0, 0.0), 3.0, Vec3::new(10.0, 5.0, 0.0), Vec3::Z);
        assert_eq!(view.zoom(), SpaceZoom::Planet);
        let planet = view.planet.as_ref().unwrap();
        assert_eq!(planet.altitude, 1000.0);
        assert_eq!(planet.auto, Some(275.0));
        // The ship is straight above the centre, heading along the tangent.
        assert!(planet.up.distance(Vec3::Y) < 1.0e-4);
        assert!(planet.heading.dot(planet.up).abs() < 1.0e-4);
        let mut wheel = WheelRamp::default();
        let mut p = planet.clone();
        for _ in 0..600 {
            p.zoom(0.0, dt, false, &mut wheel);
        }
        close(p.altitude, 275.0);
        assert!(p.auto.is_none());
    }

    #[test]
    fn the_planet_camera_stays_above_the_ground_and_blends_the_table() {
        let dt = 1.0 / 60.0;
        let mut p = planet();
        p.altitude = 20.0;
        for _ in 0..1200 {
            p.blend(dt);
        }
        let (dist, offset, elevation) = planet_row(PLANET_TABLE, 20.0);
        // (the blend stops within the 1% tolerance, as the original does)
        assert!((p.dist - dist).abs() <= 0.01 * dist, "{} {}", p.dist, dist);
        assert!((p.offset - offset).abs() <= 0.5, "{} {}", p.offset, offset);
        assert!((p.elevation - elevation.to_radians()).abs() < 0.01 * elevation.to_radians() + 1.0e-4);
        // A pitch offset that looks straight down from behind: still outside the world.
        p.pitch_offset = 1.45;
        let (eye, look, _) = p.camera();
        assert!((eye - p.center).length() >= p.radius + CAMERA_TERRAIN_CLEARANCE * p.unit() - 1.0e-4);
        assert!(look.distance(p.ship_position() + p.up * (p.offset * p.unit())) < 1.0e-4);
        // Near the ground the camera never drops under the horizon angle.
        p.pitch_offset = -1.0;
        let (eye, look, _) = p.camera();
        let elevation = (eye - look).normalize().dot(p.up).asin();
        assert!(elevation >= HORIZON_ANGLE_DEG.to_radians() - 1.0e-3, "{elevation}");
        // ... and never under the terrain.
        p.terrain = Some(TerrainProbe(std::sync::Arc::new(|_| 60.0)));
        let (eye, _, _) = p.camera();
        assert!((eye - p.center).length() >= p.radius + (60.0 + CAMERA_TERRAIN_CLEARANCE) * p.unit() - 1.0e-4);
        // The ship follows the ground: 60 units of terrain lift it by 60 units.
        let lifted = p.ship_position().distance(p.center);
        close(lifted, p.radius + (60.0 + 20.0) * p.unit());
    }

    #[test]
    fn the_camera_ray_stops_at_the_first_terrain_hit() {
        let mut p = planet();
        p.altitude = 20.0;
        let unit = p.unit();
        let from = p.ship_position() + p.up * (10.0 * unit);
        let behind = -p.heading;
        let to = from + behind * (80.0 * unit);
        // No terrain probe, or flat ground: nothing is hit.
        assert!(p.terrain_hit(from, to).is_none());
        p.terrain = Some(TerrainProbe(std::sync::Arc::new(|_| 0.0)));
        assert!(p.terrain_hit(from, to).is_none());
        // A wall of 100 units begins 30 units behind the ship (an angle of
        // 30 / R around the planet): the ray from the look-at point to the eye
        // is cut where it enters the ground, which is at about that distance.
        let wall = (30.0 * unit / p.radius).cos();
        let behind_axis = behind;
        let up = p.up;
        p.terrain = Some(TerrainProbe(std::sync::Arc::new(move |dir: Vec3| {
            // The flat angle behind the ship: tilt of `dir` from `up` toward `behind`.
            if dir.dot(up) < wall && dir.dot(behind_axis) > 0.0 { 100.0 } else { 0.0 }
        })));
        let hit = p.terrain_hit(from, to).expect("the ray enters the wall");
        let along = (hit - from).dot(behind) / unit;
        assert!((20.0..45.0).contains(&along), "{along}");
        // A look-at point already under the ground casts no ray.
        let buried = p.center + (up + behind * 0.5).normalize() * (p.radius + 50.0 * unit);
        assert!(p.terrain_hit(buried, to).is_none());
    }

    #[test]
    fn a_blocked_camera_ray_raises_the_pitch_offset() {
        let mut p = planet();
        p.altitude = 20.0;
        for _ in 0..600 {
            p.blend(1.0 / 60.0);
        }
        // Free: nothing changes.
        p.terrain = Some(TerrainProbe(std::sync::Arc::new(|_| 0.0)));
        p.lift_when_blocked(0.5);
        assert_eq!(p.pitch_offset, 0.0);
        // A wall behind the ship blocks the ray from the look-at point to the eye.
        let (up, behind) = (p.up, -p.heading);
        let wall = (10.0 * p.unit() / p.radius).cos();
        p.terrain = Some(TerrainProbe(std::sync::Arc::new(move |dir: Vec3| {
            if dir.dot(up) < wall && dir.dot(behind) > 0.0 { 400.0 } else { 0.0 }
        })));
        p.lift_when_blocked(0.5);
        assert!((p.pitch_offset - CAMERA_BLOCKED_PITCH_RATE * 0.5).abs() < 1.0e-6, "{}", p.pitch_offset);
        // Never past the elevation limit.
        for _ in 0..100 {
            p.lift_when_blocked(0.5);
        }
        let hi = ELEVATION_LIMITS_DEG.1.to_radians();
        assert!(p.elevation + p.pitch_offset <= hi + 1.0e-5);
    }

    #[test]
    fn the_table_elevation_looks_down_at_height_and_low_near_the_ground() {
        // 0x101CFE0: the camera swings up about the ship's right axis by the
        // table's fourth column; the FOV is the constant spaceFOV.
        let mut p = planet();
        p.altitude = 500.0;
        p.velocity = Vec3::ZERO;
        for _ in 0..1200 {
            p.blend(1.0 / 60.0);
        }
        let (eye, look, fov) = p.camera();
        let elevation = (eye - look).normalize().dot(p.up).asin().to_degrees();
        assert!((elevation - 80.0).abs() < 0.5, "{elevation}");
        assert_eq!(fov, FOV_DEG.to_radians(), "the planet camera keeps the spaceFOV");
        // The distance along the view line is the table's 200.
        assert!(((eye - look).length() / p.unit() - 200.0).abs() < 2.0);
        p.altitude = 20.0;
        for _ in 0..1200 {
            p.blend(1.0 / 60.0);
        }
        let (eye, look, _) = p.camera();
        let elevation = (eye - look).normalize().dot(p.up).asin().to_degrees();
        assert!((elevation - 20.0).abs() < 0.5, "{elevation}");
        // The eye is behind the look-at point (opposite the heading).
        assert!((eye - look).dot(p.heading) < 0.0);
        // The planet buster tuning looks nearly straight down.
        p.buster = true;
        for _ in 0..1200 {
            p.blend(1.0 / 60.0);
        }
        let (eye, look, _) = p.camera();
        let elevation = (eye - look).normalize().dot(p.up).asin().to_degrees();
        assert!(elevation > 80.0, "{elevation}");
    }

    #[test]
    fn flying_fast_looks_the_table_up_higher() {
        let mut p = planet();
        p.altitude = 20.0;
        close(p.table_altitude(), 20.0);
        let top = crate::space_controls::speed_limits(20.0).0 * p.unit();
        p.velocity = p.heading * top;
        // smoothstep(1) x ufoMinAltitude at full speed.
        close(p.table_altitude(), 40.0);
        p.velocity = p.heading * top * 0.5;
        close(p.table_altitude(), 20.0 + 0.5 * UFO_MIN_ALTITUDE);
    }

    #[test]
    fn the_camera_follows_the_ship_until_the_player_rotates_it() {
        let dt = 1.0 / 60.0;
        let mut p = planet();
        p.altitude = 100.0;
        // Not rotated: the camera heading is the ship's, whatever the offset was.
        p.cam_yaw = 0.7;
        p.steer_camera(0.0, 0.0, dt, false);
        assert_eq!(p.cam_yaw, 0.0);
        // A rotation input turns it and keeps it while the ship stands still.
        p.steer_camera(0.5, 0.0, dt, false);
        assert!(p.yaw_manual);
        close(p.cam_yaw, 0.5);
        for _ in 0..120 {
            p.steer_camera(0.0, 0.0, dt, false);
        }
        close(p.cam_yaw, 0.5);
        // Flying low swings it back toward the movement direction.
        let top = crate::space_controls::speed_limits(p.altitude).0 * p.unit();
        p.velocity = p.heading * top;
        for _ in 0..600 {
            p.steer_camera(0.0, 0.0, dt, false);
        }
        assert!(p.cam_yaw.abs() < 0.05 && !p.yaw_manual, "{} {}", p.cam_yaw, p.yaw_manual);
        // Flying high does not swing a rotated camera.
        let mut p = planet();
        p.altitude = 600.0;
        p.steer_camera(0.5, 0.0, dt, false);
        let top = crate::space_controls::speed_limits(p.altitude).0 * p.unit();
        p.velocity = p.heading * top;
        for _ in 0..300 {
            p.steer_camera(0.0, 0.0, dt, false);
        }
        close(p.cam_yaw, 0.5);
    }

    #[test]
    fn the_pitch_offset_stays_until_the_ship_flies_and_is_limited() {
        let dt = 1.0 / 60.0;
        let mut p = planet();
        p.altitude = 100.0;
        p.blend(dt);
        p.steer_camera(0.0, 0.2, dt, false);
        assert!(p.pitch_manual);
        close(p.pitch_offset, 0.2);
        // Standing still: the offset stays.
        for _ in 0..120 {
            p.steer_camera(0.0, 0.0, dt, false);
        }
        close(p.pitch_offset, 0.2);
        // The ship flies: the manual flag clears and the offset returns.
        let top = crate::space_controls::speed_limits(p.altitude).0 * p.unit();
        p.velocity = p.heading * top;
        for _ in 0..180 {
            p.steer_camera(0.0, 0.0, dt, false);
        }
        assert!(!p.pitch_manual && p.pitch_offset.abs() < 1.0e-3, "{}", p.pitch_offset);
        // The camera button held keeps it.
        p.steer_camera(0.0, -0.1, dt, true);
        for _ in 0..60 {
            p.steer_camera(0.0, 0.0, dt, true);
        }
        assert!(p.pitch_offset < -0.09);
        // The total elevation stays inside 1..89 degrees.
        p.steer_camera(0.0, 10.0, dt, false);
        let top = (p.elevation + p.pitch_offset).to_degrees();
        assert!((top - ELEVATION_LIMITS_DEG.1).abs() < 0.01, "{top}");
        p.steer_camera(0.0, -10.0, dt, false);
        let bottom = (p.elevation + p.pitch_offset).to_degrees();
        assert!((bottom - ELEVATION_LIMITS_DEG.0).abs() < 0.01, "{bottom}");
    }

    #[test]
    fn the_projection_is_the_retail_one_and_the_galaxy_revolves_slowly() {
        // 60 degrees is the 4:3 horizontal angle: vertical half tangent 0.75 x tan(30).
        let v = vertical_fov(60.0, 4.0 / 3.0);
        close((v * 0.5).tan(), 0.75 * (30.0f32).to_radians().tan());
        assert!((v.to_degrees() - 46.83).abs() < 0.01, "{}", v.to_degrees());
        assert_eq!(vertical_fov(60.0, 16.0 / 9.0), v, "wider views keep the vertical angle");
        close(vertical_fov(60.0, 1.0).to_degrees(), 60.0);
        // SpaceGalacticConstants: no revolution at or under 10, -0.04e-4 rad/ms at 1500.
        close(galaxy_revolution_rate(10.0), 0.0);
        close(galaxy_revolution_rate(1500.0), -0.04e-4);
        assert!(galaxy_revolution_rate(1200.0) < galaxy_revolution_rate(200.0));
        assert!(galaxy_revolution_rate(1200.0) < 0.0, "the galaxy turns clockwise");
    }

    #[test]
    fn the_legacy_zoom_steps_still_walk_the_three_levels() {
        let mut view = SpaceView::default();
        assert_eq!(view.zoom(), SpaceZoom::System);
        for _ in 0..30 {
            view.zoom_by(1.0);
        }
        assert_eq!(view.zoom(), SpaceZoom::Galaxy);
        for _ in 0..60 {
            view.zoom_by(-1.0);
        }
        assert_eq!(view.zoom(), SpaceZoom::System);
        // Z walks the discrete levels out, the galaxy cannot be left mid-jump.
        view.level_out();
        view.level_out();
        view.open_galaxy();
        assert_eq!(view.zoom(), SpaceZoom::Galaxy);
        view.travel = Some(Travel {
            from: 0,
            to: 1,
            t: 0.5,
        });
        view.level_in();
        view.zoom_by(-30.0);
        assert_eq!(view.zoom(), SpaceZoom::Galaxy);
    }

    #[test]
    fn the_drift_is_small_and_slow() {
        let mut cam = OrbitCam::new(500.0, 0.0, 1.0);
        let gate = ZoomGate::default();
        for frame in 0..600 {
            cam.step(&SOLAR, 1.0 / 60.0, gate);
        }
        let (r, t, p) = cam.drift();
        assert!(r.abs() <= 0.05 && t.abs() <= 0.05 && p.abs() <= 0.1);
    }

    #[test]
    fn requests_parse_and_open_the_asked_view() {
        assert!(SpaceViewRequest::parse("planet").is_some());
        assert!(SpaceViewRequest::parse("nonsense").is_none());
        let galaxy = SpaceView::entering(3, Vec2::ZERO, Some(SpaceZoom::Galaxy));
        assert!(galaxy.galaxy);
        let planet = SpaceView::entering(3, Vec2::ZERO, Some(SpaceZoom::Planet));
        assert!(planet.want_planet && planet.planet.is_none());
    }
}
