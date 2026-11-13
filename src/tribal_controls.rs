//! Tribal stage camera and controls as the original binds them.
//!
//! Sources: `TriggerConfigTribeGame` ("TRG/CVG Camera") and `TriggerConfigWASD`
//! (notes\tribal-re-notes.md T6), the full `Spore_Graphics!TribeCamera` prop
//! (dumped; nine properties), the installed manual's "Tribe Controls" pages, the
//! ModAPI `cTribeInputStrategy` / `CitizenAction` interface and, since the
//! tribal-cam round, the retail planet camera itself (notes\tribal-cam-notes.md):
//! the Tribal stage drives the SAME `PlanetCamera` class as the Civilization
//! stage (0xB127D0 constructor, 0xB14800 per-frame input, 0xB0F1D0 script I/O,
//! 0xB10810 script loader) with its own Arithmetica script `tribe_camera`
//! (`0x024A4F5A!0xCF0A771B`, loaded at 0xCDA114 / 0xCD0B03). That script and the
//! class code give the zoom ladder, the pitch limits, the key ramps, the scroll
//! and rotate arithmetic, the drag scroll and the edge scroll. Since the
//! tribal-cam2 round the rig also moves its channels the way the class does
//! (0xB11930, `notes\civ-cam-notes.md`; the primitives are the ones of
//! `civ_controls`): every channel has a GOAL and a cubic Hermite segment that is
//! re-solved each update from the current value and velocity (yaw 0.2 s, pitch
//! 0.65 s, anchor and distance `lerp(min, max interpolation time, angle / pi)`),
//! a pan puts the goal `scroll` ahead of the CURRENT anchor, the pan keys'
//! release coasts the anchor on (`|velocity| x min_interp x 250`), the orbit
//! button's release coasts yaw and pitch (0.1 s), the mouse orbit uses the
//! retail units (1 and 10 degrees per pixel unit, goal offset from the current
//! angle) and the edge scroll the script's dwell ramp in the two pixel border.
//! What the code does not show (the update rate the per-update increments
//! assume, how a frame's mouse motion maps to events) stays `ADAPTED` and is
//! marked where used.
//!
//! The Tribal stage has no avatar walking: the camera is a free orbit camera
//! over a ground point and the tribe is run through selection and commands.
//!
//! Keys (Tribal stage; the manual and the two trigger files):
//! * W A S D / arrows / numpad 8 4 2 6 pan the camera over the ground, Q E pan
//!   sideways ("strafe" in `TriggerConfigWASD`; retail 0xB14800 reads only the
//!   four direction triggers, so Q / E are an OpenSpore extra); the right mouse
//!   button held and dragged scrolls like a joystick (the farther the cursor is
//!   from where the button went down, the faster); with Edge Scroll on the
//!   cursor at the window edge pans.
//! * `.` and Shift+Left rotate left (the yaw goal grows), `,` and Shift+Right
//!   rotate right, PageUp / numpad 9 / Shift+Up pitch up (the camera's elevation
//!   FALLS: the view tilts up toward the horizon, 0xB109F0), PageDown / numpad 3 /
//!   Shift+Down pitch down, `=` `+` numpad+ zoom in, `-` numpad- zoom out; the
//!   wheel zooms one ladder step per notch; left and right mouse buttons together
//!   (or the middle button, retail orbit button 0x3E9) rotate and pitch with the
//!   mouse.
//! * Space follows a tribe member (again: the next one); the minimap's left
//!   click moves the camera, its right click sends the selection
//!   ([`TribalControls::goto_request`] / [`TribalControls::move_request`]).
//! * Left click selects a member (Ctrl+click toggles one into or out of the
//!   selection, a drag draws a box), Esc deselects everyone (and only then
//!   opens the pause menu), Ctrl+A selects the whole tribe (an extra: the
//!   original clicks the tribe header bar, which presses [`KEY_SELECT_TRIBE`]).
//! * Right click on the world gives the selection ONE command chosen by what is
//!   under the cursor (`cTribeInputStrategy::ChooseInputActionID` for the id the
//!   cursor shows, `DoActionGeneric` for what the click does with it): see
//!   `tribal_hud::click`; social orders against a tribe need the chieftain
//!   selected.
//!
//! The retail camera at stage start (0xCD9F20): `SnapPolar(azimuth 1.5,
//! pitch 0.4 rad, distance 16)` and then `LoadScript(tribe_camera)`, whose
//! `default_distance` 70 becomes the goal distance: the camera opens 16 units
//! from the village at 23 degrees of elevation and glides out to 70. The retail
//! azimuth 1.5 is relative to the camera's frame quaternion at the anchor:
//! `SetAnchor` 0xB145E0 builds it with the planet's `GetOrientation` (0xF9C610,
//! `planet_query::Planet::orientation`) from the anchor position and the vector
//! `anchor direction + (0, 0, 1)`, which is the local east / north / up frame
//! (north = toward the planet-local +Z pole), and `0xB133E0` composes the
//! camera as `anchor + frame * Rz(yaw) * Rx(-pitch) * (0, -distance, 0)` (Spore
//! frame: x right, y forward, z up). [`TribalRig::basis`] is that frame, so
//! azimuth 0 puts the camera south of the village looking north and the
//! retail 1.5 puts it east of the village looking west (the heading follows the
//! planet's pole, not the path the camera took: panning around the planet does
//! not turn the view). The zoom
//! ladder is 15, 25, 35, 50, 70, 90 and the pitch limits -20 .. 60 degrees; the
//! pitch does not follow the zoom (nothing reads the script's own
//! `default_pitch_for_distance`).
//!
//! On the planet (planet-re 4E step 5) the rig orbits a point of the sphere:
//! its frame is the local up at the focus (the minimal turn of +Y, as
//! `terrain::ground` turns entities), panning moves the focus over the
//! curved ground, and nothing bounds it: the camera can circle the planet.
//!
//! Changes from OpenSpore's earlier bindings (see the CHANGES.md of this helper):
//! W A S D / arrows pan the camera instead of walking the chieftain (he is
//! selected and commanded like a member), Q (select tribe) is Ctrl+A / the
//! HUD key, E (perform) became [`KEY_PERFORM`] (the right click on a village in
//! the social stance sends the chieftain and presses it on arrival), `,` and `.`
//! (role in the tribe editor) became `[` and `]`, left click no longer commands,
//! right click does, and Ctrl joins Shift as the multi-select modifier.

use crate::{
    civ_controls::{
        drag_scroll_vector, edge_distance, edge_scroll_block, interp, ramp,
        CameraScript, ScriptInputs, ScriptOutputs, Segment, VecSegment, FLAT_RADIUS,
        MOUSE_ORBIT_CLAMP, NOMINAL_UPDATE_SECONDS,
    },
    game::{dist_xz, Player, RunProgress},
    stages::GameStage,
    tribal::{self, RivalStatus},
    tribal_rts::{formation_offset, Order, Selected, TribalRts, TribeUnit, UnitTask},
    tribal_tools::ToolKind,
    Originals,
};
use bevy::{input::mouse::*, prelude::*};

// ---------------------------------------------------------------------------
// Original values and key bindings
// ---------------------------------------------------------------------------

/// Values of `Spore_Graphics!TribeCamera` (0x40410100!0xE3057616),
/// `TribeGame!TribeTuning` and the `tribe_camera` Arithmetica script.
#[allow(dead_code)]
pub mod tuning {
    /// cameraZoomScale / cameraRotateScale / cameraPitchScale
    pub const ZOOM_SCALE: f32 = 1.0;
    pub const ROTATE_SCALE: f32 = 0.5;
    pub const PITCH_SCALE: f32 = 0.1;
    /// cameraNearClip / cameraFarClip
    pub const NEAR_CLIP: f32 = 1.0;
    pub const FAR_CLIP: f32 = 10000.0;
    /// kPlanetElevationTable. RE (0x6F3350): the planet renderer compares the
    /// camera elevation against these three heights and switches its detail
    /// layers on or off. They are NOT zoom stops (notes T6 guessed so), so
    /// nothing here uses them; they are kept for a future layer switch.
    pub const ELEVATION_TABLE: [f32; 3] = [20.0, 200.0, 330.0];
    /// cameraMaterialLODs (the same kind of distance thresholds).
    pub const MATERIAL_LODS: [f32; 4] = [20.0, 60.0, 200.0, 500.0];
    /// tribeCameraSkipDistance: a camera move longer than this cuts instead of
    /// gliding (ADAPTED reading of the name).
    pub const CAMERA_SKIP_DISTANCE: f32 = 16.0;
    /// miniMapZoomLevel (the retail minimap of the HUD helper reads it).
    pub const MINIMAP_ZOOM_LEVEL: f32 = 0.7;

    // -- the `tribe_camera` script (notes\civ-re\camera\tribe_camera.arth_t) --
    /// The goal distance snaps through this ladder, one step per zoom event
    /// (zoom out: the smallest step above the goal; zoom in: the largest below).
    pub const ZOOM_LADDER: [f32; 6] = [15.0, 25.0, 35.0, 50.0, 70.0, 90.0];
    /// `default_distance`: the loader (0xB10810) makes it the goal distance.
    pub const DEFAULT_DISTANCE: f32 = 70.0;
    /// `min_pitch` / `max_pitch` in degrees (the script writes `20 * -1`).
    pub const MIN_PITCH_DEG: f32 = -20.0;
    pub const MAX_PITCH_DEG: f32 = 60.0;
    /// `min_height`: the camera and its anchor stay this far above the ground.
    pub const MIN_HEIGHT: f32 = 0.75;
    /// `keyboard_rotate = keyboard_rotate * 5` (degrees per update).
    pub const KEYBOARD_ROTATE_DEGREES: f32 = 5.0;
    /// `keyboard_scroll = scroll * -distance / 25`.
    pub const KEYBOARD_SCROLL_DIVISOR: f32 = 25.0;
    /// `mouse_scroll_sensitivity = 20 + distance`.
    pub const MOUSE_SCROLL_SENSITIVITY_BASE: f32 = 20.0;
    /// `mouse_rotate_sensitivity_x / y`.
    pub const MOUSE_ROTATE_SENSITIVITY_X: f32 = 1.0;
    pub const MOUSE_ROTATE_SENSITIVITY_Y: f32 = 1.0;
    /// `alt_zoom_threshold` / `alt_zoom_timer`: set by the script but the Tribal
    /// ladder has no alt step (15 is an ordinary step), so nothing reads them.
    pub const ALT_ZOOM_THRESHOLD: f32 = 20.0;
    pub const ALT_ZOOM_TIMER: f32 = 0.2;
    /// The edge scroll block of the script: the dwell counters grow by
    /// `camera_delta_time * 0.5` up to `time_limit` 3; the divisors of
    /// `anchor_delta` are 300 (x) and 200 (y).
    pub const EDGE_DWELL_RATE: f32 = 0.5;
    pub const EDGE_TIME_LIMIT: f32 = 3.0;
    pub const EDGE_DIVISOR_X: f32 = 300.0;
    pub const EDGE_DIVISOR_Y: f32 = 200.0;

    // -- the defaults the code sets before every script run (0xB0F1D0) --
    /// The script never writes these four, so the code's values stand.
    pub const KEYBOARD_ACCELERATION_TIME: f32 = 0.7;
    pub const KEYBOARD_TRANSLATION_SPEED: f32 = 10.0;
    pub const MIN_INTERPOLATION_TIME: f32 = 0.7;
    pub const MAX_INTERPOLATION_TIME: f32 = 6.0;
    /// The camera object's edge scroll speed field (0xB127D0: 1500.0).
    pub const EDGE_SCROLL_SPEED: f32 = 1500.0;

    // -- the stage start (0xCD9F20: SnapPolar(1.5, 0.4, 16) then LoadScript) --
    /// The retail yaw, relative to the retail frame at the village (0xCDA0ED:
    /// the first stack argument `[0x13EED94]` of `SnapPolar`).
    pub const START_AZIMUTH: f32 = 1.5;
    pub const START_PITCH: f32 = 0.4;
    pub const START_DISTANCE: f32 = 16.0;
}

/// `TriggerConfigTribeGame` action ids (documentation and tests).
#[allow(dead_code)]
pub mod trigger {
    pub const PITCH_UP: u32 = 0x07;
    pub const PITCH_DOWN: u32 = 0x08;
    pub const ZOOM_IN: u32 = 0x09;
    pub const ZOOM_OUT: u32 = 0x10;
    pub const ROTATE_RIGHT: u32 = 0x11;
    pub const ROTATE_LEFT: u32 = 0x12;
}

/// Keys the HUD presses (virtual keys no keyboard has, like the Creature
/// stance keys) and that the older gameplay code reads.
/// The tribe header bar of the tribal list: select every member (Ctrl+A on the
/// keyboard). Was `Q`, which `TriggerConfigWASD` binds to strafe left.
pub const KEY_SELECT_TRIBE: KeyCode = KeyCode::F15;
/// The social stance button selects the social stance (the C key toggles).
pub const KEY_STANCE_SOCIAL: KeyCode = KeyCode::F16;
/// The attack stance button selects the attack stance.
pub const KEY_STANCE_ATTACK: KeyCode = KeyCode::F17;
/// Start the social performance for the village the chieftain stands at. Was
/// `E` (strafe right in `TriggerConfigWASD`); the right click on a village in
/// the social stance sends the chieftain and presses this key on arrival.
pub const KEY_PERFORM: KeyCode = KeyCode::F18;
/// Edge Scroll on or off: the original's Settings option (config 0x636EC26),
/// which `front_end::settings` applies to [`TribalControls::edge_scroll`]; F8
/// toggles the same flag in play (a developer extra).
pub const KEY_EDGE_SCROLL: KeyCode = KeyCode::F8;
/// Tribe editor role cycling (was `,` and `.`, which rotate the camera).
pub const KEY_ROLE_PREV: KeyCode = KeyCode::BracketLeft;
pub const KEY_ROLE_NEXT: KeyCode = KeyCode::BracketRight;

// ---------------------------------------------------------------------------
// Camera timing (the retail Hermite channels of `civ_controls::interp`)
// ---------------------------------------------------------------------------

/// Closest and farthest ladder steps (distance from the focus point).
pub const ZOOM_MIN: f32 = tuning::ZOOM_LADDER[0];
pub const ZOOM_MAX: f32 = tuning::ZOOM_LADDER[5];
#[allow(dead_code)]
pub const PITCH_MIN_DEG: f32 = tuning::MIN_PITCH_DEG;
#[allow(dead_code)]
pub const PITCH_MAX_DEG: f32 = tuning::MAX_PITCH_DEG;
/// ADAPTED: the per-update increments of the original (5 degrees of rotation per
/// update at full ramp) assume its camera update rate; 30 per second is taken
/// (the Civ port of the same class makes the same choice).
const UPDATES_PER_SECOND: f32 = crate::civ_controls::UPDATES_PER_SECOND;
/// Wheel notches and held zoom keys: one retail zoom event per notch, and one
/// per camera UPDATE while a zoom key is held (0xB14800 tests the triggers 9 /
/// 0x10 with `IsTriggered` every update and sets the script's `direction` to
/// +-`zoom speed` (0xB14ADF..0xB14B4E); there is no delay or repeat period, so
/// a held key walks the 6-step ladder one step per update). The update rate
/// is the ADAPTED [`UPDATES_PER_SECOND`] (rows 7 / 23 of the audit).
/// PageUp / PageDown: `0xB109F0` changes the pitch by
/// `mouse_rotate_sensitivity_y * 10 deg * 5` per second, not ramped.
const PITCH_KEY_DEGREES_PER_UNIT: f32 = 10.0 * 5.0;
/// Mouse orbit (0xB0FDF0): the yaw goal is `current yaw - sens_x_rad * dx` and
/// the pitch goal `current pitch + sens_y_rad * dy` with `sens_x_rad` = the
/// script's value x 1 degree and `sens_y_rad` = its value x 10 degrees (Tribal
/// 1.0 and 1.0), one event clamped to +-20 pixels. The goals are offsets from
/// the CURRENT angles, so on a 0.2 s / 0.65 s Hermite channel the gain per
/// pixel is a fraction of the unit (see `notes\civ-cam-notes.md`).
const MOUSE_X_DEGREES_PER_UNIT: f32 = 1.0;
const MOUSE_Y_DEGREES_PER_UNIT: f32 = 10.0;
/// The camera never goes below this height above the terrain, and the anchor
/// it looks at stands this high over the ground (0xB14800 sets the anchor
/// height to `min_height` over the terrain).
const LOOK_HEIGHT: f32 = tuning::MIN_HEIGHT;
const ABOVE_TERRAIN: f32 = tuning::MIN_HEIGHT;
/// A press that travels farther than this is a drag, not a click. The Civ
/// stage's `clickDragDistance` (12); the Tribal tuning has none (ADAPTED).
pub const DRAG_SLOP: f32 = crate::civ_controls::CLICK_DRAG_DISTANCE;
/// Vertical field of view assumed when the camera has none (Bevy's default).
#[allow(dead_code)]
const DEFAULT_FOV: f32 = std::f32::consts::FRAC_PI_4;
const TAU: f32 = std::f32::consts::TAU;

/// The start azimuth for a value of `OPENSPORE_TRIBE_START_AZIMUTH` (radians,
/// a developer override): absent, empty or not a finite number is the retail
/// [`tuning::START_AZIMUTH`].
pub fn parse_start_azimuth(value: Option<&str>) -> f32 {
    value
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|a| a.is_finite())
        .unwrap_or(tuning::START_AZIMUTH)
}

/// The azimuth the Tribal camera opens at: the retail `SnapPolar` yaw 1.5
/// (`tuning::START_AZIMUTH`, 0xCDA0ED) in the retail frame at the village
/// ([`anchor_frame`]); `OPENSPORE_TRIBE_START_AZIMUTH` (radians) overrides it
/// for captures.
pub fn start_azimuth() -> f32 {
    parse_start_azimuth(std::env::var("OPENSPORE_TRIBE_START_AZIMUTH").ok().as_deref())
}

#[allow(dead_code)] // the tests compare azimuths modulo a turn
fn wrap_angle(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI
}

/// `Math::Vector3` perpendicular of 0x6985B0: the vector crossed with the axis
/// of its smallest component (ties as the code's two compares resolve them).
fn retail_perpendicular(v: Vec3) -> Vec3 {
    let (ax, ay, az) = (v.x.abs(), v.y.abs(), v.z.abs());
    if ay > ax {
        if az > ax {
            // x smallest: v x X
            Vec3::new(0.0, v.z, -v.y)
        } else {
            // z smallest: v x Z
            Vec3::new(v.y, -v.x, 0.0)
        }
    } else if az > ay {
        // y smallest: v x Y
        Vec3::new(-v.z, 0.0, v.x)
    } else {
        // x largest, z not larger than y: v x Z
        Vec3::new(v.y, -v.x, 0.0)
    }
}

/// The camera frame the retail planet camera keeps at its anchor. `SetAnchor`
/// (0xB145E0) calls the planet's `GetOrientation` 0xF9C610 with the anchor
/// position (the "up" vector) and `anchor direction + (0, 0, 1)` and stores
/// the resulting quaternion in the frame channel (+0x90); `0xB133E0` rotates
/// the camera offset by it. With `up` the anchor direction in planet-local
/// Spore space (Z is the pole) the three returned unit vectors are the frame's
/// x (east), y (north, toward +Z) and z (up) axes. At the pole itself the
/// code falls back to the perpendicular of `up` (0xF9C74E..0xF9C788); a
/// direction shorter than 2^-8 yields the identity frame.
pub fn anchor_frame(up: Vec3) -> (Vec3, Vec3, Vec3) {
    const EPS: f32 = 1.525_878_9e-5; // 2^-16, 0x14904B0
    if up.length_squared() < EPS {
        return (Vec3::X, Vec3::Y, Vec3::Z);
    }
    let u = up.normalize();
    let toward = u + Vec3::Z;
    if toward.length_squared() < EPS {
        // The south pole: both vectors are not independent, 0xF9C659 answers
        // with the identity quaternion.
        return (Vec3::X, Vec3::Y, Vec3::Z);
    }
    let mut x = toward.cross(u);
    x = if x.length() < EPS {
        retail_perpendicular(u).normalize_or(Vec3::X)
    } else {
        x.normalize()
    };
    let y = u.cross(x);
    if y.length() < EPS {
        return (Vec3::X, Vec3::Y, Vec3::Z);
    }
    (x, y.normalize(), u)
}

