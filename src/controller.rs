//! Xbox (XInput) controller support.
//!
//! Spore has no controller support, so OpenSpore adds it as a layer on top of
//! the game's own keyboard and mouse controls: Bevy's gamepad input (gilrs)
//! is read once per frame, and for the screen that is showing (`contexts`)
//! pad buttons press the keys and mouse buttons the game already reads, a
//! stick moves the mouse cursor, triggers turn the mouse wheel, and a stick can
//! drag the camera the way the mouse does. Every system of the game keeps
//! reading the same `ButtonInput<KeyCode>`, `ButtonInput<MouseButton>`,
//! cursor position, `MouseWheel` and `MouseMotion` it reads today.
//!
//! The device in use is detected automatically (`InputMode`): any pad button
//! or stick switches to the controller and shows button prompts (`prompts`);
//! moving the mouse, clicking or typing switches back and hides them.

use std::collections::HashSet;

use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

pub mod badges;
pub mod contexts;
pub mod glyphs;
pub mod prompts;
pub mod script;

pub use contexts::ControllerContext;
#[cfg(test)]
use contexts::ContextKind;

/// A control of an Xbox controller, named as printed on the pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pad {
    A,
    B,
    X,
    Y,
    LB,
    RB,
    LT,
    RT,
    View,
    Menu,
    LS,
    RS,
    Up,
    Down,
    Left,
    Right,
}

impl Pad {
    pub const ALL: [Pad; 16] = [
        Pad::A,
        Pad::B,
        Pad::X,
        Pad::Y,
        Pad::LB,
        Pad::RB,
        Pad::LT,
        Pad::RT,
        Pad::View,
        Pad::Menu,
        Pad::LS,
        Pad::RS,
        Pad::Up,
        Pad::Down,
        Pad::Left,
        Pad::Right,
    ];

    /// Bevy's (gilrs) name for the control. LB / RB are the bumpers
    /// (`LeftTrigger`), LT / RT the analog triggers (`LeftTrigger2`).
    pub fn button(self) -> GamepadButton {
        match self {
            Pad::A => GamepadButton::South,
            Pad::B => GamepadButton::East,
            Pad::X => GamepadButton::West,
            Pad::Y => GamepadButton::North,
            Pad::LB => GamepadButton::LeftTrigger,
            Pad::RB => GamepadButton::RightTrigger,
            Pad::LT => GamepadButton::LeftTrigger2,
            Pad::RT => GamepadButton::RightTrigger2,
            Pad::View => GamepadButton::Select,
            Pad::Menu => GamepadButton::Start,
            Pad::LS => GamepadButton::LeftThumb,
            Pad::RS => GamepadButton::RightThumb,
            Pad::Up => GamepadButton::DPadUp,
            Pad::Down => GamepadButton::DPadDown,
            Pad::Left => GamepadButton::DPadLeft,
            Pad::Right => GamepadButton::DPadRight,
        }
    }

    /// The script name (`--pad a@30`, see `script`).
    pub fn parse(name: &str) -> Option<Pad> {
        Some(match name.to_ascii_lowercase().as_str() {
            "a" => Pad::A,
            "b" => Pad::B,
            "x" => Pad::X,
            "y" => Pad::Y,
            "lb" => Pad::LB,
            "rb" => Pad::RB,
            "lt" => Pad::LT,
            "rt" => Pad::RT,
            "view" | "back" | "select" => Pad::View,
            "menu" | "start" => Pad::Menu,
            "ls" => Pad::LS,
            "rs" => Pad::RS,
            "up" => Pad::Up,
            "down" => Pad::Down,
            "left" => Pad::Left,
            "right" => Pad::Right,
            _ => return None,
        })
    }
}

/// Which kind of device the player is using right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Device {
    #[default]
    KeyboardMouse,
    Gamepad,
}

/// The device in use, detected from the last input (see the module docs).
#[derive(Resource, Debug, Default)]
pub struct InputMode {
    pub device: Device,
    /// The pad that was used last (several can be connected).
    pub pad: Option<Entity>,
    /// Connected pads.
    pub connected: usize,
}

