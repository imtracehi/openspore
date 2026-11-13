//! Civilization stage camera and controls as the original binds them.
//!
//! Sources (see `notes\civ-camera-notes.md`): the installed camera script
//! `civ_camera` (an Arithmetica program, run by `arithmetica.rs`), the
//! decompiled planet camera class (input mapping, ramps, orbit, drag scroll,
//! the alt-zoom rule), `CityGameGeneralTuning` (initial polar coordinates,
//! `clickDragDistance`, `PlumpDistance`, the `*ZoomOutScale` values),
//! `TriggerConfigTribeGame` + `TriggerConfigWASD` and the installed manual's
//! Tribe and Civilization controls. What the code does not show (units of the
//! mouse sensitivities, the update rate the per-update increments assume, the
//! easing curve) is marked `ADAPTED`.
//!
//! Camera (the retail camera orbits an anchor on the planet; here the anchor is
//! the `CivCommander` entity, the one thing other systems already move):
//! * `W A S D` / arrows / numpad 8 2 4 6 pan relative to the camera, with the
//!   retail ramp (smoothstep over 0.25 s) and a speed proportional to the
//!   zoom distance; `,` `.` (and Shift+Right / Shift+Left) rotate; PageUp /
//!   PageDown / numpad 9 3 (and Shift+Up / Shift+Down) pitch; `=` `+` and `-`
//!   (numpad too) or the wheel zoom through the script's discrete ladder
//!   (15 with the alt-zoom rule, 50, 75, 100, 150, 200, 250, 300, 350, 450).
//! * Middle-button drag orbits; so does a left + right drag (manual). A right
//!   button drag beyond `clickDragDistance` (12 px) pans like a joystick; a
//!   right click without moving is the order (`civ_rts`).
//! * The channels (anchor, distance, yaw, pitch) move the way the planet
//!   camera class moves them (0xB11930, `notes\civ-cam-notes.md`): the input
//!   sets a GOAL (a pan puts it `scroll` ahead of the current anchor on every
//!   update, not ahead of the old goal) and every update re-solves a cubic
//!   Hermite from the current value and velocity to the goal over the
//!   remaining segment time (yaw 0.2 s, pitch 0.65 s, anchor and distance
//!   `lerp(min, max interpolation time, angle / pi)` of the pan), so the
//!   motion has retail's acceleration and settle; releasing the pan keys
//!   coasts the anchor on (`|velocity| x min_interp x 250`), releasing the
//!   orbit button coasts yaw and pitch (`velocity x 0.1 x 0.5`).
//! * Edge scroll (the Settings option, `front_end::settings::apply`) uses the script's
//!   dwell ramp on the retail 2 pixel border zone (`edge_distance`).
//! * The stage starts at azimuth 0, pitch 0.7636 rad (43.8 deg), distance
//!   221.791 (retail units: the stage plays on the planet, `civ_planet`).
//!   The orbit turns about the local up at the anchor (its frame is carried
//!   along the surface as the anchor moves), so the camera can look at any
//!   city on the planet.
//!
//! Controls from the manual (all `// ADAPTED` where the original is not
//! traced): `Space` cycles through your vehicles (or the defined groups) and
//! the camera follows, `Backspace` moves the camera over the closest owned
//! city, `Ctrl` + a digit defines a group of the selection, `Tab` switches the
//! stance of the selected vehicles (stand ground / aggressive), `Delete`
//! decommissions them (half the price back), `Esc` deselects (and only then
//! opens the pause menu). Multi-select adds with `Ctrl` (`Shift` still works).
//! The in-stage assembly editor of `civ.rs` now needs `Ctrl` for the keys that
//! clashed (`, . - = Tab Backspace`); its other keys are unchanged.
//! `Q` (select the doctrine fleet), `E` (engine), `F` (send the fleet), `X`, `G`,
//! `H`, `J`, `B`, `T`, `1`-`6` and `7`-`0` / `U I O P` keep their meaning; the
//! original's `Q` / `E` strafe keys are not read by the Civ camera.

use bevy::{input::mouse::*, prelude::*};

use crate::{
    arithmetica::{Arithmetica, GROUP_SCRIPTS, TYPE_ARITHMETICA},
    civ::{self, CivCommander, HOME_INDEX},
    civ_planet::{Shape, CITY_SCALE},
    civ_rts::{self, CivUnit, Selected},
    dbpf::ResourceKey,
    game::{Player, RunProgress},
    game_audio::{Cue, PlayCue},
    stages::GameStage,
    world_objects::name_id,
    Originals,
};

// ---------------------------------------------------------------------------
// Original values
// ---------------------------------------------------------------------------

/// Retail numbers (`civ_camera` script, `CityGameGeneralTuning`).
pub mod tuning {
    /// InitialCameraPolarCoords (azimuth, pitch in radians, distance).
    pub const INITIAL_AZIMUTH: f32 = 0.0;
    pub const INITIAL_PITCH: f32 = 0.7636;
    pub const INITIAL_DISTANCE: f32 = 221.791;
    /// The goal distance snaps through this ladder; 15 only with the alt-zoom
    /// rule (script `alt_zoom_mode`).
    pub const ZOOM_STEPS: [f32; 9] = [50.0, 75.0, 100.0, 150.0, 200.0, 250.0, 300.0, 350.0, 450.0];
    pub const ALT_ZOOM_STEP: f32 = 15.0;
    /// `alt_zoom_threshold` / `alt_zoom_timer` (seconds).
    pub const ALT_ZOOM_THRESHOLD: f32 = 75.0;
    pub const ALT_ZOOM_TIMER: f32 = 0.2;
    /// `min_pitch` / `max_pitch` (degrees), `min_height` above the terrain.
    pub const MIN_PITCH_DEG: f32 = 5.0;
    pub const MAX_PITCH_DEG: f32 = 65.0;
    pub const MIN_HEIGHT: f32 = 0.75;
    /// `keyboard_acceleration_time`, `keyboard_translation_speed`.
    pub const KEYBOARD_ACCELERATION_TIME: f32 = 0.25;
    pub const KEYBOARD_TRANSLATION_SPEED: f32 = 10.0;
    /// `min_interpolation_time` / `max_interpolation_time` (seconds).
    pub const MIN_INTERPOLATION_TIME: f32 = 0.5;
    pub const MAX_INTERPOLATION_TIME: f32 = 5.0;
    /// `keyboard_rotate = keyboard_rotate * 5` (degrees per update).
    pub const KEYBOARD_ROTATE_DEGREES: f32 = 5.0;
    /// `keyboard_scroll = ramp * speed * -distance / 20`.
    pub const KEYBOARD_SCROLL_DIVISOR: f32 = 20.0;
    /// `mouse_scroll_sensitivity = 10 + distance`.
    pub const MOUSE_SCROLL_SENSITIVITY_BASE: f32 = 10.0;
    /// `mouse_rotate_sensitivity_x` (degrees per unit) and `_y`.
    pub const MOUSE_ROTATE_SENSITIVITY_X: f32 = 1.25;
    pub const MOUSE_ROTATE_SENSITIVITY_Y: f32 = 2.0;
    /// PlumpDistance: above it the display scales below apply.
    pub const PLUMP_DISTANCE: f32 = 250.0;
    /// CityHallZoomOutScale, VehicleZoomOutScale, BuildingType1/2ZoomOutScale.
    pub const CITY_HALL_ZOOM_OUT_SCALE: f32 = 3.0;
    pub const VEHICLE_ZOOM_OUT_SCALE: f32 = 1.3;
    pub const BUILDING_ZOOM_OUT_SCALE: f32 = 1.25;
    /// clickDragDistance: a press moved further than this is a drag.
    pub const CLICK_DRAG_DISTANCE: f32 = 12.0;
    /// The planet camera's clip planes (`TribeCamera` PROP, clip 1..10000).
    pub const CLIP_NEAR: f32 = 1.0;
    pub const CLIP_FAR: f32 = 10000.0;
    /// CityMapZoomLevel (the minimap zoom, for the HUD).
    pub const CITY_MAP_ZOOM_LEVEL: f32 = 0.66;
    /// civSpeed1..4 (loaded by the tuning loader; no key or button found).
    pub const GAME_SPEEDS: [f32; 4] = [1.0, 2.0, 4.0, 8.0];
}

/// Interpolation constants of the planet camera class (the `.data` globals
/// read by 0xB11930 / 0xB10BA0 and the literals of 0xB14800), the same for
/// every stage that drives the class.
pub mod interp {
    /// 0x1567D9C: the yaw channel's segment time (seconds).
    pub const YAW_SEGMENT: f32 = 0.2;
    /// 0x1567DA0: the pitch channel's segment time.
    pub const PITCH_SEGMENT: f32 = 0.65;
    /// 0x1567D98: while the end velocity is zero the remaining time is
    /// shortened by this factor.
    pub const SHORTEN: f32 = 0.95;
    /// 0x1567D94: the orbit release coast: duration, and the goal is
    /// `velocity x this x 0.5` ahead.
    pub const ORBIT_COAST: f32 = 0.1;
    /// 0xB14800: after the pan keys come up the anchor coasts on for
    /// `|velocity| x min_interpolation_time x 250`.
    pub const COAST_SPEED_SCALE: f32 = 250.0;
    /// Camera object +0x2A8 (0xB127D0): edge scroll speed per second.
    pub const EDGE_SCROLL_SPEED: f32 = 1500.0;
    /// 0x13EB954: added to the pixel distance of the cursor from the nearest
    /// window border to give the script's `edge_distance` (`0xB5D1CB`).
    pub const EDGE_PIXEL_OFFSET: f32 = 0.01;
    /// 0xB11930: |velocity|^2 below which the end velocity counts as zero;
    /// the settled channels converge below this distance.
    pub const EPSILON: f32 = 1.5258789e-5;
    /// +0x27C / +0x288 (0xB127D0): the exponential settle of a finished
    /// channel is `1 - exp(-0.30103 dt / this)`.
    pub const SETTLE_TIME: f32 = 0.1;
}

pub use tuning::CLICK_DRAG_DISTANCE;

/// The camera's distances in world units per retail unit: the world scale
/// (1.0 on the planet; the island builds used 28/180).
pub const CAMERA_WORLD_SCALE: f32 = crate::civ_rules::CIV_WORLD_SCALE;
/// ADAPTED: the per-update increments of the original (5 degrees of rotation)
/// assume its camera update rate; 30 updates per second is taken (the Tribal
/// port of the same class takes the same).
pub(crate) const UPDATES_PER_SECOND: f32 = 30.0;
/// ADAPTED: the updates the per-update inputs of the original (mouse orbit
/// offsets, edge scroll) were tuned at; the goals they set ride the update
/// rate, so the offsets are scaled to this rate (60 Hz, the vsync of the
/// original) when the frame rate differs.
pub(crate) const NOMINAL_UPDATE_SECONDS: f32 = 1.0 / 60.0;
/// 0xB0FDF0: the mouse offset of one orbit move is clamped to this many pixels.
pub(crate) const MOUSE_ORBIT_CLAMP: f32 = 20.0;
/// ADAPTED: zoom keys repeat after this delay, then at this period.
const ZOOM_REPEAT_DELAY: f32 = 0.5;
const ZOOM_REPEAT_PERIOD: f32 = 0.25;
/// ADAPTED: the planet's radius in retail units, to turn the flat chart's
/// pan distance into the angle the segment time is interpolated with.
pub(crate) const FLAT_RADIUS: f32 = 500.0;
/// `terrain::BASE_Y`: the floor of a world without a sampled surface.
const TERRAIN_BASE_Y: f32 = -0.6;
/// Pick radius of a vehicle under the cursor, as `civ_rts` (pixels).
const PICK_PIXELS: f32 = 30.0;
/// Radius of a city's click target (as the right-click order of `civ_rts`).
const CITY_CLICK_RADIUS: f32 = 7.0 * CITY_SCALE;
const GEYSER_CLICK_RADIUS: f32 = 3.2 * CITY_SCALE;
/// Decommissioning returns half the purchase price (manual, Fleet).
pub const DECOMMISSION_REFUND: f32 = 0.5;
/// ADAPTED: an aggressive vehicle chases hostile vehicles within this range.
pub const AGGRESSIVE_CHASE_RANGE: f32 = 14.0 * CITY_SCALE;

// ---------------------------------------------------------------------------
// The camera script (Arithmetica `civ_camera`, or the same numbers)
// ---------------------------------------------------------------------------

/// What the code sets before running the script (0xB0F1D0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScriptInputs {
    /// +1 zoom in, -1 zoom out, 0 none (one event).
    pub direction: f32,
    /// The goal distance (original units).
    pub distance: f32,
    pub pitch_deg: f32,
    pub yaw_deg: f32,
    /// `keyboard_scroll_x / y` before the script (ramp x speed).
    pub scroll: Vec2,
    pub rotate: f32,
    pub dt: f32,
    pub alt_zoom_mode: bool,
    /// `edge_distance_x / y` (0xB5D1CB: pixels from the nearest border, signed,
    /// plus 0.01; zero away from the window).
    pub edge: Vec2,
    pub scratch: [f32; 5],
}

/// What the code reads back after the script.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScriptOutputs {
    pub distance: f32,
    /// `keyboard_scroll_x / y` after the script (negated, scaled by distance).
    pub scroll: Vec2,
    pub rotate_deg: f32,
    pub mouse_scroll_sensitivity: f32,
    pub mouse_rotate_x: f32,
    pub mouse_rotate_y: f32,
    pub min_pitch_deg: f32,
    pub max_pitch_deg: f32,
    pub min_height: f32,
    pub acceleration_time: f32,
    pub translation_speed: f32,
    pub min_interpolation: f32,
    pub max_interpolation: f32,
    pub alt_zoom_threshold: f32,
    pub alt_zoom_timer: f32,
    /// `anchor_delta_x / y` (the edge scroll formula of the script).
    pub anchor_delta: Vec2,
    pub scratch: [f32; 5],
}

/// The camera script: the installed `civ_camera` when it could be read, the
/// same numbers in Rust otherwise (no install, tests).
#[derive(Clone, Debug, Default)]
pub struct CameraScript {
    vm: Option<Arithmetica>,
}

#[allow(dead_code)] // from_bytes / is_installed: tests and the HUD
impl CameraScript {
    /// The installed `civ_camera`, or the built-in numbers when it is missing or
    /// does not parse.
    pub fn from_install(originals: &Originals) -> Self {
        Self::from_install_named(originals, "civ_camera")
    }