// ---------------------------------------------------------------------------
// The camera script (Arithmetica `tribe_camera`, or the same numbers)
// ---------------------------------------------------------------------------

/// Pure: the zoom ladder of `tribe_camera`. Zooming in picks the largest step
/// below the goal distance, zooming out the smallest step above it.
pub fn zoom_ladder(distance: f32, direction: f32) -> f32 {
    if direction > 0.0 {
        if let Some(step) = tuning::ZOOM_LADDER.iter().rev().find(|s| distance > **s) {
            return *step;
        }
    } else if direction < 0.0 {
        if let Some(step) = tuning::ZOOM_LADDER.iter().find(|s| distance < **s) {
            return *step;
        }
    }
    distance
}

/// The `tribe_camera` script in Rust: used without an install, and checked
/// against the installed script by an ignored test. The four values the script
/// does not write are the defaults the code sets before each run.
pub fn builtin(input: &ScriptInputs) -> ScriptOutputs {
    let distance = zoom_ladder(input.distance, input.direction);
    // The edge scroll block of the script (the mouse drag fields are zero).
    let mut scratch = input.scratch;
    let anchor_delta = edge_scroll_block(
        input.edge,
        distance,
        input.dt,
        &mut scratch,
        tuning::EDGE_DWELL_RATE,
        tuning::EDGE_TIME_LIMIT,
        [tuning::EDGE_DIVISOR_X, tuning::EDGE_DIVISOR_Y],
    );
    ScriptOutputs {
        distance,
        scroll: input.scroll * (-distance / tuning::KEYBOARD_SCROLL_DIVISOR),
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

/// The Tribal camera script: the installed `tribe_camera` when it could be
/// read and is the Tribal one, the same numbers in Rust otherwise.
#[derive(Clone, Debug, Default)]
pub struct TribalScript {
    installed: Option<CameraScript>,
}

#[allow(dead_code)] // from_script / is_installed: tests
impl TribalScript {
    /// The installed `tribe_camera` (group 0x024A4F5A), or the built-in numbers.
    pub fn from_install(originals: &Originals) -> Self {
        Self::from_script(CameraScript::from_install_named(originals, "tribe_camera"))
    }

    /// Accept a script only when it behaves like the Tribal one (the shared
    /// evaluator falls back to the Civ numbers when a script fails to run).
    pub fn from_script(mut script: CameraScript) -> Self {
        if !script.is_installed() {
            return Self::default();
        }
        let probe = script.evaluate(&ScriptInputs {
            distance: tuning::DEFAULT_DISTANCE,
            ..default()
        });
        let tribal = (probe.max_pitch_deg - tuning::MAX_PITCH_DEG).abs() < 1e-3
            && (probe.min_pitch_deg - tuning::MIN_PITCH_DEG).abs() < 1e-3;
        if tribal {
            Self {
                installed: Some(script),
            }
        } else {
            Self::default()
        }
    }

    pub fn is_installed(&self) -> bool {
        self.installed.is_some()
    }

    pub fn evaluate(&mut self, input: &ScriptInputs) -> ScriptOutputs {
        match self.installed.as_mut() {
            Some(script) => script.evaluate(input),
            None => builtin(input),
        }
    }
}

// ---------------------------------------------------------------------------
// The camera rig (pure)
// ---------------------------------------------------------------------------

/// A free orbit camera over a ground point. Angles follow the convention of
/// the Creature rig: the camera sits at
/// `look + (sin(az) cos(p), sin(p), cos(az) cos(p)) * distance`, so azimuth 0
/// looks toward -Z and positive azimuth moves the camera to its right.
///
/// The channels are the retail ones (0xB11930): the `goal_*` fields are what the
/// input sets, the eased fields (`azimuth`, `pitch`, `distance`, `focus`) are
/// what a cubic Hermite [`Segment`] / [`VecSegment`] makes of them each update.
#[derive(Clone, Debug)]
pub struct TribalRig {
    /// The eased point the camera orbits (x, z on the ground; y is eased toward
    /// the terrain height).
    focus: Vec3,
    goal_focus: Vec3,
    azimuth: f32,
    pitch: f32,
    distance: f32,
    goal_azimuth: f32,
    goal_pitch: f32,
    goal_distance: f32,
    /// The interpolated channels (yaw 0.2 s, pitch 0.65 s, distance and anchor
    /// the interpolation time).
    yaw_seg: Segment,
    pitch_seg: Segment,
    distance_seg: Segment,
    /// The anchor: the unit direction under the focus on the planet, `(x, 0, z)`
    /// on the flat world.
    anchor_seg: VecSegment,
    /// The angle between the anchor and its goal at the last update (the
    /// segment time of the anchor and distance restarts interpolates with it).
    anchor_angle: f32,
    /// The last non-zero scroll (camera frame) and whether the anchor was
    /// scrolling on the last update (the coast on key release).
    last_scroll: Vec2,
    scrolling: bool,
    /// The segment time of the coast restart that is pending.
    coast: Option<f32>,
    ready: bool,
    following: bool,
    /// The planet's centre (world) when the stage is on the planet: the rig's
    /// frame then follows the local up at the focus. `None`: the flat world.
    centre: Option<Vec3>,
    /// Planet-local -> world rotation of the stage planet (`PlanetFrame`): the
    /// retail frame is built in planet-local space, where +Z is the pole.
    frame_rotation: Quat,
    script: TribalScript,
    script_checked: bool,
    last: ScriptOutputs,
    /// Seconds each movement / rotation key has been held: right, left,
    /// forward, back, rotate left (`.`), rotate right (`,`).
    held: [f32; 6],
    /// The script's `camera_scratch_var_0..4` (edge scroll dwell: right, left,
    /// forward, back), persisted between updates like the camera object does.
    scratch: [f32; 5],
    wheel: f32,
    zoom_hold: f32,
    zoom_held: f32,
}

impl Default for TribalRig {
    fn default() -> Self {
        Self::new()
    }
}

// Some accessors are only read by the tests and by the HUD helper.
#[allow(dead_code)]
impl TribalRig {
    /// The opening of the retail camera: `SnapPolar(az, 0.4, 16)` then the
    /// script loader's `default_distance` 70 as the goal distance (the camera
    /// glides out). `az` is [`start_azimuth`]: the retail 1.5 (0xCDA0ED) in the
    /// retail frame at the village ([`anchor_frame`], used by [`Self::basis`]).
    pub fn new() -> Self {
        Self::with_start_azimuth(start_azimuth())
    }

    /// [`new`](Self::new) with an explicit start azimuth (radians).
    pub fn with_start_azimuth(azimuth: f32) -> Self {
        let mut rig = Self {
            focus: Vec3::ZERO,
            goal_focus: Vec3::ZERO,
            azimuth,
            pitch: tuning::START_PITCH,
            distance: tuning::START_DISTANCE,
            goal_azimuth: azimuth,
            goal_pitch: tuning::START_PITCH,
            goal_distance: tuning::DEFAULT_DISTANCE,
            yaw_seg: Segment::at_rest(azimuth),
            pitch_seg: Segment::at_rest(tuning::START_PITCH),
            // The goal (70) differs from the snapped distance (16): the first
            // update restarts the segment, as the loader's `+0x108 = +0x10C` does.
            distance_seg: Segment::at_rest(tuning::START_DISTANCE),
            anchor_seg: VecSegment::at_rest(Vec3::ZERO),
            anchor_angle: 0.0,
            last_scroll: Vec2::ZERO,
            scrolling: false,
            coast: None,
            ready: false,
            following: false,
            centre: None,
            frame_rotation: Quat::IDENTITY,
            script: TribalScript::default(),
            script_checked: false,
            last: ScriptOutputs::default(),
            held: [0.0; 6],
            scratch: [0.0; 5],
            wheel: 0.0,
            zoom_hold: 0.0,
            zoom_held: 0.0,
        };
        rig.last = rig.script.evaluate(&rig.script_inputs(0.0, 0.0));
        rig
    }

    /// Back to the stage-start view; the focus is placed again on the next call
    /// of [`place`](Self::place). The loaded script is kept.
    pub fn restart(&mut self) {
        let (script, checked) = (self.script.clone(), self.script_checked);
        *self = Self::new();
        self.script = script;
        self.script_checked = checked;
        self.last = self.script.evaluate(&self.script_inputs(0.0, 0.0));
    }

    /// Install the Tribal camera script (or the built-in numbers).
    pub fn set_script(&mut self, script: TribalScript) {
        self.script = script;
        self.script_checked = true;
        self.last = self.script.evaluate(&self.script_inputs(0.0, 0.0));
    }

    pub fn script_checked(&self) -> bool {
        self.script_checked
    }

    pub fn script(&self) -> &TribalScript {
        &self.script
    }

    pub fn last_outputs(&self) -> &ScriptOutputs {
        &self.last
    }

    fn script_inputs(&self, direction: f32, dt: f32) -> ScriptInputs {
        ScriptInputs {
            direction,
            distance: self.goal_distance,
            pitch_deg: self.goal_pitch.to_degrees(),
            yaw_deg: self.goal_azimuth.to_degrees(),
            dt,
            ..default()
        }
    }

    /// The planet's centre (world), or `None` for the flat world.
    pub fn set_globe(&mut self, centre: Option<Vec3>) {
        let centre = centre.filter(|c| c.is_finite());
        if centre != self.centre {
            self.centre = centre;
            self.rest_anchor();
        }
    }

    /// Like [`set_globe`](Self::set_globe) with the stage planet's frame: the
    /// retail camera frame is built from the planet-local anchor direction,
    /// so the rotation to the world is needed too.
    pub fn set_planet_frame(&mut self, frame: Option<&crate::planet_query::PlanetFrame>) {
        match frame {
            Some(f) if f.centre.is_finite() && f.rotation.is_finite() => {
                self.frame_rotation = f.rotation;
                self.set_globe(Some(f.centre));
            }
            _ => self.set_globe(None),
        }
    }

    /// The rig's frame at world point `p`, in rig axes (+X right, +Y up, +Z
    /// toward the viewer at azimuth 0): the retail frame channel ([`anchor_frame`],
    /// east / north / up of the planet-local pole) with the Spore camera axes
    /// (x right, y forward, z up) mapped onto the rig's, `X = east`, `Y = up`,
    /// `Z = -north`: azimuth 0 puts the camera south of `p` looking north
    /// (0xB133E0: `frame * Rz(yaw) * Rx(-pitch) * (0, -distance, 0)`). Identity
    /// on the flat world, which has no pole.
    fn basis(&self, p: Vec3) -> Quat {
        match self.centre {
            Some(c) => {
                let up_world = (p - c).normalize_or(Vec3::Y);
                let local = self.frame_rotation.inverse() * up_world;
                let (east, north, up) = anchor_frame(local);
                let (east, north, up) = (
                    self.frame_rotation * east,
                    self.frame_rotation * north,
                    self.frame_rotation * up,
                );
                Quat::from_mat3(&Mat3::from_cols(east, up, -north)).normalize()
            }
            None => Quat::IDENTITY,
        }
    }

    /// The anchor channel's value for a world point: the unit direction from
    /// the planet's centre, or `(x, 0, z)` on the flat world.
    fn anchor_point(&self, p: Vec3) -> Vec3 {
        match self.centre {
            Some(c) => (p - c).normalize_or(Vec3::Y),
            None => Vec3::new(p.x, 0.0, p.z),
        }
    }

    /// The anchor channel stands still at the focus (a cut, the stage start).
    fn rest_anchor(&mut self) {
        self.anchor_seg = VecSegment::at_rest(self.anchor_point(self.focus));
        self.anchor_angle = 0.0;
        self.cancel_coast();
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }
    pub fn is_following(&self) -> bool {
        self.following
    }
    pub fn focus(&self) -> Vec3 {
        self.focus
    }
    pub fn goal_focus(&self) -> Vec3 {
        self.goal_focus
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
    pub fn goal_azimuth(&self) -> f32 {
        self.goal_azimuth
    }
    pub fn goal_pitch(&self) -> f32 {
        self.goal_pitch
    }
    pub fn goal_distance(&self) -> f32 {
        self.goal_distance
    }

    /// 0 at the closest ladder step, 1 at the farthest.
    pub fn zoom_fraction(&self) -> f32 {
        ((self.distance - ZOOM_MIN) / (ZOOM_MAX - ZOOM_MIN)).clamp(0.0, 1.0)
    }

    /// The camera's height over the point it looks at. ADAPTED stand-in for the
    /// retail height above the sea (`height_above_water`, 0xB13B90), which the
    /// high-altitude wind curve reads.
    pub fn altitude(&self) -> f32 {
        self.distance * self.pitch.sin()
    }

    /// Offset of the camera from the look point (eased values) in the rig's
    /// frame (+Y up).
    pub fn orbit_offset(&self) -> Vec3 {
        let horizontal = self.distance * self.pitch.cos();
        Vec3::new(
            self.azimuth.sin() * horizontal,
            self.pitch.sin() * self.distance,
            self.azimuth.cos() * horizontal,
        )
    }

    /// The ground direction the camera looks along, and the one to its right
    /// (world; tangent to the planet at the focus).
    pub fn ground_axes(&self) -> (Vec3, Vec3) {
        let forward = Vec3::new(-self.azimuth.sin(), 0.0, -self.azimuth.cos());
        let right = Vec3::new(self.azimuth.cos(), 0.0, -self.azimuth.sin());
        let basis = self.basis(self.focus);
        (basis * forward, basis * right)
    }

    /// Put the focus on `point` at once (the stage start).
    pub fn place(&mut self, point: Vec3) {
        if !point.is_finite() {
            return;
        }
        self.focus = point;
        self.goal_focus = point;
        self.ready = true;
        self.following = false;
        self.rest_anchor();
    }

    /// Move the camera to `point`: farther than `tribeCameraSkipDistance`
    /// (16) from where it looks now it cuts there, nearer it glides. Returns
    /// true when it cut.
    pub fn go_to(&mut self, point: Vec3) -> bool {
        if !point.is_finite() {
            return false;
        }
        let skip = match self.centre {
            Some(_) => self.focus.distance(point) > tuning::CAMERA_SKIP_DISTANCE,
            None => dist_xz(self.focus, point) > tuning::CAMERA_SKIP_DISTANCE,
        };
        self.goal_focus = point;
        if skip {
            if self.centre.is_some() {
                self.focus = point;
            } else {
                self.focus.x = point.x;
                self.focus.z = point.z;
            }
            self.rest_anchor();
        }
        self.cancel_coast();
        self.following = false;
        skip
    }

    /// Track a moving point (a followed member): the goal anchor is the point
    /// and the eased anchor chases it on the retail segments (the goal moves
    /// every update, so the chase is velocity-continuous).
    pub fn follow_point(&mut self, point: Vec3) {
        if point.is_finite() {
            self.goal_focus = point;
            self.following = true;
            self.cancel_coast();
        }
    }

    /// Somebody moved the goal anchor by hand (a jump, a follow): a pan coast
    /// that was about to start does not apply.
    pub fn cancel_coast(&mut self) {
        self.scrolling = false;
        self.coast = None;
    }

    /// The goal anchor `delta` (a ground displacement) ahead of the CURRENT
    /// anchor, not ahead of the old goal (0xB14800 restarts the anchor segment
    /// from the current anchor on every update that scrolls); on the planet the
    /// goal slides over the sphere (no edge anywhere).
    pub fn pan_world(&mut self, delta: Vec3) {
        if delta.is_finite() && delta != Vec3::ZERO {
            self.goal_focus = match self.centre {
                Some(c) => {
                    let r = (self.focus - c).length();
                    c + (self.focus + delta - c).normalize_or(Vec3::Y) * r
                }
                None => Vec3::new(
                    self.focus.x + delta.x,
                    self.goal_focus.y,
                    self.focus.z + delta.z,
                ),
            };
            self.following = false;
        }
    }

    /// The anchor speed in radians a second on the unit sphere (the retail
    /// `|+0x78|`); on the flat world the speed over [`FLAT_RADIUS`] (ADAPTED).
    pub fn anchor_speed(&self) -> f32 {
        let speed = self.anchor_seg.vel.length();
        if self.centre.is_some() {
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

    /// The pitch limits of the script, in radians.
    fn pitch_limits(&self) -> (f32, f32) {
        (
            self.last.min_pitch_deg.to_radians(),
            self.last.max_pitch_deg.to_radians(),
        )
    }

    fn tilt_by(&mut self, radians: f32) {
        let (low, high) = self.pitch_limits();
        self.goal_pitch = (self.goal_pitch + radians).clamp(low, high);
    }

    /// PageUp (positive) lowers the camera's elevation, PageDown raises it,
    /// held for `dt` seconds: `sens_y * 10 degrees * 5` a second, not ramped
    /// (0xB109F0, triggers 7 and 8).
    pub fn tilt_keys(&mut self, direction: f32, dt: f32) {
        if !direction.is_finite() || !dt.is_finite() {
            return;
        }
        let rate = (self.last.mouse_rotate_y * PITCH_KEY_DEGREES_PER_UNIT).to_radians();
        self.tilt_by(-direction * rate * dt.max(0.0));
    }

    /// Mouse drag with both buttons (or the middle one), in pixels, as one
    /// event at the nominal update rate: `yaw goal = yaw - sens_x * dx`,
    /// `pitch goal = pitch + sens_y * dy` from the CURRENT angles (0xB0FDF0).
    pub fn orbit_by_pixels(&mut self, delta: Vec2) {
        self.orbit(delta, NOMINAL_UPDATE_SECONDS);
    }

    /// [`orbit_by_pixels`](Self::orbit_by_pixels) for a frame of `dt` seconds.
    /// ADAPTED: the retail offset is per mouse event (one per update); a frame
    /// that is not the nominal 60 Hz scales the offset so the speed holds.
    fn orbit(&mut self, delta: Vec2, dt: f32) {
        if !delta.is_finite() {
            return;
        }
        let rate = if dt > 0.0 {
            (NOMINAL_UPDATE_SECONDS / dt).clamp(0.25, 4.0)
        } else {
            1.0
        };
        let dx = (delta.x * rate).clamp(-MOUSE_ORBIT_CLAMP, MOUSE_ORBIT_CLAMP);
        let dy = (delta.y * rate).clamp(-MOUSE_ORBIT_CLAMP, MOUSE_ORBIT_CLAMP);
        if dx != 0.0 {
            let across = (self.last.mouse_rotate_x * MOUSE_X_DEGREES_PER_UNIT).to_radians();
            self.goal_azimuth = self.azimuth - across * dx;
        }
        if dy != 0.0 {
            let up = (self.last.mouse_rotate_y * MOUSE_Y_DEGREES_PER_UNIT).to_radians();
            let (low, high) = self.pitch_limits();
            self.goal_pitch = (self.pitch + up * dy).clamp(low, high);
        }
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
            let (low, high) = self.pitch_limits();
            self.goal_pitch =
                (self.pitch + self.pitch_seg.vel * interp::ORBIT_COAST * 0.5).clamp(low, high);
            self.pitch_seg.restart(self.goal_pitch, interp::ORBIT_COAST);
        }
    }

    /// One retail zoom event (0xB14800 with a direction): the script snaps the
    /// goal distance to the next ladder step. Positive zooms in.
    pub fn zoom_step(&mut self, direction: i32) {
        if direction == 0 {
            return;
        }
        let out = self
            .script
            .evaluate(&self.script_inputs(direction.signum() as f32, 0.0));
        self.last = out;
        if out.distance.is_finite() {
            self.goal_distance = out.distance.max(0.1);
        }
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

    /// Zoom key events of one frame: one event per camera update while the key
    /// is held (0xB14800; the first update after the press is the first event).
    /// `direction` is the held zoom direction (+1 in, -1 out, 0 none); the
    /// updates of a frame are counted from `dt` at [`UPDATES_PER_SECOND`] (the
    /// fraction left over carries to the next frame). Returns the signed
    /// number of events.
    pub fn zoom_key_events(&mut self, direction: f32, dt: f32) -> i32 {
        let dir = if direction > 0.0 {
            1
        } else if direction < 0.0 {
            -1
        } else {
            0
        };
        if dir == 0 || !dt.is_finite() {
            self.zoom_hold = 0.0;
            self.zoom_held = 0.0;
            return 0;
        }
        if self.zoom_held != dir as f32 {
            // A press (or a turn to the other key): the update that sees it
            // raises the first event.
            self.zoom_held = dir as f32;
            self.zoom_hold = 0.0;
            return dir;
        }
        self.zoom_hold += dt.max(0.0) * UPDATES_PER_SECOND;
        let updates = self.zoom_hold.floor();
        self.zoom_hold -= updates;
        dir * updates.min(4.0) as i32
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

    /// Keep both azimuths in a bounded range without a visible jump.
    fn unwind_azimuth(&mut self) {
        if self.goal_azimuth.abs() > TAU * 2.0 {
            let turn = (self.goal_azimuth / TAU).trunc() * TAU;
            self.goal_azimuth -= turn;
            self.azimuth -= turn;
            self.yaw_seg.shift(-turn);
        }
    }

    /// Step the eased values toward their goals: the channels of 0xB11930 (yaw
    /// 0.2 s, pitch 0.65 s, distance and anchor the interpolation time).
    pub fn ease(&mut self, dt: f32) {
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.1) } else { 0.0 };
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
        if let Some(c) = self.centre {
            // Over the sphere: the direction travels on the unit sphere, the
            // height stays (the pose eases it toward the ground).
            let goal = (self.goal_focus - c).normalize_or(Vec3::Y);
            let current = (self.focus - c).normalize_or(goal);
            let radius = (self.focus - c).length();
            self.anchor_angle = current.dot(goal).clamp(-1.0, 1.0).acos();
            let dur = self.restart_duration(self.anchor_seg.changed(goal, 1.0e-6));
            let dir = self.anchor_seg.step(current, goal, dt, dur, true, 1.0e-6);
            self.focus = c + dir * radius;
        } else {
            let goal = Vec3::new(self.goal_focus.x, 0.0, self.goal_focus.z);
            let current = Vec3::new(self.focus.x, 0.0, self.focus.z);
            // ADAPTED: the flat world has no radius; the angle uses the
            // planet radius of the Civ port.
            self.anchor_angle = (goal - current).length() / FLAT_RADIUS;
            let dur = self.restart_duration(self.anchor_seg.changed(goal, 1.0e-4));
            let at = self.anchor_seg.step(current, goal, dt, dur, false, 1.0e-4);
            self.focus.x = at.x;
            self.focus.z = at.z;
        }
    }

    /// The camera transform for this frame on the planet: the focus eases to
    /// the surface (`ToSurface`: ground, or the sea over water) along its
    /// local up, the orbit offset turns with the local frame, and the camera
    /// never dips under the surface below it.
    pub fn pose_on(&mut self, dt: f32, planet: &crate::planet_query::StagePlanet) -> Transform {
        let c = planet.frame.centre;
        self.set_planet_frame(Some(&planet.frame));
        self.ease(dt);
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let up = (self.focus - c).normalize_or(Vec3::Y);
        let ground = (planet.to_surface_world(c + up * planet.planet.radius) - c).length();
        let mut r = (self.focus - c).length();
        r += (ground - r) * (1.0 - (-dt * 6.0).exp());
        if !r.is_finite() || (r - ground).abs() > 200.0 {
            r = ground;
        }
        self.focus = c + up * r;
        let look = self.focus + up * LOOK_HEIGHT;
        let mut position = look + self.basis(self.focus) * self.orbit_offset();
        let floor = (planet.to_surface_world(position) - c).length() + ABOVE_TERRAIN;
        if (position - c).length() < floor {
            position = c + (position - c).normalize_or(up) * floor;
        }
        Transform::from_translation(position).looking_at(look, up)
    }

    /// The camera transform for this frame. `terrain` returns the ground height
    /// under a point when there is a land surface.
    pub fn pose(&mut self, dt: f32, terrain: impl Fn(f32, f32) -> Option<f32>) -> Transform {
        self.ease(dt);
        let ground = terrain(self.focus.x, self.focus.z).unwrap_or(0.0);
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        // The look height follows the terrain more slowly than the focus.
        self.focus.y += (ground - self.focus.y) * (1.0 - (-dt * 6.0).exp());
        if !self.ready {
            self.focus.y = ground;
        }
        let look = Vec3::new(self.focus.x, self.focus.y + LOOK_HEIGHT, self.focus.z);
        let mut position = look + self.orbit_offset();
        if let Some(floor) = terrain(position.x, position.z) {
            position.y = position.y.max(floor + ABOVE_TERRAIN);
        }
        Transform::from_translation(position).looking_at(look, Vec3::Y)
    }
}

// ---------------------------------------------------------------------------
// Input (pure readers)
// ---------------------------------------------------------------------------

/// What the keyboard asks of the camera this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KeyInput {
    /// `x` right, `y` forward, each -1..1.
    pub pan: Vec2,
    /// +1 rotate left (`.` / Shift+Left: the yaw goal grows), -1 rotate right
    /// (`,` / Shift+Right), triggers 0x12 / 0x11.
    pub rotate: f32,
    /// +1 pitch up (PageUp / numpad 9 / Shift+Up: the elevation falls), -1
    /// pitch down (trigger 7 / 8).
    pub tilt: f32,
    /// +1 zoom in (`=` `+` numpad+), -1 zoom out (`-` numpad-).
    pub zoom: f32,
}

fn axis(positive: bool, negative: bool) -> f32 {
    match (positive, negative) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        _ => 0.0,
    }
}

/// Read the camera keys of `TriggerConfigTribeGame` and `TriggerConfigWASD`.
/// Shift turns the arrows into the rotate / pitch keys.
pub fn read_keys(keys: &ButtonInput<KeyCode>) -> KeyInput {
    let any = |list: &[KeyCode]| list.iter().any(|key| keys.pressed(*key));
    let shift = any(&[KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let arrows = |key: KeyCode| !shift && keys.pressed(key);
    let forward = any(&[KeyCode::KeyW, KeyCode::Numpad8]) || arrows(KeyCode::ArrowUp);
    let back = any(&[KeyCode::KeyS, KeyCode::Numpad2]) || arrows(KeyCode::ArrowDown);
    let left = any(&[KeyCode::KeyA, KeyCode::Numpad4, KeyCode::KeyQ]) || arrows(KeyCode::ArrowLeft);
    let right = any(&[KeyCode::KeyD, KeyCode::Numpad6, KeyCode::KeyE]) || arrows(KeyCode::ArrowRight);
    let rotate_right = keys.pressed(KeyCode::Comma) || (shift && keys.pressed(KeyCode::ArrowRight));
    let rotate_left = keys.pressed(KeyCode::Period) || (shift && keys.pressed(KeyCode::ArrowLeft));
    let pitch_up = any(&[KeyCode::PageUp, KeyCode::Numpad9]) || (shift && keys.pressed(KeyCode::ArrowUp));
    let pitch_down =
        any(&[KeyCode::PageDown, KeyCode::Numpad3]) || (shift && keys.pressed(KeyCode::ArrowDown));
    let zoom_in = any(&[KeyCode::Equal, KeyCode::NumpadAdd]);
    let zoom_out = any(&[KeyCode::Minus, KeyCode::NumpadSubtract]);
    KeyInput {
        pan: Vec2::new(axis(right, left), axis(forward, back)),
        rotate: axis(rotate_left, rotate_right),
        tilt: axis(pitch_up, pitch_down),
        zoom: axis(zoom_in, zoom_out),
    }
}

/// The four hold buttons of the HUD camera cluster (retail ids 0x04CAB570 zoom
/// in, 0x04CAB571 zoom out, 0x04CAB56E and 0x04CAB56F rotate; the retail code
/// raises a flag while each is held: camera +0x14 / +0x15 zoom, +0x16 / +0x17
/// rotate, read by 0xB14800 like the triggers 9, 0x10, 0x12, 0x11).
/// VERIFIED which button turns which way: 0xCC9B30 sets +0x16 from 0x04CAB56E
/// and +0x17 from 0x04CAB56F; 0xB14800 reads +0x16 together with trigger 0x12
/// (`.`, the yaw goal grows) and +0x17 with trigger 0x11 (`,`, it shrinks). The
/// fields below keep the order of `tribal_hud::CAMERA_BUTTON_IDS` (`rotate_left`
/// = 0x04CAB56F, `rotate_right` = 0x04CAB56E), so `rotate_right` is the `.` key's
/// direction ([`HudCamera::as_keys`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HudCamera {
    pub zoom_in: bool,
    pub zoom_out: bool,
    pub rotate_left: bool,
    pub rotate_right: bool,
}

impl HudCamera {
    pub fn any(&self) -> bool {
        self.zoom_in || self.zoom_out || self.rotate_left || self.rotate_right
    }

    fn as_keys(&self) -> KeyInput {
        KeyInput {
            pan: Vec2::ZERO,
            // 0x04CAB56E (`rotate_right` here) acts as `.` (+), 0x04CAB56F as `,` (-).
            rotate: axis(self.rotate_right, self.rotate_left),
            tilt: 0.0,
            zoom: axis(self.zoom_in, self.zoom_out),
        }
    }
}

/// What the mouse asks of the camera this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseInput {
    /// Motion of a rotate / pitch drag.
    pub orbit_pixels: Vec2,
    /// The orbit button (middle, or left + right) came up this frame: yaw and
    /// pitch coast on ([`TribalRig::end_orbit`], 0xB10BA0).
    pub orbit_end: bool,
    /// Drag scroll: the joystick deflection of the held right button, `-1..1`
    /// per axis (`x` right, `y` forward), squared with its sign
    /// ([`drag_scroll_vector`]).
    pub drag_scroll: Vec2,
    /// Wheel notches (positive zooms in).
    pub wheel_lines: f32,
    /// The script's `edge_distance_x / y` ([`edge_distance`]: the pixel distance
    /// of the cursor from the nearest border, signed, plus 0.01; zero when the
    /// cursor is off the window or edge scroll is off).
    pub edge: Vec2,
}

/// The mouse buttons' state between frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gesture {
    right_travel: f32,
    right_over_ui: bool,
    right_origin: Vec2,
    orbiting: bool,
}

impl Gesture {
    /// Resolve the buttons into a drag: both buttons (or the middle one) orbit,
    /// the right button alone scrolls like a joystick once it moved past the
    /// click slop (a still press is a context click, which the command code
    /// handles). `cursor` and `window` are the pointer and the window in
    /// pixels (retail 0xB14800: the offset of the cursor from where the button
    /// went down, over half the window).
    #[allow(clippy::too_many_arguments)]
    pub fn drag(
        &mut self,
        left: bool,
        right: bool,
        middle: bool,
        right_just_pressed: bool,
        over_ui: bool,
        delta: Vec2,
        cursor: Vec2,
        window: Vec2,
    ) -> MouseInput {
        self.drag_with(left, right, middle, right_just_pressed, over_ui, delta, cursor, window, false)
    }

    /// [`drag`](Self::drag) with the Tribe control option
    /// (`OptionTribeControls`, `control_options::tribe_rotate_on_right`):
    /// `UIMachineTribeRotate` sends the right-button drag to the camera's
    /// rotation (`TIMRotateCamera`) once the cursor travelled
    /// `LONG_MOUSE_MOVE` pixels, where `UIMachineLeftRightTranslate` (off)
    /// scrolls with it (`TIMDragCamera`).
    #[allow(clippy::too_many_arguments)]
    pub fn drag_with(
        &mut self,
        left: bool,
        right: bool,
        middle: bool,
        right_just_pressed: bool,
        over_ui: bool,
        delta: Vec2,
        cursor: Vec2,
        window: Vec2,
        rotate_on_right: bool,
    ) -> MouseInput {
        let mut out = MouseInput::default();
        if !delta.is_finite() {
            return out;
        }
        if right_just_pressed {
            self.right_travel = 0.0;
            self.right_over_ui = over_ui;
            self.right_origin = if cursor.is_finite() { cursor } else { Vec2::ZERO };
        }
        if !right {
            self.right_travel = 0.0;
            self.right_over_ui = false;
        }
        let chord = (left && right) || middle;
        let mut rotate_drag = false;
        if rotate_on_right && right && !chord && !self.right_over_ui {
            self.right_travel += delta.length();
            rotate_drag = self.right_travel > crate::control_options::LONG_MOUSE_MOVE;
        }
        let orbiting = chord || rotate_drag;
        out.orbit_end = self.orbiting && !orbiting;
        self.orbiting = orbiting;
        if orbiting {
            out.orbit_pixels = delta;
            // The press became a rotate: it can no longer be a click or pan.
            self.right_travel = f32::INFINITY;
        } else if right && !self.right_over_ui && !rotate_on_right {
            self.right_travel += delta.length();
            if self.right_travel > DRAG_SLOP && cursor.is_finite() && window.is_finite() {
                out.drag_scroll = drag_scroll_vector(self.right_origin, cursor, window);
            }
        }
        out
    }

    /// True while a camera gesture owns the mouse (the selection and command
    /// code must ignore it).
    pub fn owns_mouse(&self, left: bool, right: bool, middle: bool) -> bool {
        (left && right) || middle || (right && self.right_travel > DRAG_SLOP)
    }
}

impl TribalRig {
    /// Apply one frame of input. Returns true when the player moved the focus
    /// by hand (which ends a follow).
    ///
    /// Retail 0xB14800, per update: the held movement keys ramp as a smoothstep
    /// over `keyboard_acceleration_time` (0.7 s for the Tribal camera), times
    /// `keyboard_translation_speed` (10); the script turns that into the scroll
    /// (`* distance / 25`) and the rotation (`* 5` degrees). The scroll (keys,
    /// the drag joystick x `mouse_scroll_sensitivity`, the edge scroll's
    /// `anchor_delta x 1500 x dt`) puts the GOAL anchor that far ahead of the
    /// CURRENT anchor on every update that scrolls; the anchor segment chases it
    /// ([`ease`](Self::ease)). The first update without scroll after a scroll
    /// sets one more goal along the last direction, `|anchor speed| x
    /// min_interpolation_time x 250` units ahead (the key release coast). The
    /// rotation keys move the yaw goal, the pitch keys the pitch goal.
    pub fn apply(&mut self, keys: KeyInput, mouse: MouseInput, dt: f32) -> bool {
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.1) } else { 0.0 };
        // Held-time counters of the ramped keys.
        let pressed = [
            keys.pan.x > 0.0,
            keys.pan.x < 0.0,
            keys.pan.y > 0.0,
            keys.pan.y < 0.0,
            keys.rotate > 0.0,
            keys.rotate < 0.0,
        ];
        for (held, on) in self.held.iter_mut().zip(pressed) {
            *held = if on { *held + dt } else { 0.0 };
        }
        // Zoom events: the held key and the wheel, one ladder step each.
        let events = self.zoom_key_events(keys.zoom, dt) + self.take_wheel_notches(mouse.wheel_lines);
        for _ in 0..events.unsigned_abs().min(4) {
            self.zoom_step(events.signum());
        }
        // A frame without time is not an update (nothing moves).
        if dt <= 0.0 {
            return false;
        }
        // The script turns the ramped keys into scroll and rotate amounts, and
        // the cursor at the window border into the edge scroll anchor delta.
        let accel = self.last.acceleration_time;
        let ramps = self.held.map(|t| ramp(t, accel));
        let speed = self.last.translation_speed;
        let mut inputs = self.script_inputs(0.0, dt);
        inputs.scroll = Vec2::new(ramps[0] - ramps[1], ramps[2] - ramps[3]) * speed;
        inputs.rotate = ramps[4] - ramps[5];
        inputs.edge = if mouse.edge.is_finite() { mouse.edge } else { Vec2::ZERO };
        inputs.scratch = self.scratch;
        let out = self.script.evaluate(&inputs);
        self.last = out;
        self.scratch = out.scratch;

        // Scroll per update (x right, y forward; 0xB14800): the keys (the
        // script negates its input), the drag scroll joystick, and the edge
        // scroll anchor delta times 1500 per second.
        // ADAPTED: the edge scroll time step is the nominal update, so the
        // speed does not ride the frame rate (retail multiplies by its own dt).
        let keys_scroll = -out.scroll;
        let drag = if mouse.drag_scroll.is_finite() {
            mouse.drag_scroll * out.mouse_scroll_sensitivity
        } else {
            Vec2::ZERO
        };
        let edge = Vec2::new(-out.anchor_delta.x, out.anchor_delta.y)
            * (interp::EDGE_SCROLL_SPEED * NOMINAL_UPDATE_SECONDS);
        let scroll = keys_scroll + drag + edge;
        let (forward, right) = self.ground_axes();
        let to_world = |v: Vec2| right * v.x + forward * v.y;
        let mut by_hand = false;
        if scroll.is_finite() && scroll != Vec2::ZERO {
            self.last_scroll = scroll;
            self.scrolling = true;
            self.coast = None;
            self.pan_world(to_world(scroll));
            by_hand = true;
        } else if self.scrolling {
            // The pan keys came up: the anchor coasts on along the last
            // direction by `|velocity| x min_interp x 250` (retail `|velocity|`
            // is the anchor speed in radians a second on the unit sphere).
            // ADAPTED: the retail factor is a function-local static set on the
            // first update ever (0.7 when Tribal ran first); this stage's own
            // `min_interpolation_time` is used (the same 0.7).
            self.scrolling = false;
            let reach = self.anchor_speed() * out.min_interpolation * interp::COAST_SPEED_SCALE;
            if reach > 1.0e-6 {
                self.coast = Some(out.min_interpolation);
            }
            self.pan_world(to_world(self.last_scroll.normalize_or_zero() * reach));
        }

        // The orbit button came up: yaw and pitch coast on (0xB10BA0).
        if mouse.orbit_end {
            self.end_orbit();
        }
        // Rotation keys (per update, ADAPTED update rate) and pitch keys.
        self.goal_azimuth += out.rotate_deg.to_radians() * UPDATES_PER_SECOND * dt;
        if keys.tilt != 0.0 {
            self.tilt_keys(keys.tilt, dt);
        }
        // Mouse orbit (middle or left + right button).
        if mouse.orbit_pixels != Vec2::ZERO {
            self.orbit(mouse.orbit_pixels, dt);
        }
        let (low, high) = self.pitch_limits();
        self.goal_pitch = self.goal_pitch.clamp(low, high);
        self.unwind_azimuth();
        by_hand
    }
}