impl InputMode {
    pub fn gamepad(&self) -> bool {
        self.device == Device::Gamepad
    }
}

/// Radial dead zone of the sticks (XInput's recommended left-stick dead
/// zone is 7849 / 32767 = 0.24).
pub const STICK_DEAD_ZONE: f32 = 0.22;
/// A trigger counts as held past this (also the switch-to-pad threshold).
pub const TRIGGER_PRESS: f32 = 0.35;
/// Stick deflection that switches the device to the controller.
const STICK_ACTIVITY: f32 = 0.45;
/// Real mouse movement (pixels in one frame) that switches back to the mouse.
const MOUSE_ACTIVITY: f32 = 3.0;

/// The pad in use this frame, dead zones applied (zero when the keyboard and
/// mouse are in use), for systems that read the pad directly.
#[derive(Resource, Debug, Default, Clone)]
pub struct PadState {
    pub left: Vec2,
    pub right: Vec2,
    pub lt: f32,
    pub rt: f32,
    pressed: HashSet<Pad>,
}

impl PadState {
    pub fn pressed(&self, pad: Pad) -> bool {
        self.pressed.contains(&pad)
    }

    fn read(&mut self, gamepad: Option<&Gamepad>) {
        self.pressed.clear();
        let Some(gamepad) = gamepad else {
            self.left = Vec2::ZERO;
            self.right = Vec2::ZERO;
            self.lt = 0.0;
            self.rt = 0.0;
            return;
        };
        self.left = dead_zone(gamepad.left_stick());
        self.right = dead_zone(gamepad.right_stick());
        self.lt = gamepad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0);
        self.rt = gamepad.get(GamepadButton::RightTrigger2).unwrap_or(0.0);
        for pad in Pad::ALL {
            let down = match pad {
                Pad::LT => self.lt > TRIGGER_PRESS || gamepad.pressed(pad.button()),
                Pad::RT => self.rt > TRIGGER_PRESS || gamepad.pressed(pad.button()),
                _ => gamepad.pressed(pad.button()),
            };
            if down {
                self.pressed.insert(pad);
            }
        }
    }
}

/// A radial dead zone, rescaled so that output starts at zero at its edge.
pub fn dead_zone(stick: Vec2) -> Vec2 {
    let length = stick.length();
    if length <= STICK_DEAD_ZONE {
        return Vec2::ZERO;
    }
    let scaled = ((length - STICK_DEAD_ZONE) / (1.0 - STICK_DEAD_ZONE)).min(1.0);
    stick / length * scaled
}

/// What the layer is holding down on the player's behalf, so that it can
/// release exactly that, and recognise its own input messages.
#[derive(Resource, Debug, Default)]
pub struct Synthetic {
    keys: HashSet<KeyCode>,
    mouse: HashSet<MouseButton>,
    /// Messages written last frame (not the player's keyboard and mouse:
    /// they must not switch the device back).
    motion_written: Vec<Vec2>,
    wheel_written: Vec<f32>,
    buttons_written: Vec<(MouseButton, ButtonState)>,
    keys_written: Vec<(KeyCode, ButtonState)>,
    /// Fractional wheel notches carried between frames.
    wheel_carry: f32,
}


/// Order: Bevy's input systems, then `Detect` (device + pad state), then
/// `Context` (which screen, which bindings), then `Drive` (the injected
/// input), all before Bevy UI's focus system reads the mouse.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ControllerSystems {
    Detect,
    Context,
    Drive,
}

pub struct ControllerPlugin;

