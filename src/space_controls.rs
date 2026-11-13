//! Space stage keys, mouse and flight, after the retail trigger files
//! (`TriggerConfigSpaceGame` / `TriggerConfigSpaceFlight` plus
//! `TriggerConfigWASD`) and the UFO tuning (`spaceufo~`), S6.
//!
//! The retail input layer maps keys to numbered triggers and the game polls
//! them (`cGameInputManager::IsTriggered`, 0xFFE700). Here the same triggers
//! are read from the keyboard into `SpaceControls::triggers`; the camera
//! (`space_view`) and the ship (`planet_flight`, `manual_flight`) consume them.
//!
//! | trigger | keys | planet view | solar system / galaxy |
//! | --- | --- | --- | --- |
//! | 1 / 2 | W Up Numpad8 / S Down Numpad2 | fly forward / back | raise / lower the camera |
//! | 3 / 4 | A Left Numpad4 / D Right Numpad6 | turn left / right (Flight scheme: strafe) | rotate the camera |
//! | 5 / 6 | Q / E | strafe left / right | - |
//! | 7 / 8 | PageUp Numpad9 / PageDown Numpad3 | camera up / down | camera up / down |
//! | 9 / 0x10 | X + = Numpad+ / Space - Numpad- | zoom in / out | zoom in / out |
//! | 0x11 / 0x12 | `,` / `.` | rotate the camera | rotate the camera |
//! | 0x11111111 / 0x22222222 | End Ctrl+= / Home Ctrl+- | fly down / up a level | discrete zoom in / out |
//!
//! Mouse (Game scheme, the default): left = tools / pick, right = fly toward
//! the cursor (planet: thrusters while held; solar system: fly there), middle
//! = camera drag. Flight scheme: right and middle drag the camera, WASD only.

use bevy::prelude::*;

use crate::game::{Planet, Player};
use crate::persistence::WorldId;
use crate::space::ShipMotion;
use crate::space_view::{tangent, PlanetView, SpaceView, UFO_MAX_ALTITUDE, UFO_MIN_ALTITUDE};
use crate::stages::GameStage;

// ---------------------------------------------------------------------------
// Triggers
// ---------------------------------------------------------------------------

/// The retail trigger ids this stage reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TriggerSet(pub u32);

impl TriggerSet {
    pub const NONE: TriggerSet = TriggerSet(0);
    pub const FORWARD: TriggerSet = TriggerSet(1 << 0);
    pub const BACK: TriggerSet = TriggerSet(1 << 1);
    pub const LEFT: TriggerSet = TriggerSet(1 << 2);
    pub const RIGHT: TriggerSet = TriggerSet(1 << 3);
    pub const STRAFE_LEFT: TriggerSet = TriggerSet(1 << 4);
    pub const STRAFE_RIGHT: TriggerSet = TriggerSet(1 << 5);
    pub const PITCH_UP: TriggerSet = TriggerSet(1 << 6);
    pub const PITCH_DOWN: TriggerSet = TriggerSet(1 << 7);
    pub const ZOOM_IN: TriggerSet = TriggerSet(1 << 8);
    pub const ZOOM_OUT: TriggerSet = TriggerSet(1 << 9);
    pub const ROTATE_LEFT: TriggerSet = TriggerSet(1 << 10);
    pub const ROTATE_RIGHT: TriggerSet = TriggerSet(1 << 11);
    pub const QUICK_IN: TriggerSet = TriggerSet(1 << 12);
    pub const QUICK_OUT: TriggerSet = TriggerSet(1 << 13);

    /// The retail trigger id of a single trigger (documentation and tests).
    pub fn retail_id(self) -> u32 {
        match self {
            Self::FORWARD => 0x1,
            Self::BACK => 0x2,
            Self::LEFT => 0x3,
            Self::RIGHT => 0x4,
            Self::STRAFE_LEFT => 0x5,
            Self::STRAFE_RIGHT => 0x6,
            Self::PITCH_UP => 0x7,
            Self::PITCH_DOWN => 0x8,
            Self::ZOOM_IN => 0x9,
            Self::ZOOM_OUT => 0x10,
            Self::ROTATE_LEFT => 0x11,
            Self::ROTATE_RIGHT => 0x12,
            Self::QUICK_IN => 0x1111_1111,
            Self::QUICK_OUT => 0x2222_2222,
            _ => 0,
        }
    }

    pub fn has(self, other: TriggerSet) -> bool {
        self.0 & other.0 != 0
    }

    pub fn set(&mut self, other: TriggerSet) {
        self.0 |= other.0;
    }

    pub fn zoom_in(self) -> bool {
        self.has(Self::ZOOM_IN)
    }

    pub fn zoom_out(self) -> bool {
        self.has(Self::ZOOM_OUT)
    }
}

/// The two trigger files, chosen by the option 0x0679B880 (1 = Flight; the
/// Settings > Control > Space check box, `control_options::space_flight`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControlScheme {
    /// `TriggerConfigSpaceGame`: right button flies, middle drags the camera,
    /// A / D turn the UFO.
    #[default]
    Game,
    /// `TriggerConfigSpaceFlight`: right and middle drag the camera, WASD
    /// fly relative to the camera.
    Flight,
}

impl ControlScheme {
    /// The retail option value (0x0679B880): 1 is Flight, anything else Game.
    pub fn from_option(value: u32) -> Self {
        if value == 1 {
            Self::Flight
        } else {
            Self::Game
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Game => "Space controls: game (right mouse flies)",
            Self::Flight => "Space controls: flight (WASD relative to the camera)",
        }
    }

    /// The button that flies toward the cursor (0xCCCCCCCC): Game only.
    pub fn move_button(self) -> Option<MouseButton> {
        match self {
            Self::Game => Some(MouseButton::Right),
            Self::Flight => None,
        }
    }

    /// Whether a mouse button drags the camera (0xCDCDCDCD).
    pub fn camera_button(self, button: MouseButton) -> bool {
        match self {
            Self::Game => button == MouseButton::Middle,
            Self::Flight => matches!(button, MouseButton::Middle | MouseButton::Right),
        }
    }
}

fn any(keys: &ButtonInput<KeyCode>, list: &[KeyCode]) -> bool {
    list.iter().any(|key| keys.pressed(*key))
}

fn ctrl(keys: &ButtonInput<KeyCode>) -> bool {
    any(keys, &[KeyCode::ControlLeft, KeyCode::ControlRight])
}

/// Ctrl + Alt held: the debug / legacy-editor chord. The original ignores the
/// trigger files here (its cheats use it), so the camera and flight keys do
/// nothing, and the old keyboard ship loadout keys that share keys with the
/// camera (`,` `.` `-` `=` Backspace) work only with it.
pub fn debug_chord(keys: &ButtonInput<KeyCode>) -> bool {
    ctrl(keys) && any(keys, &[KeyCode::AltLeft, KeyCode::AltRight])
}