// ---------------------------------------------------------------------------
// Commands: the citizen actions and the context rule
// ---------------------------------------------------------------------------

/// `Simulator::CitizenAction` (ModAPI SimulatorEnums.h): what a tribe member
/// can be told to do. `cTribeInputStrategy::DoActionForAllSelected` hands one
/// of these to every selected member through `cCreatureCitizen::DoAction`.
/// Not every id is issued by a right click yet (the enum is the original's).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CitizenAction {
    Eat,
    Fish,
    Gather,
    GrabTool,
    WalkTo,
    Heal,
    Sleep,
    Attack,
    RaidFood,
    GiftFood,
    Hunt,
    Mate,
    Party,
    CollectEgg,
    Bundle,
    RaidHuts,
    FeedWild,
    GiftMember,
    Repair,
    Tame,
    Recruit,
    GatherMeat,
    ReturnToTribe,
}

#[allow(dead_code)]
impl CitizenAction {
    pub const ALL: [CitizenAction; 23] = [
        CitizenAction::Eat,
        CitizenAction::Fish,
        CitizenAction::Gather,
        CitizenAction::GrabTool,
        CitizenAction::WalkTo,
        CitizenAction::Heal,
        CitizenAction::Sleep,
        CitizenAction::Attack,
        CitizenAction::RaidFood,
        CitizenAction::GiftFood,
        CitizenAction::Hunt,
        CitizenAction::Mate,
        CitizenAction::Party,
        CitizenAction::CollectEgg,
        CitizenAction::Bundle,
        CitizenAction::RaidHuts,
        CitizenAction::FeedWild,
        CitizenAction::GiftMember,
        CitizenAction::Repair,
        CitizenAction::Tame,
        CitizenAction::Recruit,
        CitizenAction::GatherMeat,
        CitizenAction::ReturnToTribe,
    ];