impl Plugin for ControllerPlugin {
    fn build(&self, app: &mut App) {
        // Bevy's input and window plugins register these; a bare app (tests)
        // gets them here (both calls do nothing when already registered).
        app.add_message::<KeyboardInput>()
            .add_message::<MouseButtonInput>()
            .add_message::<MouseMotion>()
            .add_message::<MouseWheel>()
            .add_message::<bevy::window::CursorMoved>()
            .add_message::<bevy::input::gamepad::RawGamepadEvent>()
            .add_message::<bevy::input::gamepad::GamepadConnectionEvent>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>();
        app.init_resource::<InputMode>()
            .init_resource::<PadState>()
            .init_resource::<Synthetic>()
            .init_resource::<ControllerContext>()
            .configure_sets(
                PreUpdate,
                (
                    ControllerSystems::Detect,
                    ControllerSystems::Context,
                    ControllerSystems::Drive,
                )
                    .chain()
                    .after(InputSystems)
                    .before(bevy::ui::UiSystems::Focus),
            )
            .add_systems(PreUpdate, detect.in_set(ControllerSystems::Detect))
            .add_systems(
                PreUpdate,
                contexts::update.in_set(ControllerSystems::Context),
            )
            .add_systems(PreUpdate, drive.in_set(ControllerSystems::Drive))
            .add_plugins((prompts::PromptsPlugin, badges::BadgesPlugin));
    }
}

/// Picks the device in use and reads the active pad into `PadState`.
#[allow(clippy::too_many_arguments)]
fn detect(
    mut mode: ResMut<InputMode>,
    mut state: ResMut<PadState>,
    mut synthetic: ResMut<Synthetic>,
    pads: Query<(Entity, &Gamepad)>,
    mut keys: MessageReader<KeyboardInput>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
) {
    mode.connected = pads.iter().count();
    // The player's own keyboard and mouse (not the layer's own messages).
    let mut written_keys = std::mem::take(&mut synthetic.keys_written);
    let mut typed = false;
    for k in keys.read() {
        if let Some(i) = written_keys.iter().position(|w| *w == (k.key_code, k.state)) {
            written_keys.swap_remove(i);
        } else if k.state == ButtonState::Pressed {
            typed = true;
        }
    }
    let mut written_buttons = std::mem::take(&mut synthetic.buttons_written);
    let mut clicked = false;
    for b in buttons.read() {
        if let Some(i) = written_buttons.iter().position(|w| *w == (b.button, b.state)) {
            written_buttons.swap_remove(i);
        } else if b.state == ButtonState::Pressed {
            clicked = true;
        }
    }
    let mut own_motion = Vec2::ZERO;
    let mut written = std::mem::take(&mut synthetic.motion_written);
    for m in motion.read() {
        if let Some(i) = written.iter().position(|w| *w == m.delta) {
            written.swap_remove(i);
        } else {
            own_motion += m.delta;
        }
    }
    let mut written_wheel = std::mem::take(&mut synthetic.wheel_written);
    let mut scrolled = false;
    for w in wheel.read() {
        if let Some(i) = written_wheel.iter().position(|y| *y == w.y && w.x == 0.0) {
            written_wheel.swap_remove(i);
        } else {
            scrolled = true;
        }
    }
    let keyboard_mouse = typed || clicked || scrolled || own_motion.length() > MOUSE_ACTIVITY;
    // Any pad in use.
    let active_pad = pads.iter().find_map(|(entity, gamepad)| {
        let buttons = gamepad.get_just_pressed().next().is_some();
        let sticks = gamepad.left_stick().length() > STICK_ACTIVITY
            || gamepad.right_stick().length() > STICK_ACTIVITY;
        let triggers = gamepad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0) > TRIGGER_PRESS
            || gamepad.get(GamepadButton::RightTrigger2).unwrap_or(0.0) > TRIGGER_PRESS;
        (buttons || sticks || triggers).then_some(entity)
    });
    if let Some(entity) = active_pad {
        if mode.device != Device::Gamepad || mode.pad != Some(entity) {
            info!("controller in use: {entity}");
        }
        mode.device = Device::Gamepad;
        mode.pad = Some(entity);
    } else if keyboard_mouse && mode.device == Device::Gamepad {
        info!("keyboard and mouse in use");
        mode.device = Device::KeyboardMouse;
    }
    // A disconnected pad hands back to the keyboard and mouse.
    if let Some(entity) = mode.pad {
        if pads.get(entity).is_err() {
            mode.pad = None;
            mode.device = Device::KeyboardMouse;
        }
    }
    let gamepad = mode
        .gamepad()
        .then(|| mode.pad.and_then(|e| pads.get(e).ok().map(|(_, g)| g)))
        .flatten();
    state.read(gamepad);
}