    /// An installed camera script by name (`tribe_camera`, `civ_follow_camera`,
    /// `city_large_planner_camera`: the planet cameras share the format and the
    /// variables, so the Tribal camera can run its own script the same way).
    pub fn from_install_named(originals: &Originals, name: &str) -> Self {
        let key = ResourceKey {
            type_id: TYPE_ARITHMETICA,
            group_id: GROUP_SCRIPTS,
            instance_id: name_id(name),
        };
        match originals
            .resource_index
            .read(key)
            .and_then(|bytes| Arithmetica::from_bytes(&bytes))
        {
            Ok(vm) => Self { vm: Some(vm) },
            Err(_) => Self::default(),
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        Arithmetica::from_bytes(bytes).map(|vm| Self { vm: Some(vm) })
    }

    pub fn is_installed(&self) -> bool {
        self.vm.is_some()
    }

    pub fn evaluate(&mut self, input: &ScriptInputs) -> ScriptOutputs {
        let Some(vm) = self.vm.as_mut() else {
            return builtin(input);
        };
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        // The order of 0xB0F1D0 (the script overwrites the tuning defaults).
        for (i, v) in input.scratch.iter().enumerate() {
            vm.set(&format!("camera_scratch_var_{i}"), *v);
        }
        vm.set("keyboard_acceleration_time", 0.7);
        vm.set("keyboard_translation_speed", 10.0);
        vm.set("min_interpolation_time", 0.7);
        vm.set("max_interpolation_time", 6.0);
        vm.set("ballistic_max_height", 700.0);
        vm.set("default_distance", input.distance);
        vm.set("max_pitch", 45.0);
        vm.set("min_pitch", 0.0);
        vm.set("direction", input.direction);
        vm.set("distance", input.distance);
        vm.set("pitch", input.pitch_deg);
        vm.set("yaw", input.yaw_deg);
        vm.set("edge_distance_x", input.edge.x);
        vm.set("edge_distance_y", input.edge.y);
        vm.set("avatar_speed", 0.0);
        vm.set("anchor_delta_x", 0.0);
        vm.set("anchor_delta_y", 0.0);
        vm.set("mouse_move_x", 0.0);
        vm.set("mouse_move_y", 0.0);
        vm.set("keyboard_rotate", input.rotate);
        vm.set("keyboard_scroll_x", input.scroll.x);
        vm.set("keyboard_scroll_y", input.scroll.y);
        vm.set("mouse_rotate_sensitivity_x", tuning::MOUSE_ROTATE_SENSITIVITY_X);
        vm.set("mouse_rotate_sensitivity_y", tuning::MOUSE_ROTATE_SENSITIVITY_Y);
        vm.set("height_above_water", 5.0);
        vm.set("camera_delta_time", input.dt);
        vm.set("alt_zoom_mode", flag(input.alt_zoom_mode));
        if vm.compute_all().is_err() {
            return builtin(input);
        }
        let mut scratch = [0.0; 5];
        for (i, slot) in scratch.iter_mut().enumerate() {
            *slot = vm.get(&format!("camera_scratch_var_{i}"));
        }
        ScriptOutputs {
            distance: vm.get("distance"),
            scroll: Vec2::new(vm.get("keyboard_scroll_x"), vm.get("keyboard_scroll_y")),
            rotate_deg: vm.get("keyboard_rotate"),
            mouse_scroll_sensitivity: vm.get("mouse_scroll_sensitivity"),
            mouse_rotate_x: vm.get("mouse_rotate_sensitivity_x"),
            mouse_rotate_y: vm.get("mouse_rotate_sensitivity_y"),
            min_pitch_deg: vm.get("min_pitch"),
            max_pitch_deg: vm.get("max_pitch"),
            min_height: vm.get("min_height"),
            acceleration_time: vm.get("keyboard_acceleration_time"),
            translation_speed: vm.get("keyboard_translation_speed"),
            min_interpolation: vm.get("min_interpolation_time"),
            max_interpolation: vm.get("max_interpolation_time"),
            alt_zoom_threshold: vm.get("alt_zoom_threshold"),
            alt_zoom_timer: vm.get("alt_zoom_timer"),
            anchor_delta: Vec2::new(vm.get("anchor_delta_x"), vm.get("anchor_delta_y")),
            scratch,
        }
    }
}

/// Pure: the zoom ladder of the script. Zooming in picks the largest step below
/// the goal distance, zooming out the smallest above it; 15 needs the alt rule.
pub fn zoom_ladder(distance: f32, direction: f32, alt: bool) -> f32 {
    use tuning::{ALT_ZOOM_STEP, ZOOM_STEPS};
    if direction > 0.0 {
        if let Some(step) = ZOOM_STEPS.iter().rev().find(|s| **s < distance) {
            return *step;
        }
        if alt && distance > ALT_ZOOM_STEP {
            return ALT_ZOOM_STEP;
        }
    } else if direction < 0.0 {
        if alt && distance < ALT_ZOOM_STEP {
            return ALT_ZOOM_STEP;
        }
        if let Some(step) = ZOOM_STEPS.iter().find(|s| **s > distance) {
            return *step;
        }
    }
    distance
}

/// The edge scroll formula of the `civ_camera` script: `edge` is the script's
/// `edge_distance` (x, y), `scratch` the dwell timers `camera_scratch_var_0..3`
/// (x positive, x negative, y positive, y negative; each grows by
/// `3 x dt` up to 6 while the cursor stays in the zone and resets when it
/// leaves). Returns `anchor_delta`.
pub fn edge_scroll_delta(edge: Vec2, distance: f32, dt: f32, scratch: &mut [f32; 5]) -> Vec2 {
    edge_scroll_block(edge, distance, dt, scratch, 3.0, 6.0, [450.0, 300.0])
}

/// The edge scroll block shared by the planet camera scripts: the Civ script
/// uses dwell rate 3, limit 6 and divisors 450 / 300, the `tribe_camera`
/// script 0.5, 3 and 300 / 200 (the block itself is identical).
pub fn edge_scroll_block(
    edge: Vec2,
    distance: f32,
    dt: f32,
    scratch: &mut [f32; 5],
    delta_time_modifier: f32,
    time_limit: f32,
    divisors: [f32; 2],
) -> Vec2 {
    let mut delta = Vec2::ZERO;
    let fraction = edge / 2.0;
    // (fraction, the two dwell slots, divisor) per axis.
    for (axis, slots) in [(0usize, [0usize, 1]), (1usize, [2usize, 3])] {
        let divisor = divisors[axis];
        let f = fraction[axis];
        if f < 1.0 && f > 0.0 {
            scratch[slots[0]] = (scratch[slots[0]] + dt * delta_time_modifier).min(time_limit);
            delta[axis] = ((1.0 - f) + scratch[slots[0]]) * distance / divisor;
        } else if f > -1.0 && f < 0.0 {
            scratch[slots[1]] = (scratch[slots[1]] + dt * delta_time_modifier).min(time_limit);
            delta[axis] = -((1.0 + f) + scratch[slots[1]]) * distance / divisor;
        } else {
            scratch[slots[0]] = 0.0;
            scratch[slots[1]] = 0.0;
        }
    }
    delta
}

/// The `civ_camera` script in Rust: used without an install, and checked
/// against the installed script by an ignored test.
pub fn builtin(input: &ScriptInputs) -> ScriptOutputs {
    let distance = zoom_ladder(input.distance, input.direction, input.alt_zoom_mode);
    let mut scratch = input.scratch;
    let anchor_delta = edge_scroll_delta(input.edge, distance, input.dt, &mut scratch);
    ScriptOutputs {
        distance,
        scroll: Vec2::new(
            input.scroll.x * -distance / tuning::KEYBOARD_SCROLL_DIVISOR,
            input.scroll.y * -distance / tuning::KEYBOARD_SCROLL_DIVISOR,
        ),
        rotate_deg: input.rotate * tuning::KEYBOARD_ROTATE_DEGREES,
        mouse_scroll_sensitivity: tuning::MOUSE_SCROLL_SENSITIVITY_BASE + distance,
        mouse_rotate_x: tuning::MOUSE_ROTATE_SENSITIVITY_X,
        mouse_rotate_y: tuning::MOUSE_ROTATE_SENSITIVITY_Y,
        min_pitch_deg: tuning::MIN_PITCH_DEG,
        max_pitch_deg: tuning::MAX_PITCH_DEG,
        min_height: tuning::MIN_HEIGHT,
        acceleration_time: tuning::KEYBOARD_ACCELERATION_TIME,
        translation_speed: tuning::KEYBOARD_TRANSLATION_SPEED,
        min_interpolation: tuning::MIN_INTERPOLATION_TIME,
        max_interpolation: tuning::MAX_INTERPOLATION_TIME,
        alt_zoom_threshold: tuning::ALT_ZOOM_THRESHOLD,
        alt_zoom_timer: tuning::ALT_ZOOM_TIMER,
        anchor_delta,
        scratch,
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// The retail speed ramp (0xB0EFC0): smoothstep of `held / accel` clamped.
pub fn ramp(held: f32, accel: f32) -> f32 {
    if accel <= 0.0 {
        return if held > 0.0 { 1.0 } else { 0.0 };
    }
    let x = (held / accel).clamp(0.0, 1.0);
    (3.0 - 2.0 * x) * x * x
}

/// Critically damped easing toward `target` (Unity-style SmoothDamp). No stage
/// uses it since the Civ and Tribal rigs move their channels on Hermite segments.
#[allow(dead_code)]
pub fn smooth_damp(current: f32, target: f32, velocity: &mut f32, smooth_time: f32, dt: f32) -> f32 {
    let smooth_time = smooth_time.max(1.0e-4);
    let omega = 2.0 / smooth_time;
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*velocity + omega * change) * dt;
    *velocity = (*velocity - omega * temp) * exp;
    target + (change + temp) * exp
}

/// Camera offset from the anchor: azimuth 0 is behind a -Z facing view (the
/// convention of `play_camera`).
pub fn orbit_offset(azimuth: f32, pitch: f32, distance: f32) -> Vec3 {
    Vec3::new(
        azimuth.sin() * pitch.cos(),
        pitch.sin(),
        azimuth.cos() * pitch.cos(),
    ) * distance
}

/// Horizontal camera-relative forward and right (`PlayCamera` basis).
pub fn move_basis(azimuth: f32) -> (Vec3, Vec3) {
    let forward = Vec3::new(-azimuth.sin(), 0.0, -azimuth.cos());
    (forward, forward.cross(Vec3::Y))
}

/// Pure: joystick-style drag scroll (0xB14800 `+0x11` branch): the offset of
/// the cursor from where the button went down, over half the window, squared
/// with its sign, in `-1..1` per axis (x right, y forward).
pub fn drag_scroll_vector(press: Vec2, cursor: Vec2, window: Vec2) -> Vec2 {
    let half = (window * 0.5).max(Vec2::ONE);
    let n = ((cursor - press) / half).clamp(Vec2::splat(-1.0), Vec2::ONE);
    // Screen y grows downward: dragging up scrolls forward.
    Vec2::new(n.x.abs() * n.x, -(n.y.abs() * n.y))
}

/// Pure: the script's `edge_distance` for a cursor position (0xB5D1CB): the
/// pixel distance from the nearest vertical / horizontal window border, positive
/// on the left / top half and negative on the right / bottom half, plus 0.01;
/// zero while the cursor is outside the window. The script only scrolls while
/// `0 < |edge| < 2`, i.e. in the outermost two pixels of the window.
pub fn edge_distance(cursor: Vec2, window: Vec2) -> Vec2 {
    let axis = |at: f32, size: f32| {
        let (at, size) = (at.floor(), size.floor());
        if at < 0.0 || at >= size {
            return 0.0;
        }
        let nearest = if at > (size / 2.0).floor() { at - size } else { at };
        nearest + interp::EDGE_PIXEL_OFFSET
    };
    Vec2::new(axis(cursor.x, window.x), axis(cursor.y, window.y))
}

/// Pure: whether an `edge_distance` is inside the script's edge scroll zone
/// on either axis.
pub fn edge_in_zone(edge: Vec2) -> bool {
    let inside = |v: f32| v.abs() > 0.0 && v.abs() < 2.0;
    inside(edge.x) || inside(edge.y)
}

/// Cubic Hermite from `p0` (velocity `v0`) to `p1` (velocity `v1`) over
/// `total` seconds, evaluated `dt` seconds in (0xB0FBA0 for numbers, 0xB10DB0
/// for vectors: `a = 2 (p0 - p1) + T (v0 + v1)` is the cubic coefficient).
pub fn hermite(p0: f32, v0: f32, p1: f32, v1: f32, total: f32, dt: f32) -> f32 {
    let (h00, h10, h01, h11) = hermite_basis(total, dt);
    h00 * p0 + h10 * total * v0 + h01 * p1 + h11 * total * v1
}

/// [`hermite`] for vectors.
pub fn hermite3(p0: Vec3, v0: Vec3, p1: Vec3, v1: Vec3, total: f32, dt: f32) -> Vec3 {
    let (h00, h10, h01, h11) = hermite_basis(total, dt);
    p0 * h00 + v0 * (h10 * total) + p1 * h01 + v1 * (h11 * total)
}

fn hermite_basis(total: f32, dt: f32) -> (f32, f32, f32, f32) {
    let s = if total > 0.0 { (dt / total).clamp(0.0, 1.0) } else { 1.0 };
    let (s2, s3) = (s * s, s * s * s);
    (
        2.0 * s3 - 3.0 * s2 + 1.0,
        s3 - 2.0 * s2 + s,
        -2.0 * s3 + 3.0 * s2,
        s3 - s2,
    )
}

/// The exponential settle of a finished channel (0xB11930): the fraction of
/// the remaining distance covered in `dt`.
fn settle_fraction(dt: f32) -> f32 {
    1.0 - (-0.30103 * dt / interp::SETTLE_TIME).exp()
}

/// One interpolated number of the planet camera (yaw, pitch, distance, the
/// anchor's height): a cubic Hermite segment that is re-solved on every update
/// from the CURRENT value and velocity to the goal over the remaining segment
/// time, ending at rest (0xB11930). A changed goal restarts the segment
/// (`t = 0`) but keeps the velocity, so the motion never jumps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Segment {
    /// The velocity measured over the last update (units per second).
    pub vel: f32,
    t: f32,
    dur: f32,
    goal: f32,
}

impl Segment {
    pub fn at_rest(goal: f32) -> Self {
        Self {
            goal,
            ..default()
        }
    }

    /// The segment is still running (the "`dur - t > 0`" test of 0xB10BA0).
    pub fn active(&self) -> bool {
        self.t < self.dur
    }

    /// Move the goal by `delta` (a yaw re-centred by whole turns: the
    /// callers shift the current value, their goal and the segment alike).
    pub fn shift(&mut self, delta: f32) {
        self.goal += delta;
    }

    /// Start a segment of `dur` seconds to `goal` (the velocity is kept).
    pub fn restart(&mut self, goal: f32, dur: f32) {
        self.goal = goal;
        self.t = 0.0;
        self.dur = dur;
    }

    /// One update: the new value. `dur` is the segment time a restart takes
    /// (when `goal` differs from the one of the last call).
    pub fn step(&mut self, cur: f32, goal: f32, dt: f32, dur: f32) -> f32 {
        if goal != self.goal {
            self.restart(goal, dur);
        }
        if dt <= 0.0 {
            return cur;
        }
        if self.t < self.dur {
            self.t += dt;
            // The end velocity is zero, so the remaining time is shortened.
            let remaining = (self.dur - self.t) * interp::SHORTEN;
            let total = if remaining > dt { remaining } else { dt };
            let next = hermite(cur, self.vel, self.goal, 0.0, total, dt);
            self.vel = (next - cur) / dt;
            next
        } else {
            self.vel = 0.0;
            if (self.goal - cur).abs() > interp::EPSILON {
                cur + (self.goal - cur) * settle_fraction(dt)
            } else {
                self.goal
            }
        }
    }
}

/// [`Segment`] for the anchor: a point (the planet's unit direction under the
/// anchor, or the world position on the plane).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VecSegment {
    pub vel: Vec3,
    t: f32,
    dur: f32,
    goal: Vec3,
}

impl VecSegment {
    pub fn at_rest(goal: Vec3) -> Self {
        Self {
            goal,
            ..default()
        }
    }

    /// `goal` is a new goal (further than `tolerance` from the running one).
    pub fn changed(&self, goal: Vec3, tolerance: f32) -> bool {
        (goal - self.goal).length() > tolerance
    }

    pub fn restart(&mut self, goal: Vec3, dur: f32) {
        self.goal = goal;
        self.t = 0.0;
        self.dur = dur;
    }

    /// One update (0xB11930). `unit`: the point lives on the unit sphere (the
    /// result is renormalised and the velocity made tangent, as the retail
    /// anchor direction is).
    pub fn step(&mut self, cur: Vec3, goal: Vec3, dt: f32, dur: f32, unit: bool, tolerance: f32) -> Vec3 {
        if self.changed(goal, tolerance) {
            self.restart(goal, dur);
        }
        if dt <= 0.0 {
            return cur;
        }
        let fix = |p: Vec3, fallback: Vec3| if unit { p.normalize_or(fallback) } else { p };
        if self.t < self.dur {
            self.t += dt;
            let remaining = (self.dur - self.t) * interp::SHORTEN;
            let total = if remaining > dt { remaining } else { dt };
            let next = fix(hermite3(cur, self.vel, self.goal, Vec3::ZERO, total, dt), cur);
            let mut vel = (next - cur) / dt;
            if unit {
                vel -= next * vel.dot(next);
            }
            self.vel = vel;
            next
        } else {
            self.vel = Vec3::ZERO;
            let delta = self.goal - cur;
            if delta.length() > interp::EPSILON {
                fix(cur + delta * settle_fraction(dt), cur)
            } else {
                self.goal
            }
        }
    }
}

/// What the player asked of the camera this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    /// `.` (rotate left, azimuth +) and `,` (rotate right, azimuth -).
    pub rotate_left: bool,
    pub rotate_right: bool,
    /// PageUp lowers the elevation (the view tilts up), PageDown raises it.
    pub pitch_up: bool,
    pub pitch_down: bool,
    /// Net zoom events this frame (+ in, - out), already one per press / notch.
    pub zoom_events: i32,
    /// Drag scroll vector (`drag_scroll_vector`).
    pub drag_scroll: Vec2,
    /// The script's `edge_distance` (`edge_distance`; zero when edge scroll is
    /// off or the cursor is not at a border).
    pub edge: Vec2,
    /// Mouse motion in pixels while orbiting.
    pub orbit: Vec2,
}

/// The eased orbit camera. Angles in radians, distance in original units
/// (multiply by [`CAMERA_WORLD_SCALE`] for the world).
#[derive(Resource, Clone, Debug)]
pub struct CivCamera {
    pub azimuth: f32,
    pub pitch: f32,
    pub distance: f32,
    pub goal_azimuth: f32,
    pub goal_pitch: f32,
    pub goal_distance: f32,
    /// The interpolated channels (see [`Segment`]).
    yaw_seg: Segment,
    pitch_seg: Segment,
    distance_seg: Segment,
    /// The anchor: the unit direction under it on the planet (the world
    /// position on the plane), and its height / radius.
    anchor_seg: VecSegment,
    height_seg: Segment,
    /// The eased anchor (the focus entity is the goal).
    pub anchor: Vec3,
    /// The angle between the anchor and its goal at the last update.
    anchor_angle: f32,
    /// The orbit's frame at the anchor (local -> world, +Y = the local up),
    /// carried along the surface as the anchor moves.
    pub frame: Quat,
    /// The stage's surface shape (the planet's sphere, else the plane).
    pub sphere: Shape,
    alt_zoom: bool,
    alt_timer: f32,
    /// Seconds each movement / rotation key has been held.
    held: [f32; 6],
    script: CameraScript,
    last: ScriptOutputs,
    /// `camera_scratch_var_0..4`, kept by the code between updates (the edge
    /// scroll dwell timers).
    scratch: [f32; 5],
    /// The last non-zero scroll (camera frame) and whether the anchor was
    /// scrolling on the last update (the coast on key release).
    last_scroll: Vec2,
    scrolling: bool,
    /// The segment time of the coast restart that is pending.
    coast: Option<f32>,
    wheel: f32,
    zoom_hold: f32,
    /// A reset is pending (stage entry, before the focus exists).
    pub needs_reset: bool,
    /// The script has been looked up (install or fallback).
    script_checked: bool,
}

impl Default for CivCamera {
    fn default() -> Self {
        Self::new(CameraScript::default())
    }
}