/// The held triggers (TriggerConfigSpaceGame/Flight with TriggerConfigWASD;
/// the two Space files differ only in their mouse lines).
pub fn read_triggers(keys: &ButtonInput<KeyCode>) -> TriggerSet {
    let mut t = TriggerSet::NONE;
    if debug_chord(keys) {
        return t;
    }
    let table: &[(TriggerSet, &[KeyCode])] = &[
        (TriggerSet::FORWARD, &[KeyCode::KeyW, KeyCode::ArrowUp, KeyCode::Numpad8]),
        (TriggerSet::BACK, &[KeyCode::KeyS, KeyCode::ArrowDown, KeyCode::Numpad2]),
        (TriggerSet::LEFT, &[KeyCode::KeyA, KeyCode::ArrowLeft, KeyCode::Numpad4]),
        (TriggerSet::RIGHT, &[KeyCode::KeyD, KeyCode::ArrowRight, KeyCode::Numpad6]),
        (TriggerSet::STRAFE_LEFT, &[KeyCode::KeyQ]),
        (TriggerSet::STRAFE_RIGHT, &[KeyCode::KeyE]),
        (TriggerSet::PITCH_UP, &[KeyCode::PageUp, KeyCode::Numpad9]),
        (TriggerSet::PITCH_DOWN, &[KeyCode::PageDown, KeyCode::Numpad3]),
        (TriggerSet::ZOOM_IN, &[KeyCode::KeyX]),
        (TriggerSet::ZOOM_OUT, &[KeyCode::Space]),
        (TriggerSet::ROTATE_LEFT, &[KeyCode::Comma]),
        (TriggerSet::ROTATE_RIGHT, &[KeyCode::Period]),
    ];
    for (trigger, list) in table {
        if any(keys, list) {
            t.set(*trigger);
        }
    }
    // `+` and `-` with shift or alt are the plain zoom; with control they are
    // the discrete zoom.
    let plus = any(keys, &[KeyCode::Equal, KeyCode::NumpadAdd]);
    let minus = any(keys, &[KeyCode::Minus, KeyCode::NumpadSubtract]);
    if ctrl(keys) {
        if plus {
            t.set(TriggerSet::QUICK_IN);
        }
        if minus {
            t.set(TriggerSet::QUICK_OUT);
        }
    } else {
        if plus {
            t.set(TriggerSet::ZOOM_IN);
        }
        if minus {
            t.set(TriggerSet::ZOOM_OUT);
        }
    }
    if keys.pressed(KeyCode::End) {
        t.set(TriggerSet::QUICK_IN);
    }
    if keys.pressed(KeyCode::Home) {
        t.set(TriggerSet::QUICK_OUT);
    }
    t
}

/// The discrete zoom keys' edge this frame: +1 in, -1 out (the original fires
/// once per press, 0xFFE700 flag +0x56).
pub fn quick_zoom_edge(keys: &ButtonInput<KeyCode>) -> i8 {
    if debug_chord(keys) {
        return 0;
    }
    let c = ctrl(keys);
    let in_pressed = keys.just_pressed(KeyCode::End)
        || (c && (keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd)));
    let out_pressed = keys.just_pressed(KeyCode::Home)
        || (c && (keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract)));
    if in_pressed {
        1
    } else if out_pressed {
        -1
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

/// The HUD's four camera buttons (zoom in 0x4CAB570 / 0x5CAB570, zoom out
/// ..571, rotate left ..56E, right ..56F). The retail buttons feed the
/// controller while pressed: a zoom button steps one discrete level per press,
/// the rotate buttons turn the camera while held. The HUD sets the `*_edge`
/// flags for the frame of a press and the rotate flags every frame of a hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CameraButtons {
    pub zoom_in_edge: bool,
    pub zoom_out_edge: bool,
    pub rotate_left: bool,
    pub rotate_right: bool,
}

/// One frame of pointer state (tests and captures inject it).
#[derive(Clone, Copy, Debug, Default)]
pub struct PointerFrame {
    pub cursor: Option<Vec2>,
    pub ray: Option<Ray3d>,
    pub left_down: bool,
    pub left_just_pressed: bool,
    pub move_down: bool,
    pub camera_down: bool,
    pub over_ui: bool,
}

/// The Space controls state read by the camera and the ship.
#[derive(Resource, Clone, Debug, Default)]
pub struct SpaceControls {
    pub scheme: ControlScheme,
    /// The triggers held this frame.
    pub triggers: TriggerSet,
    /// A discrete zoom key went down this frame (+1 in, -1 out).
    pub quick_zoom: i8,
    /// The HUD's camera buttons (set from the HUD's systems).
    pub buttons: CameraButtons,
    /// A camera button is down and the cursor moved by `drag` pixels.
    pub camera_held: bool,
    pub drag: Vec2,
    /// The flight button (Game scheme right button) is down.
    pub move_held: bool,
    /// The left button went down this frame outside the HUD.
    pub click: bool,
    pub cursor: Option<Vec2>,
    pub ray: Option<Ray3d>,
    /// Where the ship was told to fly in the solar system (ecliptic point).
    pub fly_to: Option<Vec3>,
    /// The planet view's goto (the minimap click, 0xE0B970 -> UFO 0xFFC200): the
    /// world direction from the planet's centre of the point to fly to. Flown
    /// like the flight button (`input.toward`) until the ship is within the
    /// button's dead zone; a key, the flight button or leaving the planet
    /// drops it.
    pub planet_goto: Option<Vec3>,
    /// The planet flight moved the ship this frame (`space_fly` leaves it).
    pub ship_handled: bool,
    /// A weapon is firing (F): the planet camera uses its combat table.
    pub firing: bool,
    /// Replaces the real mouse (tests and captures).
    pub injected: Option<PointerFrame>,
    last_cursor: Option<Vec2>,
}

impl SpaceControls {
    /// The flight direction `space_fly` follows when a destination was set.
    pub fn flight_dir(&mut self, ship: Vec3) -> Vec3 {
        let Some(point) = self.fly_to else {
            return Vec3::ZERO;
        };
        let to = Vec3::new(point.x - ship.x, 0.0, point.z - ship.z);
        if to.length() < ARRIVE_DISTANCE {
            self.fly_to = None;
            return Vec3::ZERO;
        }
        to.normalize()
    }
}

/// ADAPTED: a solar-system click destination counts as reached this close.
pub const ARRIVE_DISTANCE: f32 = 1.2;

// ---------------------------------------------------------------------------
// The ship: manual flight in the solar system, flight over a planet
// ---------------------------------------------------------------------------

/// What `space::space_fly` asks for: whether the keys steer the ship, the
/// travel direction and whether a manual move cancels the autopilot. With the
/// retail controls the solar system has no keyboard flight (WASD moves the
/// camera there; the ship flies where it is sent), so only a destination
/// produces a direction; without them (older fixtures) the old keys fly.
pub fn manual_flight(
    keys: &ButtonInput<KeyCode>,
    controls: Option<&mut SpaceControls>,
    view: Option<&SpaceView>,
    ship: Vec3,
) -> (bool, Vec3, bool) {
    let galaxy = view.is_some_and(|v| v.galaxy);
    let steering = !galaxy;
    let Some(controls) = controls else {
        // The old WASD flight (tests that build a Space app without the
        // controls plugin).
        let mut dir = Vec3::ZERO;
        if steering {
            if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
                dir.z -= 1.0;
            }
            if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
                dir.z += 1.0;
            }
            if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
                dir.x -= 1.0;
            }
            if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
                dir.x += 1.0;
            }
        }
        return (steering, dir, dir != Vec3::ZERO);
    };
    let dir = if steering { controls.flight_dir(ship) } else { Vec3::ZERO };
    // A new destination replaces the autopilot's planet target.
    let cancel = dir != Vec3::ZERO;
    (steering, dir, cancel)
}

/// ufoKeyboardMovementSpeedZoomedIn / ZoomedOut and the accelerations, retail
/// units per second (over a world: x `PlanetView::unit` in the scene).
pub const KEYBOARD_SPEED: (f32, f32) = (100.0, 300.0);
pub const KEYBOARD_ACCELERATION: (f32, f32) = (100.0, 300.0);
/// ufoBrakeFactor: letting go brakes this many times harder than thrust.
pub const BRAKE_FACTOR: f32 = 4.0;
/// ufoMaxRotation (degrees, read as degrees per second: ADAPTED unit) and
/// ufoRotationPercentageRate (how fast the turn ramps to its maximum, 1/s).
pub const MAX_ROTATION_DEG: f32 = 30.0;
pub const ROTATION_RATE: f32 = 3.0;
/// ufoMaxBankTilt / BankTiltRate, ufoMaxNoseTilt / NoseTiltRate, degrees, 1/s.
pub const MAX_BANK_DEG: f32 = 30.0;
pub const BANK_RATE: f32 = 2.0;
pub const MAX_NOSE_DEG: f32 = 30.0;
pub const NOSE_RATE: f32 = 4.0;
/// ufoMaxVerticalTilt / ufoVerticalTiltFactor.
pub const MAX_VERTICAL_TILT_DEG: f32 = 30.0;