/// How a stick is used on a screen.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum StickUse {
    #[default]
    None,
    /// Turns the mouse wheel with the stick's up / down (notches per second
    /// at full tilt).
    Wheel { rate: f32 },
    /// Moves the mouse cursor.
    Cursor,
    /// Holds direction keys (8 directions).
    Keys {
        up: KeyCode,
        down: KeyCode,
        left: KeyCode,
        right: KeyCode,
    },
    /// Moves the mouse as a drag (pixels per second at full tilt), holding
    /// `button` while the stick is out of its dead zone.
    Drag {
        button: Option<MouseButton>,
        speed: f32,
    },
}

/// What a pad control does on a screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    /// Holds a key while held.
    Key(KeyCode),
    /// Holds a key while held, with a modifier key held too.
    KeyWith(KeyCode, KeyCode),
    /// Holds a mouse button while held (at the cursor).
    Mouse(MouseButton),
    /// Turns the mouse wheel while held (notches per second, + = up / in).
    Wheel(f32),
    /// Read by a stage system through `PadState` (shown in the prompts only).
    Direct,
}

/// One binding: a pad control, what it does, and its prompt label (`None`
/// hides it from the prompt bar; `extra` lists it only while View is held).
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub pad: Pad,
    pub out: Out,
    pub label: Option<&'static str>,
    pub extra: bool,
}

impl Binding {
    pub const fn new(pad: Pad, out: Out, label: &'static str) -> Self {
        Binding { pad, out, label: Some(label), extra: false }
    }
    pub const fn extra(pad: Pad, out: Out, label: &'static str) -> Self {
        Binding { pad, out, label: Some(label), extra: true }
    }
    pub const fn hidden(pad: Pad, out: Out) -> Self {
        Binding { pad, out, label: None, extra: false }
    }
}

/// Cursor speed at full tilt, in pixels per second of a 1080-line window.
const CURSOR_SPEED: f32 = 1100.0;

/// The input messages the layer writes: besides the input resources, Bevy
/// UI buttons (through picking), the intro and the name fields read these.
#[derive(bevy::ecs::system::SystemParam)]
pub struct InputMessages<'w> {
    motion: MessageWriter<'w, MouseMotion>,
    wheel: MessageWriter<'w, MouseWheel>,
    buttons: MessageWriter<'w, MouseButtonInput>,
    keys: MessageWriter<'w, KeyboardInput>,
    cursor: MessageWriter<'w, bevy::window::CursorMoved>,
}

fn key_message(key: KeyCode, state: ButtonState, window: Entity) -> KeyboardInput {
    use bevy::input::keyboard::Key;
    KeyboardInput {
        key_code: key,
        logical_key: match key {
            KeyCode::Enter => Key::Enter,
            KeyCode::Escape => Key::Escape,
            _ => Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
        },
        state,
        text: None,
        repeat: false,
        window,
    }
}