#[allow(dead_code)] // accessors for the HUD and the audio
impl CivCamera {
    pub fn new(script: CameraScript) -> Self {
        let mut camera = Self {
            azimuth: tuning::INITIAL_AZIMUTH,
            pitch: tuning::INITIAL_PITCH,
            distance: tuning::INITIAL_DISTANCE,
            goal_azimuth: tuning::INITIAL_AZIMUTH,
            goal_pitch: tuning::INITIAL_PITCH,
            goal_distance: tuning::INITIAL_DISTANCE,
            yaw_seg: Segment::at_rest(tuning::INITIAL_AZIMUTH),
            pitch_seg: Segment::at_rest(tuning::INITIAL_PITCH),
            distance_seg: Segment::at_rest(tuning::INITIAL_DISTANCE),
            anchor_seg: VecSegment::at_rest(Vec3::ZERO),
            height_seg: Segment::at_rest(0.0),
            anchor: Vec3::ZERO,
            anchor_angle: 0.0,
            frame: Quat::IDENTITY,
            sphere: Shape::FLAT,
            alt_zoom: false,
            alt_timer: -1.0,
            held: [0.0; 6],
            script,
            last: ScriptOutputs::default(),
            scratch: [0.0; 5],
            last_scroll: Vec2::ZERO,
            scrolling: false,
            coast: None,
            wheel: 0.0,
            zoom_hold: 0.0,
            needs_reset: true,
            script_checked: false,
        };
        camera.last = camera.script.evaluate(&camera.script_inputs(0.0, 0.0));
        camera
    }

    fn script_inputs(&self, direction: f32, dt: f32) -> ScriptInputs {
        ScriptInputs {
            direction,
            distance: self.goal_distance,
            pitch_deg: self.goal_pitch.to_degrees(),
            yaw_deg: self.goal_azimuth.to_degrees(),
            dt,
            alt_zoom_mode: self.alt_zoom,
            ..default()
        }
    }

    pub fn set_script(&mut self, script: CameraScript) {
        self.script = script;
        self.script_checked = true;
    }

    pub fn script_checked(&self) -> bool {
        self.script_checked
    }

    pub fn script(&self) -> &CameraScript {
        &self.script
    }

    /// The stage-start view: retail polar coordinates, snapped, over `anchor`.
    pub fn reset(&mut self, anchor: Vec3) {
        self.azimuth = tuning::INITIAL_AZIMUTH;
        self.pitch = tuning::INITIAL_PITCH;
        self.distance = tuning::INITIAL_DISTANCE;
        self.goal_azimuth = self.azimuth;
        self.goal_pitch = self.pitch;
        self.goal_distance = self.distance;
        self.yaw_seg = Segment::at_rest(self.goal_azimuth);
        self.pitch_seg = Segment::at_rest(self.goal_pitch);
        self.distance_seg = Segment::at_rest(self.goal_distance);
        self.anchor = anchor;
        match self.sphere.centre {
            Some(c) => {
                let up = self.sphere.up(anchor);
                self.anchor_seg = VecSegment::at_rest(up);
                self.height_seg = Segment::at_rest((anchor - c).length());
            }
            None => {
                self.anchor_seg = VecSegment::at_rest(anchor);
                self.height_seg = Segment::at_rest(0.0);
            }
        }
        self.anchor_angle = 0.0;
        self.scratch = [0.0; 5];
        self.last_scroll = Vec2::ZERO;
        self.scrolling = false;
        self.coast = None;
        self.frame = self.sphere.frame(anchor);
        self.alt_zoom = false;
        self.alt_timer = -1.0;
        self.held = [0.0; 6];
        self.wheel = 0.0;
        self.zoom_hold = 0.0;
        self.needs_reset = false;
    }

    /// Distance in world units.
    pub fn world_distance(&self) -> f32 {
        self.distance * CAMERA_WORLD_SCALE
    }

    /// The camera is farther than `PlumpDistance`: vehicles and halls are shown
    /// at their zoom-out scale.
    pub fn plump(&self) -> bool {
        self.distance > tuning::PLUMP_DISTANCE
    }

    pub fn alt_zoom(&self) -> bool {
        self.alt_zoom
    }

    pub fn last_outputs(&self) -> &ScriptOutputs {
        &self.last
    }

    /// One retail zoom event (0xB14800 with a direction): the alt-zoom rule,
    /// then the script snaps the goal distance to the next ladder step.
    pub fn zoom_step(&mut self, direction: i32) {
        if direction == 0 {
            return;
        }
        let dir = direction.signum() as f32;
        let threshold = self.last.alt_zoom_threshold;
        if dir > 0.0 && !self.alt_zoom && self.distance <= threshold {
            if self.alt_timer == 0.0 {
                self.alt_zoom = true;
                self.alt_timer = -1.0;
            } else if self.alt_timer < 0.0 {
                self.alt_timer = self.last.alt_zoom_timer;
            }
        }
        if dir < 0.0 && self.alt_zoom && threshold <= self.distance {
            self.alt_zoom = false;
        }
        let out = self.script.evaluate(&self.script_inputs(dir, 0.0));
        self.last = out;
        self.goal_distance = out.distance.max(0.1);
    }

    fn ramps(&self) -> [f32; 6] {
        let accel = self.last.acceleration_time;
        self.held.map(|t| ramp(t, accel))
    }

    /// One frame of input. Returns the goal offset of the anchor in world
    /// units: where the goal is relative to the CURRENT (eased) anchor
    /// (0xB14800 restarts the anchor segment `scroll` ahead of the current
    /// anchor on every update that scrolls, never ahead of the old goal), or
    /// zero when nothing scrolls. The caller puts the focus there
    /// (`shape.advance(camera.anchor, offset)`) and clamps it to the world.
    pub fn update(&mut self, dt: f32, input: &CameraInput) -> Vec3 {
        let dt = dt.clamp(0.0, 0.1);
        // Alt-zoom dwell timer.
        if self.alt_timer > 0.0 {
            self.alt_timer = (self.alt_timer - dt).max(0.0);
        }
        // Held-time counters.
        let pressed = [
            input.forward,
            input.backward,
            input.left,
            input.right,
            input.rotate_left,
            input.rotate_right,
        ];
        for (held, on) in self.held.iter_mut().zip(pressed) {
            *held = if on { *held + dt } else { 0.0 };
        }
        for _ in 0..input.zoom_events.unsigned_abs().min(4) {
            self.zoom_step(input.zoom_events.signum());
        }
        let ramps = self.ramps();
        // The script turns the ramped keys into scroll and rotate amounts, and
        // the cursor at the window border into the edge scroll anchor delta.
        let speed = self.last.translation_speed;
        let mut inputs = self.script_inputs(0.0, dt);
        inputs.scroll = Vec2::new(ramps[3] - ramps[2], ramps[0] - ramps[1]) * speed;
        inputs.rotate = ramps[4] - ramps[5];
        inputs.edge = input.edge;
        inputs.scratch = self.scratch;
        let out = self.script.evaluate(&inputs);
        self.last = out;
        self.scratch = out.scratch;

        // Scroll per update (x right, y forward; 0xB14800): the keys (the
        // script negates its input), the drag scroll joystick, and the edge
        // scroll anchor delta times 1500 per second.
        // ADAPTED: the edge scroll time step is the nominal update, so the
        // speed does not ride the frame rate (retail multiplies by its own dt).
        let keys = -out.scroll;
        let drag = input.drag_scroll * out.mouse_scroll_sensitivity;
        let edge = Vec2::new(-out.anchor_delta.x, out.anchor_delta.y)
            * (interp::EDGE_SCROLL_SPEED * NOMINAL_UPDATE_SECONDS);
        let scroll = keys + drag + edge;
        let (forward, right) = move_basis(self.azimuth);
        let frame = self.frame;
        let to_world = |v: Vec2| frame * ((right * v.x + forward * v.y) * CAMERA_WORLD_SCALE);
        let pan = if scroll != Vec2::ZERO {
            self.last_scroll = scroll;
            self.scrolling = true;
            self.coast = None;
            to_world(scroll)
        } else if self.scrolling {
            // The pan keys came up: the anchor coasts on along the last
            // direction by `|velocity| x min_interp x 250` (retail `|velocity|`
            // is the anchor speed in radians a second on the unit sphere).
            // ADAPTED: the retail factor is a function-local static set on the
            // first update ever (0.7 when Tribal ran first); this stage's own
            // `min_interpolation_time` is used.
            self.scrolling = false;
            let reach = self.anchor_speed() * out.min_interpolation * interp::COAST_SPEED_SCALE;
            if reach > 1.0e-6 {
                self.coast = Some(out.min_interpolation);
            }
            to_world(self.last_scroll.normalize_or_zero() * reach)
        } else {
            Vec3::ZERO
        };

        // Rotation and pitch keys move the goals (0xB14800 / 0xB109F0).
        self.goal_azimuth += out.rotate_deg.to_radians() * UPDATES_PER_SECOND * dt;
        let tilt = (input.pitch_down as i32 - input.pitch_up as i32) as f32;
        let pitch_rate = (out.mouse_rotate_y * 10.0).to_radians() * 5.0;
        self.goal_pitch += tilt * pitch_rate * dt;
        // Mouse orbit (middle or left + right button, 0xB0FDF0): each move puts
        // the goal an offset away from the CURRENT angle, not from the goal.
        if input.orbit != Vec2::ZERO {
            // ADAPTED: the offset is per update; keep the speed at other rates.
            let rate = if dt > 0.0 {
                (NOMINAL_UPDATE_SECONDS / dt).clamp(0.25, 4.0)
            } else {
                1.0
            };
            let dx = (input.orbit.x * rate).clamp(-MOUSE_ORBIT_CLAMP, MOUSE_ORBIT_CLAMP);
            let dy = (input.orbit.y * rate).clamp(-MOUSE_ORBIT_CLAMP, MOUSE_ORBIT_CLAMP);
            if dx != 0.0 {
                self.goal_azimuth = self.azimuth - out.mouse_rotate_x.to_radians() * dx;
            }
            if dy != 0.0 {
                self.goal_pitch = self.pitch + (out.mouse_rotate_y * 10.0).to_radians() * dy;
            }
        }
        self.goal_pitch = self
            .goal_pitch
            .clamp(out.min_pitch_deg.to_radians(), out.max_pitch_deg.to_radians());
        // Keep both azimuths in a bounded range without a visible jump.
        if self.goal_azimuth.abs() > std::f32::consts::TAU * 2.0 {
            let turn = (self.goal_azimuth / std::f32::consts::TAU).trunc() * std::f32::consts::TAU;
            self.goal_azimuth -= turn;
            self.azimuth -= turn;
            self.yaw_seg.goal -= turn;
        }
        self.smooth(dt);
        pan
    }

    /// Somebody moved the goal anchor by hand (a jump to a city, a vehicle,
    /// the minimap): a pan coast that was about to start does not apply.
    pub fn cancel_coast(&mut self) {
        self.scrolling = false;
        self.coast = None;
    }

    /// The anchor speed in radians a second on the unit sphere (the retail
    /// `|+0x78|`); on the plane the speed over [`FLAT_RADIUS`].
    pub fn anchor_speed(&self) -> f32 {
        let speed = self.anchor_seg.vel.length();
        if self.sphere.on_sphere() {
            speed
        } else {
            speed / FLAT_RADIUS
        }
    }

    /// The segment time of a restart of the anchor and distance channels:
    /// `lerp(min, max interpolation time, angle / pi)` with the angle between
    /// the current anchor and its goal (0xB0F7E0).
    pub fn interpolation_time(&self) -> f32 {
        let t = (self.anchor_angle / std::f32::consts::PI).clamp(0.0, 1.0);
        self.last.min_interpolation + (self.last.max_interpolation - self.last.min_interpolation) * t
    }

    /// The orbit button came up (0xB10BA0): a yaw / pitch segment that is
    /// still running coasts on, the goal `velocity x 0.1 x 0.5` ahead of the
    /// current angle, over 0.1 s.
    pub fn end_orbit(&mut self) {
        if self.yaw_seg.active() {
            self.goal_azimuth = self.azimuth + self.yaw_seg.vel * interp::ORBIT_COAST * 0.5;
            self.yaw_seg.restart(self.goal_azimuth, interp::ORBIT_COAST);
        }
        if self.pitch_seg.active() {
            // ADAPTED: kept inside the pitch limits (0xB10BA0 does not clamp).
            self.goal_pitch = (self.pitch + self.pitch_seg.vel * interp::ORBIT_COAST * 0.5).clamp(
                self.last.min_pitch_deg.to_radians(),
                self.last.max_pitch_deg.to_radians(),
            );
            self.pitch_seg.restart(self.goal_pitch, interp::ORBIT_COAST);
        }
    }

    /// Move the current angles and the distance toward their goals (the
    /// channels of 0xB11930: yaw 0.2 s, pitch 0.65 s, distance the
    /// interpolation time).
    pub fn smooth(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        let segment = self.interpolation_time();
        self.azimuth = self
            .yaw_seg
            .step(self.azimuth, self.goal_azimuth, dt, interp::YAW_SEGMENT);
        self.pitch = self
            .pitch_seg
            .step(self.pitch, self.goal_pitch, dt, interp::PITCH_SEGMENT);
        self.distance = self
            .distance_seg
            .step(self.distance, self.goal_distance, dt, segment)
            .max(0.1);
    }

    /// Move the anchor toward the focus entity position (the retail goal
    /// anchor). On the planet the anchor is a unit direction that travels over
    /// the sphere with a radius of its own (the retail anchor direction and
    /// height channels), and the orbit frame is carried along by the rotation
    /// between the old and the new local up (no heading jumps anywhere on the
    /// sphere).
    pub fn follow_focus(&mut self, focus: Vec3, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        match self.sphere.centre {
            Some(c) => {
                let goal_dir = (focus - c).normalize_or(self.sphere.up(focus));
                let goal_radius = (focus - c).length();
                let cur_dir = (self.anchor - c).normalize_or(goal_dir);
                let cur_radius = (self.anchor - c).length();
                self.anchor_angle = cur_dir.dot(goal_dir).clamp(-1.0, 1.0).acos();
                let dur = self.restart_duration(self.anchor_seg.changed(goal_dir, 1.0e-6));
                let dir = self
                    .anchor_seg
                    .step(cur_dir, goal_dir, dt, dur, true, 1.0e-6);
                let radius = self.height_seg.step(cur_radius, goal_radius, dt, dur);
                self.anchor = c + dir * radius;
                // From the frame's own up, so no step is ever lost.
                let turn = crate::civ_planet::arc(
                    (self.frame * Vec3::Y).normalize(),
                    self.sphere.up(self.anchor),
                );
                self.frame = (turn * self.frame).normalize();
            }
            None => {
                self.anchor_angle = (focus - self.anchor).length() / FLAT_RADIUS;
                let dur = self.restart_duration(self.anchor_seg.changed(focus, 1.0e-4));
                self.anchor = self
                    .anchor_seg
                    .step(self.anchor, focus, dt, dur, false, 1.0e-4);
                self.frame = Quat::IDENTITY;
            }
        }
    }

    /// The segment time of an anchor restart: the pending coast duration after
    /// the pan keys came up (0xB0FB40 sets the segment to `min_interp`), else
    /// the angle based interpolation time.
    fn restart_duration(&mut self, restarting: bool) -> f32 {
        match (restarting, self.coast.take()) {
            (true, Some(coast)) => coast,
            (true, None) => self.interpolation_time(),
            (false, coast) => {
                // Not restarting: a pending coast stays for the restart it
                // belongs to.
                self.coast = coast;
                self.interpolation_time()
            }
        }
    }

    /// The eased camera transform: the orbit offset from the anchor in its
    /// frame, raised to `min_height` above the terrain along the local up,
    /// looking at the anchor. `ground` is the surface point under a position.
    pub fn pose(&self, ground: impl Fn(Vec3) -> Vec3) -> Transform {
        let offset = orbit_offset(self.azimuth, self.pitch, self.world_distance());
        let mut eye = self.anchor + self.frame * offset;
        let up = self.sphere.up(eye);
        let height = (eye - ground(eye)).dot(up);
        let min = self.last.min_height.max(0.0);
        if height < min {
            eye += up * (min - height);
        }
        Transform::from_translation(eye).looking_at(self.anchor, self.frame * Vec3::Y)
    }

    /// Wheel accumulator: whole notches become zoom events (positive = in).
    pub fn take_wheel_notches(&mut self, lines: f32) -> i32 {
        if !lines.is_finite() {
            return 0;
        }
        self.wheel += lines;
        let notches = self.wheel.trunc();
        self.wheel -= notches;
        notches as i32
    }

    /// Zoom key held-time bookkeeping: one event on the press, repeats after a
    /// delay (ADAPTED, the retail polls the key every update).
    pub fn zoom_key_events(&mut self, in_pressed: bool, out_pressed: bool, just: bool, dt: f32) -> i32 {
        let dir = match (in_pressed, out_pressed) {
            (true, false) => 1,
            (false, true) => -1,
            _ => {
                self.zoom_hold = 0.0;
                return 0;
            }
        };
        if just {
            self.zoom_hold = 0.0;
            return dir;
        }
        let before = self.zoom_hold;
        self.zoom_hold += dt;
        if self.zoom_hold >= ZOOM_REPEAT_DELAY {
            let period = ((self.zoom_hold - ZOOM_REPEAT_DELAY) / ZOOM_REPEAT_PERIOD).floor();
            let last = ((before - ZOOM_REPEAT_DELAY) / ZOOM_REPEAT_PERIOD).floor();
            if before < ZOOM_REPEAT_DELAY || period > last {
                return dir;
            }
        }
        0
    }
}

// ---------------------------------------------------------------------------
// Controls state shared with the HUD and the audio
// ---------------------------------------------------------------------------