/// Keyboard speed and acceleration at an altitude, retail units per second.
pub fn speed_limits(altitude: f32) -> (f32, f32) {
    let t = ((altitude - UFO_MIN_ALTITUDE) / (crate::space_view::PLANET_ZOOM_CEILING - UFO_MIN_ALTITUDE))
        .clamp(0.0, 1.0);
    let speed = KEYBOARD_SPEED.0 + (KEYBOARD_SPEED.1 - KEYBOARD_SPEED.0) * t;
    let accel = KEYBOARD_ACCELERATION.0 + (KEYBOARD_ACCELERATION.1 - KEYBOARD_ACCELERATION.0) * t;
    (speed, accel)
}

/// Movement wishes of a frame over a planet.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlyInput {
    /// -1..1: W forward, S back.
    pub forward: f32,
    /// -1..1: E right, Q left (Flight scheme: D / A too).
    pub strafe: f32,
    /// -1..1: A left, D right (Game scheme).
    pub turn: f32,
    /// A tangent direction to fly toward (the right button, the cursor).
    pub toward: Option<Vec3>,
    /// The camera's heading (Flight scheme: WASD are relative to it).
    pub camera_heading: Option<Vec3>,
}

/// The wishes of the held triggers in a scheme.
pub fn fly_input(triggers: TriggerSet, scheme: ControlScheme) -> FlyInput {
    let axis = |plus: bool, minus: bool| plus as i32 as f32 - minus as i32 as f32;
    let forward = axis(triggers.has(TriggerSet::FORWARD), triggers.has(TriggerSet::BACK));
    let q_e = axis(triggers.has(TriggerSet::STRAFE_RIGHT), triggers.has(TriggerSet::STRAFE_LEFT));
    let a_d = axis(triggers.has(TriggerSet::RIGHT), triggers.has(TriggerSet::LEFT));
    match scheme {
        // A / D turn, Q / E strafe.
        ControlScheme::Game => FlyInput {
            forward,
            strafe: q_e,
            turn: -a_d,
            ..default()
        },
        // A / D (and Q / E) strafe.
        ControlScheme::Flight => FlyInput {
            forward,
            strafe: (q_e + a_d).clamp(-1.0, 1.0),
            turn: 0.0,
            ..default()
        },
    }
}

/// One frame of flight over a planet (retail 0xFFE700 + the UFO update with
/// the `spaceufo~` limits): the ship stays on its shell around the world,
/// turns with the turn ramp, accelerates toward the keyboard speed and brakes
/// harder than it accelerates.
pub fn planet_step(p: &mut PlanetView, input: FlyInput, dt: f32, scheme: ControlScheme) {
    let up = p.up;
    // Retail speeds in the world's proportion (scene units per second).
    let unit = p.unit();
    let (top, accel) = speed_limits(p.altitude);
    let (top, accel) = (top * unit, accel * unit);
    let altitude_t = ((p.altitude - UFO_MIN_ALTITUDE) / (UFO_MAX_ALTITUDE - UFO_MIN_ALTITUDE)).clamp(0.0, 1.0);
    // --- heading ---
    let max_turn = MAX_ROTATION_DEG.to_radians() * (1.0 + 2.0 * altitude_t);
    let mut desired: Vec3;
    let right = p.heading.cross(up).normalize_or_zero();
    let mut steer: Option<Vec3> = None;
    match scheme {
        ControlScheme::Game => {
            p.turn += (input.turn - p.turn) * (ROTATION_RATE * dt).min(1.0);
            let mut yaw = p.turn * max_turn * dt;
            if let Some(toward) = input.toward {
                // The right button turns toward the cursor at twice the rate
                // (ADAPTED: the original steers to the clicked destination).
                steer = Some(toward);
                yaw = 0.0;
            }
            p.heading = (Quat::from_axis_angle(up, yaw) * p.heading).normalize_or_zero();
            let forward = if input.toward.is_some() { 1.0 } else { input.forward };
            desired = p.heading * forward + right * input.strafe;
        }
        ControlScheme::Flight => {
            let camera = tangent(input.camera_heading.unwrap_or(p.heading), up);
            let cam_right = camera.cross(up).normalize_or_zero();
            desired = camera * input.forward + cam_right * input.strafe;
            if let Some(toward) = input.toward {
                desired = toward;
            }
            if desired.length_squared() > 1.0e-6 {
                steer = Some(desired);
            }
        }
    }
    if let Some(dir) = steer {
        let dir = tangent(dir, up);
        let angle = p.heading.angle_between(dir);
        let sign = p.heading.cross(dir).dot(up).signum();
        let step = angle.min(max_turn * 2.0 * dt);
        p.heading = (Quat::from_axis_angle(up, sign * step) * p.heading).normalize_or_zero();
        p.turn = if angle > 1.0e-3 { sign * (step / (max_turn * dt)).min(1.0) } else { 0.0 };
    }
    // --- velocity ---
    let wish = desired.clamp_length_max(1.0) * top;
    let thrust = wish.length() > 1.0e-4;
    let rate = if thrust { accel } else { accel * BRAKE_FACTOR };
    let delta = wish - p.velocity;
    let limit = rate * dt;
    p.velocity += if delta.length() > limit { delta.normalize() * limit } else { delta };
    if !thrust && p.velocity.length() < 3.0 * unit {
        p.velocity = Vec3::ZERO;
    }
    // --- move along the shell (parallel transport of up, heading, velocity) ---
    let speed = p.velocity.length();
    if speed > 1.0e-6 {
        let shell = p.radius + p.height();
        let angle = speed * dt / shell;
        let axis = up.cross(p.velocity).normalize_or_zero();
        if axis != Vec3::ZERO {
            let rot = Quat::from_axis_angle(axis, angle);
            p.up = (rot * p.up).normalize();
            p.heading = tangent(rot * p.heading, p.up);
            p.velocity = tangent(rot * p.velocity, p.up) * speed;
        }
    }
    // --- tilt (the UFO banks into a turn and noses into thrust) ---
    let speed_ratio = if top > 0.0 { (p.velocity.dot(p.heading) / top).clamp(-1.0, 1.0) } else { 0.0 };
    let bank_goal = -p.turn * MAX_BANK_DEG.to_radians() * (0.4 + 0.6 * speed_ratio.abs());
    let nose_goal = -speed_ratio * MAX_NOSE_DEG.to_radians() * 0.5;
    let lateral = if top > 0.0 { (p.velocity.dot(right) / top).clamp(-1.0, 1.0) } else { 0.0 };
    let vertical_goal = lateral * MAX_VERTICAL_TILT_DEG.to_radians() * 0.05 * 6.0;
    p.bank += (bank_goal - p.bank) * (BANK_RATE * dt).min(1.0);
    p.nose += (nose_goal - p.nose) * (NOSE_RATE * dt).min(1.0);
    p.vtilt += (vertical_goal - p.vtilt) * (BANK_RATE * dt).min(1.0);
}

/// The ship's orientation over a planet: heading, up, then the visual tilt.
pub fn ship_rotation(p: &PlanetView) -> Quat {
    let right = p.heading.cross(p.up).normalize_or_zero();
    let base = Quat::from_mat3(&Mat3::from_cols(right, p.up, -p.heading));
    Quat::from_axis_angle(p.heading, p.bank + p.vtilt) * Quat::from_axis_angle(right, p.nose) * base
}