/// Presses and releases the keys / mouse buttons of the current screen's
/// bindings, moves the cursor, turns the wheel and drags.
#[allow(clippy::too_many_arguments)]
fn drive(
    mode: Res<InputMode>,
    state: Res<PadState>,
    context: Res<ControllerContext>,
    time: Res<Time>,
    mut synthetic: ResMut<Synthetic>,
    mut windows: Query<(Entity, &mut Window, Has<PrimaryWindow>)>,
    mut messages: InputMessages,
) {
    let mut want_keys: HashSet<KeyCode> = HashSet::new();
    let mut want_mouse: HashSet<MouseButton> = HashSet::new();
    let mut wheel_rate = 0.0;
    let mut drag = Vec2::ZERO;
    let mut cursor = Vec2::ZERO;
    if mode.gamepad() {
        for binding in &context.bindings {
            if !state.pressed(binding.pad) {
                continue;
            }
            match binding.out {
                Out::Key(key) => {
                    want_keys.insert(key);
                }
                Out::KeyWith(modifier, key) => {
                    want_keys.insert(modifier);
                    want_keys.insert(key);
                }
                Out::Mouse(button) => {
                    want_mouse.insert(button);
                }
                Out::Wheel(rate) => {
                    let amount = match binding.pad {
                        Pad::LT => state.lt.max(TRIGGER_PRESS),
                        Pad::RT => state.rt.max(TRIGGER_PRESS),
                        _ => 1.0,
                    };
                    wheel_rate += rate * amount;
                }
                Out::Direct => {}
            }
        }
        for (stick, usage) in [(state.left, context.left), (state.right, context.right)] {
            match usage {
                StickUse::None => {}
                StickUse::Wheel { rate } => wheel_rate += stick.y * rate,
                StickUse::Cursor => cursor += stick * stick.length(),
                StickUse::Keys { up, down, left, right } => {
                    for key in stick_keys(stick, up, down, left, right) {
                        want_keys.insert(key);
                    }
                }
                StickUse::Drag { button, speed } => {
                    if stick != Vec2::ZERO {
                        if let Some(button) = button {
                            want_mouse.insert(button);
                        }
                        // Screen y grows downwards; stick y grows upwards.
                        drag += Vec2::new(stick.x, -stick.y) * speed * time.delta_secs();
                    }
                }
            }
        }
    }
    let window_entity = windows
        .iter()
        .max_by_key(|(_, _, primary)| *primary)
        .map(|(entity, _, _)| entity);
    // The cursor first, so that a click this frame lands where it points.
    // A quadratic response gives fine aiming near the centre.
    if let (true, Some(entity)) = (cursor != Vec2::ZERO, window_entity) {
        if let Ok((_, mut window, _)) = windows.get_mut(entity) {
            let size = Vec2::new(window.width(), window.height());
            if window.focused && size.min_element() > 0.0 {
                let speed = CURSOR_SPEED * size.y / 1080.0;
                let at = window.cursor_position().unwrap_or(size * 0.5);
                let next = (at + Vec2::new(cursor.x, -cursor.y) * speed * time.delta_secs())
                    .clamp(Vec2::ZERO, size - Vec2::ONE);
                window.set_cursor_position(Some(next));
                messages.cursor.write(bevy::window::CursorMoved {
                    window: entity,
                    position: next,
                    delta: Some(next - at),
                });
            }
        }
    }
    let Some(window_entity) = window_entity else {
        return;
    };
    // Keys and mouse buttons: press what is newly wanted, release the rest.
    // They go in as input messages, the way the keyboard and mouse arrive
    // (Bevy turns them into `ButtonInput` next frame): every reader of input
    // (button resources, Bevy UI picking, the intro, the name fields) sees
    // one press and one release, and a cinematic's input reset cannot make
    // a held key press again.
    let held_keys: Vec<KeyCode> = synthetic.keys.iter().copied().collect();
    for key in held_keys {
        if !want_keys.contains(&key) {
            synthetic.keys.remove(&key);
            messages.keys.write(key_message(key, ButtonState::Released, window_entity));
            synthetic.keys_written.push((key, ButtonState::Released));
        }
    }
    // Modifiers first, so that Ctrl is down when its Z arrives.
    let mut new_keys: Vec<KeyCode> = want_keys.into_iter().filter(|k| !synthetic.keys.contains(k)).collect();
    new_keys.sort_by_key(|k| !matches!(k, KeyCode::ControlLeft | KeyCode::ShiftLeft | KeyCode::AltLeft));
    for key in new_keys {
        synthetic.keys.insert(key);
        messages.keys.write(key_message(key, ButtonState::Pressed, window_entity));
        synthetic.keys_written.push((key, ButtonState::Pressed));
    }
    let held_mouse: Vec<MouseButton> = synthetic.mouse.iter().copied().collect();
    for button in held_mouse {
        if !want_mouse.contains(&button) {
            synthetic.mouse.remove(&button);
            messages.buttons.write(MouseButtonInput { button, state: ButtonState::Released, window: window_entity });
            synthetic.buttons_written.push((button, ButtonState::Released));
        }
    }
    for button in want_mouse {
        if synthetic.mouse.insert(button) {
            messages.buttons.write(MouseButtonInput { button, state: ButtonState::Pressed, window: window_entity });
            synthetic.buttons_written.push((button, ButtonState::Pressed));
        }
    }
    if drag != Vec2::ZERO {
        messages.motion.write(MouseMotion { delta: drag });
        synthetic.motion_written.push(drag);
    }
    // The wheel, in whole notches.
    if wheel_rate != 0.0 {
        synthetic.wheel_carry += wheel_rate * time.delta_secs();
    } else {
        synthetic.wheel_carry = 0.0;
    }
    while synthetic.wheel_carry.abs() >= 1.0 {
        let notch = synthetic.wheel_carry.signum();
        synthetic.wheel_carry -= notch;
        messages.wheel.write(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: notch,
            phase: bevy::input::touch::TouchPhase::Moved,
            window: window_entity,
        });
        synthetic.wheel_written.push(notch);
    }
}