    /// The original's id.
    pub const fn id(self) -> i32 {
        match self {
            CitizenAction::Eat => 0,
            CitizenAction::Fish => 1,
            CitizenAction::Gather => 2,
            CitizenAction::GrabTool => 3,
            CitizenAction::WalkTo => 4,
            CitizenAction::Heal => 5,
            CitizenAction::Sleep => 6,
            CitizenAction::Attack => 7,
            CitizenAction::RaidFood => 8,
            CitizenAction::GiftFood => 9,
            CitizenAction::Hunt => 10,
            CitizenAction::Mate => 11,
            CitizenAction::Party => 12,
            CitizenAction::CollectEgg => 13,
            CitizenAction::Bundle => 14,
            CitizenAction::RaidHuts => 15,
            CitizenAction::FeedWild => 20,
            CitizenAction::GiftMember => 21,
            CitizenAction::Repair => 24,
            CitizenAction::Tame => 25,
            CitizenAction::Recruit => 26,
            CitizenAction::GatherMeat => 27,
            CitizenAction::ReturnToTribe => 28,
        }
    }

    /// The sound `cTribeInputStrategy` plays for this action (the table of
    /// `PlayAudio` calls at 0xCCFEBA-0xCD003A, notes T7); none for the walk,
    /// tool grab, sleep and return actions.
    pub const fn sound(self) -> Option<&'static str> {
        Some(match self {
            CitizenAction::Eat => "sfx_trg_action_eat",
            CitizenAction::Fish => "sfx_trg_action_fish",
            CitizenAction::Gather => "sfx_trg_action_gather",
            CitizenAction::Heal => "sfx_trg_action_heal",
            CitizenAction::Attack => "sfx_trg_action_attack",
            CitizenAction::RaidFood | CitizenAction::RaidHuts => "sfx_trg_action_raid",
            CitizenAction::GiftFood | CitizenAction::GiftMember => "sfx_trg_action_gift",
            CitizenAction::Hunt => "sfx_trg_action_hunt",
            CitizenAction::Mate => "sfx_trg_action_mate",
            CitizenAction::Party => "sfx_trg_action_party",
            CitizenAction::CollectEgg => "sfx_trg_action_collect_egg",
            CitizenAction::Bundle => "sfx_trg_action_bundle",
            CitizenAction::FeedWild => "sfx_trg_action_feed_wild",
            CitizenAction::Repair => "sfx_trg_action_repair",
            CitizenAction::Tame => "sfx_trg_action_tame",
            CitizenAction::Recruit => "sfx_trg_action_recruit",
            CitizenAction::GatherMeat => "sfx_trg_action_gather_meat",
            CitizenAction::GrabTool
            | CitizenAction::WalkTo
            | CitizenAction::Sleep
            | CitizenAction::ReturnToTribe => return None,
        })
    }
}

/// `Simulator::TribeInputAction`: what hovering a tool shack offers
/// (`ChooseInputActionID`). Grab the tool, repair it, or dance at the fire pit.
#[allow(dead_code)]
pub fn tribe_input_action(kind: ToolKind) -> i32 {
    match kind {
        ToolKind::SpikedClubs => 19,
        ToolKind::StoneAxes => 20,
        ToolKind::ThrowingSpears => 21,
        ToolKind::WoodenHorns => 22,
        ToolKind::Maracas => 23,
        ToolKind::Aerophones => 24,
        ToolKind::HealingRods => 25,
        ToolKind::Campfire => 26,
        // The gather and fish racks have no input action of their own in the
        // enum (the original lists only the attack, social, heal and fire
        // tools): they use the generic grab.
        ToolKind::FishingSpears | ToolKind::FruitSticks => -1,
    }
}

/// A right click, resolved: the one action the selected members are told to do
/// and the OpenSpore order that carries it out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextCommand {
    pub action: CitizenAction,
    pub order: Order,
}

/// Why a social command was refused.
pub const NEED_CHIEFTAIN: &str =
    "Select your chieftain to perform social interactions with other tribes.";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// One command given this frame (the audio module plays its sound).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Issued {
    pub action: CitizenAction,
    /// Members that accepted it.
    pub count: usize,
    pub point: Vec3,
}

/// What the HUD reads and writes.
#[derive(Resource, Debug, Clone, Default)]
pub struct TribalControls {
    /// The member the camera follows (Space).
    pub follow: Option<Entity>,
    /// Edge Scroll (a setting in the original; off by default here).
    pub edge_scroll: bool,
    /// The commands given this frame, newest last (cleared every frame).
    pub issued: Vec<Issued>,
    /// A social order is under way: press [`KEY_PERFORM`] when the chieftain
    /// stands at this village.
    pub pending_perform: Option<usize>,
    pending_wait: f32,
    pending_retry: f32,
    /// Held state of the HUD camera buttons (the HUD sets it every frame).
    pub hud: HudCamera,
    /// Minimap left click: the world point to move the camera to.
    pub goto_request: Option<Vec3>,
    /// Minimap right click: the world point to send the selection to.
    pub move_request: Option<Vec3>,
    /// Minimap left clicks taken so far (the tutorial's minimap step, retail
    /// 0xE0BBF9).
    pub minimap_gotos: u32,
}

impl TribalControls {
    /// Record a command the selection accepted.
    pub fn issue(&mut self, command: ContextCommand, count: usize, point: Vec3) {
        self.issued.push(Issued {
            action: command.action,
            count,
            point,
        });
        match command.order {
            Order::Perform(village) => {
                self.pending_perform = Some(village);
                self.pending_wait = 0.0;
                self.pending_retry = 0.0;
            }
            _ => self.pending_perform = None,
        }
    }
}

/// The camera: the rig plus the mouse gesture state.
#[derive(Resource, Debug, Clone, Default)]
pub struct TribalCamera {
    pub rig: TribalRig,
    gesture: Gesture,
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Where the camera looks when the stage starts: the middle of the village,
/// between the hut and the food mat (the earlier camera looked at the
/// chieftain, who stood a few units off).
pub fn start_focus() -> Vec3 {
    (tribal::home_position() + tribal::food_pile_position()) * 0.5
}

/// The next member to follow: chieftain first, then by id; wraps around.
pub fn cycle_follow(order: &[Entity], current: Option<Entity>) -> Option<Entity> {
    if order.is_empty() {
        return None;
    }
    let next = current
        .and_then(|e| order.iter().position(|o| *o == e))
        .map_or(0, |i| (i + 1) % order.len());
    Some(order[next])
}

fn press_virtual(
    keys: &mut ButtonInput<KeyCode>,
    injected: &mut crate::player_hud::InjectedKeys,
    key: KeyCode,
) {
    if !keys.pressed(key) {
        keys.press(key);
        injected.push(key);
    }
}

/// Keyboard: Ctrl+A, the stance keys, Esc, Space.
#[allow(clippy::too_many_arguments)]
fn keyboard(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<crate::player_hud::InjectedKeys>,
    mut controls: ResMut<TribalControls>,
    mut run: ResMut<RunProgress>,
    mut commands: Commands,
    selected: Query<Entity, With<Selected>>,
    units: Query<(Entity, &TribeUnit, Option<&crate::persistence::WorldId>)>,
) {
    controls.issued.clear();
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if ctrl && keys.just_pressed(KeyCode::KeyA) {
        press_virtual(&mut keys, &mut injected, KEY_SELECT_TRIBE);
    }
    let stance = if keys.just_pressed(KEY_STANCE_SOCIAL) {
        Some(true)
    } else if keys.just_pressed(KEY_STANCE_ATTACK) {
        Some(false)
    } else {
        None
    };
    if let Some(social) = stance {
        if run.tribal_campaign.friendly_stance != social {
            run.tribal_campaign.friendly_stance = social;
            run.tribal_campaign.say(if social {
                "Social stance: right-click a wild grazer to tame it, or a tribe to perform."
            } else {
                "Attack stance: right-click creatures to hunt them, or a tribe to raid it."
            });
            run.request_save();
        }
    }
    if keys.just_pressed(KEY_EDGE_SCROLL) {
        controls.edge_scroll = !controls.edge_scroll;
        run.tribal_campaign.say(if controls.edge_scroll {
            "Edge Scroll on: move the cursor to the edge of the screen to pan."
        } else {
            "Edge Scroll off."
        });
    }
    if keys.just_pressed(KeyCode::Escape) && !selected.is_empty() {
        for entity in &selected {
            commands.entity(entity).remove::<Selected>();
        }
        // The press deselects; it does not also open the pause menu.
        keys.clear_just_pressed(KeyCode::Escape);
    }
    if keys.just_pressed(KeyCode::Space) {
        let mut order: Vec<(bool, u32, Entity)> = units
            .iter()
            .map(|(e, unit, id)| (!unit.chieftain, id.map_or(0, |id| id.0), e))
            .collect();
        order.sort();
        let order: Vec<Entity> = order.into_iter().map(|(_, _, e)| e).collect();
        controls.follow = cycle_follow(&order, controls.follow);
    }
}

/// Camera: keys, mouse, HUD buttons, edge scroll, the minimap's goto.
#[allow(clippy::too_many_arguments)]
fn camera_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    pixels_per_line: Option<Res<MouseScrollPixelsPerLine>>,
    windows: Query<&Window>,
    spui: Option<Res<crate::spui::view::SpuiPointer>>,
    mut camera: ResMut<TribalCamera>,
    mut controls: ResMut<TribalControls>,
    rts: Option<ResMut<TribalRts>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
) {
    let dt = time.delta_secs();
    let camera = &mut *camera;
    // Chart points (the start, minimap clicks) stand on the surface: on the
    // planet their height decides their direction from the centre.
    let lift = |p: Vec3| match surface.as_deref() {
        Some(s) if s.planet().is_some() && p.is_finite() => Vec3::new(p.x, s.height(p.x, p.z), p.z),
        _ => p,
    };
    if !camera.rig.is_ready() {
        camera.rig.place(lift(start_focus()));
    }
    let over_ui = spui.as_deref().is_some_and(|pointer| pointer.over_ui);
    let window = windows.iter().find(|w| w.cursor_position().is_some());
    let window_size = window.map(|w| Vec2::new(w.width(), w.height()));
    let cursor = window.and_then(|w| w.cursor_position());
    let (left, right, middle, right_pressed) = mouse.as_deref().map_or(
        (false, false, false, false),
        |m| {
            (
                m.pressed(MouseButton::Left),
                m.pressed(MouseButton::Right),
                m.pressed(MouseButton::Middle),
                m.just_pressed(MouseButton::Right),
            )
        },
    );
    let delta = motion.map_or(Vec2::ZERO, |m| m.delta);
    let mut input = camera.gesture.drag_with(
        left,
        right,
        middle,
        right_pressed,
        over_ui,
        delta,
        cursor.unwrap_or(Vec2::NAN),
        window_size.unwrap_or(Vec2::NAN),
        crate::control_options::tribe_rotate_on_right(),
    );
    // A camera gesture owns the mouse: the selection box and the click the
    // command code is tracking must not complete when the buttons come up.
    if camera.gesture.owns_mouse(left, right, middle) {
        if let Some(mut rts) = rts {
            rts.left_press = None;
            rts.drag = None;
            rts.right_press = None;
        }
    }
    if !over_ui {
        input.wheel_lines = scroll.map_or(0.0, |scroll| {
            let default_ratio = MouseScrollPixelsPerLine::default();
            let ratio = pixels_per_line.as_deref().unwrap_or(&default_ratio);
            scroll.to_lines(ratio).delta.y
        });
        if input.wheel_lines.abs() > 64.0 {
            input.wheel_lines = 0.0;
        }
    }
    // Edge scroll (the Edge Scroll option): the cursor in the outermost two
    // pixels of the focused window (`edge_distance`); the script ramps the
    // speed up while it stays there. The edge scroll is set by the camera
    // manager's per-frame update (0xB5D0F0, which also calls the planet camera
    // update), so it is the same code for every stage that drives the class.
    if controls.edge_scroll && !left && !right && !middle && !over_ui {
        if let (Some(cursor), Some(size), Some(window)) = (cursor, window_size, window) {
            if window.focused {
                input.edge = edge_distance(cursor, size);
            }
        }
    }
    let mut keyboard = read_keys(&keys);
    let hud = controls.hud.as_keys();
    keyboard.rotate = (keyboard.rotate + hud.rotate).clamp(-1.0, 1.0);
    keyboard.zoom = (keyboard.zoom + hud.zoom).clamp(-1.0, 1.0);
    if let Some(point) = controls.goto_request.take() {
        camera.rig.go_to(lift(point));
        controls.follow = None;
        controls.minimap_gotos += 1;
    }
    let planet = surface.as_deref().and_then(|s| s.planet());
    camera.rig.set_planet_frame(planet.map(|p| &p.frame));
    if camera.rig.apply(keyboard, input, dt) {
        // Panning by hand ends a follow.
        controls.follow = None;
    }
}

/// Load the installed `tribe_camera` script once the resource index is up
/// (the Civ stage does the same for `civ_camera`); without an install the
/// built-in numbers of [`builtin`] stand.
fn load_script(mut camera: ResMut<TribalCamera>, originals: Option<Res<Originals>>) {
    if camera.rig.script_checked() {
        return;
    }
    let Some(originals) = originals else {
        return;
    };
    if originals.resource_index.records.is_empty() {
        return;
    }
    camera.rig.set_script(TribalScript::from_install(&originals));
}

/// The followed member's position becomes the camera's goal; a member that is
/// gone ends the follow. A cut (Space from far away) obeys the skip distance.
fn follow_target(
    mut camera: ResMut<TribalCamera>,
    mut controls: ResMut<TribalControls>,
    units: Query<&Transform, With<TribeUnit>>,
    mut last: Local<Option<Entity>>,
) {
    let Some(target) = controls.follow else {
        *last = None;
        return;
    };
    let Ok(at) = units.get(target).map(|t| t.translation) else {
        controls.follow = None;
        *last = None;
        return;
    };
    if *last != Some(target) {
        // First frame on a new member: cut when it is far, glide when near.
        camera.rig.go_to(at);
        *last = Some(target);
    }
    camera.rig.follow_point(at);
}

/// The minimap's right click: send the selection to the point.
fn minimap_move(
    mut controls: ResMut<TribalControls>,
    mut units: Query<(&Transform, &mut TribeUnit, &mut UnitTask, Has<Selected>)>,
) {
    let Some(point) = controls.move_request.take() else {
        return;
    };
    let mut index = 0;
    let mut count = 0;
    for (_, mut unit, mut task, selected) in &mut units {
        if !selected {
            continue;
        }
        unit.offset = formation_offset(index);
        unit.work = 0.0;
        unit.resume_task = None;
        *task = UnitTask::MoveTo(point);
        index += 1;
        count += 1;
    }
    if count > 0 {
        controls.pending_perform = None;
        controls.issued.push(Issued {
            action: CitizenAction::WalkTo,
            count,
            point,
        });
    }
}

/// A social order against a village: when the chieftain stands there, press
/// the perform key once a second until the performance starts (the campaign
/// refuses it while on cooldown or short of instruments, and says why).
fn perform_when_arrived(
    time: Res<Time>,
    mut controls: ResMut<TribalControls>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<crate::player_hud::InjectedKeys>,
    run: Res<RunProgress>,
    player: Query<&Transform, With<Player>>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    let Some(village) = controls.pending_perform else {
        return;
    };
    let dt = time.delta_secs();
    let campaign = &run.tribal_campaign;
    let hostile = campaign
        .village(village)
        .is_some_and(|v| v.status == RivalStatus::Hostile);
    controls.pending_wait += dt;
    controls.pending_retry -= dt;
    if !hostile
        || campaign.performance.is_some_and(|p| p.village == village)
        || controls.pending_wait > 90.0
    {
        controls.pending_perform = None;
        return;
    }
    let Ok(chief) = player.single() else {
        return;
    };
    if dist_xz(chief.translation, tribal::village_position(village)) <= tribal::INTERACT_RANGE - 0.75
        && controls.pending_retry <= 0.0
    {
        press_virtual(&mut keys, &mut injected, KEY_PERFORM);
        controls.pending_retry = 1.0;
    }
}

/// Stage start and end: forget the camera and the follow target.
fn reset_controls(mut controls: ResMut<TribalControls>, mut camera: ResMut<TribalCamera>) {
    let edge_scroll = controls.edge_scroll;
    *controls = TribalControls {
        edge_scroll,
        ..default()
    };
    camera.rig.restart();
    camera.gesture = Gesture::default();
}

/// Leaving the stage restores the shared camera's clip planes.
fn restore_projection(mut cameras: Query<&mut Projection, With<crate::GameCamera>>) {
    let default = PerspectiveProjection::default();
    for mut projection in &mut cameras {
        if let Projection::Perspective(p) = &mut *projection {
            p.near = default.near;
            p.far = default.far;
        }
    }
}

/// Write the camera pose (after `camera_follow`, which would chase the
/// chieftain) and the stage's clip planes.
#[allow(clippy::too_many_arguments)]
fn camera_pose(
    time: Res<Time>,
    mut camera: ResMut<TribalCamera>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    capture: Option<Res<crate::runtime::RenderSmokeCamera>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<crate::GameCamera>>,
) {
    if capture.is_some() {
        return;
    }
    let Ok((mut transform, mut projection)) = cameras.single_mut() else {
        return;
    };
    if let Projection::Perspective(p) = &mut *projection {
        if p.near != tuning::NEAR_CLIP || p.far != tuning::FAR_CLIP {
            p.near = tuning::NEAR_CLIP;
            p.far = tuning::FAR_CLIP;
        }
    }
    if !camera.rig.is_ready() {
        return;
    }
    if let Some(planet) = surface.as_deref().and_then(|s| s.planet()) {
        *transform = camera.rig.pose_on(time.delta_secs(), planet);
        return;
    }
    *transform = camera.rig.pose(time.delta_secs(), |x, z| {
        surface.as_ref().map(|surface| surface.height(x, z))
    });
}

pub struct TribalControlsPlugin;