/// A vehicle's stance (manual: Tab switches it). Runtime only: not saved.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CivStance {
    /// Holds its position and answers whatever comes within weapon range.
    #[default]
    StandGround,
    /// Actively engages hostile vehicles that come near.
    Aggressive,
}

impl CivStance {
    pub fn toggled(self) -> Self {
        match self {
            Self::StandGround => Self::Aggressive,
            Self::Aggressive => Self::StandGround,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::StandGround => "Stand ground",
            Self::Aggressive => "Aggressive",
        }
    }
}

/// What the cursor is over (the context command of a right click).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum CivHover {
    #[default]
    None,
    Ground(Vec3),
    Vehicle {
        id: u32,
        nation: usize,
    },
    City(usize),
    Geyser(usize),
}

/// The HUD's camera buttons (retail ids 0x04CAB570 / 71 / 6E / 6F) are hold
/// buttons that raise four flags of the camera. The HUD sets them every frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HudCamera {
    pub zoom_in: bool,
    pub zoom_out: bool,
    pub rotate_left: bool,
    pub rotate_right: bool,
}

/// The Civ controls state the HUD and audio read.
#[derive(Resource, Debug, Clone)]
pub struct CivControls {
    /// HUD camera buttons currently held (written by the HUD).
    pub hud: HudCamera,
    /// Written by the minimap: move the camera there (left click)...
    pub goto_request: Option<Vec3>,
    /// ...or send the selection there (right click).
    pub move_request: Option<Vec3>,
    /// Edge scrolling: the retail "Edge Scroll" option of the Settings page
    /// (`front_end::settings::apply` copies it here; its default is off).
    pub edge_scroll: bool,
    /// The city the player clicked last (own or rival).
    pub selected_city: Option<usize>,
    pub hover: CivHover,
    /// Vehicle ids of the groups defined with Ctrl + digit (index 0 = key 1,
    /// ..., 8 = key 9, 9 = key 0).
    pub groups: [Vec<u32>; 10],
    /// The group the last Space press selected.
    pub group_cursor: Option<usize>,
    /// The vehicle the camera follows (Space), until the camera is panned.
    pub follow: Option<u32>,
    /// The mouse is orbiting / drag scrolling this frame (cursor icons).
    pub orbiting: bool,
    pub panning: bool,
    /// Mouse state of the camera, kept between frames.
    right_press: Option<Vec2>,
    right_drag: bool,
    /// The right-button drag has become a camera rotation (the Civ control
    /// option, `UIMachineCivRotate`), until the button comes up.
    right_rotating: bool,
    left_down: bool,
    right_down: bool,
    left_press: Option<Vec2>,
    hud_zoom_was_down: bool,
    /// Where the camera system left the focus last frame: a focus anywhere
    /// else was moved by somebody else (a jump), which ends a pan coast.
    last_goal: Option<Vec3>,
}

impl Default for CivControls {
    fn default() -> Self {
        Self {
            hud: HudCamera::default(),
            goto_request: None,
            move_request: None,
            edge_scroll: false,
            selected_city: None,
            hover: CivHover::None,
            groups: Default::default(),
            group_cursor: None,
            follow: None,
            orbiting: false,
            panning: false,
            right_press: None,
            right_drag: false,
            right_rotating: false,
            left_down: false,
            right_down: false,
            left_press: None,
            hud_zoom_was_down: false,
            last_goal: None,
        }
    }
}

#[allow(dead_code)]
/// The group slot of a digit key (`Digit1` = 0 ... `Digit9` = 8, `Digit0` = 9).
pub fn group_slot(key: KeyCode) -> Option<usize> {
    Some(match key {
        KeyCode::Digit1 => 0,
        KeyCode::Digit2 => 1,
        KeyCode::Digit3 => 2,
        KeyCode::Digit4 => 3,
        KeyCode::Digit5 => 4,
        KeyCode::Digit6 => 5,
        KeyCode::Digit7 => 6,
        KeyCode::Digit8 => 7,
        KeyCode::Digit9 => 8,
        KeyCode::Digit0 => 9,
        _ => return None,
    })
}

const GROUP_KEYS: [KeyCode; 10] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
];

/// Pure: the next entry after `current` in a wrapping list of ids (the first
/// entry when `current` is absent).
pub fn next_in_cycle(ids: &[u32], current: Option<u32>) -> Option<u32> {
    if ids.is_empty() {
        return None;
    }
    let at = current.and_then(|c| ids.iter().position(|id| *id == c));
    Some(ids[at.map_or(0, |i| (i + 1) % ids.len())])
}

/// Pure: the next non-empty group after `current` (wrapping).
pub fn next_group(groups: &[Vec<u32>], current: Option<usize>) -> Option<usize> {
    let n = groups.len();
    let start = current.map_or(0, |c| c + 1);
    (0..n)
        .map(|k| (start + k) % n)
        .find(|i| !groups[*i].is_empty())
}

/// Vehicle cost by domain (the refund base of a decommission).
fn vehicle_cost(domain: civ::VehicleDomain) -> f32 {
    match domain {
        civ::VehicleDomain::Land => civ::LAND_VEHICLE_COST,
        civ::VehicleDomain::Air => civ::AIR_VEHICLE_COST,
        civ::VehicleDomain::Sea => civ::SEA_VEHICLE_COST,
    }
}

/// Pure: spice returned for decommissioned vehicles.
pub fn decommission_refund(domains: impl Iterator<Item = civ::VehicleDomain>) -> f32 {
    domains.map(vehicle_cost).sum::<f32>() * DECOMMISSION_REFUND
}

/// Zoom-out display scale target (1.0 while the camera is near).
pub fn zoom_out_target(distance: f32, scale: f32) -> f32 {
    if distance > tuning::PLUMP_DISTANCE {
        scale
    } else {
        1.0
    }
}

/// The live display scale approaches its target by `clamp(dt_ms * 0.008, 0, 1)`
/// per frame (0xE956D0).
pub fn zoom_out_step(current: f32, target: f32, dt: f32) -> f32 {
    let k = (dt * 1000.0 * 0.008).clamp(0.0, 1.0);
    if k >= 1.0 {
        target
    } else {
        current + (target - current) * k
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

fn ctrl_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
}

fn shift_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)
}

/// The keys the in-stage assembly editor of `civ.rs` may read: its `,` `.` `-`
/// `=` Tab and Backspace keys clashed with the camera (`,` `.` `-` `=`), the
/// stance (Tab) and the camera-to-city key (Backspace), so they only count
/// while Ctrl is held. Its other keys (`N V [ ] R I J K L U O`) are unchanged.
pub fn assembly_keys(keys: &ButtonInput<KeyCode>) -> ButtonInput<KeyCode> {
    let mut out = keys.clone();
    if !ctrl_held(keys) {
        for key in [
            KeyCode::Comma,
            KeyCode::Period,
            KeyCode::Minus,
            KeyCode::Equal,
            KeyCode::Tab,
            KeyCode::Backspace,
        ] {
            out.reset(key);
        }
    }
    out
}

/// Pure: the camera keys held (Ctrl chords belong to the assembly editor and
/// the groups, so they never move the camera).
pub fn camera_keys(keys: &ButtonInput<KeyCode>) -> CameraInput {
    let mut input = CameraInput::default();
    if ctrl_held(keys) {
        return input;
    }
    let shift = shift_held(keys);
    let any = |list: &[KeyCode]| list.iter().any(|k| keys.pressed(*k));
    let arrow = |key: KeyCode| !shift && keys.pressed(key);
    let shifted = |key: KeyCode| shift && keys.pressed(key);
    input.forward = any(&[KeyCode::KeyW, KeyCode::Numpad8]) || arrow(KeyCode::ArrowUp);
    input.backward = any(&[KeyCode::KeyS, KeyCode::Numpad2]) || arrow(KeyCode::ArrowDown);
    input.left = any(&[KeyCode::KeyA, KeyCode::Numpad4]) || arrow(KeyCode::ArrowLeft);
    input.right = any(&[KeyCode::KeyD, KeyCode::Numpad6]) || arrow(KeyCode::ArrowRight);
    input.rotate_left = keys.pressed(KeyCode::Period) || shifted(KeyCode::ArrowLeft);
    input.rotate_right = keys.pressed(KeyCode::Comma) || shifted(KeyCode::ArrowRight);
    input.pitch_up = any(&[KeyCode::PageUp, KeyCode::Numpad9]) || shifted(KeyCode::ArrowUp);
    input.pitch_down = any(&[KeyCode::PageDown, KeyCode::Numpad3]) || shifted(KeyCode::ArrowDown);
    input
}

/// Zoom keys held: `(in, out, a zoom key went down this frame)`.
fn zoom_keys(keys: &ButtonInput<KeyCode>) -> (bool, bool, bool) {
    if ctrl_held(keys) {
        return (false, false, false);
    }
    let zoom_in = [KeyCode::Equal, KeyCode::NumpadAdd];
    let zoom_out = [KeyCode::Minus, KeyCode::NumpadSubtract];
    let in_pressed = zoom_in.iter().any(|k| keys.pressed(*k));
    let out_pressed = zoom_out.iter().any(|k| keys.pressed(*k));
    let just = zoom_in
        .iter()
        .chain(zoom_out.iter())
        .any(|k| keys.just_pressed(*k));
    (in_pressed, out_pressed, just)
}

/// Stage entry and exit: the camera starts in the retail opening view over
/// the focus again; the controls forget groups, follow and clicks.
fn mark_reset(mut camera: ResMut<CivCamera>, mut controls: ResMut<CivControls>) {
    camera.needs_reset = true;
    let edge = controls.edge_scroll;
    *controls = CivControls::default();
    controls.edge_scroll = edge;
}

/// The clip planes the game camera had before the stage (put back on exit).
#[derive(Resource, Default)]
struct SavedClip(Option<(f32, f32)>);

/// The retail planet camera's clip planes (1..10000) while the stage runs:
/// the whole hemisphere can be in view (the island builds kept Bevy's 1000).
fn civ_clip_planes(
    mut saved: ResMut<SavedClip>,
    mut cameras: Query<&mut Projection, With<crate::GameCamera>>,
) {
    for mut projection in &mut cameras {
        if let Projection::Perspective(p) = &mut *projection {
            if p.near != tuning::CLIP_NEAR || p.far != tuning::CLIP_FAR {
                saved.0.get_or_insert((p.near, p.far));
                p.near = tuning::CLIP_NEAR;
                p.far = tuning::CLIP_FAR;
            }
        }
    }
}

fn restore_clip_planes(
    mut saved: ResMut<SavedClip>,
    mut cameras: Query<&mut Projection, With<crate::GameCamera>>,
) {
    let Some((near, far)) = saved.0.take() else {
        return;
    };
    for mut projection in &mut cameras {
        if let Projection::Perspective(p) = &mut *projection {
            p.near = near;
            p.far = far;
        }
    }
}

/// Look the installed script up once the resource index is loaded.
fn load_script(mut camera: ResMut<CivCamera>, originals: Option<Res<Originals>>) {
    if camera.script_checked() {
        return;
    }
    let Some(originals) = originals else {
        return;
    };
    if originals.resource_index.records.is_empty() {
        return;
    }
    let script = CameraScript::from_install(&originals);
    camera.set_script(script);
}

/// Keyboard commands of the manual's Civilization controls (Controls set,
/// before the pause menu reads Esc): Esc deselects, Space cycles vehicles or
/// groups, Backspace goes to the closest city, Ctrl + digit defines a group,
/// Tab switches stance, Delete decommissions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn commands_input(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut controls: ResMut<CivControls>,
    mut run: ResMut<RunProgress>,
    mut focus: Query<&mut Transform, (With<CivCommander>, Without<CivUnit>)>,
    units: Query<
        (Entity, &CivUnit, &Transform, Has<Selected>, Option<&CivStance>),
        Without<CivCommander>,
    >,
    mut cues: MessageWriter<PlayCue>,
) {
    let ctrl = ctrl_held(&keys);
    let mut home: Vec<(Entity, &CivUnit, Vec3, bool, CivStance)> = units
        .iter()
        .filter(|(_, u, ..)| u.nation == HOME_INDEX)
        .map(|(e, u, t, s, st)| (e, u, t.translation, s, st.copied().unwrap_or_default()))
        .collect();
    home.sort_by_key(|(_, u, ..)| u.id);
    let selected: Vec<usize> = (0..home.len()).filter(|i| home[*i].3).collect();
    let mut error = |run: &mut RunProgress, message: &str| {
        run.civ_campaign.message = message.into();
        cues.write(PlayCue(Cue::CivError));
    };

    // Esc: deselect everything first; only with nothing selected does it open
    // the pause menu.
    if keys.just_pressed(KeyCode::Escape)
        && (!selected.is_empty() || controls.selected_city.is_some())
    {
        for i in &selected {
            commands.entity(home[*i].0).remove::<Selected>();
        }
        controls.selected_city = None;
        controls.follow = None;
        keys.clear_just_pressed(KeyCode::Escape);
        return;
    }

    // Ctrl + digit: define a group of the selection.
    if ctrl {
        for (slot, key) in GROUP_KEYS.iter().enumerate() {
            if keys.just_pressed(*key) {
                if selected.is_empty() {
                    error(
                        &mut run,
                        "Select vehicles first, then press Ctrl and a number to make a group",
                    );
                } else {
                    controls.groups[slot] = selected.iter().map(|i| home[*i].1.id).collect();
                    controls.group_cursor = Some(slot);
                    run.civ_campaign.message = format!(
                        "Group {} defined: {} vehicle{}",
                        (slot + 1) % 10,
                        selected.len(),
                        if selected.len() == 1 { "" } else { "s" }
                    );
                }
            }
        }
        return;
    }

    // Space: the next defined group, else the next vehicle (the camera follows).
    if keys.just_pressed(KeyCode::Space) {
        // The legacy "centre on the selection" handler of `rts_input` must not
        // also run.
        keys.clear_just_pressed(KeyCode::Space);
        let live: std::collections::HashSet<u32> = home.iter().map(|h| h.1.id).collect();
        for group in controls.groups.iter_mut() {
            group.retain(|id| live.contains(id));
        }
        let pick = |commands: &mut Commands, ids: &[u32]| {
            for (e, u, _, was, _) in &home {
                let want = ids.contains(&u.id);
                if want && !was {
                    commands.entity(*e).insert(Selected);
                } else if !want && *was {
                    commands.entity(*e).remove::<Selected>();
                }
            }
        };
        if let Some(slot) = next_group(&controls.groups, controls.group_cursor) {
            let ids = controls.groups[slot].clone();
            pick(&mut commands, &ids);
            let at: Vec3 = home
                .iter()
                .filter(|h| ids.contains(&h.1.id))
                .map(|h| h.2)
                .sum::<Vec3>()
                / ids.len().max(1) as f32;
            if let Ok(mut f) = focus.single_mut() {
                f.translation.x = at.x;
                f.translation.z = at.z;
            }
            controls.group_cursor = Some(slot);
            controls.follow = None;
            run.civ_campaign.message = format!("Group {}: {} vehicles", (slot + 1) % 10, ids.len());
        } else {
            let ids: Vec<u32> = home.iter().map(|h| h.1.id).collect();
            let current = selected.first().map(|i| home[*i].1.id).or(controls.follow);
            match next_in_cycle(&ids, current) {
                None => error(&mut run, "You have no vehicles"),
                Some(id) => {
                    pick(&mut commands, &[id]);
                    controls.follow = Some(id);
                    controls.group_cursor = None;
                    if let Some(h) = home.iter().find(|h| h.1.id == id) {
                        if let Ok(mut f) = focus.single_mut() {
                            f.translation.x = h.2.x;
                            f.translation.z = h.2.z;
                        }
                    }
                    let n = ids.iter().position(|v| *v == id).unwrap_or(0) + 1;
                    run.civ_campaign.message = format!("Vehicle {n} of {}", ids.len());
                }
            }
        }
    }

    // Backspace: the camera over the closest city of yours.
    if keys.just_pressed(KeyCode::Backspace) {
        run.civ_campaign.ensure_cities();
        let from = focus.single().map(|t| t.translation).unwrap_or(Vec3::ZERO);
        let city = run
            .civ_campaign
            .cities
            .iter()
            .filter(|c| c.owned())
            .min_by(|a, b| {
                run.civ_campaign
                    .distance(from, a.center())
                    .total_cmp(&run.civ_campaign.distance(from, b.center()))
            })
            .map(|c| (c.index, c.center()));
        match city {
            None => error(&mut run, "You have no city"),
            Some((index, at)) => {
                let shape = run.civ_campaign.shape();
                if let Ok(mut f) = focus.single_mut() {
                    move_focus(&mut f, at, shape);
                }
                controls.follow = None;
                run.civ_campaign.message = format!("Camera over {}", civ::city_label(index));
            }
        }
    }

    // Tab: stand ground <-> aggressive for the selected vehicles.
    if keys.just_pressed(KeyCode::Tab) && !selected.is_empty() {
        let next = home[selected[0]].4.toggled();
        for i in &selected {
            commands.entity(home[*i].0).insert(next);
        }
        run.civ_campaign.message = format!("{} stance", next.title());
    }

    // Delete: decommission the selected vehicles for half their price.
    if keys.just_pressed(KeyCode::Delete) {
        if selected.is_empty() {
            error(&mut run, "Select a vehicle to decommission it");
        } else {
            let refund = decommission_refund(selected.iter().map(|i| home[*i].1.domain));
            let gone: Vec<u32> = selected.iter().map(|i| home[*i].1.id).collect();
            for i in &selected {
                commands.entity(home[*i].0).despawn();
            }
            run.civ_campaign.units.retain(|u| !gone.contains(&u.id));
            run.spice += refund;
            controls.follow = controls.follow.filter(|id| !gone.contains(id));
            run.civ_campaign.message = format!(
                "{} vehicle{} decommissioned: {:.0} spice back",
                gone.len(),
                if gone.len() == 1 { "" } else { "s" },
                refund
            );
            run.request_save();
        }
    }
}