/// The direction keys for a stick, in eight 45-degree sectors.
fn stick_keys(stick: Vec2, up: KeyCode, down: KeyCode, left: KeyCode, right: KeyCode) -> Vec<KeyCode> {
    if stick == Vec2::ZERO {
        return Vec::new();
    }
    // Sector 0 = right, counter-clockwise.
    let angle = stick.y.atan2(stick.x);
    let sector = ((angle / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8);
    match sector {
        0 => vec![right],
        1 => vec![up, right],
        2 => vec![up],
        3 => vec![up, left],
        4 => vec![left],
        5 => vec![down, left],
        6 => vec![down],
        _ => vec![down, right],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_zone_starts_at_zero_and_reaches_one() {
        assert_eq!(dead_zone(Vec2::new(0.2, 0.0)), Vec2::ZERO);
        let edge = dead_zone(Vec2::new(STICK_DEAD_ZONE + 0.001, 0.0));
        assert!(edge.x > 0.0 && edge.x < 0.01, "{edge:?}");
        let full = dead_zone(Vec2::new(0.0, -1.0));
        assert!((full.y + 1.0).abs() < 1e-5, "{full:?}");
        // Over-range input (diagonals of square gates) stays at length 1.
        assert!((dead_zone(Vec2::new(1.0, 1.0)).length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn stick_keys_cover_eight_directions() {
        use KeyCode::*;
        let keys = |x: f32, y: f32| stick_keys(Vec2::new(x, y), KeyW, KeyS, KeyA, KeyD);
        assert_eq!(keys(1.0, 0.0), vec![KeyD]);
        assert_eq!(keys(0.7, 0.7), vec![KeyW, KeyD]);
        assert_eq!(keys(0.0, 1.0), vec![KeyW]);
        assert_eq!(keys(-0.7, 0.7), vec![KeyW, KeyA]);
        assert_eq!(keys(-1.0, 0.1), vec![KeyA]);
        assert_eq!(keys(-0.7, -0.7), vec![KeyS, KeyA]);
        assert_eq!(keys(0.0, -1.0), vec![KeyS]);
        assert_eq!(keys(0.7, -0.7), vec![KeyS, KeyD]);
        assert!(keys(0.0, 0.0).is_empty());
    }

    fn pad_app(kind: ContextKind) -> (App, Entity) {
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, ControllerPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)))
            .insert_resource(contexts::ContextOverride(kind));
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app.update();
        let pad = app.world_mut().spawn_empty().id();
        app.world_mut()
            .resource_mut::<Messages<bevy::input::gamepad::GamepadConnectionEvent>>()
            .write(bevy::input::gamepad::GamepadConnectionEvent::new(
                pad,
                bevy::input::gamepad::GamepadConnection::Connected {
                    name: "test pad".into(),
                    vendor_id: None,
                    product_id: None,
                },
            ));
        app.update();
        (app, pad)
    }

    fn raw(app: &mut App, event: bevy::input::gamepad::RawGamepadEvent) {
        app.world_mut()
            .resource_mut::<Messages<bevy::input::gamepad::RawGamepadEvent>>()
            .write(event);
    }

    fn button(app: &mut App, pad: Entity, pad_button: Pad, value: f32) {
        raw(app, bevy::input::gamepad::RawGamepadEvent::Button(
            bevy::input::gamepad::RawGamepadButtonChangedEvent::new(pad, pad_button.button(), value),
        ));
    }

    fn axis(app: &mut App, pad: Entity, axis: bevy::input::gamepad::GamepadAxis, value: f32) {
        raw(app, bevy::input::gamepad::RawGamepadEvent::Axis(
            bevy::input::gamepad::RawGamepadAxisChangedEvent::new(pad, axis, value),
        ));
    }

    /// The pad's frame (the layer writes input messages) and the next one
    /// (Bevy's input systems read them).
    fn settle(app: &mut App) {
        app.update();
        app.update();
    }

    fn cursor(app: &mut App) -> Option<Vec2> {
        let world = app.world_mut();
        world.query::<&Window>().iter(world).next().and_then(|w| w.cursor_position())
    }

    #[test]
    fn a_pad_clicks_points_and_hands_back_to_the_mouse() {
        let (mut app, pad) = pad_app(ContextKind::Menu);
        assert!(!app.world().resource::<InputMode>().gamepad());
        // A clicks (the button resource, and a message for Bevy UI picking).
        button(&mut app, pad, Pad::A, 1.0);
        settle(&mut app);
        assert!(app.world().resource::<InputMode>().gamepad());
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        button(&mut app, pad, Pad::A, 0.0);
        settle(&mut app);
        assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        // The layer's own click messages did not count as the mouse.
        settle(&mut app);
        assert!(app.world().resource::<InputMode>().gamepad());
        // The left stick moves the cursor right, from the window centre.
        axis(&mut app, pad, bevy::input::gamepad::GamepadAxis::LeftStickX, 1.0);
        for _ in 0..10 {
            settle(&mut app);
        }
        let at = cursor(&mut app).expect("the stick placed the cursor");
        assert!(at.x > 640.0 + 50.0, "{at:?}");
        assert!((at.y - 360.0).abs() < 1.0, "{at:?}");
        axis(&mut app, pad, bevy::input::gamepad::GamepadAxis::LeftStickX, 0.0);
        // B is Escape on menus; then the player's mouse takes over and the
        // held key is let go.
        button(&mut app, pad, Pad::B, 1.0);
        settle(&mut app);
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Escape));
        app.world_mut()
            .resource_mut::<Messages<MouseMotion>>()
            .write(MouseMotion { delta: Vec2::new(30.0, 4.0) });
        settle(&mut app);
        assert!(!app.world().resource::<InputMode>().gamepad());
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Escape));
    }

    #[test]
    fn stage_sticks_hold_keys_and_triggers_turn_the_wheel() {
        let (mut app, pad) = pad_app(ContextKind::CreatureEditor);
        // The right stick turns the camera with the editor's own keys.
        axis(&mut app, pad, bevy::input::gamepad::GamepadAxis::RightStickX, -1.0);
        settle(&mut app);
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Comma));
        axis(&mut app, pad, bevy::input::gamepad::GamepadAxis::RightStickX, 0.0);
        settle(&mut app);
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Comma));
        // LB is Ctrl+Z.
        button(&mut app, pad, Pad::LB, 1.0);
        settle(&mut app);
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(keys.pressed(KeyCode::ControlLeft) && keys.just_pressed(KeyCode::KeyZ));
        button(&mut app, pad, Pad::LB, 0.0);
        // RT turns the wheel up.
        button(&mut app, pad, Pad::RT, 1.0);
        let mut notches = 0.0;
        let mut reader = app.world().resource::<Messages<MouseWheel>>().get_cursor_current();
        for _ in 0..30 {
            app.update();
            let messages = app.world().resource::<Messages<MouseWheel>>();
            notches += reader.read(messages).map(|w| w.y).sum::<f32>();
        }
        assert!(notches >= 3.0, "{notches}");
        assert!(app.world().resource::<InputMode>().gamepad(), "own wheel messages are not the mouse");
    }

    #[test]
    fn pad_names_parse_for_scripts() {
        for pad in Pad::ALL {
            let name = format!("{pad:?}").to_ascii_lowercase();
            assert_eq!(Pad::parse(&name), Some(pad), "{name}");
        }
        assert_eq!(Pad::parse("start"), Some(Pad::Menu));
        assert_eq!(Pad::parse("back"), Some(Pad::View));
        assert_eq!(Pad::parse("nope"), None);
    }
}