impl Plugin for TribalControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TribalControls>()
            .init_resource::<TribalCamera>()
            .add_systems(OnEnter(GameStage::Tribal), reset_controls)
            .add_systems(
                OnExit(GameStage::Tribal),
                (reset_controls, restore_projection),
            )
            .add_systems(
                Update,
                (
                    keyboard,
                    load_script,
                    camera_input,
                    follow_target,
                    minimap_move,
                    perform_when_arrived,
                )
                    .chain()
                    .after(crate::player_hud::actions)
                    .before(crate::pause_menu::input)
                    .in_set(crate::GameSystems::Controls)
                    .run_if(in_state(GameStage::Tribal))
                    .run_if(crate::editor::simulation_active)
                    .run_if(crate::mechanical_creator::simulation_active)
                    .run_if(crate::pause_menu::simulation_active),
            )
            .add_systems(
                Update,
                camera_pose
                    .in_set(crate::GameSystems::Presentation)
                    .after(crate::camera_follow)
                    .before(crate::foliage_visibility::update)
                    .run_if(in_state(GameStage::Tribal))
                    .run_if(crate::editor::simulation_active),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tribal_rts::ClickWorld;

    fn close(a: f32, b: f32, tolerance: f32) {
        assert!((a - b).abs() <= tolerance, "{a} != {b}");
    }

    fn flat(_: f32, _: f32) -> Option<f32> {
        None
    }

    fn keys(pressed: &[KeyCode]) -> ButtonInput<KeyCode> {
        let mut input = ButtonInput::default();
        for key in pressed {
            input.press(*key);
        }
        input
    }

    #[test]
    fn tribe_camera_prop_values_are_the_data() {
        assert_eq!(tuning::NEAR_CLIP, 1.0);
        assert_eq!(tuning::FAR_CLIP, 10000.0);
        assert_eq!(tuning::ELEVATION_TABLE, [20.0, 200.0, 330.0]);
        assert_eq!(tuning::MATERIAL_LODS, [20.0, 60.0, 200.0, 500.0]);
        assert_eq!(tuning::ZOOM_SCALE, 1.0);
        assert_eq!(tuning::ROTATE_SCALE, 0.5);
        assert_eq!(tuning::PITCH_SCALE, 0.1);
        assert_eq!(tuning::CAMERA_SKIP_DISTANCE, 16.0);
        assert_eq!(tuning::MINIMAP_ZOOM_LEVEL, 0.7);
        // The trigger file's action ids.
        assert_eq!(trigger::ROTATE_RIGHT, 0x11);
        assert_eq!(trigger::ROTATE_LEFT, 0x12);
        assert_eq!(trigger::PITCH_UP, 7);
        assert_eq!(trigger::ZOOM_IN, 9);
        // The `tribe_camera` script's numbers.
        assert_eq!(tuning::ZOOM_LADDER, [15.0, 25.0, 35.0, 50.0, 70.0, 90.0]);
        assert_eq!((tuning::MIN_PITCH_DEG, tuning::MAX_PITCH_DEG), (-20.0, 60.0));
        assert_eq!(tuning::KEYBOARD_SCROLL_DIVISOR, 25.0);
        assert_eq!(tuning::MOUSE_SCROLL_SENSITIVITY_BASE, 20.0);
    }

    #[test]
    fn the_stage_opens_where_the_retail_start_function_puts_the_camera() {
        // 0xCD9F20: SnapPolar(1.5, 0.4, 16) then LoadScript(tribe_camera), whose
        // default_distance (70) becomes the goal distance.
        // The retail yaw (SnapPolar 1.5) is relative to the retail frame at the
        // village (`anchor_frame`): the rig starts at it unless the developer
        // override is set.
        assert_eq!(parse_start_azimuth(None), 1.5);
        assert_eq!(parse_start_azimuth(Some(" 0.25 ")), 0.25);
        assert_eq!(parse_start_azimuth(Some("")), 1.5);
        assert_eq!(parse_start_azimuth(Some("north")), 1.5);
        assert_eq!(parse_start_azimuth(Some("NaN")), 1.5);
        assert_eq!(parse_start_azimuth(Some("inf")), 1.5);
        assert_eq!(tuning::START_AZIMUTH, 1.5);
        let by_default = TribalRig::new();
        close(by_default.azimuth(), start_azimuth(), 1e-6);
        close(by_default.goal_azimuth(), start_azimuth(), 1e-6);
        let rig = TribalRig::with_start_azimuth(tuning::START_AZIMUTH);
        close(rig.distance(), 16.0, 1e-5);
        close(rig.azimuth(), 1.5, 1e-6);
        close(rig.goal_azimuth(), 1.5, 1e-6);
        close(rig.pitch(), 0.4, 1e-6);
        close(rig.goal_pitch(), 0.4, 1e-6);
        close(rig.goal_distance(), 70.0, 1e-5);
        assert!(!rig.is_ready());
        let offset = rig.orbit_offset();
        close(offset.length(), 16.0, 1e-3);
        close(offset.y, 16.0 * 0.4_f32.sin(), 1e-3);
        // Azimuth 1.5: the camera is on the +X side of the focus, a little toward +Z.
        close(offset.x, 16.0 * 0.4_f32.cos() * 1.5_f32.sin(), 1e-3);
        close(offset.z, 16.0 * 0.4_f32.cos() * 1.5_f32.cos(), 1e-3);
        // Azimuth 0: behind the focus on the +Z side of the rig's frame.
        let behind = TribalRig::with_start_azimuth(0.0).orbit_offset();
        assert!(behind.x.abs() < 1e-5 && behind.z > 14.0, "{behind}");
        // The camera glides out to the goal on the distance segment (0.7 s for a
        // pan-free restart: `min_interpolation_time`) and stays there.
        let mut rig = rig;
        let mut previous = rig.distance();
        for _ in 0..30 {
            rig.ease(1.0 / 60.0);
            assert!(rig.distance() >= previous);
            previous = rig.distance();
        }
        assert!(previous > 50.0 && previous < 70.0, "half a second in: {previous}");
        for _ in 0..240 {
            rig.ease(1.0 / 60.0);
        }
        close(rig.distance(), 70.0, 0.05);
        // The angles stay where SnapPolar put them.
        close(rig.azimuth(), 1.5, 1e-6);
        close(rig.pitch(), 0.4, 1e-6);
    }

    #[test]
    fn the_zoom_ladder_snaps_like_the_script() {
        // Zoom out: the smallest step above the goal; zoom in: the largest below.
        assert_eq!(zoom_ladder(70.0, -1.0), 90.0);
        assert_eq!(zoom_ladder(90.0, -1.0), 90.0);
        assert_eq!(zoom_ladder(10.0, -1.0), 15.0);
        assert_eq!(zoom_ladder(40.0, -1.0), 50.0);
        assert_eq!(zoom_ladder(70.0, 1.0), 50.0);
        assert_eq!(zoom_ladder(15.0, 1.0), 15.0);
        assert_eq!(zoom_ladder(100.0, 1.0), 90.0);
        assert_eq!(zoom_ladder(40.0, 1.0), 35.0);
        assert_eq!(zoom_ladder(40.0, 0.0), 40.0);
        let mut rig = TribalRig::new();
        let mut steps = vec![rig.goal_distance()];
        for _ in 0..6 {
            rig.zoom_step(1);
            steps.push(rig.goal_distance());
        }
        assert_eq!(steps, vec![70.0, 50.0, 35.0, 25.0, 15.0, 15.0, 15.0]);
        for _ in 0..8 {
            rig.zoom_step(-1);
        }
        assert_eq!(rig.goal_distance(), 90.0);
        // The wheel: one step per notch, fractions add up; NaN is ignored.
        let mut rig = TribalRig::new();
        assert_eq!(rig.take_wheel_notches(0.5), 0);
        assert_eq!(rig.take_wheel_notches(0.5), 1);
        assert_eq!(rig.take_wheel_notches(-2.0), -2);
        assert_eq!(rig.take_wheel_notches(f32::NAN), 0);
        let wheel = MouseInput {
            wheel_lines: 1.0,
            ..default()
        };
        rig.apply(KeyInput::default(), wheel, 0.016);
        assert_eq!(rig.goal_distance(), 50.0);
        // The eased distance follows its goal.
        for _ in 0..600 {
            rig.ease(1.0 / 60.0);
        }
        close(rig.distance(), 50.0, 0.05);
        assert!((ZOOM_MIN, ZOOM_MAX) == (15.0, 90.0));
        close(rig.zoom_fraction(), (50.0 - 15.0) / 75.0, 1e-3);
    }

    #[test]
    fn held_zoom_keys_raise_one_event_per_update() {
        // 0xB14800: a held zoom trigger sets `direction` on every update, so
        // there is no repeat delay: the press is one event and a held second
        // at the nominal rate is one per update.
        let mut rig = TribalRig::new();
        assert_eq!(rig.zoom_key_events(1.0, 0.016), 1, "the press is one event");
        let step = 1.0 / UPDATES_PER_SECOND;
        let mut events = 0;
        for _ in 0..30 {
            events += rig.zoom_key_events(1.0, step);
        }
        assert_eq!(events, UPDATES_PER_SECOND as i32, "{events}");
        // No event once the key is up; the other key is a new press.
        assert_eq!(rig.zoom_key_events(0.0, 0.016), 0);
        assert_eq!(rig.zoom_key_events(-1.0, 0.016), -1, "a new direction is a new press");
        // A held key reaches the far end of the ladder within a fraction of a
        // second (six steps in six updates).
        let mut rig = TribalRig::new();
        for _ in 0..6 {
            rig.apply(KeyInput { zoom: -1.0, ..Default::default() }, MouseInput::default(), step);
        }
        for _ in 0..600 {
            rig.ease(1.0 / 60.0);
        }
        close(rig.distance(), ZOOM_MAX, 0.05);
    }

    #[test]
    fn mouse_orbit_turns_and_tilts_inside_the_script_limits() {
        let mut rig = TribalRig::new();
        let (az, pitch) = (rig.azimuth(), rig.pitch());
        // Dragging right turns the yaw goal down from the CURRENT yaw by the
        // retail unit (0xB0FDF0: `mouse_rotate_sensitivity_x` 1.0 x 1 degree).
        rig.orbit_by_pixels(Vec2::new(10.0, 0.0));
        close(
            (rig.goal_azimuth() - az).to_degrees(),
            -10.0 * MOUSE_X_DEGREES_PER_UNIT,
            1e-3,
        );
        // Dragging down raises the elevation goal (`..._y` 1.0 x 10 degrees).
        rig.orbit_by_pixels(Vec2::new(0.0, 1.0));
        close(
            (rig.goal_pitch() - pitch).to_degrees(),
            1.0 * MOUSE_Y_DEGREES_PER_UNIT,
            1e-3,
        );
        // The goal is an offset from the current angle, not from the old goal.
        rig.orbit_by_pixels(Vec2::new(10.0, 0.0));
        close((rig.goal_azimuth() - az).to_degrees(), -10.0, 1e-3);
        // One event is clamped to 20 pixels.
        rig.orbit_by_pixels(Vec2::new(500.0, 0.0));
        close((rig.goal_azimuth() - az).to_degrees(), -20.0, 1e-3);
        // The pitch stops at the script's range.
        let mut rig = TribalRig::new();
        rig.orbit_by_pixels(Vec2::new(0.0, 1.0e6));
        close(rig.goal_pitch().to_degrees(), PITCH_MAX_DEG, 1e-3);
        rig.orbit_by_pixels(Vec2::new(0.0, -1.0e6));
        close(rig.goal_pitch().to_degrees(), PITCH_MIN_DEG, 1e-3);
        rig.orbit_by_pixels(Vec2::new(f32::INFINITY, 3.0));
        assert!(rig.goal_azimuth().is_finite());
    }

    #[test]
    fn the_mouse_orbit_gain_per_pixel_is_a_fraction_of_the_unit() {
        // One pixel a frame at 60 Hz for a second: the goal rides a degree
        // ahead of the 0.2 s yaw channel (pitch 10 degrees ahead of the 0.65 s
        // one), so the camera turns far less than 60 degrees (`notes\civ-cam-notes.md`:
        // about 0.14 x the unit for yaw, 0.04 x for pitch at 60 updates a second).
        let mut rig = TribalRig::new();
        let none = KeyInput::default();
        let (az, pitch) = (rig.azimuth(), rig.pitch());
        let orbit = MouseInput {
            orbit_pixels: Vec2::new(1.0, 1.0),
            ..default()
        };
        for _ in 0..60 {
            rig.apply(none, orbit, 1.0 / 60.0);
            rig.ease(1.0 / 60.0);
        }
        let yaw = (az - rig.azimuth()).to_degrees();
        let tilt = (rig.pitch() - pitch).to_degrees();
        println!("mouse orbit gain: {yaw:.2} degrees of yaw, {tilt:.2} of pitch for 60 pixels");
        assert!(yaw > 4.0 && yaw < 14.0, "yaw {yaw} degrees for 60 pixels");
        assert!(tilt > 10.0 && tilt < 40.0, "pitch {tilt} degrees for 60 pixels");
    }

    #[test]
    fn releasing_the_orbit_button_coasts_yaw_and_pitch_over_a_tenth_of_a_second() {
        let none = KeyInput::default();
        let mut rig = TribalRig::new();
        let start = rig.azimuth();
        let orbit = MouseInput {
            orbit_pixels: Vec2::new(20.0, 0.0),
            ..default()
        };
        for _ in 0..6 {
            rig.apply(none, orbit, 1.0 / 60.0);
            rig.ease(1.0 / 60.0);
        }
        let chasing = rig.goal_azimuth();
        let (at, speed) = (rig.azimuth(), rig.yaw_seg.vel);
        assert!(speed < -0.1 && rig.yaw_seg.active(), "{speed}");
        // Letting go: the goal becomes `velocity x 0.1 x 0.5` from the current
        // yaw, so the camera stops close to where it is.
        let release = MouseInput {
            orbit_end: true,
            ..default()
        };
        rig.apply(none, release, 1.0 / 60.0);
        close(rig.goal_azimuth(), at + speed * 0.05, 1e-5);
        for _ in 0..60 {
            rig.apply(none, MouseInput::default(), 1.0 / 60.0);
            rig.ease(1.0 / 60.0);
        }
        close(rig.azimuth(), rig.goal_azimuth(), 1e-3);
        assert!(
            (rig.azimuth() - start).abs() < (chasing - start).abs() * 0.8,
            "{} against the chased goal {}",
            rig.azimuth() - start,
            chasing - start
        );
        assert!(rig.azimuth() < at, "still turns the same way a little");
        // The gesture reports the release exactly once.
        let (press, window) = (Vec2::new(500.0, 400.0), Vec2::new(1000.0, 800.0));
        let mut g = Gesture::default();
        assert!(!g.drag(false, false, true, false, false, Vec2::ONE, press, window).orbit_end);
        assert!(g.drag(false, false, false, false, false, Vec2::ZERO, press, window).orbit_end);
        assert!(!g.drag(false, false, false, false, false, Vec2::ZERO, press, window).orbit_end);
    }

    #[test]
    fn yaw_and_pitch_channels_take_the_retail_segment_times() {
        // Yaw 0.2 s, pitch 0.65 s (0x1567D9C / 0x1567DA0): fixed constants.
        let mut rig = TribalRig::new();
        let (az, pitch) = (rig.azimuth(), rig.pitch());
        rig.goal_azimuth = az + 1.0;
        rig.goal_pitch = pitch + 0.5;
        let mut yaw_at_tenth = 0.0;
        let mut pitch_at_third = 0.0;
        for frame in 1..=60 {
            rig.ease(1.0 / 60.0);
            if frame == 6 {
                yaw_at_tenth = rig.azimuth() - az;
            }
            if frame == 20 {
                pitch_at_third = rig.pitch() - pitch;
            }
            if frame == 15 {
                assert!(rig.azimuth() - az > 0.98, "yaw arrived: {}", rig.azimuth() - az);
                assert!(rig.pitch() - pitch < 0.4, "pitch has not: {}", rig.pitch() - pitch);
            }
        }
        assert!(yaw_at_tenth > 0.3 && yaw_at_tenth < 0.8, "{yaw_at_tenth}");
        assert!(pitch_at_third > 0.05 && pitch_at_third < 0.35, "{pitch_at_third}");
        close(rig.azimuth(), az + 1.0, 1e-3);
        close(rig.pitch(), pitch + 0.5, 1e-3);
    }

    #[test]
    fn period_rotates_left_and_comma_right_at_five_degrees_an_update() {
        let none = MouseInput::default();
        let dt = 1.0 / 60.0;
        let mut rig = TribalRig::new();
        let start = rig.goal_azimuth();
        let rotate = |amount: f32| KeyInput {
            rotate: amount,
            ..default()
        };
        // Held for a second: the smoothstep ramp over 0.7 s, then 5 degrees per
        // update at the ADAPTED 30 updates a second.
        let mut expected = 0.0;
        for k in 1..=60 {
            rig.apply(rotate(1.0), none, dt);
            expected += crate::civ_controls::ramp(k as f32 * dt, 0.7)
                * tuning::KEYBOARD_ROTATE_DEGREES
                * UPDATES_PER_SECOND
                * dt;
        }
        close((rig.goal_azimuth() - start).to_degrees(), expected, 0.05);
        assert!(expected > 80.0 && expected < 110.0, "{expected}");
        // Letting go and holding the other key turns it back (and starts slowly).
        rig.apply(rotate(0.0), none, dt);
        let turned = rig.goal_azimuth();
        rig.apply(rotate(-1.0), none, dt);
        assert!(rig.goal_azimuth() < turned);
        assert!(turned - rig.goal_azimuth() < 0.01, "the ramp starts from zero");
        // Reading the keys: `.` and Shift+Left are +1, `,` and Shift+Right -1
        // (triggers 0x12 rotate left, 0x11 rotate right).
        assert_eq!(read_keys(&keys(&[KeyCode::Period])).rotate, 1.0);
        assert_eq!(read_keys(&keys(&[KeyCode::Comma])).rotate, -1.0);
        assert_eq!(
            read_keys(&keys(&[KeyCode::ShiftLeft, KeyCode::ArrowLeft])).rotate,
            1.0
        );
        assert_eq!(
            read_keys(&keys(&[KeyCode::ShiftRight, KeyCode::ArrowRight])).rotate,
            -1.0
        );
        assert_eq!(read_keys(&keys(&[KeyCode::Comma, KeyCode::Period])).rotate, 0.0);
    }

    #[test]
    fn page_keys_and_shift_arrows_pitch_and_page_up_lowers_the_camera() {
        let none = MouseInput::default();
        let mut rig = TribalRig::new();
        let start = rig.goal_pitch();
        let tilt = |amount: f32| KeyInput {
            tilt: amount,
            ..default()
        };
        // PageUp (trigger 7): the elevation falls by sens_y * 10 degrees * 5 per
        // second, not ramped (0xB109F0): 50 degrees a second.
        rig.apply(tilt(1.0), none, 0.1);
        close((rig.goal_pitch() - start).to_degrees(), -5.0, 1e-3);
        rig.apply(tilt(-1.0), none, 0.1);
        close((rig.goal_pitch() - start).to_degrees(), 0.0, 1e-3);
        // The script's limits: -20 .. 60 degrees.
        for _ in 0..100 {
            rig.apply(tilt(1.0), none, 0.1);
        }
        close(rig.goal_pitch().to_degrees(), -20.0, 1e-3);
        for _ in 0..100 {
            rig.apply(tilt(-1.0), none, 0.1);
        }
        close(rig.goal_pitch().to_degrees(), 60.0, 1e-3);
        for key in [KeyCode::PageUp, KeyCode::Numpad9] {
            assert_eq!(read_keys(&keys(&[key])).tilt, 1.0);
        }
        for key in [KeyCode::PageDown, KeyCode::Numpad3] {
            assert_eq!(read_keys(&keys(&[key])).tilt, -1.0);
        }
        assert_eq!(
            read_keys(&keys(&[KeyCode::ShiftLeft, KeyCode::ArrowUp])).tilt,
            1.0
        );
        assert_eq!(
            read_keys(&keys(&[KeyCode::ShiftLeft, KeyCode::ArrowDown])).tilt,
            -1.0
        );
        // Shift+arrow does not pan.
        assert_eq!(
            read_keys(&keys(&[KeyCode::ShiftLeft, KeyCode::ArrowUp])).pan,
            Vec2::ZERO
        );
    }

    #[test]
    fn zoom_keys_are_equal_plus_minus_and_the_numpad() {
        for key in [KeyCode::Equal, KeyCode::NumpadAdd] {
            assert_eq!(read_keys(&keys(&[key])).zoom, 1.0);
        }
        for key in [KeyCode::Minus, KeyCode::NumpadSubtract] {
            assert_eq!(read_keys(&keys(&[key])).zoom, -1.0);
        }
        assert_eq!(read_keys(&keys(&[])), KeyInput::default());
    }

    #[test]
    fn wasd_arrows_numpad_and_qe_pan() {
        let pan = |list: &[KeyCode]| read_keys(&keys(list)).pan;
        assert_eq!(pan(&[KeyCode::KeyW]), Vec2::Y);
        assert_eq!(pan(&[KeyCode::ArrowUp]), Vec2::Y);
        assert_eq!(pan(&[KeyCode::Numpad8]), Vec2::Y);
        assert_eq!(pan(&[KeyCode::KeyS]), -Vec2::Y);
        assert_eq!(pan(&[KeyCode::ArrowDown]), -Vec2::Y);
        assert_eq!(pan(&[KeyCode::Numpad2]), -Vec2::Y);
        assert_eq!(pan(&[KeyCode::KeyA]), -Vec2::X);
        assert_eq!(pan(&[KeyCode::ArrowLeft]), -Vec2::X);
        assert_eq!(pan(&[KeyCode::Numpad4]), -Vec2::X);
        assert_eq!(pan(&[KeyCode::KeyD]), Vec2::X);
        assert_eq!(pan(&[KeyCode::ArrowRight]), Vec2::X);
        assert_eq!(pan(&[KeyCode::Numpad6]), Vec2::X);
        // Q and E strafe (TriggerConfigWASD).
        assert_eq!(pan(&[KeyCode::KeyQ]), -Vec2::X);
        assert_eq!(pan(&[KeyCode::KeyE]), Vec2::X);
        assert_eq!(pan(&[KeyCode::KeyW, KeyCode::KeyS]), Vec2::ZERO);
        assert_eq!(pan(&[KeyCode::KeyW, KeyCode::KeyD]), Vec2::ONE);
    }

    /// A rig standing still at `distance` (eased and goal), over the origin,
    /// facing azimuth 0 (forward is -Z, right is +X).
    fn rig_at(distance: f32) -> TribalRig {
        let mut rig = TribalRig::new();
        rig.place(Vec3::ZERO);
        rig.distance = distance;
        rig.goal_distance = distance;
        rig.distance_seg = Segment::at_rest(distance);
        rig.azimuth = 0.0;
        rig.goal_azimuth = 0.0;
        rig.yaw_seg = Segment::at_rest(0.0);
        rig
    }

    /// What the systems do every frame: the input update (`camera_input`), then
    /// the pose's ease. Returns how far the focus travelled.
    fn drive_rig(rig: &mut TribalRig, keys: KeyInput, mouse: MouseInput, seconds: f32) -> Vec3 {
        let start = rig.focus();
        for _ in 0..(seconds * 60.0).round() as usize {
            rig.apply(keys, mouse, 1.0 / 60.0);
            rig.ease(1.0 / 60.0);
        }
        rig.focus() - start
    }

    /// Hold `keys` for `seconds` in 1/60 s frames.
    fn hold_rig(rig: &mut TribalRig, keys: KeyInput, seconds: f32) {
        drive_rig(rig, keys, MouseInput::default(), seconds);
    }

    #[test]
    fn panning_keys_move_the_anchor_like_the_retail_hermite_chase() {
        let none = MouseInput::default();
        let forward = KeyInput {
            pan: Vec2::Y,
            ..default()
        };
        // The first update after the press: the goal is `scroll` ahead of the
        // anchor (0.7 s ramp, so still tiny); the anchor has hardly moved.
        let mut rig = rig_at(70.0);
        rig.apply(forward, none, 1.0 / 60.0);
        let first = rig.goal_focus() - rig.focus();
        assert!(first.x.abs() < 1e-4 && first.z < 0.0 && first.length() < 0.1, "{first}");
        // Held for 2 s: the goal stays one scroll ahead of the CURRENT anchor
        // (0.4 distance a update at the full ramp: 28 at 70), never ahead of
        // the old goal; the steady speed of the chase is about 1.5 x scroll / T
        // with T the 0.95 shortened segment time (the 28 unit offset is 0.056
        // rad on the flat world, `lerp(0.7, 6, 0.018)` = 0.79 s), i.e. about
        // 0.8 distances a second straight ahead (-Z at azimuth 0).
        let mut rig = rig_at(70.0);
        drive_rig(&mut rig, forward, none, 1.0);
        let travelled = drive_rig(&mut rig, forward, none, 1.0);
        let per_second = travelled.length() / 70.0;
        println!("tribal chase: {per_second:.3} distances a second at the full key ramp");
        assert!(travelled.x.abs() < 1e-3 && travelled.z < 0.0, "{travelled}");
        assert!(per_second > 0.7 && per_second < 0.95, "{per_second}");
        let ahead = rig.goal_focus() - rig.focus();
        assert!(ahead.length() <= 28.0 + 1e-3 && ahead.length() > 20.0, "{ahead}");
        // Right is +X; both keys go along the diagonal.
        let mut rig = rig_at(70.0);
        let right = drive_rig(&mut rig, KeyInput { pan: Vec2::X, ..default() }, none, 2.0);
        assert!(right.x > 40.0 && right.z.abs() < 1e-3, "{right}");
        let mut rig = rig_at(70.0);
        let diagonal = drive_rig(&mut rig, KeyInput { pan: Vec2::ONE, ..default() }, none, 2.0);
        assert!(diagonal.x > 40.0 && diagonal.z < -40.0, "{diagonal}");
        // Rotated a quarter turn the same key goes another way.
        let mut rig = rig_at(70.0);
        rig.azimuth = std::f32::consts::FRAC_PI_2;
        rig.goal_azimuth = std::f32::consts::FRAC_PI_2;
        rig.yaw_seg = Segment::at_rest(std::f32::consts::FRAC_PI_2);
        let turned = drive_rig(&mut rig, forward, none, 2.0);
        assert!(turned.x < -40.0 && turned.z.abs() < 1e-2, "{turned}");
        // Zoomed out it pans faster in proportion to the distance (a longer
        // offset is a slightly longer segment, so a little less than 6).
        let mut near = rig_at(15.0);
        let near = drive_rig(&mut near, forward, none, 2.0);
        let mut far = rig_at(90.0);
        let far = drive_rig(&mut far, forward, none, 2.0);
        let ratio = far.length() / near.length();
        assert!(ratio > 4.5 && ratio < 6.2, "{ratio}");
        // Nothing bounds the camera (the island's square is gone), and a key
        // released starts the ramp again from the current anchor.
        let mut rig = rig_at(90.0);
        hold_rig(&mut rig, KeyInput { pan: Vec2::new(1.0, -1.0), ..default() }, 40.0);
        assert!(rig.focus().x > 1000.0 && rig.focus().z > 1000.0, "{}", rig.focus());
        rig.apply(KeyInput::default(), none, 1.0 / 60.0);
        rig.apply(forward, none, 1.0 / 60.0);
        assert!((rig.goal_focus() - rig.focus()).length() < 0.2, "the ramp starts from zero");
        rig.apply(KeyInput { pan: Vec2::NAN, ..default() }, none, 1.0 / 60.0);
        assert!(rig.goal_focus().is_finite());
    }

    #[test]
    fn releasing_the_pan_keys_coasts_the_anchor_to_a_stop() {
        let none = MouseInput::default();
        let forward = KeyInput {
            pan: Vec2::Y,
            ..default()
        };
        let mut rig = rig_at(70.0);
        drive_rig(&mut rig, forward, none, 1.5);
        let speed = rig.anchor_speed();
        assert!(speed > 0.05, "{speed}");
        // The release update puts the goal `|velocity| x min_interp x 250`
        // ahead along the last direction (velocity in radians a second).
        rig.apply(KeyInput::default(), none, 1.0 / 60.0);
        let reach = speed * 0.7 * 250.0;
        let ahead = rig.goal_focus() - rig.focus();
        close(ahead.length(), reach, reach * 1e-3);
        assert!(ahead.z < 0.0 && ahead.x.abs() < 1e-3);
        let (at_release, goal) = (rig.focus(), rig.goal_focus());
        // The coast ends at the goal after the minimum interpolation time, at rest.
        drive_rig(&mut rig, KeyInput::default(), none, 1.5);
        let travelled = (rig.focus() - at_release).length();
        assert!(travelled > 0.9 * reach && travelled <= reach + 0.5, "{travelled} {reach}");
        assert!((rig.focus() - goal).length() < 0.05);
        assert!(rig.anchor_speed() < 1e-3);
        // No second coast.
        rig.apply(KeyInput::default(), none, 1.0 / 60.0);
        assert_eq!(rig.goal_focus(), goal);
        // A jump by hand cancels a coast that was about to start.
        let mut rig = rig_at(70.0);
        drive_rig(&mut rig, forward, none, 1.0);
        rig.go_to(Vec3::new(3.0, 0.0, 3.0));
        rig.apply(KeyInput::default(), none, 1.0 / 60.0);
        assert_eq!(rig.goal_focus(), Vec3::new(3.0, 0.0, 3.0));
    }

    #[test]
    fn segments_end_at_rest_on_the_goal_and_the_anchor_never_jumps() {
        // The anchor chases a goal that moves every update with a continuous
        // velocity: no step is larger than the one before plus a little.
        let mut rig = rig_at(70.0);
        let mut last = rig.focus().x;
        let mut last_step = 0.0_f32;
        for frame in 0..120 {
            rig.follow_point(Vec3::new(frame as f32 * 0.2, 0.0, 0.0));
            rig.ease(1.0 / 60.0);
            let step = rig.focus().x - last;
            last = rig.focus().x;
            assert!(step >= 0.0 && step <= last_step + 0.08, "frame {frame}: {step} after {last_step}");
            last_step = step;
        }
        // A goal that stops: the anchor settles on it.
        for _ in 0..180 {
            rig.ease(1.0 / 60.0);
        }
        close(rig.focus().x, 23.8, 0.05);
    }

    #[test]
    fn the_azimuth_is_kept_in_a_bounded_range_without_a_jump() {
        // Turning for a long time re-centres the yaw by whole turns: the eased
        // value, the goal and the segment move together, so nothing jumps.
        let mut rig = rig_at(70.0);
        let turns = 3.0 * TAU;
        rig.azimuth = turns + 0.2;
        rig.goal_azimuth = turns + 0.5;
        rig.yaw_seg = Segment::at_rest(turns + 0.5);
        rig.apply(KeyInput::default(), MouseInput::default(), 1.0 / 60.0);
        assert!(rig.goal_azimuth().abs() < 2.0 * TAU, "{}", rig.goal_azimuth());
        close(rig.goal_azimuth() - rig.azimuth(), 0.3, 1e-4);
        rig.ease(1.0 / 60.0);
        assert!(rig.azimuth() > 0.2 && rig.azimuth() < 0.5, "{}", rig.azimuth());
        for _ in 0..120 {
            rig.ease(1.0 / 60.0);
        }
        close(rig.azimuth(), rig.goal_azimuth(), 1e-3);
    }

    #[test]
    fn the_right_button_scrolls_like_a_joystick() {
        let mut rig = rig_at(70.0);
        // A deflection of 1 scrolls `mouse_scroll_sensitivity` (20 + distance =
        // 90) per update ahead of the current anchor: the angle is 0.18 rad on
        // the flat world, `lerp(0.7, 6, 0.057)` = 1.0 s, so the chase runs at
        // about 1.5 x 90 / (0.95 x 1.0) = 140 units a second.
        let drag = MouseInput {
            drag_scroll: Vec2::X,
            ..default()
        };
        let mut moved = Vec3::ZERO;
        for _ in 0..2 {
            moved = drive_rig(&mut rig, KeyInput::default(), drag, 1.0);
            assert!(rig.is_ready());
        }
        assert!(moved.x > 110.0 && moved.x < 160.0, "{moved}");
        assert!(moved.z.abs() < 1e-3);
        // The goal is one scroll ahead of the CURRENT anchor on every update.
        rig.apply(KeyInput::default(), drag, 1.0 / 60.0);
        close((rig.goal_focus() - rig.focus()).x, 90.0, 1e-2);
        // Half a deflection is a quarter of the scroll (squared with its sign).
        let (press, window) = (Vec2::new(500.0, 400.0), Vec2::new(1000.0, 800.0));
        close(drag_scroll_vector(press, press + Vec2::new(250.0, 0.0), window).x, 0.25, 1e-6);
        close(drag_scroll_vector(press, press + Vec2::new(0.0, -400.0), window).y, 1.0, 1e-6);
        close(drag_scroll_vector(press, press + Vec2::new(-500.0, 0.0), window).x, -1.0, 1e-6);
        // A hand scroll ends a follow.
        let mut rig = rig_at(70.0);
        rig.follow_point(Vec3::new(1.0, 0.0, 1.0));
        assert!(rig.is_following());
        assert!(rig.apply(KeyInput::default(), drag, 1.0 / 60.0));
        assert!(!rig.is_following());
    }

    #[test]
    fn far_moves_cut_and_near_moves_glide() {
        let mut rig = TribalRig::new();
        rig.place(Vec3::ZERO);
        // Within the skip distance (16) the focus glides.
        assert!(!rig.go_to(Vec3::new(10.0, 0.0, 0.0)));
        assert_eq!(rig.focus().x, 0.0);
        rig.ease(0.05);
        assert!(rig.focus().x > 0.0 && rig.focus().x < 10.0);
        // Beyond it, the camera cuts.
        assert!(rig.go_to(Vec3::new(-30.0, 0.0, 5.0)));
        assert_eq!(rig.focus().x, -30.0);
        assert_eq!(rig.focus().z, 5.0);
        // The skip rule measures from where the camera looks, not the goal.
        rig.place(Vec3::ZERO);
        assert!(!rig.go_to(Vec3::new(16.0, 0.0, 0.0)));
        assert!(rig.go_to(Vec3::new(16.1, 0.0, 0.0)));
        // Far targets are kept; NaN is ignored.
        rig.place(Vec3::ZERO);
        rig.go_to(Vec3::new(500.0, 0.0, -500.0));
        close(rig.goal_focus().x, 500.0, 1e-4);
        assert!(!rig.go_to(Vec3::NAN));
    }

    #[test]
    fn following_eases_after_a_moving_point_and_manual_pan_ends_it() {
        let mut rig = TribalRig::new();
        rig.place(Vec3::ZERO);
        let mut at = Vec3::ZERO;
        for _ in 0..120 {
            at.x += 7.0 / 60.0;
            rig.follow_point(at);
            rig.ease(1.0 / 60.0);
        }
        assert!(rig.is_following());
        // The chase of a goal that moves every update lags by about
        // speed x T / 1.5 (T the shortened 0.7 s segment): 3 units at 7 a second.
        let lag = at.x - rig.focus().x;
        assert!(lag > 1.0 && lag < 4.5, "{lag}");
        hold_rig(&mut rig, KeyInput { pan: Vec2::X, ..default() }, 0.2);
        assert!(!rig.is_following());
    }

    #[test]
    fn pose_looks_at_the_focus_and_clears_the_terrain() {
        let mut rig = TribalRig::with_start_azimuth(0.0);
        rig.place(Vec3::new(5.0, 0.0, -3.0));
        let pose = rig.pose(1.0 / 60.0, flat);
        let look = Vec3::new(5.0, LOOK_HEIGHT, -3.0);
        let toward = (look - pose.translation).normalize();
        assert!((pose.rotation * Vec3::NEG_Z - toward).length() < 1e-4);
        // The opening: 16 units from the look point (the eased distance).
        assert!(pose.translation.distance(look) - rig.distance() < 0.01);
        close(pose.translation.distance(look), 16.0, 0.5); // already gliding out
        // At azimuth 0 the camera is on the +Z side of the flat rig's
        // focus, above it, and looks toward -Z.
        assert!(pose.translation.z > look.z + 10.0 && pose.translation.y > look.y);
        assert!((pose.translation.x - look.x).abs() < 1e-3);
        // The anchor stands `min_height` over the ground.
        close(LOOK_HEIGHT, 0.75, 1e-6);
        // A hill under the camera lifts it `min_height` above the ground.
        let hill = |_x: f32, z: f32| Some(if z > 0.0 { 30.0 } else { 0.0 });
        let mut rig = TribalRig::with_start_azimuth(0.0);
        rig.place(Vec3::ZERO);
        let pose = rig.pose(1.0 / 60.0, hill);
        assert!(pose.translation.y >= 30.0 + ABOVE_TERRAIN - 1e-4);
        // The clip planes are the camera prop's.
        assert!(tuning::NEAR_CLIP < ZOOM_MIN && tuning::FAR_CLIP > 10.0 * ZOOM_MAX);
    }

    #[test]
    fn on_the_planet_the_camera_orbits_the_local_up() {
        let land = crate::creature_planet::tests::island_surface(700.0);
        let planet = land.planet().unwrap().clone();
        let c = planet.frame.centre;
        let mut rig = TribalRig::with_start_azimuth(0.0);
        rig.set_planet_frame(Some(&planet.frame));
        rig.place(Vec3::new(0.0, planet.surface_y(0.0, 0.0), 0.0));
        let pose = rig.pose_on(1.0 / 60.0, &planet);
        // Azimuth 0: the camera stands south of the focus (opposite the retail
        // frame's north, toward the planet-local +Z pole) and above it.
        let focus0 = rig.focus();
        let north0 = planet.frame.dir_to_world(
            anchor_frame(planet.frame.dir_to_local((focus0 - c).normalize())).1,
        );
        assert!((pose.translation - focus0).dot(north0) < -10.0 && pose.translation.y > 2.0, "{}", pose.translation);
        // Zoomed out to the last ladder step and panned ~600 units the focus
        // stays on the sphere and the camera above it.
        for _ in 0..4 {
            rig.zoom_step(-1);
        }
        close(rig.goal_distance(), 90.0, 1e-4);
        for _ in 0..240 {
            rig.ease(1.0 / 60.0);
        }
        rig.distance = 90.0;
        for _ in 0..600 {
            rig.apply(
                KeyInput {
                    pan: Vec2::Y,
                    ..default()
                },
                MouseInput::default(),
                1.0 / 60.0,
            );
            rig.pose_on(1.0 / 60.0, &planet);
        }
        let pose = rig.pose_on(1.0 / 60.0, &planet);
        let focus = rig.focus();
        let up = (focus - c).normalize();
        assert!(up.dot(Vec3::Y) < 0.95, "the focus travelled over the curve: {focus}");
        let ground = (planet.to_surface_world(focus) - c).length();
        assert!(((focus - c).length() - ground).abs() < 2.0);
        // The camera is above the focus along the local up, looking at it.
        let height = (pose.translation - focus).dot(up);
        close(height, 90.0 * rig.pitch().sin(), 3.0);
        let toward = (focus + up * LOOK_HEIGHT - pose.translation).normalize();
        assert!((pose.rotation * Vec3::NEG_Z - toward).length() < 1e-3);
    }

    #[test]
    fn the_camera_frame_is_the_retail_east_north_up_of_the_pole() {
        // 0xB145E0 -> 0xF9C610: x = (anchor + Z) x anchor (east), y = up x x
        // (north, toward +Z), z = the anchor direction.
        for up in [
            Vec3::new(0.3, 0.8, 0.5),
            Vec3::new(-0.7, 0.1, 0.4),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.9, -0.2, -0.3),
        ] {
            let (east, north, u) = anchor_frame(up);
            close(east.length(), 1.0, 1e-5);
            close(north.length(), 1.0, 1e-5);
            close(east.dot(north), 0.0, 1e-5);
            close(east.dot(u), 0.0, 1e-5);
            assert!((u - up.normalize()).length() < 1e-5);
            assert!((east.cross(north) - u).length() < 1e-4, "right-handed frame");
            assert!(north.dot(Vec3::Z) > 0.0, "north points toward the +Z pole: {north}");
            // The frame depends on the anchor alone: east is horizontal.
            assert!(east.dot(Vec3::Z).abs() < 1e-5);
        }
        // At +Y (the Tribal chart's hut pole): east -X, north +Z.
        let (east, north, _) = anchor_frame(Vec3::Y);
        assert!((east - Vec3::NEG_X).length() < 1e-5 && (north - Vec3::Z).length() < 1e-5);
        // At the +Z pole itself the code falls back to the perpendicular of up.
        let (east, north, _) = anchor_frame(Vec3::Z);
        assert!((east - Vec3::NEG_X).length() < 1e-5 && (north - Vec3::NEG_Y).length() < 1e-5);
        // At the -Z pole the planet answers the identity.
        assert_eq!(anchor_frame(Vec3::NEG_Z), (Vec3::X, Vec3::Y, Vec3::Z));
        // 0x6985B0: the vector crossed with the axis of its smallest component.
        assert_eq!(retail_perpendicular(Vec3::new(0.0, 0.0, 1.0)), Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(retail_perpendicular(Vec3::new(1.0, 2.0, 3.0)), Vec3::new(0.0, 3.0, -2.0));
        assert_eq!(retail_perpendicular(Vec3::new(3.0, 2.0, 1.0)), Vec3::new(2.0, -3.0, 0.0));
        assert_eq!(retail_perpendicular(Vec3::new(3.0, 1.0, 2.0)), Vec3::new(-2.0, 0.0, 3.0));
        // The rig turns the frame to the world: azimuth 0 looks north, the retail
        // start 1.5 sits east of the anchor and looks (almost exactly) west.
        let mut rig = TribalRig::with_start_azimuth(0.0);
        rig.set_globe(Some(Vec3::ZERO));
        rig.place(Vec3::new(0.0, 500.0, 0.0));
        let (forward, _) = rig.ground_axes();
        assert!((forward - Vec3::Z).length() < 1e-4, "north at +Y is +Z: {forward}");
        let rig = {
            let mut r = TribalRig::with_start_azimuth(tuning::START_AZIMUTH);
            r.set_globe(Some(Vec3::ZERO));
            r.place(Vec3::new(0.0, 500.0, 0.0));
            r
        };
        let (forward, _) = rig.ground_axes();
        let (east, north, _) = anchor_frame(Vec3::Y);
        close(forward.dot(-east), 1.5_f32.sin(), 1e-4);
        close(forward.dot(north), 1.5_f32.cos(), 1e-4);
        // Moving the anchor does not turn the view relative to the pole: the
        // forward direction at another point is that point's frame, not the
        // minimal-rotation transport of the start frame.
        let mut rig = TribalRig::with_start_azimuth(0.0);
        rig.set_globe(Some(Vec3::ZERO));
        let there = Vec3::new(300.0, 400.0, 0.0);
        rig.place(there);
        let (forward, _) = rig.ground_axes();
        let (_, north, _) = anchor_frame(there);
        assert!((forward - north).length() < 1e-4);
    }

    #[test]
    fn focus_height_eases_over_changing_terrain() {
        let mut rig = TribalRig::new();
        rig.place(Vec3::ZERO);
        rig.pose(1.0 / 60.0, |_, _| Some(0.0));
        let first = rig.focus().y;
        rig.pose(1.0 / 60.0, |_, _| Some(10.0));
        let step = rig.focus().y;
        assert!(step > first && step < 10.0, "{step}");
        for _ in 0..300 {
            rig.pose(1.0 / 60.0, |_, _| Some(10.0));
        }
        close(rig.focus().y, 10.0, 0.05);
    }

    #[test]
    fn right_drag_scrolls_after_the_slop_and_both_buttons_orbit() {
        let window = Vec2::new(1000.0, 800.0);
        let press = Vec2::new(500.0, 400.0);
        let mut g = Gesture::default();
        // A still right click is not a drag.
        let out = g.drag(false, true, false, true, false, Vec2::new(2.0, 1.0), press, window);
        assert_eq!(out, MouseInput::default());
        assert!(!g.owns_mouse(false, true, false));
        // Past the slop it scrolls by how far the cursor is from the press.
        let out = g.drag(
            false,
            true,
            false,
            false,
            false,
            Vec2::new(15.0, 0.0),
            press + Vec2::new(250.0, 0.0),
            window,
        );
        close(out.drag_scroll.x, 0.25, 1e-6);
        assert!(g.owns_mouse(false, true, false));
        // Releasing ends it.
        g.drag(false, false, false, false, false, Vec2::ZERO, press, window);
        assert!(!g.owns_mouse(false, false, false));
        // Both buttons orbit and never scroll.
        let out = g.drag(true, true, false, true, false, Vec2::new(8.0, 3.0), press, window);
        assert_eq!(out.orbit_pixels, Vec2::new(8.0, 3.0));
        assert_eq!(out.drag_scroll, Vec2::ZERO);
        assert!(g.owns_mouse(true, true, false));
        // Left up, right still down: the mouse stays the camera's until the
        // button is released.
        let out = g.drag(false, true, false, false, false, Vec2::new(8.0, 3.0), press, window);
        assert_eq!(out.orbit_pixels, Vec2::ZERO);
        assert!(g.owns_mouse(false, true, false));
        // The middle button orbits too (retail orbit button 0x3E9).
        let mut g = Gesture::default();
        let out = g.drag(false, false, true, false, false, Vec2::new(4.0, 4.0), press, window);
        assert_eq!(out.orbit_pixels, Vec2::new(4.0, 4.0));
        // A press that started over the interface never scrolls.
        let mut g = Gesture::default();
        g.drag(false, true, false, true, true, Vec2::ZERO, press, window);
        let out = g.drag(false, true, false, false, true, Vec2::new(30.0, 0.0), press, window);
        assert_eq!(out.drag_scroll, Vec2::ZERO);
        assert_eq!(
            g.drag(false, true, false, false, false, Vec2::NAN, press, window),
            MouseInput::default()
        );
    }

    #[test]
    fn with_the_tribe_option_the_right_drag_rotates_after_sixteen_pixels() {
        // `OptionTribeControls` on = `UIMachineTribeRotate`: `LongMouseMove 16`
        // turns the right-button drag into the camera's rotation; the click
        // stays a click and nothing scrolls.
        let window = Vec2::new(1000.0, 800.0);
        let press = Vec2::new(500.0, 400.0);
        let mut g = Gesture::default();
        let out = g.drag_with(false, true, false, true, false, Vec2::new(2.0, 1.0), press, window, true);
        assert_eq!(out, MouseInput::default());
        // 14 pixels in all: not yet (and the default scheme would be scrolling).
        let out = g.drag_with(false, true, false, false, false, Vec2::new(11.0, 0.0), press + Vec2::new(13.0, 0.0), window, true);
        assert_eq!(out, MouseInput::default());
        // Past 16: the motion orbits, there is no joystick scroll.
        let out = g.drag_with(false, true, false, false, false, Vec2::new(6.0, 2.0), press + Vec2::new(19.0, 2.0), window, true);
        assert_eq!(out.orbit_pixels, Vec2::new(6.0, 2.0));
        assert_eq!(out.drag_scroll, Vec2::ZERO);
        assert!(g.owns_mouse(false, true, false));
        // It stays a rotation as the cursor moves back.
        let out = g.drag_with(false, true, false, false, false, Vec2::new(-30.0, 0.0), press, window, true);
        assert_eq!(out.orbit_pixels, Vec2::new(-30.0, 0.0));
        assert!(!out.orbit_end);
        // The button coming up ends it (the rotation coasts, 0xB10BA0).
        let out = g.drag_with(false, false, false, false, false, Vec2::ZERO, press, window, true);
        assert!(out.orbit_end);
        assert!(!g.owns_mouse(false, false, false));
        // Off: the same drag scrolls and never orbits.
        let mut g = Gesture::default();
        g.drag_with(false, true, false, true, false, Vec2::ZERO, press, window, false);
        let out = g.drag_with(false, true, false, false, false, Vec2::new(40.0, 0.0), press + Vec2::new(40.0, 0.0), window, false);
        assert_eq!(out.orbit_pixels, Vec2::ZERO);
        assert!(out.drag_scroll.x > 0.0);
        // A press over the interface never rotates.
        let mut g = Gesture::default();
        g.drag_with(false, true, false, true, true, Vec2::ZERO, press, window, true);
        let out = g.drag_with(false, true, false, false, true, Vec2::new(60.0, 0.0), press, window, true);
        assert_eq!(out, MouseInput::default());
    }

    #[test]
    fn edge_scroll_follows_the_script_dwell_ramp_in_the_two_pixel_border() {
        use crate::civ_controls::edge_in_zone;
        let window = Vec2::new(1280.0, 720.0);
        // The cursor's pixel distance to the nearest border, signed, plus 0.01
        // (0xB5D1CB, the camera manager's update: the same for Civ and Tribal).
        close(edge_distance(Vec2::new(0.0, 360.0), window).x, 0.01, 1e-6);
        close(edge_distance(Vec2::new(1.0, 360.0), window).x, 1.01, 1e-6);
        close(edge_distance(Vec2::new(1279.0, 360.0), window).x, -0.99, 1e-5);
        close(edge_distance(Vec2::new(640.0, 0.0), window).y, 0.01, 1e-6);
        close(edge_distance(Vec2::new(640.0, 719.0), window).y, -0.99, 1e-5);
        assert_eq!(edge_distance(Vec2::new(-1.0, 360.0), window).x, 0.0);
        assert_eq!(edge_distance(Vec2::new(1280.0, 360.0), window).x, 0.0);
        assert!(edge_in_zone(edge_distance(Vec2::new(1.0, 360.0), window)));
        assert!(!edge_in_zone(edge_distance(Vec2::new(2.0, 360.0), window)));
        assert!(!edge_in_zone(edge_distance(Vec2::new(640.0, 360.0), window)));
        // The `tribe_camera` block: delta = ((1 - |f|) + dwell) x distance / 300
        // (x) or 200 (y); the dwell timer grows 0.5 a second up to 3 and
        // resets away from the border.
        let eval = |edge: Vec2, scratch: [f32; 5]| {
            builtin(&ScriptInputs {
                distance: 70.0,
                edge,
                dt: 0.1,
                scratch,
                ..default()
            })
        };
        let left = Vec2::new(0.01, 0.0);
        let first = eval(left, [0.0; 5]);
        close(first.anchor_delta.x, ((1.0 - 0.005) + 0.05) * 70.0 / 300.0, 1e-4);
        assert_eq!(first.anchor_delta.y, 0.0);
        let mut scratch = first.scratch;
        for _ in 0..80 {
            scratch = eval(left, scratch).scratch;
        }
        assert_eq!(scratch[0], 3.0, "the dwell timer is capped at the script's limit");
        let right = eval(Vec2::new(-0.99, 0.0), scratch);
        assert!(right.anchor_delta.x < 0.0 && right.scratch[1] > 0.0);
        let top = eval(Vec2::new(0.0, 0.01), [0.0; 5]);
        close(top.anchor_delta.y, ((1.0 - 0.005) + 0.05) * 70.0 / 200.0, 1e-4);
        assert!(top.anchor_delta.x == 0.0);
        assert_eq!(eval(Vec2::ZERO, scratch).scratch[0], 0.0);
    }

    #[test]
    fn edge_scroll_speeds_up_the_longer_the_cursor_stays() {
        let none = KeyInput::default();
        // The left border scrolls left; the dwell ramp (0.5 a second up to 3)
        // speeds it up while the cursor stays.
        let at_left = MouseInput {
            edge: Vec2::new(0.01, 0.0),
            ..default()
        };
        let mut rig = rig_at(70.0);
        let early = drive_rig(&mut rig, none, at_left, 0.5);
        drive_rig(&mut rig, none, at_left, 6.0);
        let late = drive_rig(&mut rig, none, at_left, 1.0);
        assert!(early.x < 0.0 && early.z.abs() < 1e-3, "{early}");
        assert!(late.x < 2.5 * early.x, "{early} then {late}: the dwell ramp speeds it up");
        // At the limit the scroll per update is `((1 - 0.005) + 3) x 70 / 300 x
        // 1500 / 60` = 23.3 units; the angle is 0.047 rad on the flat world, so
        // the segment time is 0.78 s and the chase runs at about 48 units a second.
        close(-late.x, 48.0, 5.0);
        // Leaving the border starts the dwell again and coasts a little.
        let mut rig2 = rig_at(70.0);
        rig2.apply(none, at_left, 1.0 / 60.0);
        assert!(rig2.scratch[0] > 0.0);
        rig2.apply(none, MouseInput::default(), 1.0 / 60.0);
        assert_eq!(rig2.scratch[0], 0.0);
        // The top border scrolls forward (-Z at azimuth 0), the right one right.
        let mut rig = rig_at(70.0);
        let up = drive_rig(&mut rig, none, MouseInput { edge: Vec2::new(0.0, 0.01), ..default() }, 1.0);
        assert!(up.z < 0.0 && up.x.abs() < 1e-3, "{up}");
        let mut rig = rig_at(70.0);
        let right = drive_rig(&mut rig, none, MouseInput { edge: Vec2::new(-0.99, 0.0), ..default() }, 1.0);
        assert!(right.x > 0.0 && right.z.abs() < 1e-3, "{right}");
        // The scroll ends a follow, and no edge value means no scroll.
        let mut rig = rig_at(70.0);
        rig.follow_point(Vec3::new(1.0, 0.0, 1.0));
        assert!(rig.apply(none, at_left, 1.0 / 60.0));
        assert!(!rig.is_following());
        let mut rig = rig_at(70.0);
        assert!(!rig.apply(none, MouseInput { edge: Vec2::NAN, ..default() }, 1.0 / 60.0));
    }

    #[test]
    fn apply_combines_keys_mouse_and_hud_and_reports_a_hand_pan() {
        let mut rig = TribalRig::new();
        rig.place(Vec3::ZERO);
        let none = MouseInput::default();
        // Rotating and zooming are not a pan.
        let keys = KeyInput {
            rotate: 1.0,
            zoom: 1.0,
            ..default()
        };
        let start = rig.goal_azimuth();
        assert!(!rig.apply(keys, none, 0.1));
        assert!(rig.goal_azimuth() > start && rig.goal_distance() < 70.0);
        // A key pan, an edge scroll and a drag scroll are.
        let pan = KeyInput {
            pan: Vec2::Y,
            ..default()
        };
        assert!(rig.apply(pan, none, 0.1));
        let edge = MouseInput {
            edge: Vec2::new(1.01, 0.0),
            ..default()
        };
        assert!(rig.apply(KeyInput::default(), edge, 0.1));
        let drag = MouseInput {
            drag_scroll: Vec2::new(0.5, 0.2),
            ..default()
        };
        assert!(rig.apply(KeyInput::default(), drag, 0.1));
        // The HUD hold buttons zoom and rotate (the flags feed the same reader).
        let hud = HudCamera {
            zoom_out: true,
            rotate_left: true,
            ..default()
        };
        assert!(hud.any());
        assert_eq!(hud.as_keys().zoom, -1.0);
        // 0x04CAB56F (`rotate_left` here) raises flag +0x17 = trigger 0x11, the
        // `,` key (yaw goal shrinks); 0x04CAB56E is the `.` key (0xCC9B30 / 0xB14800).
        assert_eq!(hud.as_keys().rotate, -1.0);
        let other = HudCamera { rotate_right: true, ..default() };
        assert_eq!(other.as_keys().rotate, 1.0);
        let mut pressed = ButtonInput::<KeyCode>::default();
        pressed.press(KeyCode::Period);
        assert_eq!(read_keys(&pressed).rotate, other.as_keys().rotate);
        assert!(!HudCamera::default().any());
        // The wheel zooms.
        let before = rig.goal_distance();
        let wheel = MouseInput {
            wheel_lines: 2.0,
            ..default()
        };
        rig.apply(KeyInput::default(), wheel, 0.1);
        assert!(rig.goal_distance() < before);
        // A hostile frame time changes nothing.
        let (az, focus) = (rig.goal_azimuth(), rig.goal_focus());
        rig.apply(keys, drag, f32::NAN);
        assert_eq!(rig.goal_focus(), focus);
        assert_eq!(rig.goal_azimuth(), az);
    }

    #[test]
    fn the_builtin_numbers_are_the_script_arithmetic() {
        let out = builtin(&ScriptInputs {
            distance: 70.0,
            scroll: Vec2::new(10.0, -5.0),
            rotate: 1.0,
            ..default()
        });
        close(out.scroll.x, -28.0, 1e-4);
        close(out.scroll.y, 14.0, 1e-4);
        close(out.rotate_deg, 5.0, 1e-6);
        close(out.mouse_scroll_sensitivity, 90.0, 1e-4);
        assert_eq!((out.min_pitch_deg, out.max_pitch_deg), (-20.0, 60.0));
        assert_eq!(out.acceleration_time, 0.7);
        assert_eq!(out.translation_speed, 10.0);
        assert_eq!((out.min_interpolation, out.max_interpolation), (0.7, 6.0));
        assert!(!TribalScript::default().is_installed());
    }

    /// The installed `tribe_camera` script and the Rust numbers agree on a grid
    /// of inputs (the data check of this module).
    #[test]
    #[ignore = "requires the installed Spore packages"]
    fn installed_tribe_camera_script_matches_the_builtin_numbers() {
        use crate::{
            arithmetica::{GROUP_SCRIPTS, TYPE_ARITHMETICA},
            dbpf::ResourceKey,
            world_objects::name_id,
        };
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let bytes = index
            .read(ResourceKey {
                type_id: TYPE_ARITHMETICA,
                group_id: GROUP_SCRIPTS,
                instance_id: name_id("tribe_camera"),
            })
            .expect("tribe_camera is installed");
        let mut script = CameraScript::from_bytes(&bytes).unwrap();
        assert!(TribalScript::from_script(script.clone()).is_installed());
        let mut checked = 0;
        for distance in [
            8.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 200.0,
        ] {
            for direction in [-1.0, 0.0, 1.0] {
                let input = ScriptInputs {
                    direction,
                    distance,
                    scroll: Vec2::new(7.5, -2.5),
                    rotate: 0.6,
                    dt: 1.0 / 60.0,
                    ..default()
                };
                let from_script = script.evaluate(&input);
                let from_rust = builtin(&input);
                let label = format!("distance {distance} direction {direction}");
                close(from_script.distance, from_rust.distance, 1e-3);
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
        println!("{checked} script evaluations match the Rust numbers");
        // The edge scroll block (anchor_delta and the dwell timers it keeps).
        let mut edge_checked = 0;
        for edge in [
            Vec2::ZERO,
            Vec2::new(0.01, 0.0),
            Vec2::new(1.01, 0.0),
            Vec2::new(-0.99, 0.0),
            Vec2::new(0.0, 0.01),
            Vec2::new(0.0, -0.99),
            Vec2::new(1.5, -1.5),
            Vec2::new(1.99, 0.01),
            Vec2::new(2.5, 300.0),
        ] {
            for distance in [15.0, 70.0, 90.0] {
                for scratch in [[0.0; 5], [0.2, 0.1, 0.3, 0.4, 0.0], [3.0, 3.0, 3.0, 3.0, 0.0]] {
                    let input = ScriptInputs {
                        distance,
                        edge,
                        dt: 0.1,
                        scratch,
                        ..default()
                    };
                    let from_script = script.evaluate(&input);
                    let from_rust = builtin(&input);
                    let label = format!("edge {edge} distance {distance} scratch {scratch:?}");
                    close(from_script.anchor_delta.x, from_rust.anchor_delta.x, 1e-4);
                    close(from_script.anchor_delta.y, from_rust.anchor_delta.y, 1e-4);
                    for i in 0..5 {
                        assert!(
                            (from_script.scratch[i] - from_rust.scratch[i]).abs() < 1e-5,
                            "{label}: scratch {i}"
                        );
                    }
                    edge_checked += 1;
                }
            }
        }
        println!("{edge_checked} edge scroll evaluations match the Rust block");
    }

    #[test]
    fn citizen_action_ids_are_the_modapi_values() {
        let ids: Vec<i32> = CitizenAction::ALL.iter().map(|a| a.id()).collect();
        assert_eq!(
            ids,
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 20, 21, 24, 25, 26, 27, 28]
        );
        assert_eq!(CitizenAction::Recruit.id(), 26);
        assert_eq!(CitizenAction::ReturnToTribe.id(), 28);
        // Seventeen sounds, one per named action; the walk, tool, sleep and
        // return actions are silent.
        let mut names: Vec<&str> = CitizenAction::ALL.iter().filter_map(|a| a.sound()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 17);
        assert!(names.iter().all(|n| n.starts_with("sfx_trg_action_")));
        assert_eq!(CitizenAction::WalkTo.sound(), None);
        assert_eq!(CitizenAction::GrabTool.sound(), None);
        assert_eq!(CitizenAction::Gather.sound(), Some("sfx_trg_action_gather"));
        assert_eq!(CitizenAction::RaidHuts.sound(), Some("sfx_trg_action_raid"));
        // Tool actions of the input strategy.
        assert_eq!(tribe_input_action(ToolKind::SpikedClubs), 19);
        assert_eq!(tribe_input_action(ToolKind::HealingRods), 25);
        assert_eq!(tribe_input_action(ToolKind::Campfire), 26);
    }

    #[test]
    fn issued_commands_track_a_pending_social_order() {
        let mut controls = TribalControls::default();
        let perform = ContextCommand {
            action: CitizenAction::Recruit,
            order: Order::Perform(3),
        };
        controls.issue(perform, 2, Vec3::X);
        assert_eq!(controls.pending_perform, Some(3));
        assert_eq!(controls.issued.len(), 1);
        assert_eq!(controls.issued[0].count, 2);
        // Any other order cancels it.
        let walk = ContextCommand {
            action: CitizenAction::WalkTo,
            order: Order::Move(Vec3::Z),
        };
        controls.issue(walk, 1, Vec3::Z);
        assert_eq!(controls.pending_perform, None);
        assert_eq!(controls.issued.len(), 2);
    }

    #[test]
    fn follow_cycles_through_the_roster_and_wraps() {
        let mut w = World::new();
        let a = w.spawn_empty().id();
        let b = w.spawn_empty().id();
        let c = w.spawn_empty().id();
        let order = [a, b, c];
        assert_eq!(cycle_follow(&order, None), Some(a));
        assert_eq!(cycle_follow(&order, Some(a)), Some(b));
        assert_eq!(cycle_follow(&order, Some(c)), Some(a));
        // A member that left the roster restarts the cycle.
        let gone = w.spawn_empty().id();
        assert_eq!(cycle_follow(&order, Some(gone)), Some(a));
        assert_eq!(cycle_follow(&[], Some(a)), None);
    }

    // --- through the real schedule -----------------------------------------

    use crate::runtime::headless_app;

    fn step(app: &mut App) {
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }

    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        step(app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(key);
    }

    fn hold(app: &mut App, keys: &[KeyCode], frames: usize) {
        for _ in 0..frames {
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            for key in keys {
                input.press(*key);
            }
            step(app);
        }
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        for key in keys {
            input.release(*key);
        }
    }

    fn tribal_app() -> App {
        let mut app = headless_app(RunProgress::default());
        app.world_mut()
            .resource_mut::<RunProgress>()
            .enter_stage(GameStage::Tribal);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Tribal);
        for _ in 0..3 {
            step(&mut app);
        }
        app
    }

    fn chieftain_at(app: &mut App) -> Vec3 {
        let world = app.world_mut();
        world
            .query_filtered::<&Transform, With<Player>>()
            .single(world)
            .expect("the chieftain")
            .translation
    }

    fn selected_count(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query_filtered::<(), With<Selected>>().iter(world).count()
    }

    #[test]
    fn wasd_pans_the_camera_and_the_chieftain_stays_put() {
        let mut app = tribal_app();
        let chief = chieftain_at(&mut app);
        let start = app.world().resource::<TribalCamera>().rig.goal_focus();
        assert!(app.world().resource::<TribalCamera>().rig.is_ready());
        close(start.x, start_focus().x, 0.01);
        close(start.z, start_focus().z, 0.01);
        // "Forward" and "right" are the rig's own ground axes (tangent to the
        // ground at the focus), whatever the start azimuth.
        let (forward, right) = app.world().resource::<TribalCamera>().rig.ground_axes();
        hold(&mut app, &[KeyCode::KeyW], 60);
        let focus = app.world().resource::<TribalCamera>().rig.goal_focus();
        assert!((focus - start).dot(forward) > 5.0, "W pans forward: {focus} from {start}");
        hold(&mut app, &[KeyCode::KeyD], 60);
        let focus = app.world().resource::<TribalCamera>().rig.goal_focus();
        assert!((focus - start).dot(right) > 5.0, "D pans right");
        assert_eq!(chieftain_at(&mut app), chief, "nobody walks with WASD");
    }

    #[test]
    fn q_and_e_pan_sideways_and_no_longer_select_or_perform() {
        let mut app = tribal_app();
        assert_eq!(selected_count(&mut app), 0);
        let start = app.world().resource::<TribalCamera>().rig.goal_focus();
        let (_, right) = app.world().resource::<TribalCamera>().rig.ground_axes();
        hold(&mut app, &[KeyCode::KeyQ], 60);
        assert_eq!(selected_count(&mut app), 0, "Q is strafe, not select");
        let moved = app.world().resource::<TribalCamera>().rig.goal_focus() - start;
        assert!(moved.dot(right) < -3.0, "{moved}");
        let before = app
            .world()
            .resource::<RunProgress>()
            .tribal_campaign
            .performance;
        hold(&mut app, &[KeyCode::KeyE], 60);
        assert_eq!(
            app.world().resource::<RunProgress>().tribal_campaign.performance,
            before
        );
    }

    #[test]
    fn camera_keys_rotate_pitch_and_zoom_in_the_schedule() {
        let mut app = tribal_app();
        let start = app.world().resource::<TribalCamera>().rig.goal_azimuth();
        hold(&mut app, &[KeyCode::Period], 60);
        let rig = app.world().resource::<TribalCamera>().rig.clone();
        assert!(
            rig.goal_azimuth() > start + 0.5,
            "`.` rotates left (the yaw goal grows)"
        );
        hold(&mut app, &[KeyCode::PageUp], 30);
        assert!(
            app.world().resource::<TribalCamera>().rig.goal_pitch() < rig.goal_pitch(),
            "PageUp lowers the elevation"
        );
        let before = app.world().resource::<TribalCamera>().rig.goal_distance();
        assert_eq!(before, 70.0, "the stage opens with the script's default distance");
        hold(&mut app, &[KeyCode::Equal], 30);
        let zoomed = app.world().resource::<TribalCamera>().rig.goal_distance();
        assert!(zoomed < 70.0 && tuning::ZOOM_LADDER.contains(&zoomed), "{zoomed}");
        hold(&mut app, &[KeyCode::Minus], 120);
        assert!(app.world().resource::<TribalCamera>().rig.goal_distance() > zoomed);
        let rig = app.world().resource::<TribalCamera>().rig.clone();
        hold(&mut app, &[KeyCode::ShiftLeft, KeyCode::ArrowRight], 90);
        // Shift+Right rotates right (and does not pan).
        let after = app.world().resource::<TribalCamera>().rig.clone();
        assert!(after.goal_azimuth() < rig.goal_azimuth());
        assert_eq!(after.goal_focus(), rig.goal_focus());
    }

    #[test]
    fn the_hud_key_and_ctrl_a_select_the_whole_tribe_and_escape_deselects() {
        let mut app = tribal_app();
        let units = {
            let world = app.world_mut();
            world.query_filtered::<(), With<TribeUnit>>().iter(world).count()
        };
        assert!(units >= 2);
        tap(&mut app, KEY_SELECT_TRIBE);
        assert_eq!(selected_count(&mut app), units);
        // Esc deselects everyone and does not open the pause menu.
        tap(&mut app, KeyCode::Escape);
        assert_eq!(selected_count(&mut app), 0);
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
        // Ctrl+A is the keyboard form.
        {
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            input.press(KeyCode::ControlLeft);
            input.press(KeyCode::KeyA);
        }
        step(&mut app);
        step(&mut app);
        assert_eq!(selected_count(&mut app), units);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::ControlLeft);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyA);
        tap(&mut app, KeyCode::Escape);
        // With nothing selected the next Esc opens the pause menu.
        tap(&mut app, KeyCode::Escape);
        assert!(app.world().resource::<crate::pause_menu::PauseMenu>().open);
    }

    #[test]
    fn the_stance_keys_select_a_stance_and_c_still_toggles() {
        let mut app = tribal_app();
        let social = |app: &App| {
            app.world()
                .resource::<RunProgress>()
                .tribal_campaign
                .friendly_stance
        };
        tap(&mut app, KEY_STANCE_SOCIAL);
        assert!(social(&app));
        tap(&mut app, KEY_STANCE_SOCIAL);
        assert!(social(&app), "the social button selects, never toggles");
        tap(&mut app, KEY_STANCE_ATTACK);
        assert!(!social(&app));
        tap(&mut app, KeyCode::KeyC);
        assert!(social(&app), "C toggles");
    }

    #[test]
    fn f8_toggles_edge_scroll() {
        let mut app = tribal_app();
        assert!(!app.world().resource::<TribalControls>().edge_scroll);
        tap(&mut app, KEY_EDGE_SCROLL);
        assert!(app.world().resource::<TribalControls>().edge_scroll);
        tap(&mut app, KEY_EDGE_SCROLL);
        assert!(!app.world().resource::<TribalControls>().edge_scroll);
    }

    #[test]
    fn space_follows_a_member_and_the_next_press_the_next_one() {
        let mut app = tribal_app();
        tap(&mut app, KeyCode::Space);
        let first = app.world().resource::<TribalControls>().follow;
        assert!(first.is_some());
        let first_position = {
            let world = app.world();
            world.get::<Transform>(first.unwrap()).unwrap().translation
        };
        for _ in 0..30 {
            step(&mut app);
        }
        let focus = app.world().resource::<TribalCamera>().rig.goal_focus();
        close(focus.x, first_position.x, 0.3);
        tap(&mut app, KeyCode::Space);
        let second = app.world().resource::<TribalControls>().follow;
        assert!(second.is_some() && second != first);
        // Panning by hand ends the follow.
        hold(&mut app, &[KeyCode::KeyA], 5);
        assert_eq!(app.world().resource::<TribalControls>().follow, None);
    }

    #[test]
    fn the_minimap_moves_the_camera_and_sends_the_selection() {
        let mut app = tribal_app();
        tap(&mut app, KEY_SELECT_TRIBE);
        app.world_mut().resource_mut::<TribalControls>().goto_request =
            Some(Vec3::new(-30.0, 0.0, 20.0));
        step(&mut app);
        let rig = app.world().resource::<TribalCamera>().rig.clone();
        close(rig.focus().x, -30.0, 1e-3);
        close(rig.focus().z, 20.0, 1e-3);
        app.world_mut().resource_mut::<TribalControls>().move_request =
            Some(Vec3::new(10.0, 0.0, 10.0));
        step(&mut app);
        let world = app.world_mut();
        let moving = world
            .query::<(&UnitTask, Has<Selected>)>()
            .iter(world)
            .filter(|(task, selected)| *selected && matches!(task, UnitTask::MoveTo(p) if p.x == 10.0))
            .count();
        assert!(moving >= 2, "the selection walks to the minimap point");
        assert!(world
            .resource::<TribalControls>()
            .issued
            .iter()
            .any(|i| i.action == CitizenAction::WalkTo));
    }

    /// Run `tribal_rts::command_at` for the selected units as a one-shot
    /// system (the click resolution the right button uses).
    fn right_click(app: &mut App, at: Vec3, world: ClickWorld) {
        use bevy::ecs::system::RunSystemOnce;
        app.world_mut()
            .run_system_once(
                move |mut commands: Commands,
                      mut run: ResMut<RunProgress>,
                      mut controls: ResMut<TribalControls>,
                      mut units: Query<
                    (
                        Entity,
                        &Transform,
                        &mut TribeUnit,
                        &mut UnitTask,
                        Option<&Selected>,
                        Option<&crate::persistence::WorldId>,
                    ),
                    Without<crate::tribal_rts::RivalMember>,
                >| {
                    let selected: Vec<Entity> = units
                        .iter()
                        .filter(|u| u.4.is_some())
                        .map(|u| u.0)
                        .collect();
                    crate::tribal_rts::command_at(
                        at,
                        &selected,
                        &world,
                        &mut units,
                        &mut run,
                        &mut commands,
                        &mut controls,
                    );
                },
            )
            .expect("the click system runs");
    }

    #[test]
    fn a_right_click_commands_the_selection_with_one_named_action() {
        let mut app = tribal_app();
        // Nothing selected: nothing happens.
        let bush = app.world_mut().spawn_empty().id();
        let click = || ClickWorld {
            bushes: vec![(bush, Vec3::new(8.0, 0.0, 8.0), 50.0)],
            ..default()
        };
        right_click(&mut app, Vec3::new(8.0, 0.0, 8.5), click());
        assert!(app.world().resource::<TribalControls>().issued.is_empty());
        // The members selected: every one of them gathers, and the command is
        // one Gather action.
        tap(&mut app, KEY_SELECT_TRIBE);
        right_click(&mut app, Vec3::new(8.0, 0.0, 8.5), click());
        let issued = app.world().resource::<TribalControls>().issued.clone();
        assert_eq!(issued.len(), 1, "{issued:?}");
        assert_eq!(issued[0].action, CitizenAction::Gather);
        let world = app.world_mut();
        let gathering = world
            .query::<(&UnitTask, &TribeUnit)>()
            .iter(world)
            .filter(|(task, unit)| !unit.chieftain && matches!(task, UnitTask::Gather(e) if *e == bush))
            .count();
        assert!(gathering >= 2);
        // Walking: the ground.
        right_click(&mut app, Vec3::new(-10.0, 0.0, 3.0), ClickWorld::default());
        let issued = app.world().resource::<TribalControls>().issued.clone();
        let walk = issued.last().expect("the walk was recorded");
        assert_eq!(walk.action, CitizenAction::WalkTo);
        assert_eq!(walk.count, {
            let world = app.world_mut();
            world.query_filtered::<(), With<Selected>>().iter(world).count()
        });
    }

    /// tribal_click: the right click follows the retail ids the cursor shows
    /// (`tribal_hud::click`), including the diet gate.
    #[test]
    fn a_right_click_follows_the_diet_the_stance_and_the_object() {
        use crate::creature_hud::Diet;
        let mut app = tribal_app();
        tap(&mut app, KEY_SELECT_TRIBE);
        let bush = app.world_mut().spawn_empty().id();
        let egg = app.world_mut().spawn_empty().id();
        let bush_at = Vec3::new(12.0, 0.0, 12.0);
        let egg_at = Vec3::new(-12.0, 0.0, 12.0);
        let world = |diet| ClickWorld {
            bushes: vec![(bush, bush_at, 50.0), (egg, egg_at, 5.0)],
            eggs: vec![egg],
            diet,
            ..default()
        };
        let issued = |app: &App| app.world().resource::<TribalControls>().issued.clone();
        // A carnivore tribe does not gather from a bush (0xCCFC0D: herbivorous
        // mouth needed): the click does nothing at all.
        right_click(&mut app, bush_at, world(Some(Diet::Carnivore)));
        assert!(issued(&app).is_empty(), "{:?}", issued(&app));
        // A herbivore does; so does an omnivore.
        right_click(&mut app, bush_at, world(Some(Diet::Herbivore)));
        assert_eq!(issued(&app).len(), 1);
        assert_eq!(issued(&app)[0].action, CitizenAction::Gather);
        // Eggs are gathered with the attack stance only, as GatherMeat.
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.friendly_stance = true;
        right_click(&mut app, egg_at, world(None));
        assert_eq!(issued(&app).len(), 1, "the social stance does not gather eggs");
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.friendly_stance = false;
        right_click(&mut app, egg_at, world(None));
        assert_eq!(issued(&app).last().map(|i| i.action), Some(CitizenAction::GatherMeat));
        // The hut shows its card and does nothing; the food mat is eaten at.
        let before = issued(&app).len();
        right_click(&mut app, crate::tribal::home_position(), world(None));
        assert_eq!(issued(&app).len(), before, "the hut card is not a command");
        right_click(&mut app, crate::tribal::food_pile_position(), world(None));
        assert_eq!(issued(&app).last().map(|i| i.action), Some(CitizenAction::Eat));
    }

    /// tribal_click: a gift (0xE / 0x10) needs `giftFoodAmount` food first and
    /// the hint trg-notenoughfoodforgift otherwise (0xCD0DC9 / 0xCD0FFE).
    #[test]
    fn a_gift_click_needs_the_food_and_a_tribe_below_cautious() {
        let mut app = tribal_app();
        tap(&mut app, KEY_SELECT_TRIBE);
        {
            let run = &mut *app.world_mut().resource_mut::<RunProgress>();
            run.tribal_campaign.friendly_stance = true;
            let state = &mut run.tribal_campaign.villages[1];
            state.appeared = true;
            state.status = crate::tribal::RivalStatus::Hostile;
            state.relations = -50.0;
        }
        let at = tribal::village_position(1) + Vec3::new(1.0, 0.0, 1.0);
        let member = app.world_mut().spawn_empty().id();
        let click = || ClickWorld {
            foes: vec![(member, at)],
            rivals: vec![(member, 1)],
            ..default()
        };
        // Too poor: the hint, no gift, no order.
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = crate::tribal::GIFT_FOOD - 1.0;
        right_click(&mut app, at, click());
        let run = app.world().resource::<RunProgress>();
        assert!(run.tribal_campaign.message.contains("Not enough food"), "{}", run.tribal_campaign.message);
        assert_eq!(run.dna_this_stage, crate::tribal::GIFT_FOOD - 1.0);
        assert!(app.world().resource::<TribalControls>().issued.is_empty());
        // Enough food: one member is sent to carry the bundle (the food leaves
        // the stock when it is picked up at the mat, the relationship moves when
        // it is put down on the other tribe's mat, 0xDBB7D0), the rest walk over.
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 50.0;
        right_click(&mut app, at, click());
        let run = app.world().resource::<RunProgress>();
        assert!(
            run.dna_this_stage == 50.0 || run.dna_this_stage == 50.0 - crate::tribal::GIFT_FOOD,
            "{}",
            run.dna_this_stage
        );
        assert_eq!(run.tribal_campaign.villages[1].relations, -50.0, "nothing is delivered yet");
        let carriers = app
            .world_mut()
            .query::<&UnitTask>()
            .iter(app.world())
            .filter(|task| matches!(task, UnitTask::Gift(1)))
            .count();
        assert_eq!(carriers, 1, "one member carries the gift");
        let issued = app.world().resource::<TribalControls>().issued.clone();
        assert_eq!(issued.len(), 1);
        assert_eq!(issued[0].action, CitizenAction::GiftFood);
        // A Cautious tribe is not offered gifts: its hut is the wrong target.
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.villages[1].relations = 0.0;
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.cooldown = 0.0;
        let before = app.world().resource::<RunProgress>().dna_this_stage;
        right_click(&mut app, tribal::village_position(1), ClickWorld::default());
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.dna_this_stage, before);
        assert!(run.tribal_campaign.message.contains("member"), "{}", run.tribal_campaign.message);
    }

    #[test]
    fn performing_for_a_village_needs_the_chieftain_selected() {
        let mut app = tribal_app();
        app.world_mut()
            .resource_mut::<RunProgress>()
            .tribal_campaign
            .friendly_stance = true;
        // NPC tribes appear over time (`tribal_rules`): bring V2 into view.
        app.world_mut()
            .resource_mut::<RunProgress>()
            .tribal_campaign
            .villages[1]
            .appeared = true;
        {
            // A Cautious tribe: its members are social targets (tribal_click:
            // the right click follows the cursor's retail ids; the hut is not).
            let state = &mut app.world_mut().resource_mut::<RunProgress>().tribal_campaign.villages[1];
            state.relations = 0.0;
            state.status = crate::tribal::RivalStatus::Hostile;
        }
        let village = tribal::village_position(1) + Vec3::new(1.0, 0.0, 1.0);
        let member = app.world_mut().spawn_empty().id();
        let click = || ClickWorld {
            foes: vec![(member, village)],
            rivals: vec![(member, 1)],
            ..default()
        };
        // Only the members selected (select all, then take the chieftain out).
        tap(&mut app, KEY_SELECT_TRIBE);
        {
            let world = app.world_mut();
            let chief = world
                .query_filtered::<Entity, With<Player>>()
                .single(world)
                .unwrap();
            world.entity_mut(chief).remove::<Selected>();
        }
        right_click(&mut app, village, click());
        assert!(app.world().resource::<TribalControls>().issued.is_empty());
        assert!(app
            .world()
            .resource::<RunProgress>()
            .tribal_campaign
            .message
            .contains("chieftain"));
        // With the chieftain selected too the order goes out and waits for his
        // arrival.
        {
            let world = app.world_mut();
            let chief = world
                .query_filtered::<Entity, With<Player>>()
                .single(world)
                .unwrap();
            world.entity_mut(chief).insert(Selected);
        }
        right_click(&mut app, village, click());
        let controls = app.world().resource::<TribalControls>();
        assert_eq!(controls.issued.len(), 1);
        assert_eq!(controls.issued[0].action, CitizenAction::Recruit);
        assert_eq!(controls.pending_perform, Some(1));
        let world = app.world_mut();
        assert!(world
            .query::<&UnitTask>()
            .iter(world)
            .any(|task| *task == UnitTask::Perform(1)));
    }

    #[test]
    fn a_pending_performance_starts_when_the_chieftain_arrives() {
        let mut app = tribal_app();
        let village = 0;
        // NPC tribes appear over time (`tribal_rules`): bring V1 into view.
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.villages[village].appeared = true;
        let at = tribal::village_position(village);
        app.world_mut().resource_mut::<TribalControls>().pending_perform = Some(village);
        // Far away: nothing happens.
        for _ in 0..30 {
            step(&mut app);
        }
        assert!(app.world().resource::<RunProgress>().tribal_campaign.performance.is_none());
        assert_eq!(app.world().resource::<TribalControls>().pending_perform, Some(village));
        // Standing at the village the perform key is pressed and the song
        // (or the reason it cannot start yet) is reported.
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Transform, With<Player>>();
            players.single_mut(world).unwrap().translation = at + Vec3::new(0.0, 0.0, -4.0);
        }
        let mut started = false;
        for _ in 0..240 {
            step(&mut app);
            started |= app
                .world()
                .resource::<RunProgress>()
                .tribal_campaign
                .performance
                .is_some();
            if started {
                break;
            }
        }
        // The first village needs no instrument: the performance starts and
        // the pending order is dropped.
        assert!(started, "{}", app.world().resource::<RunProgress>().tribal_campaign.message);
        step(&mut app);
        assert_eq!(app.world().resource::<TribalControls>().pending_perform, None);
    }
}