/// Camera input: keys, HUD buttons, wheel and mouse (Controls set). Pans move
/// the `CivCommander` focus, which is the camera's goal anchor; everything
/// else is the eased state of [`CivCamera`].
#[allow(clippy::too_many_arguments)]
fn camera_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Option<ResMut<ButtonInput<MouseButton>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    pixels_per_line: Option<Res<MouseScrollPixelsPerLine>>,
    windows: Query<&Window>,
    spui: Option<Res<crate::spui::view::SpuiPointer>>,
    mut camera: ResMut<CivCamera>,
    mut controls: ResMut<CivControls>,
    mut focus: Query<&mut Transform, (With<CivCommander>, Without<CivUnit>)>,
    units: Query<(&CivUnit, &Transform), Without<CivCommander>>,
    run: Res<RunProgress>,
) {
    let dt = time.delta_secs();
    let Ok(mut focus) = focus.single_mut() else {
        return;
    };
    let shape = run.civ_campaign.shape();
    camera.sphere = shape;
    if camera.needs_reset {
        camera.reset(focus.translation);
    }
    let mut input = camera_keys(&keys);
    let hud = controls.hud;
    input.rotate_left |= hud.rotate_left;
    input.rotate_right |= hud.rotate_right;
    let (zin, zout, mut just) = zoom_keys(&keys);
    let (zin, zout) = (zin || hud.zoom_in, zout || hud.zoom_out);
    let hud_down = hud.zoom_in || hud.zoom_out;
    just |= hud_down && !controls.hud_zoom_was_down;
    controls.hud_zoom_was_down = hud_down;
    input.zoom_events = camera.zoom_key_events(zin, zout, just, dt);

    let window = windows.iter().find(|w| w.cursor_position().is_some());
    let window_size = windows
        .iter()
        .next()
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    let cursor = window.and_then(|w| w.cursor_position());
    let over_ui = spui.as_deref().is_some_and(|s| s.over_ui);

    if let Some(scroll) = scroll {
        let default_ratio = MouseScrollPixelsPerLine::default();
        let ratio = pixels_per_line.as_deref().unwrap_or(&default_ratio);
        let lines = scroll.to_lines(ratio).delta.y;
        if !over_ui {
            input.zoom_events += camera.take_wheel_notches(lines);
        }
    }

    // Mouse: orbit (middle, or left + right) and drag scroll (right). With the
    // Civ control option (`OptionCivControls`, `UIMachineCivRotate`) the right
    // button's drag orbits instead of scrolling, once the cursor travelled
    // `LONG_MOUSE_MOVE` pixels from the press.
    let rotate_on_right = crate::control_options::civ_rotate_on_right();
    let mut orbit_delta = Vec2::ZERO;
    let was_orbiting = controls.orbiting;
    controls.orbiting = false;
    controls.panning = false;
    let motion_delta = motion
        .map(|m| m.delta)
        .filter(|delta| delta.is_finite() && delta.abs().max_element() < 4096.0)
        .unwrap_or(Vec2::ZERO);
    if let Some(mut mouse) = mouse {
        let middle = mouse.pressed(MouseButton::Middle);
        let left = mouse.pressed(MouseButton::Left);
        let right = mouse.pressed(MouseButton::Right);
        // While the chord holds, the left button is reset so that no selection
        // box is drawn; `left_down` remembers that it is still physically down.
        let chord = right && (left || controls.left_down);
        if (middle || chord) && !over_ui {
            orbit_delta = motion_delta;
            controls.orbiting = true;
        }
        if chord {
            mouse.reset(MouseButton::Left);
            controls.left_down = true;
        } else {
            controls.left_down = left;
        }
        controls.right_down = right;
        if mouse.just_pressed(MouseButton::Right) && !over_ui {
            controls.right_press = cursor;
            controls.right_drag = false;
        }
        if right {
            if let (Some(press), Some(at)) = (controls.right_press, cursor) {
                if !chord && press.distance(at) > CLICK_DRAG_DISTANCE {
                    controls.right_drag = true;
                }
                if controls.right_drag && !chord {
                    if !rotate_on_right {
                        input.drag_scroll = drag_scroll_vector(press, at, window_size);
                        controls.panning = true;
                    } else if controls.right_rotating || press.distance(at) > crate::control_options::LONG_MOUSE_MOVE {
                        controls.right_rotating = true;
                        orbit_delta = motion_delta;
                        controls.orbiting = true;
                    }
                }
            }
        } else {
            controls.right_press = None;
            controls.right_drag = false;
            controls.right_rotating = false;
        }
    }
    input.orbit = orbit_delta;
    // The orbit button came up: yaw and pitch coast on (0xB10BA0).
    if was_orbiting && !controls.orbiting {
        camera.end_orbit();
    }
    // Edge scroll: the cursor in the outermost two pixels of the window
    // (`edge_distance`); the script ramps the speed up while it stays there.
    if controls.edge_scroll && !controls.panning && !controls.orbiting && !over_ui {
        if let (Some(at), Some(w)) = (cursor, window) {
            if w.focused {
                input.edge = edge_distance(at, window_size);
            }
        }
    }

    // A minimap click moves the camera; Space-follow ends when the player pans.
    if let Some(at) = controls.goto_request.take() {
        move_focus(&mut focus, at, shape);
        controls.follow = None;
    }
    let panned = input.forward
        || input.backward
        || input.left
        || input.right
        || input.drag_scroll != Vec2::ZERO
        || edge_in_zone(input.edge);
    if panned {
        controls.follow = None;
    }
    if let Some(id) = controls.follow {
        match units.iter().find(|(u, _)| u.id == id) {
            Some((_, t)) => move_focus(&mut focus, t.translation, shape),
            None => controls.follow = None,
        }
    }

    // The pan puts the focus (the retail goal anchor) `scroll` ahead of the
    // CURRENT eased anchor, over the surface (no island edge any more: the
    // whole planet can be looked at).
    if controls
        .last_goal
        .is_some_and(|last| shape.distance(last, focus.translation) > 1.0e-3)
    {
        camera.cancel_coast();
    }
    let pan = camera.update(dt, &input);
    if pan != Vec3::ZERO {
        focus.translation = shape.advance(camera.anchor, pan);
    }
    controls.last_goal = Some(focus.translation);
}

/// Put the camera focus over a world point (`terrain::ground` keeps it on
/// the surface; on the plane only x and z move).
pub fn move_focus(focus: &mut Transform, at: Vec3, shape: Shape) {
    if shape.on_sphere() {
        focus.translation = at;
    } else {
        focus.translation.x = at.x;
        focus.translation.z = at.z;
    }
}

/// What the cursor is over, and a click on a city selects it (Simulation set,
/// before `rts_input`, which keeps the vehicle picking and the orders).
#[allow(clippy::too_many_arguments)]
fn pointer_state(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    spui: Option<Res<crate::spui::view::SpuiPointer>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    run: Res<RunProgress>,
    units: Query<(&CivUnit, &Transform), Without<CivCommander>>,
    mut controls: ResMut<CivControls>,
) {
    let cursor = windows.iter().find_map(|w| w.cursor_position());
    let over_ui = spui.as_deref().is_some_and(|s| s.over_ui);
    let (Some(cursor), Ok((camera, transform))) = (cursor, cameras.single()) else {
        controls.hover = CivHover::None;
        return;
    };
    if over_ui {
        controls.hover = CivHover::None;
        return;
    }
    let shape = run.civ_campaign.shape();
    let eye = transform.translation();
    let project = |p: Vec3| {
        shape
            .visible_from(eye, p)
            .then(|| camera.world_to_viewport(transform, p).ok())
            .flatten()
    };
    let vehicle = units
        .iter()
        .filter_map(|(u, t)| {
            let d = project(t.translation + Vec3::Y * 0.6)?.distance(cursor);
            (d <= PICK_PIXELS).then_some((u.id, u.nation, d))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2));
    let ground = camera
        .viewport_to_world(transform, cursor)
        .ok()
        .and_then(|ray| civ_rts::ground_hit(ray.origin, *ray.direction, surface.as_deref()));
    controls.hover = if let Some((id, nation, _)) = vehicle {
        CivHover::Vehicle { id, nation }
    } else if let Some(g) = ground {
        let campaign = &run.civ_campaign;
        let geyser = (0..civ::GEYSER_COUNT)
            .find(|i| shape.distance(g, campaign.geyser_at(*i)) <= GEYSER_CLICK_RADIUS);
        let city = campaign
            .cities
            .iter()
            .find(|c| shape.distance(g, c.center()) <= CITY_CLICK_RADIUS)
            .map(|c| c.index);
        match (geyser, city) {
            (Some(i), _) => CivHover::Geyser(i),
            (None, Some(c)) => CivHover::City(c),
            _ => CivHover::Ground(g),
        }
    } else {
        CivHover::None
    };
    // A left click (not a drag) on a city selects it; elsewhere it clears the
    // city selection.
    if let Some(mouse) = mouse {
        if mouse.just_pressed(MouseButton::Left) {
            controls.left_press = Some(cursor);
        }
        if mouse.just_released(MouseButton::Left) {
            if let Some(press) = controls.left_press.take() {
                if press.distance(cursor) <= CLICK_DRAG_DISTANCE {
                    controls.selected_city = match controls.hover {
                        CivHover::City(c) => Some(c),
                        CivHover::Vehicle { .. } => controls.selected_city,
                        _ => None,
                    };
                }
            }
        }
    }
}

/// Marks a vehicle chasing a target because of its aggressive stance.
#[derive(Component, Clone, Copy, Debug)]
pub struct AggressiveChase(pub u32);

/// Aggressive vehicles go after hostile vehicles that come within
/// [`AGGRESSIVE_CHASE_RANGE`] (Simulation set, after `rts_input`, before
/// `rts_move`): they take an ordinary attack order, which `rts_move` clears
/// when the target is gone; a chase that left the range is dropped.
fn aggressive_stance(
    run: Res<RunProgress>,
    mut commands: Commands,
    mut units: Query<(
        Entity,
        &mut CivUnit,
        &Transform,
        Option<&CivStance>,
        Option<&AggressiveChase>,
    )>,
) {
    let others: Vec<(u32, usize, Vec3)> = units
        .iter()
        .map(|(_, u, t, ..)| (u.id, u.nation, t.translation))
        .collect();
    let shape = run.civ_campaign.shape();
    let dist_xz = |a: Vec3, b: Vec3| shape.distance(a, b);
    for (entity, mut unit, t, stance, chase) in &mut units {
        if unit.nation != HOME_INDEX {
            continue;
        }
        let aggressive =
            stance.copied() == Some(CivStance::Aggressive) && unit.kind == civ::Approach::Military;
        if let Some(AggressiveChase(target)) = chase {
            let gone = !aggressive
                || unit.order != civ::UnitOrder::Unit(*target)
                || others
                    .iter()
                    .find(|o| o.0 == *target)
                    .is_none_or(|o| dist_xz(t.translation, o.2) > AGGRESSIVE_CHASE_RANGE * 1.6);
            if gone {
                if aggressive && unit.order == civ::UnitOrder::Unit(*target) {
                    unit.order = civ::UnitOrder::Idle;
                }
                commands.entity(entity).remove::<AggressiveChase>();
            }
            continue;
        }
        if !aggressive || unit.order != civ::UnitOrder::Idle {
            continue;
        }
        let target = others
            .iter()
            .filter(|o| civ_rts::hostile(&run.civ_campaign, HOME_INDEX, o.1))
            .filter(|o| dist_xz(t.translation, o.2) <= AGGRESSIVE_CHASE_RANGE)
            .min_by(|a, b| dist_xz(t.translation, a.2).total_cmp(&dist_xz(t.translation, b.2)));
        if let Some((id, ..)) = target {
            unit.order = civ::UnitOrder::Unit(*id);
            commands.entity(entity).insert(AggressiveChase(*id));
        }
    }
}

/// The minimap's right click (`CivControls::move_request`) sends the selected
/// vehicles there (Simulation set, after `rts_input`).
fn minimap_orders(
    run: Res<RunProgress>,
    mut controls: ResMut<CivControls>,
    mut units: Query<&mut CivUnit, With<Selected>>,
) {
    let Some(at) = controls.move_request.take() else {
        return;
    };
    let shape = run.civ_campaign.shape();
    let mut k = 0usize;
    for mut unit in &mut units {
        if unit.nation != HOME_INDEX {
            continue;
        }
        // The spread of `civ_rts::formation`, over the surface.
        let goal = if k == 0 {
            at
        } else {
            let angle = k as f32 * 2.39996;
            shape.toward(at, shape.bearing(at, angle), 1.7 * CITY_SCALE * (k as f32).sqrt())
        };
        unit.order = civ::UnitOrder::Goto(goal.to_array());
        unit.idle = 0.0;
        k += 1;
    }
}

/// Ease the anchor toward the focus entity (every frame the stage runs).
fn camera_follow_focus(
    time: Res<Time>,
    run: Res<RunProgress>,
    mut camera: ResMut<CivCamera>,
    focus: Query<&Transform, With<CivCommander>>,
) {
    let Ok(focus) = focus.single() else {
        return;
    };
    camera.sphere = run.civ_campaign.shape();
    if camera.needs_reset {
        camera.reset(focus.translation);
        return;
    }
    camera.follow_focus(focus.translation, time.delta_secs().clamp(0.0, 0.1));
}

/// Pose the game camera from the eased state (after the shared
/// `camera_follow`, which keeps the other stages).
#[allow(clippy::too_many_arguments)]
fn camera_apply(
    camera: Res<CivCamera>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    editor: Option<Res<crate::editor::EditorState>>,
    creator: Option<Res<crate::mechanical_creator::MechanicalCreator>>,
    capture: Option<Res<crate::runtime::RenderSmokeCamera>>,
    mut game_camera: Query<&mut Transform, (With<crate::GameCamera>, Without<Player>)>,
) {
    if capture.is_some()
        || editor.is_some_and(|e| e.open)
        || creator.is_some_and(|c| c.open)
        || camera.needs_reset
    {
        return;
    }
    let Ok(mut transform) = game_camera.single_mut() else {
        return;
    };
    let globe = crate::civ_planet::Globe::from_surface(
        surface
            .as_deref()
            .filter(|s| s.stage == GameStage::Civilization),
    );
    *transform = camera.pose(|p| {
        if globe.on_planet() {
            globe.surface(p)
        } else {
            Vec3::new(
                p.x,
                surface.as_ref().map_or(TERRAIN_BASE_Y, |s| s.height(p.x, p.z)),
                p.z,
            )
        }
    });
}

/// Pure: `base` drawn `factor` times larger around `pivot` (local space).
pub fn scaled_about(base: &Transform, pivot: Vec3, factor: f32) -> Transform {
    Transform {
        translation: pivot + (base.translation - pivot) * factor,
        rotation: base.rotation,
        scale: base.scale * factor,
    }
}

/// What a presentation entity looked like before the zoom-out factor and what
/// was written last, so that other systems (the vehicle bob, the hall grounding)
/// keep writing their own components: whatever they change is the new base.
#[derive(Component, Clone, Copy, Debug)]
pub struct ZoomOutState {
    pub base: Transform,
    pub applied: Transform,
    pub factor: f32,
}

impl ZoomOutState {
    pub fn new(current: &Transform) -> Self {
        Self {
            base: *current,
            applied: *current,
            factor: 1.0,
        }
    }

    /// Adopt the components somebody else wrote since the last frame.
    pub fn sync(&mut self, current: &Transform) {
        if current.translation != self.applied.translation {
            self.base.translation = current.translation;
        }
        if current.rotation != self.applied.rotation {
            self.base.rotation = current.rotation;
        }
        if current.scale != self.applied.scale {
            self.base.scale = current.scale;
        }
    }

    /// Ease the factor toward `target` and write the scaled transform; the
    /// pivot is local to the parent, `None` scales in place.
    pub fn step(&mut self, current: &mut Transform, target: f32, dt: f32, pivot: Option<Vec3>) {
        self.sync(current);
        self.factor = zoom_out_step(self.factor, target, dt);
        let pivot = pivot.unwrap_or(self.base.translation);
        let next = scaled_about(&self.base, pivot, self.factor);
        if *current != next {
            *current = next;
        }
        self.applied = next;
    }
}

/// Vehicles are drawn larger while the camera is zoomed far out
/// (`VehicleZoomOutScale` above `PlumpDistance`).
fn vehicle_zoom_out(
    time: Res<Time>,
    camera: Res<CivCamera>,
    mut commands: Commands,
    units: Query<&civ_rts::UnitRig, With<CivUnit>>,
    mut arts: Query<(&mut Transform, Option<&mut ZoomOutState>), Without<CivUnit>>,
) {
    let target = zoom_out_target(camera.distance, tuning::VEHICLE_ZOOM_OUT_SCALE);
    let dt = time.delta_secs();
    for rig in &units {
        let Ok((mut art, state)) = arts.get_mut(rig.art) else {
            continue;
        };
        match state {
            None => {
                commands.entity(rig.art).insert(ZoomOutState::new(&art));
            }
            Some(mut state) => {
                // Scaled in place: the pivot is the model's own origin.
                state.step(&mut art, target, dt, None);
            }
        }
    }
}