/// Where the ray meets the sphere (nearest hit), else the sphere point nearest
/// the ray.
pub fn ray_sphere_point(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Vec3 {
    let to = center - origin;
    let along = to.dot(dir);
    let closest = origin + dir * along;
    let off2 = (closest - center).length_squared();
    if off2 <= radius * radius {
        let half = (radius * radius - off2).sqrt();
        origin + dir * (along - half).max(0.0)
    } else {
        center + (closest - center).normalize_or(Vec3::Y) * radius
    }
}

/// The point where the ray meets the horizontal plane at height `y`.
pub fn ray_plane_point(origin: Vec3, dir: Vec3, y: f32) -> Option<Vec3> {
    if dir.y.abs() < 1.0e-5 {
        return None;
    }
    let t = (y - origin.y) / dir.y;
    (t > 0.0).then(|| origin + dir * t)
}

/// Nearest of the spheres (`id`, centre, radius) along a ray.
pub fn pick_sphere(origin: Vec3, dir: Vec3, spheres: impl IntoIterator<Item = (u32, Vec3, f32)>) -> Option<u32> {
    let mut best: Option<(f32, u32)> = None;
    for (id, center, radius) in spheres {
        let to = center - origin;
        let along = to.dot(dir);
        if along < 0.0 {
            continue;
        }
        if to.length_squared() - along * along <= radius * radius
            && best.is_none_or(|(t, _)| along < t)
        {
            best = Some((along, id));
        }
    }
    best.map(|(_, id)| id)
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Reads the keyboard and the mouse into `SpaceControls` (runs with the other
/// controls, after the HUD's key injection).
#[allow(clippy::too_many_arguments)]
pub fn gather(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    spui: Option<Res<crate::spui::view::SpuiPointer>>,
    hover: Option<Res<bevy::picking::hover::HoverMap>>,
    ui_buttons: Query<(), With<bevy::ui_widgets::Button>>,
    parents: Query<&ChildOf>,
    mut controls: ResMut<SpaceControls>,
    mut option: Local<Option<bool>>,
) {
    // `OptionSpaceControls` (Settings > Control > Space, 0x0679B880): the
    // stage reads it on entering and when the page's check box sends 0x0679C40D
    // (0xFE1A10 picks the trigger file). The value in force when the stage
    // starts wins over `--space-controls`, but only when it is on.
    let flight = crate::control_options::space_flight();
    if *option != Some(flight) && (option.is_some() || flight) {
        controls.scheme = ControlScheme::from_option(u32::from(flight));
        info!("{}", controls.scheme.label());
    }
    *option = Some(flight);
    controls.triggers = read_triggers(&keys);
    controls.quick_zoom = quick_zoom_edge(&keys);
    controls.ship_handled = false;
    controls.firing = keys.pressed(KeyCode::KeyF);
    let frame = if let Some(frame) = controls.injected {
        frame
    } else {
        let mut frame = PointerFrame::default();
        if let Some(mouse) = mouse.as_deref() {
            let scheme = controls.scheme;
            frame.left_down = mouse.pressed(MouseButton::Left);
            frame.left_just_pressed = mouse.just_pressed(MouseButton::Left);
            frame.move_down = scheme.move_button().is_some_and(|b| mouse.pressed(b));
            frame.camera_down = [MouseButton::Middle, MouseButton::Right]
                .into_iter()
                .any(|b| scheme.camera_button(b) && mouse.pressed(b));
        }
        let window = windows.iter().find(|w| w.cursor_position().is_some());
        frame.cursor = window.and_then(|w| w.cursor_position());
        frame.over_ui = spui.as_deref().is_some_and(|s| s.over_ui)
            || hover.as_deref().is_some_and(|map| {
                map.0.values().any(|hits| {
                    hits.keys().any(|entity| {
                        let mut at = *entity;
                        for _ in 0..5 {
                            if ui_buttons.contains(at) {
                                return true;
                            }
                            match parents.get(at) {
                                Ok(parent) => at = parent.parent(),
                                Err(_) => break,
                            }
                        }
                        false
                    })
                })
            });
        if let (Some(cursor), Some(window)) = (frame.cursor, window) {
            frame.over_ui |= crate::creature_controls::over_hud_margin(
                cursor,
                Vec2::new(window.width(), window.height()),
            );
        }
        frame.ray = frame
            .cursor
            .zip(cameras.single().ok())
            .and_then(|(cursor, (camera, at))| camera.viewport_to_world(at, cursor).ok());
        frame
    };
    controls.cursor = frame.cursor;
    controls.ray = frame.ray;
    controls.click = frame.left_just_pressed && !frame.over_ui;
    controls.move_held = frame.move_down && !frame.over_ui;
    controls.camera_held = frame.camera_down;
    controls.drag = match (frame.cursor, controls.last_cursor) {
        (Some(now), Some(before)) if frame.camera_down => now - before,
        _ => Vec2::ZERO,
    };
    controls.last_cursor = frame.cursor;
}

/// Mouse actions of the solar system: a click on a world flies there (the
/// autopilot), the flight button flies to the point of the ecliptic under the
/// cursor. Keys that move the ship cancel a destination.
#[allow(clippy::type_complexity)]
pub fn pointer_actions(
    mut controls: ResMut<SpaceControls>,
    view: Option<Res<SpaceView>>,
    mut player: Query<(&Transform, &mut ShipMotion), With<Player>>,
    planets: Query<(&Transform, &Planet, &WorldId), Without<Player>>,
    cameras: Query<&GlobalTransform, With<crate::GameCamera>>,
) {
    let Some(view) = view else { return };
    if view.zoom() != crate::space_view::SpaceZoom::Planet {
        controls.planet_goto = None;
    }
    if view.zoom() != crate::space_view::SpaceZoom::System {
        controls.fly_to = None;
        return;
    }
    let Ok((ship, mut motion)) = player.single_mut() else {
        return;
    };
    let Some(ray) = controls.ray else { return };
    let (origin, dir) = (ray.origin, *ray.direction);
    let camera_distance = cameras
        .single()
        .map(|at| at.translation().distance(ship.translation))
        .unwrap_or(30.0);
    if controls.click {
        // Planets are pickable a little beyond their rim so a distant world
        // is not a pixel-perfect target.
        let slack = camera_distance * 0.02;
        let hit = pick_sphere(
            origin,
            dir,
            planets
                .iter()
                .map(|(t, p, id)| (id.0, t.translation, p.radius * 1.4 + slack)),
        );
        if let Some(id) = hit {
            motion.autopilot_target = Some(id);
            controls.fly_to = None;
        }
    }
    if controls.move_held {
        if let Some(point) = ray_plane_point(origin, dir, ship.translation.y) {
            controls.fly_to = Some(point);
            motion.autopilot_target = None;
        }
    }
}

/// Flight over a planet: the ship hugs the world's shell, steered by the
/// triggers or the flight button, and the camera keeps its chase pose.
#[allow(clippy::type_complexity)]
pub fn planet_flight(
    time: Res<Time>,
    mut controls: ResMut<SpaceControls>,
    view: Option<ResMut<SpaceView>>,
    mut player: Query<(&mut Transform, &mut ShipMotion), With<Player>>,
    planets: Query<(&Transform, &Planet, &WorldId), Without<Player>>,
    cameras: Query<&GlobalTransform, With<crate::GameCamera>>,
) {
    let Some(mut view) = view else { return };
    let Some(planet) = view.planet.as_mut() else {
        return;
    };
    let Ok((mut ship, mut motion)) = player.single_mut() else {
        return;
    };
    // The world drifts along its orbit: follow its centre.
    if let Some((t, p, _)) = planets.iter().find(|(_, _, id)| id.0 == planet.world) {
        planet.center = t.translation;
        planet.radius = p.radius;
    }
    let dt = time.delta_secs().min(0.1);
    let mut input = fly_input(controls.triggers, controls.scheme);
    // The keys and the flight button take over from a minimap goto.
    if input.forward != 0.0 || input.strafe != 0.0 || input.turn != 0.0 || controls.move_held {
        controls.planet_goto = None;
    }
    if let Some(dir) = controls.planet_goto {
        let point = planet.center + dir * (planet.radius + planet.height());
        // ADAPTED: the goto ends inside the flight button's dead zone.
        if (point - planet.ship_position()).length() > 10.0 * planet.unit() {
            input.toward = Some(tangent(point - planet.ship_position(), planet.up));
        } else {
            controls.planet_goto = None;
        }
    }
    if controls.move_held {
        if let Some(ray) = controls.ray {
            let shell = planet.radius + planet.height();
            let point = ray_sphere_point(ray.origin, *ray.direction, planet.center, shell);
            // A cursor on top of the ship asks for nothing.
            if (point - planet.ship_position()).length() > 10.0 * planet.unit() {
                input.toward = Some(tangent(point - planet.ship_position(), planet.up));
            }
        }
    }
    if let Ok(camera) = cameras.single() {
        let forward = camera.forward().as_vec3();
        input.camera_heading = Some(tangent(forward, planet.up));
    }
    planet_step(planet, input, dt, controls.scheme);
    ship.translation = planet.ship_position();
    ship.rotation = ship_rotation(planet);
    motion.velocity = planet.velocity;
    motion.thrusting = planet.velocity.length() > 5.0 * planet.unit();
    // Moving by hand cancels an autopilot target.
    if input.forward != 0.0 || input.strafe != 0.0 || input.turn != 0.0 || input.toward.is_some() {
        motion.autopilot_target = None;
    }
    controls.fly_to = None;
    controls.ship_handled = true;
}

pub struct SpaceControlsPlugin;

impl Plugin for SpaceControlsPlugin {
    fn build(&self, app: &mut App) {
        // `--space-controls flight` starts with the Flight trigger file.
        let args: Vec<String> = std::env::args().collect();
        let scheme = args
            .windows(2)
            .find(|w| w[0] == "--space-controls")
            .map(|w| ControlScheme::from_option(u32::from(w[1] == "flight")))
            .unwrap_or_default();
        // The planets drawn from the planet engine (orbit and planet view).
        app.add_plugins(crate::space_planets::SpacePlanetsPlugin);
        app.insert_resource(SpaceControls {
            scheme,
            ..default()
        })
            .add_systems(
                Update,
                gather
                    .after(crate::player_hud::actions)
                    .in_set(crate::GameSystems::Controls)
                    .run_if(in_state(GameStage::Space))
                    .run_if(crate::editor::simulation_active)
                    .run_if(crate::mechanical_creator::simulation_active)
                    .run_if(crate::pause_menu::simulation_active),
            )
            .add_systems(
                Update,
                (pointer_actions, planet_flight)
                    .chain()
                    .before(crate::space_view::zoom_input)
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Space)),
            )
            // The paused free camera and the hidden cursor of a camera drag
            // run outside the simulation set (the pause holds it).
            .add_systems(
                Update,
                (crate::space_view::paused_camera, crate::space_view::drag_cursor)
                    .chain()
                    .after(gather)
                    .before(crate::space_view::camera)
                    .run_if(in_state(GameStage::Space)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(pressed: &[KeyCode]) -> ButtonInput<KeyCode> {
        let mut input = ButtonInput::default();
        for key in pressed {
            input.press(*key);
        }
        input
    }

    #[test]
    fn the_trigger_files_map_every_listed_key() {
        // TriggerConfigWASD.
        for key in [KeyCode::KeyW, KeyCode::ArrowUp, KeyCode::Numpad8] {
            assert!(read_triggers(&keys(&[key])).has(TriggerSet::FORWARD));
        }
        for key in [KeyCode::KeyS, KeyCode::ArrowDown, KeyCode::Numpad2] {
            assert!(read_triggers(&keys(&[key])).has(TriggerSet::BACK));
        }
        for key in [KeyCode::KeyA, KeyCode::ArrowLeft, KeyCode::Numpad4] {
            assert!(read_triggers(&keys(&[key])).has(TriggerSet::LEFT));
        }
        for key in [KeyCode::KeyD, KeyCode::ArrowRight, KeyCode::Numpad6] {
            assert!(read_triggers(&keys(&[key])).has(TriggerSet::RIGHT));
        }
        assert!(read_triggers(&keys(&[KeyCode::KeyQ])).has(TriggerSet::STRAFE_LEFT));
        assert!(read_triggers(&keys(&[KeyCode::KeyE])).has(TriggerSet::STRAFE_RIGHT));
        // TriggerConfigSpaceGame / Flight: pitch, rotate, zoom, discrete zoom.
        for key in [KeyCode::PageUp, KeyCode::Numpad9] {
            assert!(read_triggers(&keys(&[key])).has(TriggerSet::PITCH_UP));
        }
        for key in [KeyCode::PageDown, KeyCode::Numpad3] {
            assert!(read_triggers(&keys(&[key])).has(TriggerSet::PITCH_DOWN));
        }
        assert!(read_triggers(&keys(&[KeyCode::Comma])).has(TriggerSet::ROTATE_LEFT));
        assert!(read_triggers(&keys(&[KeyCode::Period])).has(TriggerSet::ROTATE_RIGHT));
        for key in [KeyCode::KeyX, KeyCode::Equal, KeyCode::NumpadAdd] {
            assert!(read_triggers(&keys(&[key])).zoom_in(), "{key:?}");
        }
        for key in [KeyCode::Space, KeyCode::Minus, KeyCode::NumpadSubtract] {
            assert!(read_triggers(&keys(&[key])).zoom_out(), "{key:?}");
        }
        // Shift and alt keep the plain zoom on + and -.
        assert!(read_triggers(&keys(&[KeyCode::ShiftLeft, KeyCode::Equal])).zoom_in());
        assert!(read_triggers(&keys(&[KeyCode::AltLeft, KeyCode::Minus])).zoom_out());
        // Control turns them into the discrete zoom; End / Home always are.
        let ctrl_plus = read_triggers(&keys(&[KeyCode::ControlLeft, KeyCode::Equal]));
        assert!(ctrl_plus.has(TriggerSet::QUICK_IN) && !ctrl_plus.zoom_in());
        let ctrl_minus = read_triggers(&keys(&[KeyCode::ControlLeft, KeyCode::NumpadSubtract]));
        assert!(ctrl_minus.has(TriggerSet::QUICK_OUT) && !ctrl_minus.zoom_out());
        assert!(read_triggers(&keys(&[KeyCode::End])).has(TriggerSet::QUICK_IN));
        assert!(read_triggers(&keys(&[KeyCode::Home])).has(TriggerSet::QUICK_OUT));
        // Ctrl + Alt is the debug chord: nothing moves the camera or the ship.
        let chord = keys(&[KeyCode::ControlLeft, KeyCode::AltLeft, KeyCode::Minus, KeyCode::Comma]);
        assert!(debug_chord(&chord));
        assert_eq!(read_triggers(&chord), TriggerSet::NONE);
        assert_eq!(quick_zoom_edge(&chord), 0);
        assert!(!debug_chord(&keys(&[KeyCode::ControlLeft, KeyCode::Minus])));
        // The retail ids.
        assert_eq!(TriggerSet::ZOOM_IN.retail_id(), 0x9);
        assert_eq!(TriggerSet::ZOOM_OUT.retail_id(), 0x10);
        assert_eq!(TriggerSet::QUICK_IN.retail_id(), 0x1111_1111);
        assert_eq!(TriggerSet::QUICK_OUT.retail_id(), 0x2222_2222);
        assert_eq!(TriggerSet::ROTATE_RIGHT.retail_id(), 0x12);
    }

    #[test]
    fn discrete_zoom_fires_once_per_press() {
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::End);
        assert_eq!(quick_zoom_edge(&input), 1);
        input.clear_just_pressed(KeyCode::End);
        assert_eq!(quick_zoom_edge(&input), 0, "holding does not repeat");
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::ControlLeft);
        input.press(KeyCode::Minus);
        assert_eq!(quick_zoom_edge(&input), -1);
        let mut z = ButtonInput::<KeyCode>::default();
        z.press(KeyCode::KeyZ);
        assert_eq!(quick_zoom_edge(&z), 0, "Z is unbound in the retail trigger tables (space-re-notes S6A)");
        let mut plain = ButtonInput::<KeyCode>::default();
        plain.press(KeyCode::Equal);
        assert_eq!(quick_zoom_edge(&plain), 0);
    }

    #[test]
    fn the_schemes_differ_in_their_mouse_lines_and_turning() {
        assert_eq!(ControlScheme::default(), ControlScheme::Game);
        assert_eq!(ControlScheme::from_option(1), ControlScheme::Flight);
        assert_eq!(ControlScheme::from_option(0), ControlScheme::Game);
        assert_eq!(ControlScheme::from_option(7), ControlScheme::Game);
        // Game: right flies, middle drags; Flight: right and middle drag.
        assert_eq!(ControlScheme::Game.move_button(), Some(MouseButton::Right));
        assert_eq!(ControlScheme::Flight.move_button(), None);
        assert!(ControlScheme::Game.camera_button(MouseButton::Middle));
        assert!(!ControlScheme::Game.camera_button(MouseButton::Right));
        assert!(ControlScheme::Flight.camera_button(MouseButton::Right));
        assert!(ControlScheme::Flight.camera_button(MouseButton::Middle));
        // A / D turn in Game and strafe in Flight; Q / E strafe in Game.
        let mut t = TriggerSet::NONE;
        t.set(TriggerSet::LEFT);
        t.set(TriggerSet::FORWARD);
        let game = fly_input(t, ControlScheme::Game);
        assert_eq!((game.forward, game.turn, game.strafe), (1.0, 1.0, 0.0));
        let flight = fly_input(t, ControlScheme::Flight);
        assert_eq!((flight.forward, flight.turn, flight.strafe), (1.0, 0.0, -1.0));
        let mut q = TriggerSet::NONE;
        q.set(TriggerSet::STRAFE_RIGHT);
        assert_eq!(fly_input(q, ControlScheme::Game).strafe, 1.0);
    }

    #[test]
    fn the_solar_system_has_no_keyboard_flight_with_the_retail_controls() {
        let input = keys(&[KeyCode::KeyW, KeyCode::KeyD]);
        let mut controls = SpaceControls::default();
        let (steering, dir, cancel) = manual_flight(&input, Some(&mut controls), None, Vec3::ZERO);
        assert!(steering);
        assert_eq!(dir, Vec3::ZERO);
        assert!(!cancel);
        // A destination flies there and clears itself on arrival.
        controls.fly_to = Some(Vec3::new(10.0, 0.0, 0.0));
        let (_, dir, cancel) = manual_flight(&input, Some(&mut controls), None, Vec3::ZERO);
        assert!((dir - Vec3::X).length() < 1.0e-5);
        assert!(cancel);
        let (_, dir, _) = manual_flight(&input, Some(&mut controls), None, Vec3::new(9.5, 0.0, 0.0));
        assert_eq!(dir, Vec3::ZERO);
        assert!(controls.fly_to.is_none());
        // Without the controls the old WASD still fly.
        let (_, dir, cancel) = manual_flight(&input, None, None, Vec3::ZERO);
        assert_eq!(dir, Vec3::new(1.0, 0.0, -1.0));
        assert!(cancel);
        // The galaxy map never steers the ship.
        let mut view = SpaceView::default();
        view.galaxy = true;
        let (steering, ..) = manual_flight(&input, None, Some(&view), Vec3::ZERO);
        assert!(!steering);
    }

    #[test]
    fn the_speed_limits_follow_the_keyboard_tuning() {
        let (slow, slow_accel) = speed_limits(UFO_MIN_ALTITUDE);
        let (fast, fast_accel) = speed_limits(crate::space_view::PLANET_ZOOM_CEILING);
        assert!((slow - 100.0).abs() < 1.0e-4);
        assert!((fast - 300.0).abs() < 1.0e-4);
        assert!((slow_accel - 100.0).abs() < 1.0e-4);
        assert!((fast_accel - 300.0).abs() < 1.0e-4);
        // Faster the higher the UFO flies, capped at the zoom ceiling.
        assert!(speed_limits(400.0).0 > speed_limits(100.0).0);
        assert_eq!(speed_limits(1500.0).0, fast);
    }

    fn over_planet() -> PlanetView {
        let mut p = PlanetView::new(1, Vec3::ZERO, 3.0, Vec3::Y, Vec3::Z, 300.0);
        p.auto = None;
        p
    }

    #[test]
    fn the_ship_stays_on_its_shell_and_accelerates_with_the_keyboard_limit() {
        let mut p = over_planet();
        let shell = p.radius + p.height();
        let input = FlyInput {
            forward: 1.0,
            ..default()
        };
        for _ in 0..240 {
            planet_step(&mut p, input, 1.0 / 60.0, ControlScheme::Game);
            let height = p.ship_position().length();
            assert!((height - shell).abs() < 1.0e-3, "{height} vs {shell}");
            assert!(p.heading.dot(p.up).abs() < 1.0e-3, "heading stays tangent");
            assert!(p.velocity.dot(p.up).abs() < 1.0e-3);
        }
        let top = speed_limits(p.altitude).0 * p.unit();
        assert!((p.velocity.length() - top).abs() < 0.02 * top, "{} vs {top}", p.velocity.length());
        // It went somewhere: around the world, not out of it.
        assert!(p.up.distance(Vec3::Y) > 0.5);
        // Letting go brakes: harder than thrust, to a full stop.
        let mut frames = 0;
        while p.velocity.length() > 0.0 && frames < 600 {
            planet_step(&mut p, FlyInput::default(), 1.0 / 60.0, ControlScheme::Game);
            frames += 1;
        }
        assert_eq!(p.velocity, Vec3::ZERO);
        assert!(frames < 60, "braking took {frames} frames");
    }

    #[test]
    fn a_and_d_turn_the_ship_in_the_game_scheme_at_the_rotation_limit() {
        let mut p = over_planet();
        let start = p.heading;
        let input = FlyInput {
            turn: 1.0,
            ..default()
        };
        for _ in 0..60 {
            planet_step(&mut p, input, 1.0 / 60.0, ControlScheme::Game);
        }
        let turned = start.angle_between(p.heading);
        let limit = MAX_ROTATION_DEG.to_radians() * (1.0 + 2.0 * ((300.0 - UFO_MIN_ALTITUDE) / (UFO_MAX_ALTITUDE - UFO_MIN_ALTITUDE)));
        assert!(turned > 0.3 * limit && turned < limit, "{turned} {limit}");
        // Left turn: counter-clockwise seen from above.
        assert!(start.cross(p.heading).dot(Vec3::Y) > 0.0);
        // The ship banks into the turn and its heading stays tangent.
        assert!(p.bank.abs() < MAX_BANK_DEG.to_radians() + 1.0e-3);
        assert!(p.heading.dot(p.up).abs() < 1.0e-4);
        // No keyboard turn without input: the turn ramp dies out.
        for _ in 0..120 {
            planet_step(&mut p, FlyInput::default(), 1.0 / 60.0, ControlScheme::Game);
        }
        assert!(p.turn.abs() < 0.01);
    }

    #[test]
    fn strafing_does_not_turn_and_flight_scheme_flies_relative_to_the_camera() {
        let mut p = over_planet();
        let heading = p.heading;
        for _ in 0..90 {
            planet_step(
                &mut p,
                FlyInput {
                    strafe: 1.0,
                    ..default()
                },
                1.0 / 60.0,
                ControlScheme::Game,
            );
        }
        assert!(p.heading.distance(heading) < 1.0e-3, "strafing keeps the heading");
        // Strafe right = to the right of the heading.
        let right = heading.cross(Vec3::Y);
        assert!(p.up.dot(right) > 0.1, "{:?}", p.up);
        // Flight scheme: W flies away from the camera whatever the ship faces.
        let mut p = over_planet();
        let camera = Vec3::X;
        for _ in 0..120 {
            planet_step(
                &mut p,
                FlyInput {
                    forward: 1.0,
                    camera_heading: Some(camera),
                    ..default()
                },
                1.0 / 60.0,
                ControlScheme::Flight,
            );
        }
        assert!(p.up.x > 0.1, "moved along the camera heading: {:?}", p.up);
        assert!(
            p.heading.dot(p.velocity.normalize()) > 0.9,
            "the ship turns toward its travel"
        );
    }

    #[test]
    fn the_right_button_steers_toward_the_cursor() {
        let mut p = over_planet();
        let goal = Vec3::X;
        for _ in 0..40 {
            planet_step(
                &mut p,
                FlyInput {
                    toward: Some(goal),
                    ..default()
                },
                1.0 / 60.0,
                ControlScheme::Game,
            );
        }
        assert!(p.velocity.length() > 0.2, "it thrusts while the button is held");
        assert!(p.heading.dot(p.velocity.normalize()) > 0.8, "it flies where it faces");
        // Its first moments turn toward the goal.
        let mut q = over_planet();
        let before = q.heading.angle_between(goal);
        for _ in 0..20 {
            planet_step(
                &mut q,
                FlyInput {
                    toward: Some(goal),
                    ..default()
                },
                1.0 / 60.0,
                ControlScheme::Game,
            );
        }
        assert!(q.heading.angle_between(goal) < before - 0.3);
    }

    #[test]
    fn rays_pick_planets_and_land_on_the_sphere_and_the_ecliptic() {
        let hit = pick_sphere(
            Vec3::new(0.0, 0.0, 20.0),
            Vec3::NEG_Z,
            [(7, Vec3::ZERO, 3.0), (8, Vec3::new(0.0, 0.0, -10.0), 3.0)],
        );
        assert_eq!(hit, Some(7), "the nearer sphere wins");
        assert_eq!(
            pick_sphere(Vec3::new(10.0, 0.0, 20.0), Vec3::NEG_Z, [(7, Vec3::ZERO, 3.0)]),
            None
        );
        let point = ray_sphere_point(Vec3::new(0.0, 0.0, 20.0), Vec3::NEG_Z, Vec3::ZERO, 5.0);
        assert!((point - Vec3::new(0.0, 0.0, 5.0)).length() < 1.0e-4);
        // A miss lands on the silhouette side of the sphere.
        let miss = ray_sphere_point(Vec3::new(9.0, 0.0, 20.0), Vec3::NEG_Z, Vec3::ZERO, 5.0);
        assert!((miss.length() - 5.0).abs() < 1.0e-4 && miss.x > 4.9);
        let ground = ray_plane_point(Vec3::new(1.0, 10.0, 0.0), Vec3::new(0.0, -1.0, 1.0).normalize(), 0.0);
        let ground = ground.expect("hits the plane");
        assert!((ground - Vec3::new(1.0, 0.0, 10.0)).length() < 1.0e-3);
        assert!(ray_plane_point(Vec3::Y, Vec3::Y, 0.0).is_none());
    }

    // --- through the real schedule ------------------------------------------

    use crate::game::{Planet, RunProgress};
    use crate::persistence::WorldId;
    use crate::space_view::{SpaceView, SpaceZoom};

    fn space_app() -> App {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut()
            .resource_mut::<RunProgress>()
            .enter_stage(GameStage::Space);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Space);
        app.update();
        app.update();
        app
    }

    fn hold(app: &mut App, key: KeyCode, frames: usize) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        for _ in 0..frames {
            app.update();
        }
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
    }

    fn tap(app: &mut App, key: KeyCode) {
        hold(app, key, 1);
    }

    fn ship_at(app: &mut App) -> Vec3 {
        let world = app.world_mut();
        world
            .query_filtered::<&Transform, With<Player>>()
            .single(world)
            .unwrap()
            .translation
    }

    fn planet_at(app: &mut App, id: u32) -> (Vec3, f32) {
        let world = app.world_mut();
        world
            .query::<(&Transform, &Planet, &WorldId)>()
            .iter(world)
            .find(|(_, _, w)| w.0 == id)
            .map(|(t, p, _)| (t.translation, p.radius))
            .unwrap()
    }

    #[test]
    fn x_and_space_zoom_and_the_camera_keys_do_not_fly_the_ship() {
        let mut app = space_app();
        // The ship rides along with the homeworld it is docked at: compare with it.
        let relative = |app: &mut App| ship_at(app) - planet_at(app, 0).0;
        let start = relative(&mut app);
        let radius = app.world().resource::<SpaceView>().solar.radius;
        // Space zooms out, X zooms in (held keys act like wheel notches).
        hold(&mut app, KeyCode::Space, 40);
        let out = app.world().resource::<SpaceView>().solar.radius;
        assert!(out > radius * 1.05, "zooming out: {radius} -> {out}");
        hold(&mut app, KeyCode::KeyX, 90);
        let back = app.world().resource::<SpaceView>().solar.radius;
        assert!(back < out * 0.95, "zooming in: {out} -> {back}");
        // A / D rotate the camera, W / S tilt it; the ship stays where it is.
        let theta = app.world().resource::<SpaceView>().solar.theta;
        let phi = app.world().resource::<SpaceView>().solar.phi;
        hold(&mut app, KeyCode::KeyD, 30);
        hold(&mut app, KeyCode::KeyW, 20);
        let view = app.world().resource::<SpaceView>();
        assert!((view.solar.theta - theta).abs() > 0.3, "{theta} -> {}", view.solar.theta);
        assert!(view.solar.phi < phi - 0.1, "W raises the camera: {phi} -> {}", view.solar.phi);
        assert_eq!(view.zoom(), SpaceZoom::System);
        let end = relative(&mut app);
        assert!(end.distance(start) < 0.3, "{start} -> {end}");
    }

    #[test]
    fn end_descends_to_the_planet_and_a_second_push_out_leaves_it() {
        let mut app = space_app();
        // The ship starts beside its homeworld: End steps the discrete zoom in
        // and, with the ship idle there, the planet view opens.
        tap(&mut app, KeyCode::End);
        for _ in 0..900 {
            if app.world().resource::<SpaceView>().zoom() == SpaceZoom::Planet {
                break;
            }
            app.update();
        }
        assert_eq!(app.world().resource::<SpaceView>().zoom(), SpaceZoom::Planet);
        // The ship is on its shell over the homeworld, descending from 1000.
        for _ in 0..200 {
            app.update();
        }
        let (center, radius) = planet_at(&mut app, 0);
        let ship = ship_at(&mut app);
        let height = app.world().resource::<SpaceView>().planet.as_ref().unwrap().height();
        assert!(
            ((ship - center).length() - (radius + height)).abs() < 0.05,
            "on the shell: {} vs {}",
            (ship - center).length(),
            radius + height
        );
        // Space (zoom out) climbs to the ceiling, stops there, and a second push
        // leaves for the solar view.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        let mut stopped_at_the_ceiling = false;
        for _ in 0..600 {
            app.update();
            let view = app.world().resource::<SpaceView>();
            stopped_at_the_ceiling |= view.planet.as_ref().is_some_and(|p| p.at_ceiling);
            if view.zoom() != SpaceZoom::Planet {
                break;
            }
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Space);
        assert!(stopped_at_the_ceiling, "the first arrival waits at the ceiling");
        assert_eq!(app.world().resource::<SpaceView>().zoom(), SpaceZoom::System);
    }

    #[test]
    fn wasd_fly_the_ship_over_the_planet_and_the_scheme_follows_the_option() {
        let mut app = space_app();
        let (center, radius) = planet_at(&mut app, 0);
        let ship = ship_at(&mut app);
        app.world_mut()
            .resource_mut::<SpaceView>()
            .enter_planet(0, center, radius, ship, Vec3::Z);
        // Let the entry descent finish.
        for _ in 0..400 {
            app.update();
        }
        let before = ship_at(&mut app);
        hold(&mut app, KeyCode::KeyW, 120);
        let after = ship_at(&mut app);
        let planet = app.world().resource::<SpaceView>().planet.clone().unwrap();
        // Two seconds at the retail keyboard speed: well over 150 retail units.
        assert!(after.distance(before) > 150.0 * planet.unit(), "W flies: {before} -> {after}");
        let shell = planet.radius + planet.height();
        assert!(((after - planet.center).length() - shell).abs() < 0.05);
        // A turns left: the heading rotates.
        let heading = app.world().resource::<SpaceView>().planet.as_ref().unwrap().heading;
        hold(&mut app, KeyCode::KeyA, 60);
        let turned = app.world().resource::<SpaceView>().planet.as_ref().unwrap().heading;
        assert!(heading.angle_between(turned) > 0.1);
        // The scheme follows the Settings > Control > Space option only when
        // that option changes: a scheme set otherwise (`--space-controls`)
        // stays while the option is untouched.
        assert_eq!(
            app.world().resource::<SpaceControls>().scheme,
            ControlScheme::Game
        );
        app.world_mut().resource_mut::<SpaceControls>().scheme = ControlScheme::Flight;
        app.update();
        assert_eq!(
            app.world().resource::<SpaceControls>().scheme,
            ControlScheme::Flight
        );
    }

    #[test]
    fn a_minimap_goto_flies_the_ship_to_the_point_and_a_key_drops_it() {
        let mut app = space_app();
        let (center, radius) = planet_at(&mut app, 0);
        let ship = ship_at(&mut app);
        app.world_mut()
            .resource_mut::<SpaceView>()
            .enter_planet(0, center, radius, ship, Vec3::Z);
        for _ in 0..400 {
            app.update();
        }
        let (up, heading) = {
            let p = app.world().resource::<SpaceView>().planet.clone().unwrap();
            (p.up, p.heading)
        };
        // A point 0.12 rad of arc ahead of the ship.
        let goal = (up * 0.12f32.cos() + heading * 0.12f32.sin()).normalize();
        app.world_mut().resource_mut::<SpaceControls>().planet_goto = Some(goal);
        let angle = |app: &App| app.world().resource::<SpaceView>().planet.as_ref().unwrap().up.angle_between(goal);
        let start = angle(&app);
        for _ in 0..900 {
            app.update();
            if app.world().resource::<SpaceControls>().planet_goto.is_none() {
                break;
            }
        }
        assert!(angle(&app) < start * 0.2, "the ship flew to the point: {start} -> {}", angle(&app));
        assert!(
            app.world().resource::<SpaceControls>().planet_goto.is_none(),
            "arriving ends the goto"
        );
        // A key press drops a goto that is under way.
        app.world_mut().resource_mut::<SpaceControls>().planet_goto = Some(-up);
        hold(&mut app, KeyCode::KeyA, 2);
        assert!(app.world().resource::<SpaceControls>().planet_goto.is_none());
    }

    #[test]
    fn the_right_button_flies_to_the_cursor_and_a_click_on_a_world_sends_the_ship() {
        let mut app = space_app();
        let start = ship_at(&mut app);
        // A ray from above through a point 30 units from the ship on the plane.
        let target = Vec3::new(start.x + 30.0, start.y, start.z);
        let ray = Ray3d::new(target + Vec3::Y * 40.0, Dir3::NEG_Y);
        app.world_mut().resource_mut::<SpaceControls>().injected = Some(PointerFrame {
            ray: Some(ray),
            move_down: true,
            ..default()
        });
        for _ in 0..30 {
            app.update();
        }
        let destination = app.world().resource::<SpaceControls>().fly_to;
        assert!(
            destination.is_some_and(|d| d.distance(target) < 0.5),
            "{destination:?}"
        );
        app.world_mut().resource_mut::<SpaceControls>().injected = Some(PointerFrame::default());
        for _ in 0..90 {
            app.update();
        }
        let moved = ship_at(&mut app);
        assert!(moved.x > start.x + 3.0, "the ship flew toward the point: {start} -> {moved}");
        // A click on a world makes it the autopilot target.
        let (world_position, radius) = planet_at(&mut app, 3);
        let ray = Ray3d::new(world_position + Vec3::new(0.0, 30.0, 0.5 * radius), Dir3::NEG_Y);
        app.world_mut().resource_mut::<SpaceControls>().injected = Some(PointerFrame {
            ray: Some(ray),
            left_just_pressed: true,
            left_down: true,
            ..default()
        });
        app.update();
        app.world_mut().resource_mut::<SpaceControls>().injected = Some(PointerFrame::default());
        let world = app.world_mut();
        let target = world
            .query_filtered::<&ShipMotion, With<Player>>()
            .single(world)
            .unwrap()
            .autopilot_target;
        assert_eq!(target, Some(3));
        // A click over the HUD does nothing.
        let (other, _) = planet_at(&mut app, 2);
        let ray = Ray3d::new(other + Vec3::Y * 30.0, Dir3::NEG_Y);
        app.world_mut().resource_mut::<SpaceControls>().injected = Some(PointerFrame {
            ray: Some(ray),
            left_just_pressed: true,
            over_ui: true,
            ..default()
        });
        app.update();
        let world = app.world_mut();
        let target = world
            .query_filtered::<&ShipMotion, With<Player>>()
            .single(world)
            .unwrap()
            .autopilot_target;
        assert_ne!(target, Some(2));
    }

    #[test]
    fn the_ship_tilts_within_the_ufo_limits() {
        let mut p = over_planet();
        for _ in 0..300 {
            planet_step(
                &mut p,
                FlyInput {
                    forward: 1.0,
                    turn: 1.0,
                    ..default()
                },
                1.0 / 60.0,
                ControlScheme::Game,
            );
        }
        assert!(p.bank.abs() <= MAX_BANK_DEG.to_radians() + 1.0e-4);
        assert!(p.nose.abs() <= MAX_NOSE_DEG.to_radians() + 1.0e-4);
        assert!(p.vtilt.abs() <= MAX_VERTICAL_TILT_DEG.to_radians() + 1.0e-4);
        // Banked into a left turn, nose down at speed.
        assert!(p.bank < 0.0 && p.nose < 0.0, "{} {}", p.bank, p.nose);
        // The orientation is a rotation whose up is near the planet's up.
        let rotation = ship_rotation(&p);
        assert!(((rotation * Vec3::Y).dot(p.up)) > 0.8);
        assert!((rotation.length() - 1.0).abs() < 1.0e-4);
    }
}