/// The city halls are drawn larger while the camera is zoomed far out
/// (`CityHallZoomOutScale` above `PlumpDistance`), about the point of the floor
/// under the city's centre.
#[allow(clippy::too_many_arguments)]
fn hall_zoom_out(
    time: Res<Time>,
    camera: Res<CivCamera>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut commands: Commands,
    cities: Query<&Transform, (With<civ::CityMarker>, Without<civ::NativeCityHallSurface>)>,
    mut halls: Query<
        (
            Entity,
            &ChildOf,
            &mut Transform,
            Option<&mut ZoomOutState>,
        ),
        With<civ::NativeCityHallSurface>,
    >,
) {
    let target = zoom_out_target(camera.distance, tuning::CITY_HALL_ZOOM_OUT_SCALE);
    let dt = time.delta_secs();
    for (entity, parent, mut transform, state) in &mut halls {
        let Some(mut state) = state else {
            commands.entity(entity).insert(ZoomOutState::new(&transform));
            continue;
        };
        let Ok(city) = cities.get(parent.parent()) else {
            continue;
        };
        // The floor under the city's centre in its own (scaled, tilted) frame.
        let globe = crate::civ_planet::Globe::from_surface(
            surface
                .as_deref()
                .filter(|s| s.stage == GameStage::Civilization),
        );
        let pivot = Vec3::new(0.0, globe.local_ground(city, Vec2::ZERO), 0.0);
        state.step(&mut transform, target, dt, Some(pivot));
    }
}

pub struct CivControlsPlugin;

impl Plugin for CivControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CivCamera>()
            .init_resource::<CivControls>()
            .init_resource::<SavedClip>()
            .add_systems(OnEnter(GameStage::Civilization), mark_reset)
            .add_systems(OnExit(GameStage::Civilization), (mark_reset, restore_clip_planes))
            .add_systems(
                Update,
                (load_script, commands_input, camera_input)
                    .chain()
                    .after(crate::player_hud::actions)
                    .before(crate::pause_menu::input)
                    .in_set(crate::GameSystems::Controls)
                    .run_if(in_state(GameStage::Civilization))
                    .run_if(crate::editor::simulation_active)
                    .run_if(crate::mechanical_creator::simulation_active)
                    .run_if(crate::pause_menu::simulation_active),
            )
            .add_systems(
                Update,
                pointer_state
                    .before(civ_rts::rts_input)
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Civilization)),
            )
            .add_systems(
                Update,
                minimap_orders
                    .after(civ_rts::rts_input)
                    .before(civ_rts::rts_move)
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Civilization)),
            )
            .add_systems(
                Update,
                aggressive_stance
                    .after(civ_rts::rts_input)
                    .before(civ_rts::rts_move)
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Civilization)),
            )
            .add_systems(
                Update,
                (camera_follow_focus, camera_apply, civ_clip_planes, vehicle_zoom_out, hall_zoom_out)
                    .chain()
                    .in_set(crate::GameSystems::Presentation)
                    .after(crate::camera_follow)
                    .before(crate::foliage_visibility::update)
                    .run_if(in_state(GameStage::Civilization)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tolerance: f32) {
        assert!((a - b).abs() <= tolerance, "{a} != {b}");
    }

    fn keys(pressed: &[KeyCode]) -> ButtonInput<KeyCode> {
        let mut input = ButtonInput::default();
        for key in pressed {
            input.press(*key);
        }
        input
    }

    fn hold(camera: &mut CivCamera, input: &CameraInput, seconds: f32) {
        for _ in 0..(seconds * 60.0).round() as usize {
            camera.update(1.0 / 60.0, input);
        }
    }

    /// What the real systems do every frame: the update puts the goal (the
    /// focus) `scroll` ahead of the current anchor, the follow step moves the
    /// anchor. Returns how far the anchor travelled.
    fn drive(camera: &mut CivCamera, focus: &mut Vec3, input: &CameraInput, seconds: f32) -> Vec3 {
        let start = camera.anchor;
        for _ in 0..(seconds * 60.0).round() as usize {
            let pan = camera.update(1.0 / 60.0, input);
            if pan != Vec3::ZERO {
                *focus = camera.sphere.advance(camera.anchor, pan);
            }
            camera.follow_focus(*focus, 1.0 / 60.0);
        }
        camera.anchor - start
    }

    #[test]
    fn the_stage_opens_at_the_retail_polar_coordinates() {
        let mut camera = CivCamera::default();
        camera.reset(Vec3::ZERO);
        assert_eq!(camera.azimuth, 0.0);
        assert_eq!(camera.pitch, 0.7636);
        assert_eq!(camera.distance, 221.791);
        close(camera.pitch.to_degrees(), 43.75, 0.05);
        let pose = camera.pose(|p| Vec3::new(p.x, -10.0, p.z));
        let want = orbit_offset(0.0, 0.7636, 221.791 * CAMERA_WORLD_SCALE);
        assert!(pose.translation.distance(want) < 1.0e-4, "{:?}", pose.translation);
        // Behind a -Z facing view, looking at the anchor.
        assert!(pose.translation.z > 0.0 && pose.translation.x.abs() < 1e-5);
        let forward = pose.rotation * Vec3::NEG_Z;
        assert!(forward.z < 0.0 && forward.y < 0.0);
        // The world distance is the retail one (the stage is on the planet).
        close(camera.world_distance(), 221.791, 1e-3);
        // On the planet the orbit turns about the anchor's local up.
        let mut planet = CivCamera::default();
        planet.sphere = Shape::sphere(Vec3::new(0.0, -500.0, 0.0), 500.0);
        let anchor = planet.sphere.toward(Vec3::ZERO, Vec3::X, 200.0);
        planet.reset(anchor);
        let pose = planet.pose(|p| planet.sphere.centre.unwrap() + planet.sphere.up(p) * 500.0);
        let up = planet.sphere.up(anchor);
        let offset = pose.translation - anchor;
        close(offset.length(), 221.791, 1e-2);
        close(offset.normalize().dot(up), 0.7636f32.sin(), 1e-3);
        // Moving the focus carries the frame: the local up stays the orbit's up.
        let far = planet.sphere.toward(anchor, Vec3::X, 300.0);
        for _ in 0..600 {
            planet.follow_focus(far, 1.0 / 60.0);
        }
        assert!(planet.anchor.distance(far) < 0.5, "{:?}", planet.anchor);
        assert!((planet.frame * Vec3::Y - planet.sphere.up(planet.anchor)).length() < 1e-3);
    }

    #[test]
    fn zoom_ladder_snaps_like_the_script() {
        // Zooming in picks the step below, out the step above.
        assert_eq!(zoom_ladder(221.791, 1.0, false), 200.0);
        assert_eq!(zoom_ladder(221.791, -1.0, false), 250.0);
        assert_eq!(zoom_ladder(200.0, 1.0, false), 150.0);
        assert_eq!(zoom_ladder(200.0, -1.0, false), 250.0);
        assert_eq!(zoom_ladder(450.0, -1.0, false), 450.0);
        assert_eq!(zoom_ladder(500.0, 1.0, false), 450.0);
        // The closest step is the alt-zoom rule's.
        assert_eq!(zoom_ladder(50.0, 1.0, false), 50.0);
        assert_eq!(zoom_ladder(50.0, 1.0, true), 15.0);
        assert_eq!(zoom_ladder(15.0, -1.0, true), 50.0);
        assert_eq!(zoom_ladder(15.0, -1.0, false), 50.0);
        assert_eq!(zoom_ladder(10.0, -1.0, true), 15.0);
        assert_eq!(zoom_ladder(75.0, 0.0, false), 75.0);
        // The ladder is complete in both directions.
        let mut d = 450.0;
        let mut steps = Vec::new();
        loop {
            let next = zoom_ladder(d, 1.0, false);
            if next == d {
                break;
            }
            steps.push(next);
            d = next;
        }
        assert_eq!(steps, [350.0, 300.0, 250.0, 200.0, 150.0, 100.0, 75.0, 50.0]);
    }

    #[test]
    fn alt_zoom_needs_the_dwell_before_the_closest_step() {
        let mut camera = CivCamera::default();
        camera.reset(Vec3::ZERO);
        camera.distance = 60.0;
        camera.goal_distance = 60.0;
        // The first zoom in at or below 75: the step is taken, alt zoom waits.
        camera.zoom_step(1);
        assert_eq!(camera.goal_distance, 50.0);
        assert!(!camera.alt_zoom());
        // Zooming in again at once does nothing more (the dwell has not passed).
        camera.zoom_step(1);
        assert_eq!(camera.goal_distance, 50.0);
        assert!(!camera.alt_zoom());
        // After the dwell (0.2 s) the next one reaches 15.
        hold(&mut camera, &CameraInput::default(), 0.3);
        camera.distance = 50.0;
        camera.zoom_step(1);
        assert!(camera.alt_zoom());
        assert_eq!(camera.goal_distance, 15.0);
        // Zooming out past 75 leaves alt zoom.
        camera.distance = 80.0;
        camera.zoom_step(-1);
        assert!(!camera.alt_zoom());
        assert_eq!(camera.goal_distance, 50.0);
    }

    #[test]
    fn the_key_ramp_is_a_smoothstep_over_a_quarter_second() {
        assert_eq!(ramp(0.0, 0.25), 0.0);
        close(ramp(0.125, 0.25), 0.5, 1e-6);
        assert_eq!(ramp(0.25, 0.25), 1.0);
        assert_eq!(ramp(5.0, 0.25), 1.0);
        assert!(ramp(0.05, 0.25) < 0.11);
    }

    #[test]
    fn panning_keys_move_the_anchor_like_the_retail_hermite_chase() {
        let forward = CameraInput {
            forward: true,
            ..default()
        };
        let mut camera = CivCamera::default();
        camera.reset(Vec3::ZERO);
        let mut focus = Vec3::ZERO;
        // The first update after the press: the goal is `scroll` ahead of the
        // anchor (0.25 s ramp, so still small), the anchor has hardly moved.
        let first = camera.update(1.0 / 60.0, &forward);
        assert!(first.x.abs() < 1e-4 && first.z < 0.0 && first.y == 0.0, "{first:?}");
        camera.follow_focus(camera.sphere.advance(camera.anchor, first), 1.0 / 60.0);
        assert!(camera.anchor.length() < 0.01);
        camera.reset(Vec3::ZERO);
        // Held for 2 s: the steady speed of the chase is 1.5 x scroll / T with
        // scroll = distance / 2 per update and T the 0.95 shortened segment
        // time (the offset is 0.22 rad on the planet, so `lerp(0.5, 5, 0.07)`
        // = 0.81 s), i.e. about one distance a second, straight ahead (-Z at
        // azimuth 0).
        drive(&mut camera, &mut focus, &forward, 1.0);
        let travelled = drive(&mut camera, &mut focus, &forward, 1.0);
        let per_second = travelled.length() / (221.791 * CAMERA_WORLD_SCALE);
        assert!(travelled.x.abs() < 1e-3 && travelled.z < 0.0, "{travelled:?}");
        assert!(per_second > 0.85 && per_second < 1.2, "{per_second}");
        // Turned a quarter: right becomes -Z.
        let mut turned = CivCamera::default();
        turned.reset(Vec3::ZERO);
        turned.azimuth = std::f32::consts::FRAC_PI_2;
        turned.goal_azimuth = std::f32::consts::FRAC_PI_2;
        turned.yaw_seg = Segment::at_rest(std::f32::consts::FRAC_PI_2);
        let right = CameraInput {
            right: true,
            ..default()
        };
        let mut focus = Vec3::ZERO;
        let again = drive(&mut turned, &mut focus, &right, 1.0);
        assert!(again.z < -1.0 && again.x.abs() < 1e-3, "{again:?}");
        // Opposite keys cancel.
        let mut both = CivCamera::default();
        both.reset(Vec3::ZERO);
        let mut focus = Vec3::ZERO;
        let cancelled = drive(
            &mut both,
            &mut focus,
            &CameraInput {
                forward: true,
                backward: true,
                ..default()
            },
            0.5,
        );
        assert!(cancelled.length() < 1e-4);
        // Panning is slower the closer in the camera is (proportional to the
        // zoom distance).
        let mut close_up = CivCamera::default();
        close_up.reset(Vec3::ZERO);
        close_up.goal_distance = 50.0;
        close_up.distance = 50.0;
        close_up.distance_seg = Segment::at_rest(50.0);
        let mut focus = Vec3::ZERO;
        drive(&mut close_up, &mut focus, &forward, 1.0);
        let near = drive(&mut close_up, &mut focus, &forward, 1.0);
        // (the retail chase is not exactly proportional: a smaller offset is
        // a shorter angle, so a shorter segment time.)
        let ratio = near.length() / travelled.length();
        assert!(ratio > 0.2 && ratio < 0.45, "{near:?} vs {travelled:?}");
    }

    #[test]
    fn releasing_the_pan_keys_coasts_the_anchor_to_a_stop() {
        let forward = CameraInput {
            forward: true,
            ..default()
        };
        let mut camera = CivCamera::default();
        camera.reset(Vec3::ZERO);
        let mut focus = Vec3::ZERO;
        drive(&mut camera, &mut focus, &forward, 1.5);
        let speed = camera.anchor_speed();
        assert!(speed > 0.1, "{speed}");
        // The release update puts the goal `|velocity| x min_interp x 250`
        // ahead along the last direction (velocity in radians a second).
        let pan = camera.update(1.0 / 60.0, &CameraInput::default());
        let reach = speed * 0.5 * 250.0;
        close(pan.length(), reach, reach * 1e-3);
        assert!(pan.z < 0.0 && pan.x.abs() < 1e-3);
        focus = camera.sphere.advance(camera.anchor, pan);
        camera.follow_focus(focus, 1.0 / 60.0);
        let at_release = camera.anchor;
        // The coast ends at the goal after the minimum interpolation time.
        let after = drive(&mut camera, &mut focus, &CameraInput::default(), 1.0);
        let travelled = (camera.anchor - at_release).length();
        assert!(travelled > 0.3 * reach && travelled <= reach + 0.5, "{travelled} {reach}");
        assert!(after.z < 0.0);
        assert!((camera.anchor - focus).length() < 1e-2);
        assert!(camera.anchor_speed() < 1e-3);
        // No second coast.
        let again = camera.update(1.0 / 60.0, &CameraInput::default());
        assert_eq!(again, Vec3::ZERO);
    }

    #[test]
    fn segments_end_at_rest_on_the_goal_and_never_jump() {
        // A segment arrives exactly after its time and ends at rest.
        let mut seg = Segment::at_rest(0.0);
        let mut value = 0.0;
        let mut last = 0.0;
        let mut t = 0.0;
        while t < 1.0 {
            value = seg.step(value, 10.0, 1.0 / 60.0, 0.5);
            // (the shortened remaining time overshoots by a fraction of a
            // percent and the settle brings it back)
            assert!(value >= last - 0.05 && value <= 10.0 * 1.01, "{value}");
            last = value;
            t += 1.0 / 60.0;
        }
        close(value, 10.0, 1e-3);
        assert!(seg.vel.abs() < 1e-2);
        // Re-targeting keeps the velocity (no jump): the speed just after a
        // new goal is the speed just before it.
        let mut seg = Segment::at_rest(0.0);
        let mut value = 0.0;
        for _ in 0..12 {
            value = seg.step(value, 10.0, 1.0 / 60.0, 1.0);
        }
        let before = seg.vel;
        assert!(before > 1.0);
        let stepped = seg.step(value, 12.0, 1.0 / 60.0, 1.0);
        assert!(((stepped - value) * 60.0 - before).abs() < 0.35 * before, "{before}");
        // The Hermite basis: p0 at 0, p1 at the end, tangents scaled by T.
        close(hermite(2.0, 1.0, 6.0, 0.0, 1.0, 0.0), 2.0, 1e-6);
        close(hermite(2.0, 1.0, 6.0, 0.0, 1.0, 1.0), 6.0, 1e-5);
        close(hermite(0.0, 0.0, 1.0, 0.0, 1.0, 0.5), 0.5, 1e-6);
    }

    #[test]
    fn edge_scroll_follows_the_script_dwell_ramp_in_the_two_pixel_border() {
        let window = Vec2::new(1280.0, 720.0);
        // Pixel distance to the nearest border, signed, plus 0.01.
        close(edge_distance(Vec2::new(0.0, 360.0), window).x, 0.01, 1e-6);
        close(edge_distance(Vec2::new(1.0, 360.0), window).x, 1.01, 1e-6);
        close(edge_distance(Vec2::new(1279.0, 360.0), window).x, -0.99, 1e-5);
        close(edge_distance(Vec2::new(640.0, 0.0), window).y, 0.01, 1e-6);
        close(edge_distance(Vec2::new(640.0, 719.0), window).y, -0.99, 1e-5);
        assert_eq!(edge_distance(Vec2::new(-1.0, 360.0), window).x, 0.0);
        assert_eq!(edge_distance(Vec2::new(1280.0, 360.0), window).x, 0.0);
        assert!(edge_in_zone(edge_distance(Vec2::new(1.0, 360.0), window)));
        assert!(edge_in_zone(edge_distance(Vec2::new(1278.0, 360.0), window)));
        assert!(!edge_in_zone(edge_distance(Vec2::new(2.0, 360.0), window)));
        assert!(!edge_in_zone(edge_distance(Vec2::new(1277.0, 360.0), window)));
        assert!(!edge_in_zone(edge_distance(Vec2::new(640.0, 360.0), window)));
        // The script: delta = ((1 - |f|) + dwell) x distance / 450 (x) or 300 (y),
        // the dwell timer grows 3 a second up to 6 and resets away from the border.
        let mut scratch = [0.0; 5];
        let left = Vec2::new(0.01, 0.0);
        let first = edge_scroll_delta(left, 200.0, 0.1, &mut scratch);
        close(first.x, ((1.0 - 0.005) + 0.3) * 200.0 / 450.0, 1e-4);
        assert_eq!(first.y, 0.0);
        for _ in 0..40 {
            edge_scroll_delta(left, 200.0, 0.1, &mut scratch);
        }
        assert_eq!(scratch[0], 6.0, "the dwell timer is capped");
        let right = edge_scroll_delta(Vec2::new(-0.99, 0.0), 200.0, 0.1, &mut scratch);
        assert!(right.x < 0.0);
        let mut scratch = [0.0; 5];
        let top = edge_scroll_delta(Vec2::new(0.0, 0.01), 300.0, 0.1, &mut scratch);
        assert!(top.y > 0.0 && top.x == 0.0);
        close(top.y, ((1.0 - 0.005) + 0.3) * 300.0 / 300.0, 1e-4);
        edge_scroll_delta(Vec2::ZERO, 200.0, 0.1, &mut scratch);
        assert_eq!(scratch[2], 0.0);
        // Through the camera: the left border scrolls left, the top forward, and
        // the speed grows while the cursor stays.
        let mut camera = CivCamera::default();
        camera.reset(Vec3::ZERO);
        let mut focus = Vec3::ZERO;
        let at_left = CameraInput {
            edge: Vec2::new(0.01, 0.0),
            ..default()
        };
        let early = drive(&mut camera, &mut focus, &at_left, 0.5);
        drive(&mut camera, &mut focus, &at_left, 2.0);
        let late = drive(&mut camera, &mut focus, &at_left, 0.5);
        assert!(early.x < 0.0 && early.z.abs() < 1e-3, "{early:?}");
        assert!(late.x < 2.0 * early.x, "{early:?} then {late:?}: the dwell ramp speeds it up");
        let mut camera = CivCamera::default();
        camera.reset(Vec3::ZERO);
        let mut focus = Vec3::ZERO;
        let at_top = CameraInput {
            edge: Vec2::new(0.0, 0.01),
            ..default()
        };
        let up = drive(&mut camera, &mut focus, &at_top, 1.0);
        assert!(up.z < 0.0 && up.x.abs() < 1e-3, "{up:?}");
    }

    #[test]
    fn rotation_and_pitch_keys_follow_the_trigger_file() {
        let mut camera = CivCamera::default();
        camera.reset(Vec3::ZERO);
        // `.` rotates left (azimuth grows), `,` right.
        hold(
            &mut camera,
            &CameraInput {
                rotate_left: true,
                ..default()
            },
            0.5,
        );
        assert!(camera.goal_azimuth > 0.5, "{}", camera.goal_azimuth);
        let was = camera.goal_azimuth;
        hold(
            &mut camera,
            &CameraInput {
                rotate_right: true,
                ..default()
            },
            0.5,
        );
        assert!(camera.goal_azimuth < was);
        // PageUp lowers the elevation, PageDown raises it, inside 5..65 degrees.
        let start = camera.goal_pitch;
        hold(
            &mut camera,
            &CameraInput {
                pitch_up: true,
                ..default()
            },
            0.3,
        );
        assert!(camera.goal_pitch < start);
        hold(
            &mut camera,
            &CameraInput {
                pitch_up: true,
                ..default()
            },
            5.0,
        );
        close(camera.goal_pitch.to_degrees(), 5.0, 1e-3);
        hold(
            &mut camera,
            &CameraInput {
                pitch_down: true,
                ..default()
            },
            5.0,
        );
        close(camera.goal_pitch.to_degrees(), 65.0, 1e-3);
        // The eased angles arrive.
        hold(&mut camera, &CameraInput::default(), 3.0);
        close(camera.pitch, camera.goal_pitch, 1e-3);
        close(camera.azimuth, camera.goal_azimuth, 1e-3);
    }

    #[test]
    fn mouse_orbit_turns_and_tilts_inside_the_limits() {
        let mut camera = CivCamera::default();
        camera.reset(Vec3::ZERO);
        let az = camera.goal_azimuth;
        camera.update(
            1.0 / 60.0,
            &CameraInput {
                orbit: Vec2::new(40.0, 0.0),
                ..default()
            },
        );
        assert!(camera.goal_azimuth < az, "dragging right turns the azimuth down");
        let pitch = camera.goal_pitch;
        camera.update(
            1.0 / 60.0,
            &CameraInput {
                orbit: Vec2::new(0.0, 30.0),
                ..default()
            },
        );
        assert!(camera.goal_pitch > pitch, "dragging down raises the elevation");
        camera.update(
            1.0 / 60.0,
            &CameraInput {
                orbit: Vec2::new(0.0, 1.0e5),
                ..default()
            },
        );
        close(camera.goal_pitch.to_degrees(), 65.0, 1e-3);
    }

    #[test]
    fn zoom_events_walk_the_ladder_and_the_camera_eases_to_it() {
        let mut camera = CivCamera::default();
        camera.reset(Vec3::ZERO);
        camera.update(
            1.0 / 60.0,
            &CameraInput {
                zoom_events: 1,
                ..default()
            },
        );
        assert_eq!(camera.goal_distance, 200.0);
        assert!(camera.distance > 200.0, "the eased distance has not arrived");
        hold(&mut camera, &CameraInput::default(), 2.0);
        close(camera.distance, 200.0, 0.1);
        camera.update(
            1.0 / 60.0,
            &CameraInput {
                zoom_events: -2,
                ..default()
            },
        );
        assert_eq!(camera.goal_distance, 300.0);
        // Far out the zoom-out display scale applies.
        hold(&mut camera, &CameraInput::default(), 3.0);
        assert!(camera.plump());
        assert_eq!(zoom_out_target(camera.distance, 1.3), 1.3);
        assert_eq!(zoom_out_target(200.0, 1.3), 1.0);
    }

    #[test]
    fn wheel_notches_and_zoom_key_repeat() {
        let mut camera = CivCamera::default();
        assert_eq!(camera.take_wheel_notches(0.4), 0);
        assert_eq!(camera.take_wheel_notches(0.7), 1);
        assert_eq!(camera.take_wheel_notches(-2.0), -1, "0.1 was left over");
        assert_eq!(CivCamera::default().take_wheel_notches(-2.0), -2);
        assert_eq!(camera.take_wheel_notches(f32::NAN), 0);
        // A press is one event; holding repeats after the delay.
        assert_eq!(camera.zoom_key_events(true, false, true, 0.016), 1);
        let mut events = 0;
        for _ in 0..90 {
            events += camera.zoom_key_events(true, false, false, 1.0 / 60.0);
        }
        // 1.5 s held: nothing for 0.5 s, then one every 0.25 s.
        assert!((3..=5).contains(&events), "{events}");
        assert_eq!(camera.zoom_key_events(false, false, false, 0.016), 0);
        assert_eq!(camera.zoom_key_events(true, true, true, 0.016), 0);
    }

    #[test]
    fn drag_scroll_and_pose_clearance() {
        let window = Vec2::new(1280.0, 720.0);
        let press = Vec2::new(640.0, 360.0);
        assert_eq!(drag_scroll_vector(press, press, window), Vec2::ZERO);
        let right = drag_scroll_vector(press, Vec2::new(960.0, 360.0), window);
        assert!(right.x > 0.2 && right.y == 0.0);
        // Dragging up scrolls forward; the response is quadratic and capped.
        let up = drag_scroll_vector(press, Vec2::new(640.0, 180.0), window);
        close(up.y, 0.25, 1e-5);
        let far = drag_scroll_vector(press, Vec2::new(5000.0, 360.0), window);
        assert_eq!(far.x, 1.0);
        // The camera stays above the terrain.
        let mut camera = CivCamera::default();
        camera.reset(Vec3::ZERO);
        camera.pitch = 0.1;
        camera.distance = 15.0;
        let pose = camera.pose(|p| Vec3::new(p.x, 5.0, p.z));
        assert!(pose.translation.y >= 5.0 + tuning::MIN_HEIGHT - 1e-4);
    }

    #[test]
    fn keys_map_to_the_trigger_file() {
        let k = camera_keys(&keys(&[KeyCode::KeyW, KeyCode::KeyD]));
        assert!(k.forward && k.right && !k.backward && !k.left);
        let k = camera_keys(&keys(&[KeyCode::ArrowUp, KeyCode::Numpad4]));
        assert!(k.forward && k.left);
        // Shift + arrows rotate and pitch instead of moving.
        let k = camera_keys(&keys(&[KeyCode::ShiftLeft, KeyCode::ArrowLeft]));
        assert!(k.rotate_left && !k.left);
        let k = camera_keys(&keys(&[KeyCode::ShiftLeft, KeyCode::ArrowRight]));
        assert!(k.rotate_right && !k.right);
        let k = camera_keys(&keys(&[KeyCode::ShiftRight, KeyCode::ArrowUp]));
        assert!(k.pitch_up && !k.forward);
        let k = camera_keys(&keys(&[KeyCode::ShiftRight, KeyCode::ArrowDown]));
        assert!(k.pitch_down && !k.backward);
        let k = camera_keys(&keys(&[KeyCode::Comma, KeyCode::PageDown, KeyCode::Numpad3]));
        assert!(k.rotate_right && k.pitch_down);
        let k = camera_keys(&keys(&[KeyCode::Period, KeyCode::PageUp, KeyCode::Numpad9]));
        assert!(k.rotate_left && k.pitch_up);
        // Q and E are not camera keys in Civ (the camera never reads triggers 5, 6).
        let k = camera_keys(&keys(&[KeyCode::KeyQ, KeyCode::KeyE]));
        assert_eq!(k, CameraInput::default());
        // Ctrl chords never move the camera.
        let k = camera_keys(&keys(&[KeyCode::ControlLeft, KeyCode::KeyW, KeyCode::Comma]));
        assert_eq!(k, CameraInput::default());
        assert!(zoom_keys(&keys(&[KeyCode::Equal])).0);
        assert!(zoom_keys(&keys(&[KeyCode::NumpadSubtract])).1);
        assert_eq!(
            zoom_keys(&keys(&[KeyCode::ControlLeft, KeyCode::Minus])),
            (false, false, false)
        );
    }

    #[test]
    fn assembly_editor_keys_that_clash_need_ctrl() {
        let plain = keys(&[
            KeyCode::Comma,
            KeyCode::Period,
            KeyCode::Minus,
            KeyCode::Equal,
            KeyCode::Tab,
            KeyCode::Backspace,
            KeyCode::KeyN,
            KeyCode::KeyV,
            KeyCode::BracketRight,
        ]);
        let filtered = assembly_keys(&plain);
        for clash in [
            KeyCode::Comma,
            KeyCode::Period,
            KeyCode::Minus,
            KeyCode::Equal,
            KeyCode::Tab,
            KeyCode::Backspace,
        ] {
            assert!(
                !filtered.pressed(clash) && !filtered.just_pressed(clash),
                "{clash:?}"
            );
        }
        for kept in [KeyCode::KeyN, KeyCode::KeyV, KeyCode::BracketRight] {
            assert!(filtered.just_pressed(kept), "{kept:?}");
        }
        let mut chord = plain.clone();
        chord.press(KeyCode::ControlLeft);
        let filtered = assembly_keys(&chord);
        assert!(filtered.just_pressed(KeyCode::Comma) && filtered.just_pressed(KeyCode::Backspace));
    }

    #[test]
    fn groups_cycles_refunds_and_zoom_out_scale() {
        assert_eq!(group_slot(KeyCode::Digit1), Some(0));
        assert_eq!(group_slot(KeyCode::Digit0), Some(9));
        assert_eq!(group_slot(KeyCode::KeyA), None);
        assert_eq!(next_in_cycle(&[], None), None);
        assert_eq!(next_in_cycle(&[4, 7, 9], None), Some(4));
        assert_eq!(next_in_cycle(&[4, 7, 9], Some(7)), Some(9));
        assert_eq!(next_in_cycle(&[4, 7, 9], Some(9)), Some(4));
        assert_eq!(next_in_cycle(&[4, 7, 9], Some(99)), Some(4));
        let groups: [Vec<u32>; 4] = [vec![], vec![1], vec![], vec![2, 3]];
        assert_eq!(next_group(&groups, None), Some(1));
        assert_eq!(next_group(&groups, Some(1)), Some(3));
        assert_eq!(next_group(&groups, Some(3)), Some(1));
        assert_eq!(next_group(&[vec![], vec![]], None), None);
        // Half the price comes back.
        let refund =
            decommission_refund([civ::VehicleDomain::Land, civ::VehicleDomain::Air].into_iter());
        close(
            refund,
            0.5 * (civ::LAND_VEHICLE_COST + civ::AIR_VEHICLE_COST),
            1e-3,
        );
        // The display scale approaches its target by 0.008 per millisecond.
        close(zoom_out_step(1.0, 1.3, 0.016), 1.0 + 0.3 * 0.128, 1e-5);
        assert_eq!(zoom_out_step(1.0, 1.3, 1.0), 1.3);
        assert_eq!(CivStance::StandGround.toggled(), CivStance::Aggressive);
    }

    #[test]
    fn the_builtin_script_is_the_scroll_and_rotate_arithmetic() {
        let out = builtin(&ScriptInputs {
            distance: 200.0,
            scroll: Vec2::new(10.0, -5.0),
            rotate: 1.0,
            ..default()
        });
        close(out.scroll.x, -100.0, 1e-3);
        close(out.scroll.y, 50.0, 1e-3);
        close(out.rotate_deg, 5.0, 1e-6);
        close(out.mouse_scroll_sensitivity, 210.0, 1e-4);
        assert_eq!((out.min_pitch_deg, out.max_pitch_deg), (5.0, 65.0));
        // No cursor at a border: no edge scroll and the dwell timers reset.
        assert_eq!(out.anchor_delta, Vec2::ZERO);
        let edge = builtin(&ScriptInputs {
            distance: 200.0,
            edge: Vec2::new(0.01, 1.01),
            dt: 0.1,
            scratch: [1.0, 0.0, 2.0, 0.0, 0.0],
            ..default()
        });
        close(edge.anchor_delta.x, ((1.0 - 0.005) + 1.3) * 200.0 / 450.0, 1e-4);
        close(edge.anchor_delta.y, ((1.0 - 0.505) + 2.3) * 200.0 / 300.0, 1e-4);
        close(edge.scratch[0], 1.3, 1e-6);
        close(edge.scratch[2], 2.3, 1e-6);
    }

    /// The installed `civ_camera` script and the Rust numbers agree on a grid
    /// of inputs (the data check of this module).
    #[test]
    #[ignore = "requires the installed Spore packages"]
    fn installed_civ_camera_script_matches_the_builtin_numbers() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let bytes = index
            .read(ResourceKey {
                type_id: TYPE_ARITHMETICA,
                group_id: GROUP_SCRIPTS,
                instance_id: name_id("civ_camera"),
            })
            .expect("civ_camera is installed");
        let mut script = CameraScript::from_bytes(&bytes).unwrap();
        assert!(script.is_installed());
        let mut checked = 0;
        for distance in [
            8.0, 15.0, 30.0, 50.0, 60.0, 75.0, 90.0, 100.0, 120.0, 150.0, 200.0, 221.791, 250.0,
            275.0, 300.0, 350.0, 400.0, 450.0, 600.0,
        ] {
            for direction in [-1.0, 0.0, 1.0] {
                for alt in [false, true] {
                  for edge in [
                      Vec2::ZERO,
                      Vec2::new(0.01, 0.0),
                      Vec2::new(1.01, -0.99),
                      Vec2::new(-1.99, 0.01),
                      Vec2::new(0.0, 1.5),
                  ] {
                    let input = ScriptInputs {
                        direction,
                        distance,
                        scroll: Vec2::new(7.5, -2.5),
                        rotate: 0.6,
                        dt: 1.0 / 60.0,
                        alt_zoom_mode: alt,
                        edge,
                        scratch: [0.5, 0.2, 1.0, 5.9, 0.0],
                        ..default()
                    };
                    let from_script = script.evaluate(&input);
                    let from_rust = builtin(&input);
                    let label =
                        format!("distance {distance} direction {direction} alt {alt} edge {edge}");
                    close(from_script.distance, from_rust.distance, 1e-3);
                    close(from_script.anchor_delta.x, from_rust.anchor_delta.x, 1e-3);
                    close(from_script.anchor_delta.y, from_rust.anchor_delta.y, 1e-3);
                    for (a, b) in from_script.scratch.iter().zip(from_rust.scratch) {
                        assert!((a - b).abs() < 1e-4, "{label}: scratch {a} != {b}");
                    }
                    close(from_script.scroll.x, from_rust.scroll.x, 1e-3);
                    close(from_script.scroll.y, from_rust.scroll.y, 1e-3);
                    close(from_script.rotate_deg, from_rust.rotate_deg, 1e-4);
                    close(
                        from_script.mouse_scroll_sensitivity,
                        from_rust.mouse_scroll_sensitivity,
                        1e-3,
                    );
                    for (a, b) in [
                        (from_script.min_pitch_deg, from_rust.min_pitch_deg),
                        (from_script.max_pitch_deg, from_rust.max_pitch_deg),
                        (from_script.min_height, from_rust.min_height),
                        (from_script.acceleration_time, from_rust.acceleration_time),
                        (from_script.translation_speed, from_rust.translation_speed),
                        (from_script.min_interpolation, from_rust.min_interpolation),
                        (from_script.max_interpolation, from_rust.max_interpolation),
                        (from_script.alt_zoom_threshold, from_rust.alt_zoom_threshold),
                        (from_script.alt_zoom_timer, from_rust.alt_zoom_timer),
                        (from_script.mouse_rotate_x, from_rust.mouse_rotate_x),
                        (from_script.mouse_rotate_y, from_rust.mouse_rotate_y),
                    ] {
                        assert!((a - b).abs() < 1e-4, "{label}: {a} != {b}");
                    }
                    checked += 1;
                  }
                }
            }
        }
        println!("{checked} script evaluations match the Rust numbers");
        // The other planet cameras are files of the same format.
        for name in ["tribe_camera", "civ_follow_camera", "city_large_planner_camera"] {
            let bytes = index
                .read(ResourceKey {
                    type_id: TYPE_ARITHMETICA,
                    group_id: GROUP_SCRIPTS,
                    instance_id: name_id(name),
                })
                .unwrap();
            let mut vm = Arithmetica::from_bytes(&bytes).unwrap();
            vm.set("distance", 30.0);
            vm.set("direction", -1.0);
            vm.compute_all().unwrap();
            // Follow mode forces its distance (20), the others zoom out.
            if name == "civ_follow_camera" {
                assert_eq!(vm.get("distance"), 20.0);
            } else {
                assert!(vm.get("distance") > 30.0, "{name} zooms out");
            }
        }
    }

    // --- through the real schedule ------------------------------------------

    fn civ_app() -> App {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut()
            .resource_mut::<RunProgress>()
            .enter_stage(GameStage::Civilization);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Civilization);
        for _ in 0..6 {
            app.update();
        }
        app
    }

    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.release(key);
        input.clear();
    }

    fn tap_chord(app: &mut App, modifier: KeyCode, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(modifier);
        tap(app, key);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(modifier);
    }

    fn focus(app: &mut App) -> Vec3 {
        let world = app.world_mut();
        world
            .query_filtered::<&Transform, With<CivCommander>>()
            .single(world)
            .unwrap()
            .translation
    }

    fn selected_ids(app: &mut App) -> Vec<u32> {
        let world = app.world_mut();
        let mut ids: Vec<u32> = world
            .query_filtered::<&CivUnit, With<Selected>>()
            .iter(world)
            .map(|u| u.id)
            .collect();
        ids.sort_unstable();
        ids
    }

    fn home_count(app: &mut App) -> usize {
        let world = app.world_mut();
        world
            .query::<&CivUnit>()
            .iter(world)
            .filter(|u| u.nation == HOME_INDEX)
            .count()
    }

    #[test]
    fn schedule_wasd_pans_the_focus_and_the_camera_follows_it() {
        let mut app = civ_app();
        let start = focus(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        for _ in 0..60 {
            app.update();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyW);
        let moved = focus(&mut app) - start;
        assert!(moved.z < -10.0 && moved.x.abs() < 1e-3, "{moved:?}");
        for _ in 0..180 {
            app.update();
        }
        let now = focus(&mut app);
        let anchor = app.world().resource::<CivCamera>().anchor;
        assert!(
            (anchor.x - now.x).abs() < 0.05 && (anchor.z - now.z).abs() < 0.05,
            "the eased anchor arrived: {anchor:?} vs {now:?}"
        );
    }

    #[test]
    fn schedule_zoom_rotate_and_pitch_keys_reach_the_camera() {
        let mut app = civ_app();
        tap(&mut app, KeyCode::Equal);
        assert_eq!(app.world().resource::<CivCamera>().goal_distance, 200.0);
        tap(&mut app, KeyCode::Minus);
        assert_eq!(app.world().resource::<CivCamera>().goal_distance, 250.0);
        let az = app.world().resource::<CivCamera>().goal_azimuth;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Period);
        for _ in 0..30 {
            app.update();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Period);
        assert!(app.world().resource::<CivCamera>().goal_azimuth > az + 0.3);
        let pitch = app.world().resource::<CivCamera>().goal_pitch;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::PageUp);
        for _ in 0..20 {
            app.update();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::PageUp);
        assert!(app.world().resource::<CivCamera>().goal_pitch < pitch);
        // Ctrl chords do not move the camera (they belong to the editor).
        let goal = app.world().resource::<CivCamera>().goal_distance;
        tap_chord(&mut app, KeyCode::ControlLeft, KeyCode::Equal);
        assert_eq!(app.world().resource::<CivCamera>().goal_distance, goal);
        // Edge scrolling is the Settings option only (no F8 key).
        assert!(!app.world().resource::<CivControls>().edge_scroll);
        tap(&mut app, KeyCode::F8);
        assert!(!app.world().resource::<CivControls>().edge_scroll);
    }

    #[test]
    fn schedule_space_cycles_vehicles_and_follows_backspace_goes_home() {
        let mut app = civ_app();
        let total = home_count(&mut app);
        assert!(total >= 1, "the opening fleet exists");
        tap(&mut app, KeyCode::Space);
        let first = selected_ids(&mut app);
        assert_eq!(first.len(), 1);
        assert!(app.world().resource::<CivControls>().follow.is_some());
        if total > 1 {
            tap(&mut app, KeyCode::Space);
            let second = selected_ids(&mut app);
            assert_eq!(second.len(), 1);
            assert_ne!(first, second, "Space moves to the next vehicle");
        }
        // The focus rides on the followed vehicle.
        let id = app.world().resource::<CivControls>().follow.unwrap();
        let at = {
            let world = app.world_mut();
            world
                .query::<(&CivUnit, &Transform)>()
                .iter(world)
                .find(|(u, _)| u.id == id)
                .map(|(_, t)| t.translation)
                .unwrap()
        };
        let f = focus(&mut app);
        assert!((f.x - at.x).abs() < 0.5 && (f.z - at.z).abs() < 0.5);
        // Panning by hand ends the follow.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyA);
        for _ in 0..5 {
            app.update();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyA);
        assert!(app.world().resource::<CivControls>().follow.is_none());
        // Backspace: over the capital again.
        {
            let world = app.world_mut();
            let mut f = world.query_filtered::<&mut Transform, With<CivCommander>>();
            f.single_mut(world).unwrap().translation = Vec3::new(30.0, 0.0, 30.0);
        }
        tap(&mut app, KeyCode::Backspace);
        let home = civ::city_position(HOME_INDEX);
        let f = focus(&mut app);
        assert!(
            (f.x - home.x).abs() < 1e-3 && (f.z - home.z).abs() < 1e-3,
            "{f:?} {home:?}"
        );
    }

    #[test]
    fn schedule_ctrl_digit_groups_tab_stance_delete_and_esc() {
        let mut app = civ_app();
        let total = home_count(&mut app);
        assert!(total >= 1);
        // Select the whole fleet through the doctrine key and define group 1.
        tap(&mut app, KeyCode::KeyQ);
        tap(&mut app, KeyCode::KeyQ);
        let all = selected_ids(&mut app);
        assert_eq!(all.len(), total);
        tap_chord(&mut app, KeyCode::ControlLeft, KeyCode::Digit1);
        assert_eq!(app.world().resource::<CivControls>().groups[0].len(), total);
        // Ctrl + digit is not the doctrine key.
        let approach = app.world().resource::<RunProgress>().civ_campaign.approach;
        tap_chord(&mut app, KeyCode::ControlLeft, KeyCode::Digit2);
        assert_eq!(
            app.world().resource::<RunProgress>().civ_campaign.approach,
            approach
        );
        // Esc deselects and does not open the pause menu.
        tap(&mut app, KeyCode::Escape);
        assert!(selected_ids(&mut app).is_empty());
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
        // Space now selects the defined group.
        tap(&mut app, KeyCode::Space);
        assert_eq!(selected_ids(&mut app), all);
        // Tab switches the stance of the selection.
        tap(&mut app, KeyCode::Tab);
        {
            let world = app.world_mut();
            let stances: Vec<CivStance> = world
                .query_filtered::<&CivStance, With<Selected>>()
                .iter(world)
                .copied()
                .collect();
            assert_eq!(stances.len(), total);
            assert!(stances.iter().all(|s| *s == CivStance::Aggressive));
        }
        tap(&mut app, KeyCode::Tab);
        {
            let world = app.world_mut();
            assert!(world
                .query_filtered::<&CivStance, With<Selected>>()
                .iter(world)
                .all(|s| *s == CivStance::StandGround));
        }
        // Delete decommissions the selection for half the price.
        let spice = app.world().resource::<RunProgress>().spice;
        tap(&mut app, KeyCode::Delete);
        app.update();
        assert_eq!(home_count(&mut app), 0);
        assert!(app.world().resource::<RunProgress>().spice > spice);
        // With nothing selected Esc opens the pause menu.
        tap(&mut app, KeyCode::Escape);
        assert!(app.world().resource::<crate::pause_menu::PauseMenu>().open);
        tap(&mut app, KeyCode::Escape);
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
    }

    #[test]
    fn schedule_the_camera_starts_over_on_every_stage_entry() {
        let mut app = civ_app();
        tap(&mut app, KeyCode::Equal);
        assert_ne!(
            app.world().resource::<CivCamera>().goal_distance,
            tuning::INITIAL_DISTANCE
        );
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Tribal);
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().resource::<CivCamera>().needs_reset);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .enter_stage(GameStage::Civilization);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Civilization);
        for _ in 0..4 {
            app.update();
        }
        let camera = app.world().resource::<CivCamera>();
        assert_eq!(camera.goal_distance, tuning::INITIAL_DISTANCE);
        assert_eq!(camera.goal_pitch, tuning::INITIAL_PITCH);
    }

    #[test]
    fn zoom_out_scaling_keeps_other_systems_writes_and_scales_about_the_pivot() {
        let base = Transform::from_xyz(2.0, 1.0, 0.0).with_scale(Vec3::splat(2.0));
        let pivot = Vec3::new(0.0, -1.0, 0.0);
        let big = scaled_about(&base, pivot, 3.0);
        assert_eq!(big.scale, Vec3::splat(6.0));
        // The point of the floor under the centre stays, the hall grows from it.
        assert!(big.translation.distance(Vec3::new(6.0, 5.0, 0.0)) < 1e-5);
        assert_eq!(scaled_about(&base, pivot, 1.0), base);
        // In place: the pivot is the entity's own translation.
        let in_place = scaled_about(&base, base.translation, 1.3);
        assert_eq!(in_place.translation, base.translation);
        // A writer of another component (the vehicle bob) is not undone, and
        // the factor is not compounded.
        let mut t = base;
        let mut state = ZoomOutState::new(&t);
        for _ in 0..200 {
            state.step(&mut t, 1.3, 0.016, None);
        }
        close(t.scale.x, 2.6, 1e-3);
        t.translation.y += 0.5;
        state.step(&mut t, 1.3, 0.016, None);
        assert!((t.translation.y - 1.5).abs() < 1e-4, "{:?}", t.translation);
        close(t.scale.x, 2.6, 1e-3);
        for _ in 0..200 {
            state.step(&mut t, 1.0, 0.016, None);
        }
        close(t.scale.x, 2.0, 1e-3);
    }

    fn mouse_app() -> App {
        let mut app = civ_app();
        app.init_resource::<ButtonInput<MouseButton>>();
        app.init_resource::<AccumulatedMouseMotion>();
        let window = Window::default();
        app.world_mut().spawn(window);
        app
    }

    fn move_cursor(app: &mut App, at: Vec2) {
        let world = app.world_mut();
        let mut windows = world.query::<&mut Window>();
        for mut window in windows.iter_mut(world) {
            window.set_cursor_position(Some(at));
        }
    }

    fn motion(app: &mut App, delta: Vec2) {
        app.world_mut().resource_mut::<AccumulatedMouseMotion>().delta = delta;
    }

    #[test]
    fn schedule_the_middle_button_and_the_two_button_chord_orbit() {
        let mut app = mouse_app();
        let az = app.world().resource::<CivCamera>().goal_azimuth;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Middle);
        motion(&mut app, Vec2::new(40.0, 0.0));
        app.update();
        assert!(app.world().resource::<CivControls>().orbiting);
        let turned = app.world().resource::<CivCamera>().goal_azimuth;
        assert!(turned < az, "{turned} {az}");
        // Motion without a button does nothing.
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.release(MouseButton::Middle);
            mouse.clear();
        }
        motion(&mut app, Vec2::new(40.0, 0.0));
        app.update();
        // Releasing the orbit button ends the chase of the dragged goal: the
        // goal becomes the current angle plus a short coast (0xB10BA0).
        let coasted = app.world().resource::<CivCamera>().goal_azimuth;
        let now = app.world().resource::<CivCamera>().azimuth;
        assert!((coasted - now).abs() < (turned - now).abs(), "{coasted} {turned} {now}");
        assert!(!app.world().resource::<CivControls>().orbiting);
        // Motion without a button does nothing more.
        motion(&mut app, Vec2::new(40.0, 0.0));
        app.update();
        assert_eq!(app.world().resource::<CivCamera>().goal_azimuth, coasted);
        // Left + right: orbit, and no selection box (the left button is reset).
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.press(MouseButton::Left);
            mouse.press(MouseButton::Right);
        }
        motion(&mut app, Vec2::new(0.0, 30.0));
        let pitch = app.world().resource::<CivCamera>().goal_pitch;
        app.update();
        assert!(app.world().resource::<CivControls>().orbiting);
        assert!(app.world().resource::<CivCamera>().goal_pitch > pitch);
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left),
            "the chord swallows the left button"
        );
        // The chord holds while the right button is down.
        motion(&mut app, Vec2::new(20.0, 0.0));
        app.update();
        assert!(app.world().resource::<CivControls>().orbiting);
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.release(MouseButton::Right);
            mouse.release(MouseButton::Left);
            mouse.clear();
        }
        motion(&mut app, Vec2::ZERO);
        app.update();
        assert!(!app.world().resource::<CivControls>().orbiting);
    }

    #[test]
    fn schedule_a_right_drag_beyond_twelve_pixels_pans_a_still_click_does_not() {
        let mut app = mouse_app();
        let _option = crate::control_options::hold_option(crate::control_options::StageOption::Civ, false);
        let start = focus(&mut app);
        move_cursor(&mut app, Vec2::new(640.0, 360.0));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        // Within the click slop: nothing pans.
        move_cursor(&mut app, Vec2::new(648.0, 360.0));
        app.update();
        assert!(!app.world().resource::<CivControls>().panning);
        assert!((focus(&mut app) - start).length() < 1e-4);
        // Past it: the drag scrolls like a joystick (up = forward, -Z).
        move_cursor(&mut app, Vec2::new(640.0, 200.0));
        for _ in 0..30 {
            app.update();
        }
        assert!(app.world().resource::<CivControls>().panning);
        let moved = focus(&mut app) - start;
        assert!(moved.z < -1.0 && moved.x.abs() < 1e-3, "{moved:?}");
        // Releasing ends the drag.
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.release(MouseButton::Right);
            mouse.clear();
        }
        app.update();
        assert!(!app.world().resource::<CivControls>().panning);
    }

    #[test]
    fn schedule_with_the_civ_option_a_right_drag_rotates_instead_of_panning() {
        // `OptionCivControls` on = `UIMachineCivRotate`: the drag of the right
        // button turns the camera once the cursor is 16 pixels from the press.
        let mut app = mouse_app();
        // After the app is up: the front end publishes the (default) settings
        // when its first frame runs, which switches every option off.
        let _option = crate::control_options::hold_option(crate::control_options::StageOption::Civ, true);
        let start = focus(&mut app);
        move_cursor(&mut app, Vec2::new(640.0, 360.0));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        // 14 pixels: past the click slop (12) but not the long move (16).
        move_cursor(&mut app, Vec2::new(654.0, 360.0));
        motion(&mut app, Vec2::new(14.0, 0.0));
        app.update();
        {
            let controls = app.world().resource::<CivControls>();
            assert!(!controls.orbiting && !controls.panning);
        }
        // Further out: it rotates (yaw follows the horizontal motion) and never pans.
        let az = app.world().resource::<CivCamera>().goal_azimuth;
        move_cursor(&mut app, Vec2::new(700.0, 360.0));
        motion(&mut app, Vec2::new(46.0, 0.0));
        app.update();
        assert!(app.world().resource::<CivControls>().orbiting);
        assert!(!app.world().resource::<CivControls>().panning);
        assert!(app.world().resource::<CivCamera>().goal_azimuth != az);
        // It stays a rotation when the cursor comes back near the press.
        move_cursor(&mut app, Vec2::new(642.0, 360.0));
        motion(&mut app, Vec2::new(-58.0, 0.0));
        app.update();
        assert!(app.world().resource::<CivControls>().orbiting);
        assert!((focus(&mut app) - start).length() < 1e-4, "no pan");
        // Releasing ends it.
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.release(MouseButton::Right);
            mouse.clear();
        }
        motion(&mut app, Vec2::ZERO);
        app.update();
        assert!(!app.world().resource::<CivControls>().orbiting);
    }

    #[test]
    fn schedule_vehicles_grow_when_the_camera_is_far_out() {
        let mut app = civ_app();
        let art = {
            let world = app.world_mut();
            world
                .query::<&civ_rts::UnitRig>()
                .iter(world)
                .next()
                .map(|rig| rig.art)
                .expect("a vehicle")
        };
        let scale_of = |app: &App| app.world().get::<Transform>(art).unwrap().scale.x;
        for _ in 0..4 {
            app.update();
        }
        let near = scale_of(&app);
        {
            let mut camera = app.world_mut().resource_mut::<CivCamera>();
            camera.distance = 400.0;
            camera.goal_distance = 400.0;
        }
        for _ in 0..90 {
            app.update();
        }
        close(scale_of(&app), near * 1.3, 1e-3);
        {
            let mut camera = app.world_mut().resource_mut::<CivCamera>();
            camera.distance = 100.0;
            camera.goal_distance = 100.0;
        }
        for _ in 0..90 {
            app.update();
        }
        close(scale_of(&app), near, 1e-3);
    }
}
