//! Creature stage controls as the original binds them.
//!
//! Keys and mouse values come from `TriggerConfigWASD` and
//! `TriggerConfigCreatureGame` and from `CreatureGame!CreatureGameTuning`
//! (notes\creature-re-notes.md C6B); the installed manual's Creature control
//! table confirms them. The avatar controller (0xD32330..0xD32AF0) was decoded
//! for the turn and backward rules (see `WASD_TURN_SPEED`); what the locomotion
//! does with the goal it sets was not, so that part is marked `ADAPTED`.
//!
//! Keys (Creature stage):
//! * W / Up / Numpad8 walk forward, S / Down / Numpad2 backward; A / Left /
//!   Numpad4 and D / Right / Numpad6 TURN the creature (the manual: "Turn
//!   left/right"); Q / E strafe left / right. The camera follows behind.
//! * R toggles autorun; left click walks to the point under the cursor; a click
//!   held for 700 ms steers continuously; a double click (400 ms) on a creature
//!   targets it.
//! * F targets the nearest creature (the social stance starts its performance);
//!   Esc drops the target (and only then opens the pause menu); Tab toggles the
//!   verb tray between the social and the attack stance; 1-4 use the verbs.
//! * L toggles the missions window, K drops a held item (the original disables
//!   pick-up, `AllowCreaturePickup 0`, so it only raises a flag).
//! * Space / Shift / Z (jump and glide / sprint / sneak) stay where
//!   `creature_mobility` has them (the manual documents Z as the sneak toggle,
//!   so the original's first-person binding of Z becomes Ctrl+Z).
//! * Camera keys live in `play_camera` (`,` `.` `=` `-` PageUp PageDown `/`).
//!
//! Changes from OpenSpore's earlier bindings: E and F no longer switch stance
//! (Tab does; the HUD's stance buttons press [`KEY_STANCE_SOCIAL`] and
//! [`KEY_STANCE_COMBAT`]), R recruits no more (it is [`KEY_RECRUIT`] now) and
//! WASD is relative to the creature, not to the camera.

use crate::{
    creature_actions::{CreatureMind, Encounter, EncounterMode},
    epic_creatures::RoamerActor,
    game::{Health, Npc, Player},
    persistence::WorldId,
    stages::GameStage,
};
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Original values (CreatureGameTuning) and key bindings
// ---------------------------------------------------------------------------

/// creatureAvatarWASDTurningSpeed (10, degrees per update) and
/// creatureAvatarWASDSlowTurningSpeed (4). VERIFIED (avatar controller
/// 0xD32793..0xD328AF): a turn key adds `speed x pi/180` to the heading offset
/// that rotates the move-goal vector (0xD3289C), and the avatar's locomotion
/// then turns toward that goal at its own turn rate. The speed is the
/// `creatureAvatarWASDTurningSpeed` global (0x1582BB4) in the Creature mode
/// (0x1654C01); the slow value (0x1582BB8) is read only in game mode 0x1654C05,
/// so a creature that walks while it turns turns exactly as fast as one that
/// stands (the earlier "slow while walking" rule was wrong).
pub const WASD_TURN_SPEED: f32 = 10.0;
pub const WASD_SLOW_TURN_SPEED: f32 = 4.0;
/// The goal vector turns 10 degrees per update, far faster than any creature
/// can follow, so the heading changes at the locomotion's turn rate.
/// `cLocomotiveObject::mTurnRate` (+0x1E8): VERIFIED that the constructor
/// stores PI there (0xC43B62, the global 0x157424C = 3.14159) and that the
/// locomotion's heading step reads it (0xB48352); a scan of the whole code
/// image for stores to `[reg + 0x1E8]` finds none in the creature classes
/// (0xC1xxxx / 0xC2xxxx / 0xD1xxxx..0xD8xxxx), so every creature, the avatar
/// included, turns at PI. INFERRED: the unit is radians per second (the step's
/// use of the value was not followed past the load).
pub const LOCOMOTION_TURN_RATE: f32 = std::f32::consts::PI;
/// The key handler adds the negated move vector for S exactly as it adds the
/// vector for W (VERIFIED 0xD3254F..0xD32598, no backward factor): the speed
/// is the locomotion's, so walking backward is as fast as walking forward (the
/// invented 0.6 is gone). ADAPTED: the avatar backs up along its facing; whether
/// the locomotion turns round first was not traced.
pub const BACKWARD_SPEED_SCALE: f32 = 1.0;
/// creatureAvatarGoalStopDistance: a click goal is reached within this.
pub const GOAL_STOP_DISTANCE: f32 = 0.4;
/// creatureMouseDoubleClickThreshold / creatureMouseHeldMovementThreshold (ms).
pub const DOUBLE_CLICK_SECS: f64 = 0.4;
pub const HELD_MOVE_SECS: f64 = 0.7;
/// creatureMouseMoveGoalDistance: the goal of a held-button move, ahead of the
/// avatar toward the cursor.
pub const MOUSE_MOVE_GOAL_DISTANCE: f32 = 10.0;
/// creatureMouseMoveMinRadius: a held move needs the cursor this far away.
pub const MOUSE_MOVE_MIN_RADIUS: f32 = 0.5;
/// creatureGoalDistanceLimit
pub const GOAL_DISTANCE_LIMIT: f32 = 50.0;
/// ADAPTED: a click on a creature walks up to this distance of it (inside
/// `creature_actions::PERFORM_RANGE`).
pub const CREATURE_APPROACH_STOP: f32 = 3.0;
/// creatureGlideSpeed (CreatureGameTuning, global 0x1582FC0): read by the jump
/// routine `cCreatureBase::DoJump` (0xC194F2, the horizontal speed of a jump
/// that follows the move goal) and by the leap-to-target code (0xC1B586), which
/// every jump and flap (a glide use is a jump in the air, 0xD1EC3A -> 0xC1B180
/// -> DoJump 0xC18CA0) goes through. ADAPTED: applied for any walking input
/// while a flap's glide runs, with no automatic forward glide.
/// creatureMaxJumpHeight 3.5 and the flap counts per glide level (2/4/6/8/10)
/// belong to `creature_mobility` (creature-rules).
pub const GLIDE_SPEED: f32 = 10.0;

/// Recruit or dismiss the targeted ally (was R; R is autorun in the original,
/// which has no keyboard recruit: its pack is managed from the posse bar).
pub const KEY_RECRUIT: KeyCode = KeyCode::KeyP;
/// Cycle pack stance / command order (Follow, Attack, Stay).
pub const KEY_PACK_STANCE: KeyCode = KeyCode::KeyT;
/// Keys the HUD's two stance buttons press, so a click always selects its own
/// stance (Tab only toggles). Not on any real keyboard layout.
pub const KEY_STANCE_SOCIAL: KeyCode = KeyCode::F13;
pub const KEY_STANCE_COMBAT: KeyCode = KeyCode::F14;

/// The hit-test radius of a creature body under the cursor, per unit of scale.
const PICK_RADIUS: f32 = 0.9;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// What `creature_actions::update` should do with the target this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TargetRequest {
    #[default]
    None,
    /// F: pick the nearest creature, then engage it as the stance asks.
    Nearest,
    /// Engage the creature already targeted (a double click, a stance change).
    Current,
}

/// A point the avatar walks to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveGoal {
    pub point: Vec3,
    pub stop: f32,
}

/// Where the left button is in a click gesture.
#[derive(Debug, Clone, Default)]
pub struct ClickState {
    press_started: Option<f64>,
    last_press: Option<f64>,
    held: bool,
}

/// The Creature controls state: what the HUD reads (`tray_social`,
/// `autorun`, `targeted`, `missions_visible`) and the movement and request
/// state the keyboard and mouse systems share with the simulation.
#[derive(Resource, Debug, Clone)]
pub struct CreatureControls {
    /// The verb tray shows the social abilities (Tab toggles; mirrors
    /// `Encounter::mode`, which stays the source of truth).
    pub tray_social: bool,
    /// R: keep walking forward.
    pub autorun: bool,
    /// The targeted creature (mirrors `Encounter::target`).
    pub targeted: Option<Entity>,
    /// L toggles the missions window.
    pub missions_visible: bool,
    /// Left-click walk-to-point goal.
    pub goal: Option<MoveGoal>,
    /// The left button has been held long enough to steer continuously.
    pub held_move: bool,
    /// One-frame requests, cleared at the start of the next keyboard pass.
    pub target_request: TargetRequest,
    pub recruit_request: bool,
    /// T: cycle pack stance/order (Follow, Attack, Stay).
    pub pack_stance_request: bool,
    /// K: drop the held item (nothing can be held yet).
    pub drop_requested: bool,
    /// A double-clicked creature the avatar is still walking up to; it is
    /// targeted and engaged once it is in range.
    pub pending_engage: Option<u32>,
    /// The avatar walks to the nest to mate (a click on the answering mate,
    /// cursor 9); `resolve_mate_run` presses the mating key at the nest.
    pub mate_run: bool,
    click: ClickState,
}

impl Default for CreatureControls {
    fn default() -> Self {
        Self {
            tray_social: true,
            autorun: false,
            targeted: None,
            missions_visible: true,
            goal: None,
            held_move: false,
            target_request: TargetRequest::None,
            recruit_request: false,
            pack_stance_request: false,
            drop_requested: false,
            pending_engage: None,
            mate_run: false,
            click: ClickState::default(),
        }
    }
}

/// Movement keys, as -1..1 axes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KeyAxes {
    /// W / Up / Numpad8 = +1, S / Down / Numpad2 = -1.
    pub forward: f32,
    /// E = +1 (right), Q = -1 (left).
    pub strafe: f32,
    /// D / Right / Numpad6 = +1 (turn right), A / Left / Numpad4 = -1.
    pub turn: f32,
}

impl KeyAxes {
    pub fn any(&self) -> bool {
        self.forward != 0.0 || self.strafe != 0.0 || self.turn != 0.0
    }

    pub fn walking(&self) -> bool {
        self.forward != 0.0 || self.strafe != 0.0
    }
}

fn axis(keys: &ButtonInput<KeyCode>, positive: &[KeyCode], negative: &[KeyCode]) -> f32 {
    let plus = keys.any_pressed(positive.iter().copied());
    let minus = keys.any_pressed(negative.iter().copied());
    match (plus, minus) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        _ => 0.0,
    }
}

/// The movement keys currently held.
pub fn read_axes(keys: &ButtonInput<KeyCode>) -> KeyAxes {
    KeyAxes {
        forward: axis(
            keys,
            &[KeyCode::KeyW, KeyCode::ArrowUp, KeyCode::Numpad8],
            &[KeyCode::KeyS, KeyCode::ArrowDown, KeyCode::Numpad2],
        ),
        strafe: axis(keys, &[KeyCode::KeyE], &[KeyCode::KeyQ]),
        turn: axis(
            keys,
            &[KeyCode::KeyD, KeyCode::ArrowRight, KeyCode::Numpad6],
            &[KeyCode::KeyA, KeyCode::ArrowLeft, KeyCode::Numpad4],
        ),
    }
}

/// One keyboard step: turn the heading (A/D), then walk along it (W/S) and
/// across it (Q/E). Returns the new yaw (rotation about +Y, positive turns
/// left) and the displacement on the ground plane.
pub fn keyboard_step(yaw: f32, input: KeyAxes, speed: f32, dt: f32) -> (f32, Vec3) {
    let yaw = wrap(yaw - input.turn * LOCOMOTION_TURN_RATE * dt);
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    let ahead = if input.forward < 0.0 {
        input.forward * BACKWARD_SPEED_SCALE
    } else {
        input.forward
    };
    let mut direction = forward * ahead + right * input.strafe;
    if direction.length_squared() > 1.0 {
        direction = direction.normalize();
    }
    (yaw, direction * speed * dt)
}

/// One step toward a goal: the yaw that faces it (the caller eases the body
/// round), the displacement (full pace once it faces the goal) and whether the
/// goal is reached (within `stop`).
pub fn goal_step(
    position: Vec3,
    yaw: f32,
    goal: MoveGoal,
    speed: f32,
    dt: f32,
) -> (f32, Vec3, bool) {
    let delta = Vec3::new(goal.point.x - position.x, 0.0, goal.point.z - position.z);
    let distance = delta.length();
    if distance <= goal.stop + 1.0e-3 || distance < 1.0e-4 {
        return (yaw, Vec3::ZERO, true);
    }
    let direction = delta / distance;
    let facing = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let pace = crate::creature::stride_alignment(facing, direction);
    let travel = (speed * pace * dt).min(distance - goal.stop);
    (
        crate::creature::facing_yaw(direction),
        direction * travel,
        false,
    )
}

fn wrap(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Walk the avatar one frame from the held movement keys, autorun or the click
/// goal. Returns true when the keys set the yaw directly (they turn at their
/// own rate); a goal only names the yaw to ease toward. The avatar stays on
/// land (`creature_planet::Land::step`; the island's square edge is gone).
pub fn steer(
    keys: &ButtonInput<KeyCode>,
    controls: &mut CreatureControls,
    transform: &mut Transform,
    speed: f32,
    dt: f32,
    land: &crate::creature_planet::Land,
) -> bool {
    if !(dt.is_finite() && dt > 0.0 && speed.is_finite()) {
        return false;
    }
    let yaw = transform.rotation.to_euler(EulerRot::YXZ).0;
    let input = controls.resolve_axes(read_axes(keys));
    let before = transform.translation;
    if input.any() {
        controls.goal = None;
        let (yaw, step) = keyboard_step(yaw, input, speed, dt);
        transform.translation = land.step(before, before + step);
        transform.rotation = Quat::from_rotation_y(yaw);
        return true;
    }
    if let Some(goal) = controls.goal {
        let (desired, step, arrived) = goal_step(transform.translation, yaw, goal, speed, dt);
        transform.translation = land.step(before, before + step);
        transform.rotation = Quat::from_rotation_y(desired);
        // The shore can swallow the step toward a goal that cannot be
        // reached (a pace of zero while the body is still turning round is not
        // that: the intended step is zero then).
        let blocked = step.length() > 1.0e-4
            && (transform.translation - before).length() < 0.25 * step.length();
        if arrived || blocked {
            controls.goal = None;
        }
    }
    false
}


impl CreatureControls {
    /// Clear the one-frame requests (start of the keyboard pass).
    pub fn begin_frame(&mut self) {
        self.target_request = TargetRequest::None;
        self.recruit_request = false;
        self.pack_stance_request = false;
        self.drop_requested = false;
    }

    /// Autorun fills in W while held movement keys are idle; S cancels it.
    pub fn resolve_axes(&mut self, mut axes: KeyAxes) -> KeyAxes {
        if axes.forward < 0.0 {
            self.autorun = false;
        }
        if self.autorun && axes.forward == 0.0 {
            axes.forward = 1.0;
        }
        axes
    }

    /// Forget every movement command (respawn, stage change).
    pub fn reset_motion(&mut self) {
        self.autorun = false;
        self.goal = None;
        self.held_move = false;
        self.pending_engage = None;
        self.mate_run = false;
        self.click = ClickState::default();
    }

    /// Select a verb tray stance; engage a standing target when it turns
    /// social (a social target must be performing, see `creature_actions`).
    pub fn set_stance(&mut self, encounter: &mut Encounter, mode: EncounterMode) {
        if encounter.mode == mode {
            return;
        }
        encounter.mode = mode;
        encounter.message = match mode {
            EncounterMode::Social => "Social stance (Tab)".into(),
            EncounterMode::Combat => "Attack stance (Tab)".into(),
        };
        if mode == EncounterMode::Social && encounter.target.is_some() {
            self.target_request = TargetRequest::Current;
        }
        self.tray_social = mode == EncounterMode::Social;
    }

    /// Target `id` and engage it as the stance asks (see `creature_actions`).
    pub fn engage(&mut self, encounter: &mut Encounter, id: u32) {
        encounter.target = Some(id);
        self.target_request = TargetRequest::Current;
        self.pending_engage = None;
    }

    /// A double-clicked creature is engaged once the avatar is within
    /// targeting range of it; walking away (the goal ending or being replaced
    /// by the keys) forgets it. `creature` is its position, if it still exists.
    pub fn resolve_pending(
        &mut self,
        encounter: &mut Encounter,
        avatar: Vec3,
        creature: Option<Vec3>,
    ) {
        let Some(id) = self.pending_engage else {
            return;
        };
        let Some(at) = creature else {
            self.pending_engage = None;
            return;
        };
        let gap = Vec3::new(at.x - avatar.x, 0.0, at.z - avatar.z).length();
        if gap <= crate::creature_actions::TARGET_RANGE - 0.5 {
            self.engage(encounter, id);
        } else if self.goal.is_none() {
            self.pending_engage = None;
        }
    }

    /// Keep the move goal on a moving point (the target of an attack that is
    /// walking to its launch point), ending `stop` units from it.
    pub fn follow(&mut self, point: Vec3, stop: f32) {
        self.goal = Some(MoveGoal { point, stop });
        self.autorun = false;
    }

    /// A click on the answering mate walked the avatar to the nest: true on the
    /// frame the avatar stands in it and the mating key should be pressed (the
    /// same route as `M` at the nest, `begin_mating`). The run ends when the
    /// call is over (the mate went away) or the avatar was steered elsewhere.
    pub fn resolve_mate_run(&mut self, life: &crate::creature_lifecycle::CreatureLifecycle, avatar: Vec3) -> bool {
        if !self.mate_run {
            return false;
        }
        if !life.mating_call_active {
            self.mate_run = false;
            return false;
        }
        if life.at_home(avatar.to_array()) {
            self.mate_run = false;
            self.goal = None;
            return true;
        }
        // Keep walking to the nest (a click goal is limited to 50 units).
        self.follow(Vec3::from_array(life.home), crate::creature_lifecycle::HOME_RADIUS * 0.5);
        false
    }

    fn set_goal(&mut self, point: Vec3, stop: f32, avatar: Vec3) {
        let mut offset = Vec3::new(point.x - avatar.x, 0.0, point.z - avatar.z);
        let length = offset.length();
        if length > GOAL_DISTANCE_LIMIT {
            offset *= GOAL_DISTANCE_LIMIT / length;
        }
        let point = Vec3::new(avatar.x + offset.x, point.y, avatar.z + offset.z);
        self.goal = Some(MoveGoal { point, stop });
        self.autorun = false;
    }

    /// Feed one frame of the left button. Returns the creature a double click
    /// asked to engage.
    pub fn apply_pointer(&mut self, sample: &PointerSample, now: f64, avatar: Vec3) -> Option<u32> {
        let mut engaged = None;
        if sample.just_pressed && !sample.over_ui {
            let double = self
                .click
                .last_press
                .is_some_and(|before| now - before <= DOUBLE_CLICK_SECS);
            self.click.press_started = Some(now);
            self.click.held = false;
            self.held_move = false;
            self.click.last_press = if double { None } else { Some(now) };
            if let Some((id, at)) = sample.creature {
                self.set_goal(at, CREATURE_APPROACH_STOP, avatar);
                if double {
                    engaged = Some(id);
                }
            } else if let Some(ground) = sample.ground {
                self.set_goal(ground, GOAL_STOP_DISTANCE, avatar);
            }
        }
        if let Some(started) = self.click.press_started {
            if sample.pressed && now - started >= HELD_MOVE_SECS {
                self.click.held = true;
                self.held_move = true;
                // Steer toward the cursor 10 units ahead; the cursor must be
                // outside the minimum radius.
                self.goal = sample.ground.and_then(|ground| {
                    let to = Vec3::new(ground.x - avatar.x, 0.0, ground.z - avatar.z);
                    (to.length() > MOUSE_MOVE_MIN_RADIUS).then(|| {
                        let point = avatar + to.normalize() * MOUSE_MOVE_GOAL_DISTANCE;
                        MoveGoal {
                            point,
                            stop: GOAL_STOP_DISTANCE,
                        }
                    })
                });
            }
            if sample.just_released || !sample.pressed {
                if self.click.held {
                    // Letting go of a held move stops the avatar.
                    self.goal = None;
                }
                self.click.held = false;
                self.held_move = false;
                self.click.press_started = None;
            }
        }
        engaged
    }
}

// ---------------------------------------------------------------------------
// Pointer
// ---------------------------------------------------------------------------

/// The two-button scheme's mapping of the buttons onto the click gesture the
/// walk / verb code reads (`UIMachineCreatureTwoButton`): the action is the
/// right button's click, which arrives as one `just_pressed` frame (nothing is
/// held, so nothing steers); the left button only selects. Returns whether this
/// frame selects.
fn two_button_buttons(sample: &mut PointerSample, right_clicked: bool, left_just_pressed: bool) -> bool {
    sample.pressed = false;
    sample.just_released = false;
    sample.just_pressed = right_clicked;
    left_just_pressed
}

/// What the mouse is doing this frame, resolved to the world.
#[derive(Debug, Clone, Copy, Default)]
pub struct PointerSample {
    pub pressed: bool,
    pub just_pressed: bool,
    pub just_released: bool,
    /// The terrain point under the cursor.
    pub ground: Option<Vec3>,
    /// The creature under the cursor: its `WorldId` and position.
    pub creature: Option<(u32, Vec3)>,
    /// The cursor is over an interface element (the click is the interface's).
    pub over_ui: bool,
}

/// Replaces the real mouse (tests and captures drive clicks through it).
#[derive(Resource, Default)]
pub struct PointerOverride(pub Option<PointerSample>);

/// Nearest creature body (`id`, centre, radius) along the ray.
pub fn pick_creature(
    origin: Vec3,
    direction: Vec3,
    bodies: impl IntoIterator<Item = (u32, Vec3, f32)>,
) -> Option<(u32, Vec3)> {
    let mut best: Option<(f32, u32, Vec3)> = None;
    for (id, centre, radius) in bodies {
        let to = centre - origin;
        let along = to.dot(direction);
        if along < 0.0 {
            continue;
        }
        let off = to.length_squared() - along * along;
        if off <= radius * radius && best.is_none_or(|(t, _, _)| along < t) {
            best = Some((along, id, centre));
        }
    }
    best.map(|(_, id, centre)| (id, centre))
}

/// What a single left click on a creature asks of the target, in agreement
/// with the cursor over it (creature_hud audit; `creature_hud::cursor`): a
/// creature that is not the target shows the targeting ring (action 7, whose
/// retail click sets the creature target, 0xD32FD0); the target shows the
/// stance cursor (social bubble / attack crosshair) and the click engages it
/// the way a double click does. A creature farther than the untarget distance
/// (retail `creatureUntargetDistance`, 17.5) is only walked toward. ADAPTED:
/// the walking half of the click and the engage on the second click are
/// OpenSpore's (the retail click posts an intention word to the avatar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickOnCreature {
    /// Make it the target (no engagement yet).
    Target,
    /// It already is the target: engage it as the stance asks.
    Engage,
    /// Out of reach of a target: walk only.
    WalkOnly,
}

pub fn click_on_creature(targeted: Option<u32>, clicked: u32, gap: f32) -> ClickOnCreature {
    if targeted == Some(clicked) {
        ClickOnCreature::Engage
    } else if gap <= crate::creature_actions::LEAVE_RANGE {
        ClickOnCreature::Target
    } else {
        ClickOnCreature::WalkOnly
    }
}

/// ADAPTED: the HUD's bottom strip (progress bar, verb tray) and the minimap
/// corner swallow clicks, as the Cell stage does for its HUD strip, until the
/// retail layouts report their own hit regions through `SpuiPointer::over_ui`.
pub fn over_hud_margin(cursor: Vec2, window: Vec2) -> bool {
    cursor.y > window.y - 110.0 || (cursor.x < 220.0 && cursor.y > window.y - 320.0)
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Keyboard commands: stance, autorun, missions window, target requests, drop,
/// recruit and Esc (untarget, else the pause menu).
pub fn apply_keys(
    controls: &mut CreatureControls,
    encounter: &mut Encounter,
    keys: &mut ButtonInput<KeyCode>,
) {
    controls.begin_frame();
    let stance = if keys.just_pressed(KEY_STANCE_SOCIAL) {
        Some(EncounterMode::Social)
    } else if keys.just_pressed(KEY_STANCE_COMBAT) {
        Some(EncounterMode::Combat)
    } else if keys.just_pressed(KeyCode::Tab) {
        Some(match encounter.mode {
            EncounterMode::Social => EncounterMode::Combat,
            EncounterMode::Combat => EncounterMode::Social,
        })
    } else {
        None
    };
    if let Some(mode) = stance {
        controls.set_stance(encounter, mode);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        controls.autorun = !controls.autorun;
        if controls.autorun {
            controls.goal = None;
        }
    }
    // Steering takes the avatar back from the walk to an attack's launch point
    // (a new intention replaces the attack behaviour's goal).
    if read_axes(keys).any() || controls.autorun {
        encounter.launch = None;
        controls.mate_run = false;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        controls.missions_visible = !controls.missions_visible;
    }
    if keys.just_pressed(KeyCode::KeyK) {
        controls.drop_requested = true;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        controls.target_request = TargetRequest::Nearest;
        controls.pending_engage = None;
    }
    if keys.just_pressed(KEY_RECRUIT) {
        controls.recruit_request = true;
    }
    if keys.just_pressed(KEY_PACK_STANCE) {
        controls.pack_stance_request = true;
    }
    if keys.just_pressed(KeyCode::Escape) && encounter.target.is_some() {
        encounter.target = None;
        controls.pending_engage = None;
        encounter.message = "Target cleared".into();
        // The press untargets; it does not also open the pause menu.
        keys.clear_just_pressed(KeyCode::Escape);
    }
    controls.tray_social = encounter.mode == EncounterMode::Social;
}

fn keyboard(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut controls: ResMut<CreatureControls>,
    mut encounter: ResMut<Encounter>,
    run: Res<crate::game::RunProgress>,
    player: Query<&Transform, With<Player>>,
    mut injected: Option<ResMut<crate::player_hud::InjectedKeys>>,
) {
    apply_keys(&mut controls, &mut encounter, &mut keys);
    // The mate was clicked and the avatar has reached the nest: mate (the key
    // is the route `creature_lifecycle_controls` already reads).
    let at = player.single().map_or(Vec3::splat(f32::NAN), |t| t.translation);
    if controls.resolve_mate_run(&run.creature_lifecycle, at) && !keys.pressed(KeyCode::KeyM) {
        keys.press(KeyCode::KeyM);
        if let Some(injected) = injected.as_mut() {
            injected.push(KeyCode::KeyM);
        }
    }
}

/// Keep the HUD's view of the stance and the target current after the
/// simulation has moved them (runs in the presentation set).
fn mirror(
    mut controls: ResMut<CreatureControls>,
    encounter: Res<Encounter>,
    ids: Query<(Entity, &WorldId)>,
) {
    let targeted = encounter
        .target
        .and_then(|id| ids.iter().find(|(_, world)| world.0 == id).map(|(e, _)| e));
    if controls.targeted != targeted {
        controls.targeted = targeted;
    }
    let social = encounter.mode == EncounterMode::Social;
    if controls.tray_social != social {
        controls.tray_social = social;
    }
}

#[allow(clippy::too_many_arguments)]
fn pointer(
    time: Res<Time>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    spui: Option<Res<crate::spui::view::SpuiPointer>>,
    hover: Option<Res<bevy::picking::hover::HoverMap>>,
    ui_buttons: Query<(), With<bevy::ui_widgets::Button>>,
    parents: Query<&ChildOf>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    injected: Option<Res<PointerOverride>>,
    player: Query<&Transform, With<Player>>,
    npcs: Query<
        (&WorldId, &Transform, &Npc, &CreatureMind),
        (Without<Player>, Without<RoamerActor>),
    >,
    roamers: Query<(&WorldId, &Transform, &RoamerActor, &Health), (Without<Player>, Without<Npc>)>,
    mut controls: ResMut<CreatureControls>,
    mut encounter: ResMut<Encounter>,
    (motion, mut right, mates, run, hover_action, egg_order, food_order): (
        Option<Res<bevy::input::mouse::AccumulatedMouseMotion>>,
        Local<crate::control_options::RightGesture>,
        Query<(&crate::creature_nestmates::NestMate, &Transform)>,
        Res<crate::game::RunProgress>,
        Option<Res<crate::creature_hud::cursor::HoverAction>>,
        Option<ResMut<crate::creature_eggs::EggOrder>>,
        Option<ResMut<crate::creature_eggs::FoodOrder>>,
    ),
) {
    let Ok(avatar) = player.single().map(|t| t.translation) else {
        return;
    };
    // `OptionCreatureControls`: `UIMachineCreatureTwoButton` (see
    // `control_options::creature_two_button`).
    let two_button = crate::control_options::creature_two_button();
    let mut select_click = false;
    let sample = if let Some(sample) = injected.as_deref().and_then(|over| over.0) {
        sample
    } else if let Some(mouse) = mouse.as_deref() {
        let mut sample = PointerSample {
            pressed: mouse.pressed(MouseButton::Left),
            just_pressed: mouse.just_pressed(MouseButton::Left),
            just_released: mouse.just_released(MouseButton::Left),
            ..default()
        };
        sample.over_ui = spui.as_deref().is_some_and(|spui| spui.over_ui)
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
        // Two-button scheme: the right button acts (`CommandMove` on the
        // terrain, `CommandAnyVerb` on anything else) when it comes up without
        // having dragged or joined the left button, at the press position; the
        // left button only selects (`CommandSelect`). ADAPTED: a double right
        // click is two such clicks within the double-click time (retail acts on
        // the second press itself); a right drag turns the camera
        // (`play_camera`), and nothing steers while a button is held.
        if two_button {
            let window = windows.iter().find(|window| window.cursor_position().is_some());
            let hud = window.zip(window.and_then(|w| w.cursor_position())).is_some_and(|(window, cursor)| {
                over_hud_margin(cursor, Vec2::new(window.width(), window.height()))
            });
            let frame = right.update(
                mouse.pressed(MouseButton::Left),
                mouse.pressed(MouseButton::Right),
                mouse.just_pressed(MouseButton::Right),
                sample.over_ui || hud,
                motion.as_deref().map_or(Vec2::ZERO, |m| m.delta),
            );
            select_click = two_button_buttons(&mut sample, frame.clicked, mouse.just_pressed(MouseButton::Left));
        } else {
            // Switching the option mid-press starts the gesture clean.
            *right = Default::default();
        }
        if sample.pressed || sample.just_pressed || select_click {
            let window = windows
                .iter()
                .find(|window| window.cursor_position().is_some());
            let cursor = window.and_then(|window| window.cursor_position());
            if let (Some(cursor), Some(window)) = (cursor, window) {
                sample.over_ui |=
                    over_hud_margin(cursor, Vec2::new(window.width(), window.height()));
            }
            let ray =
                cursor
                    .zip(cameras.single().ok())
                    .and_then(|(cursor, (camera, transform))| {
                        camera.viewport_to_world(transform, cursor).ok()
                    });
            if let Some(ray) = ray {
                sample.ground =
                    crate::civ_rts::ground_hit(ray.origin, *ray.direction, surface.as_deref());
                let bodies = npcs
                    .iter()
                    .filter(|(_, _, npc, mind)| {
                        npc.hp > 0.0 && !mind.defeated && mind.downed <= 0.0
                    })
                    .map(|(id, t, _, _)| (id.0, t.translation, PICK_RADIUS * t.scale.x.max(0.8)))
                    .chain(roamers.iter().filter(|(_, _, _, hp)| hp.hp > 0.0).map(
                        |(id, t, actor, _)| {
                            (
                                id.0,
                                t.translation,
                                crate::epic_creatures::stats(actor.kind).body_radius,
                            )
                        },
                    ));
                // The mate that answers a mating call can be clicked too (its
                // cursor is the mate cursor, see `creature_hud::cursor`).
                let mate = run
                    .creature_lifecycle
                    .mating_call_active
                    .then(|| crate::creature_nestmates::answering_mate(mates.iter().map(|(mate, _)| mate)))
                    .flatten()
                    .and_then(|index| {
                        mates.iter().find(|(mate, _)| mate.index == index).map(|(_, t)| {
                            (crate::creature_nestmates::mate_id(index), t.translation, PICK_RADIUS * t.scale.x.max(0.8))
                        })
                    });
                sample.creature = pick_creature(ray.origin, *ray.direction, bodies.chain(mate));
            }
        }
        sample
    } else {
        return;
    };
    // A click on the answering mate (action 9, 0xD32FD0 posts the mating word to
    // the avatar): ADAPTED to OpenSpore's nest rules, the avatar walks to the
    // nest and mates there (`resolve_mate_run`, the route of `M` at the nest).
    let mut sample = sample;
    let mate_hit = sample.creature.is_some_and(|(id, _)| id >= crate::creature_nestmates::MATE_ID_BASE);
    let mate_click = mate_hit && sample.just_pressed && !sample.over_ui;
    if mate_hit {
        // Never a creature target (the mate has no creature behind it).
        sample.creature = None;
    }
    if mate_click {
        sample.ground = Some(Vec3::from_array(run.creature_lifecycle.home));
        controls.mate_run = true;
    } else if sample.just_pressed && !sample.over_ui {
        controls.mate_run = false;
    }
    if select_click && !sample.over_ui {
        // `CommandSelect`: the left button of the two-button scheme targets the
        // creature under the cursor. ADAPTED: the select command was not
        // traced; a click on bare ground leaves the target alone.
        if let Some((id, _)) = sample.creature {
            encounter.target = Some(id);
        }
    }
    if let Some(id) = controls.apply_pointer(&sample, time.elapsed_secs_f64(), avatar) {
        controls.pending_engage = Some(id);
    } else if sample.just_pressed && !sample.over_ui {
        // A single click on a creature does what its cursor says (see
        // `click_on_creature`); the double click above engages as before.
        if let Some((id, at)) = sample.creature {
            let gap = Vec3::new(at.x - avatar.x, 0.0, at.z - avatar.z).length();
            match click_on_creature(encounter.target, id, gap) {
                ClickOnCreature::Target => encounter.target = Some(id),
                ClickOnCreature::Engage => controls.pending_engage = Some(id),
                ClickOnCreature::WalkOnly => {}
            }
        }
    }
    // A click on an egg or on food does what its cursor says (0xD32FD0: the
    // action ids 0x15, 8 and 0x17 post the goal words 0x80, 8 and 0x20 with the
    // object): the eat orders of `creature_eggs` walk the avatar there. Any
    // other click ends them.
    if sample.just_pressed && !sample.over_ui {
        if let (Some(hover), Some(mut egg_order), Some(mut food_order)) = (hover_action.as_deref(), egg_order, food_order) {
            use crate::creature_eggs::ClickOrder;
            egg_order.clear();
            food_order.clear();
            match crate::creature_eggs::click_order(hover.action, hover.egg, hover.food) {
                ClickOrder::Egg(egg) => egg_order.egg = Some(egg),
                ClickOrder::Food { meat, at } => {
                    food_order.meat = meat;
                    food_order.at = Some(at);
                }
                ClickOrder::None => {}
            }
        }
    }
    let position_of = |id: u32| {
        npcs.iter()
            .find(|(world, ..)| world.0 == id)
            .map(|(_, t, ..)| t.translation)
            .or_else(|| roamers.iter().find(|(world, ..)| world.0 == id).map(|(_, t, ..)| t.translation))
    };
    if let Some(id) = controls.pending_engage {
        controls.resolve_pending(&mut encounter, avatar, position_of(id));
    }
    // A click is a new goal: it replaces the walk to an attack's launch point.
    if sample.just_pressed && !sample.over_ui {
        encounter.launch = None;
    }
    // The attack behaviour (0xD809D0) walks the avatar to the launch point of its
    // ability, following the target; `creature_actions::drive_target` starts the
    // ability once the target is within reach.
    if let Some(launch) = encounter.launch {
        match position_of(launch.target) {
            Some(at) => controls.follow(at, launch.stop),
            None => encounter.launch = None,
        }
    }
}

/// Stage start and end: forget autorun, goals and requests, and start the
/// camera in the opening view (distance 15, 12 degrees) again.
fn reset_controls(
    mut controls: ResMut<CreatureControls>,
    camera: Option<ResMut<crate::play_camera::PlayCamera>>,
) {
    *controls = CreatureControls::default();
    if let Some(mut camera) = camera {
        camera.rig_mut().restart();
    }
}

/// L hides the missions window. The legacy HUD's goal cards honour it here
/// (after `player_hud::update` has shown them); the retail HUD reads
/// `CreatureControls::missions_visible` itself.
fn missions_window(
    controls: Res<CreatureControls>,
    mut cards: Query<&mut Node, With<crate::player_hud::GoalCard>>,
) {
    if controls.missions_visible {
        return;
    }
    for mut node in &mut cards {
        if node.display != Display::None {
            node.display = Display::None;
        }
    }
}

/// First-person view hides the body the camera sits inside.
fn first_person_visibility(
    camera: Res<crate::play_camera::PlayCamera>,
    mut players: Query<&mut Visibility, With<Player>>,
    mut hidden: Local<bool>,
) {
    let want = camera.rig().is_first_person();
    if want == *hidden {
        return;
    }
    for mut visibility in &mut players {
        *visibility = if want {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    *hidden = want;
}

pub struct CreatureControlsPlugin;

impl Plugin for CreatureControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreatureControls>()
            .init_resource::<PointerOverride>()
            .add_systems(OnEnter(GameStage::Creature), reset_controls)
            .add_systems(OnExit(GameStage::Creature), reset_controls)
            .add_systems(
                Update,
                (keyboard, pointer)
                    .chain()
                    .after(crate::player_hud::actions)
                    .after(crate::creature_hud::actions)
                    .before(crate::pause_menu::input)
                    .in_set(crate::GameSystems::Controls)
                    .run_if(in_state(GameStage::Creature))
                    .run_if(crate::editor::simulation_active)
                    .run_if(crate::mechanical_creator::simulation_active)
                    .run_if(crate::pause_menu::simulation_active),
            )
            .add_systems(
                Update,
                (
                    mirror,
                    first_person_visibility,
                    missions_window.after(crate::player_hud::update),
                )
                    .in_set(crate::GameSystems::Presentation)
                    .run_if(in_state(GameStage::Creature)),
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

    fn close(a: Vec3, b: Vec3) {
        assert!(a.distance(b) < 1.0e-4, "{a:?} != {b:?}");
    }

    #[test]
    fn the_walk_to_a_launch_point_follows_the_target_and_steering_ends_it() {
        use crate::creature_actions::Launch;
        let mut controls = CreatureControls::default();
        controls.follow(Vec3::new(0.0, 0.0, -30.0), 20.0);
        let goal = controls.goal.expect("a goal on the target");
        assert_eq!((goal.point, goal.stop), (Vec3::new(0.0, 0.0, -30.0), 20.0));
        // The goal moves with the target and stops short of it.
        controls.follow(Vec3::new(5.0, 0.0, -30.0), 20.0);
        let (_, step, arrived) = goal_step(Vec3::ZERO, 0.0, controls.goal.unwrap(), 5.0, 1.0);
        assert!(!arrived && step.length() > 0.0);
        let (_, _, arrived) = goal_step(Vec3::new(0.0, 0.0, -11.0), 0.0, controls.goal.unwrap(), 5.0, 1.0);
        assert!(arrived, "within 20 of the target");
        // A movement key takes the avatar back (apply_keys), so does autorun.
        let mut encounter = Encounter {
            launch: Some(Launch { target: 4, slot: 1, stop: 20.0 }),
            ..default()
        };
        apply_keys(&mut controls, &mut encounter, &mut keys(&[]));
        assert!(encounter.launch.is_some(), "nothing pressed");
        apply_keys(&mut controls, &mut encounter, &mut keys(&[KeyCode::KeyW]));
        assert!(encounter.launch.is_none());
    }

    #[test]
    fn a_click_on_the_mate_walks_to_the_nest_and_mates_there() {
        use crate::creature_lifecycle::CreatureLifecycle;
        let mut life = CreatureLifecycle::default();
        let mut controls = CreatureControls::default();
        let home = Vec3::from_array(life.home);
        // No run, nothing to do.
        assert!(!controls.resolve_mate_run(&life, home));
        controls.mate_run = true;
        // The call is not active (the mate went away): the run is dropped.
        assert!(!controls.resolve_mate_run(&life, home));
        assert!(!controls.mate_run);
        controls.mate_run = true;
        life.mating_call_active = true;
        // Away from the nest it keeps walking, at the nest it presses the key once.
        assert!(!controls.resolve_mate_run(&life, home + Vec3::new(20.0, 0.0, 0.0)));
        assert!(controls.mate_run);
        assert_eq!(controls.goal.map(|goal| goal.point), Some(home), "the goal is the nest");
        assert!(controls.resolve_mate_run(&life, home));
        assert!(!controls.mate_run);
        assert!(!controls.resolve_mate_run(&life, home), "once");
        // A steering key cancels the run.
        controls.mate_run = true;
        apply_keys(&mut controls, &mut Encounter::default(), &mut keys(&[KeyCode::KeyD]));
        assert!(!controls.mate_run);
    }

    fn click(now: f64) -> (PointerSample, f64) {
        (
            PointerSample {
                pressed: true,
                just_pressed: true,
                ground: Some(Vec3::new(5.0, 0.0, -3.0)),
                ..default()
            },
            now,
        )
    }

    #[test]
    fn movement_keys_map_to_axes_like_the_trigger_config() {
        let cases = [
            (
                vec![KeyCode::KeyW],
                KeyAxes {
                    forward: 1.0,
                    ..default()
                },
            ),
            (
                vec![KeyCode::ArrowUp],
                KeyAxes {
                    forward: 1.0,
                    ..default()
                },
            ),
            (
                vec![KeyCode::Numpad8],
                KeyAxes {
                    forward: 1.0,
                    ..default()
                },
            ),
            (
                vec![KeyCode::KeyS],
                KeyAxes {
                    forward: -1.0,
                    ..default()
                },
            ),
            (
                vec![KeyCode::ArrowDown],
                KeyAxes {
                    forward: -1.0,
                    ..default()
                },
            ),
            (
                vec![KeyCode::Numpad2],
                KeyAxes {
                    forward: -1.0,
                    ..default()
                },
            ),
            (
                vec![KeyCode::KeyA],
                KeyAxes {
                    turn: -1.0,
                    ..default()
                },
            ),
            (
                vec![KeyCode::ArrowLeft],
                KeyAxes {
                    turn: -1.0,
                    ..default()
                },
            ),
            (
                vec![KeyCode::Numpad4],
                KeyAxes {
                    turn: -1.0,
                    ..default()
                },
            ),
            (
                vec![KeyCode::KeyD, KeyCode::Numpad6],
                KeyAxes {
                    turn: 1.0,
                    ..default()
                },
            ),
            (
                vec![KeyCode::KeyQ],
                KeyAxes {
                    strafe: -1.0,
                    ..default()
                },
            ),
            (
                vec![KeyCode::KeyE],
                KeyAxes {
                    strafe: 1.0,
                    ..default()
                },
            ),
        ];
        for (pressed, expected) in cases {
            assert_eq!(read_axes(&keys(&pressed)), expected, "{pressed:?}");
        }
        assert!(!read_axes(&keys(&[])).any());
        // Opposite keys cancel; arrows and letters of one axis do not add up.
        assert_eq!(
            read_axes(&keys(&[KeyCode::KeyW, KeyCode::KeyS])).forward,
            0.0
        );
        assert_eq!(
            read_axes(&keys(&[KeyCode::KeyW, KeyCode::ArrowUp])).forward,
            1.0
        );
        assert_eq!(
            read_axes(&keys(&[KeyCode::KeyQ, KeyCode::KeyE])).strafe,
            0.0
        );
    }

    #[test]
    fn w_walks_along_the_facing_and_a_d_turn_the_creature() {
        // Facing -Z: forward is -Z, strafing right is +X.
        let (yaw, step) = keyboard_step(
            0.0,
            KeyAxes {
                forward: 1.0,
                ..default()
            },
            4.0,
            0.5,
        );
        assert_eq!(yaw, 0.0);
        close(step, Vec3::new(0.0, 0.0, -2.0));
        let (_, step) = keyboard_step(
            0.0,
            KeyAxes {
                strafe: 1.0,
                ..default()
            },
            4.0,
            0.5,
        );
        close(step, Vec3::new(2.0, 0.0, 0.0));
        // Facing -X (yaw +90): W walks -X.
        let (_, step) = keyboard_step(
            std::f32::consts::FRAC_PI_2,
            KeyAxes {
                forward: 1.0,
                ..default()
            },
            4.0,
            0.5,
        );
        close(step, Vec3::new(-2.0, 0.0, 0.0));
        // A turns left (yaw grows), D right, at the locomotion's turn rate.
        let (left, step) = keyboard_step(
            0.0,
            KeyAxes {
                turn: -1.0,
                ..default()
            },
            4.0,
            0.1,
        );
        assert!((left - LOCOMOTION_TURN_RATE * 0.1).abs() < 1e-6);
        assert_eq!(step, Vec3::ZERO, "turning alone does not move");
        let (right, _) = keyboard_step(
            0.0,
            KeyAxes {
                turn: 1.0,
                ..default()
            },
            4.0,
            0.1,
        );
        assert!((right + left).abs() < 1e-6);
        // Turning while walking is as fast as turning standing (VERIFIED: the
        // slow speed belongs to game mode 0x1654C05 only).
        let (arc, step) = keyboard_step(
            0.0,
            KeyAxes {
                forward: 1.0,
                turn: -1.0,
                ..default()
            },
            4.0,
            0.1,
        );
        assert!((arc - left).abs() < 1e-6, "{arc} vs {left}");
        assert!(step.z < 0.0 && step.x < 0.0, "an arc to the left: {step}");
        // Backward is as fast as forward (no factor in the key handler); a
        // diagonal is not faster than straight ahead.
        let (_, back) = keyboard_step(
            0.0,
            KeyAxes {
                forward: -1.0,
                ..default()
            },
            4.0,
            0.5,
        );
        close(back, Vec3::new(0.0, 0.0, 2.0 * BACKWARD_SPEED_SCALE));
        let (_, diagonal) = keyboard_step(
            0.0,
            KeyAxes {
                forward: 1.0,
                strafe: 1.0,
                ..default()
            },
            4.0,
            0.5,
        );
        assert!((diagonal.length() - 2.0).abs() < 1e-4);
    }

    #[test]
    fn steer_moves_the_avatar_stops_at_the_shore_and_cancels_goals() {
        let flat = crate::creature_planet::Land::flat();
        let mut controls = CreatureControls::default();
        let mut t = Transform::from_xyz(0.0, 0.7, 0.0);
        let held = keys(&[KeyCode::KeyW]);
        assert!(steer(&held, &mut controls, &mut t, 3.0, 0.1, &flat));
        close(t.translation, Vec3::new(0.0, 0.7, -0.3));
        // A key press cancels a click goal.
        controls.goal = Some(MoveGoal {
            point: Vec3::new(9.0, 0.0, 9.0),
            stop: 0.4,
        });
        steer(&held, &mut controls, &mut t, 3.0, 0.1, &flat);
        assert!(controls.goal.is_none());
        // No edge on the planet's land; the waterline stops the avatar.
        let land = crate::creature_planet::tests::island_planet(200.0);
        t.translation = Vec3::new(0.0, 0.7, -180.0);
        for _ in 0..200 {
            steer(&held, &mut controls, &mut t, 3.0, 0.1, &land);
        }
        assert!(land.is_land(t.translation.xz()) && t.translation.z < -190.0, "{}", t.translation);
        // Idle: nothing changes and the yaw is left to the caller.
        let before = t;
        assert!(!steer(&keys(&[]), &mut controls, &mut t, 3.0, 0.1, &flat));
        assert_eq!(t, before);
        // Garbage frames do nothing.
        assert!(!steer(&held, &mut controls, &mut t, f32::NAN, 0.1, &flat));
        assert!(!steer(&held, &mut controls, &mut t, 3.0, 0.0, &flat));
    }

    #[test]
    fn autorun_walks_until_s_or_a_click_or_r_again() {
        let mut controls = CreatureControls::default();
        let mut encounter = Encounter::default();
        let mut input = ButtonInput::default();
        input.press(KeyCode::KeyR);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert!(controls.autorun);
        let mut t = Transform::from_xyz(0.0, 0.7, 0.0);
        let flat = crate::creature_planet::Land::flat();
        assert!(steer(&keys(&[]), &mut controls, &mut t, 4.0, 0.25, &flat));
        close(t.translation, Vec3::new(0.0, 0.7, -1.0));
        // S cancels it and walks back.
        steer(
            &keys(&[KeyCode::KeyS]),
            &mut controls,
            &mut t,
            4.0,
            0.25,
            &flat,
        );
        assert!(!controls.autorun);
        // R again toggles it off; turning keys do not cancel it.
        controls.autorun = true;
        steer(
            &keys(&[KeyCode::KeyA]),
            &mut controls,
            &mut t,
            4.0,
            0.25,
            &flat,
        );
        assert!(controls.autorun);
        input.reset_all();
        input.press(KeyCode::KeyR);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert!(!controls.autorun);
        // A click goal replaces it.
        controls.autorun = true;
        let (sample, now) = click(1.0);
        controls.apply_pointer(&sample, now, Vec3::ZERO);
        assert!(!controls.autorun && controls.goal.is_some());
    }

    #[test]
    fn goal_step_turns_toward_the_point_walks_and_stops_at_the_goal_distance() {
        let goal = MoveGoal {
            point: Vec3::new(0.0, 0.0, -10.0),
            stop: GOAL_STOP_DISTANCE,
        };
        // Facing the goal: full pace, the yaw it names is the facing.
        let (yaw, step, arrived) = goal_step(Vec3::ZERO, 0.0, goal, 4.0, 0.25);
        assert!(!arrived && yaw.abs() < 1e-5);
        close(step, Vec3::new(0.0, 0.0, -1.0));
        // Facing away: it names the way round and barely moves.
        let (yaw, step, _) = goal_step(Vec3::ZERO, std::f32::consts::PI, goal, 4.0, 0.25);
        assert!(yaw.abs() < 1e-5 && step.length() < 0.3, "{step}");
        // Within the stop distance: arrived, and the last step never overshoots.
        let (_, step, arrived) = goal_step(Vec3::new(0.0, 0.0, -9.5), 0.0, goal, 40.0, 1.0);
        assert!(!arrived);
        close(step, Vec3::new(0.0, 0.0, -0.1));
        let (_, step, arrived) = goal_step(Vec3::new(0.0, 0.0, -9.6), 0.0, goal, 40.0, 1.0);
        assert!(arrived && step == Vec3::ZERO);
        // A whole walk ends within the stop distance.
        let mut t = Transform::from_xyz(2.0, 0.7, 3.0);
        let mut controls = CreatureControls {
            goal: Some(MoveGoal {
                point: Vec3::new(-4.0, 0.0, -6.0),
                stop: GOAL_STOP_DISTANCE,
            }),
            ..default()
        };
        for _ in 0..1200 {
            steer(&keys(&[]), &mut controls, &mut t, 5.0, 1.0 / 60.0, &crate::creature_planet::Land::flat());
            // The caller eases the body round; emulate it as the yaw it names.
        }
        assert!(controls.goal.is_none());
        let rest = Vec3::new(t.translation.x + 4.0, 0.0, t.translation.z + 6.0).length();
        assert!((rest - GOAL_STOP_DISTANCE).abs() < 0.05, "{rest}");
    }

    #[test]
    fn a_goal_behind_the_avatar_survives_while_it_turns_round_and_one_beyond_the_edge_is_given_up()
    {
        // Facing -Z with the goal at +Z: the first frame only names the new
        // yaw (pace zero); the goal must not be mistaken for a blocked one.
        let mut controls = CreatureControls {
            goal: Some(MoveGoal {
                point: Vec3::new(0.0, 0.0, 8.0),
                stop: GOAL_STOP_DISTANCE,
            }),
            ..default()
        };
        let mut t = Transform::from_xyz(0.0, 0.7, 0.0);
        assert!(!steer(
            &keys(&[]),
            &mut controls,
            &mut t,
            4.0,
            1.0 / 60.0,
            &crate::creature_planet::Land::flat()
        ));
        assert!(controls.goal.is_some());
        assert_eq!(t.translation, Vec3::new(0.0, 0.7, 0.0));
        let yaw = t.rotation.to_euler(EulerRot::YXZ).0;
        assert!(
            (yaw.abs() - std::f32::consts::PI).abs() < 1e-4,
            "now facing the goal: {yaw}"
        );
        // Faced round, it walks.
        steer(&keys(&[]), &mut controls, &mut t, 4.0, 1.0 / 60.0, &crate::creature_planet::Land::flat());
        assert!(t.translation.z > 0.05);
        assert!(controls.goal.is_some());

        // A goal out at sea is dropped once the shore stops the avatar.
        let land = crate::creature_planet::tests::island_planet(200.0);
        let mut controls = CreatureControls {
            goal: Some(MoveGoal {
                point: Vec3::new(0.0, 0.0, -240.0),
                stop: GOAL_STOP_DISTANCE,
            }),
            ..default()
        };
        let mut t = Transform::from_xyz(0.0, 0.7, -185.0);
        for _ in 0..2000 {
            steer(&keys(&[]), &mut controls, &mut t, 4.0, 1.0 / 60.0, &land);
            if controls.goal.is_none() {
                break;
            }
        }
        assert!(controls.goal.is_none());
        assert!(land.is_land(t.translation.xz()) && t.translation.z < -190.0, "{}", t.translation);
    }

    #[test]
    fn a_single_click_targets_a_creature_and_the_next_one_engages_it() {
        // The cursor over a creature that is not the target is the targeting
        // ring: the click sets the target; over the target the click engages.
        assert_eq!(click_on_creature(None, 5, 4.0), ClickOnCreature::Target);
        assert_eq!(click_on_creature(Some(9), 5, 4.0), ClickOnCreature::Target);
        assert_eq!(click_on_creature(Some(5), 5, 4.0), ClickOnCreature::Engage);
        assert_eq!(click_on_creature(Some(5), 5, 40.0), ClickOnCreature::Engage);
        // Beyond the untarget distance only the walk happens.
        let limit = crate::creature_actions::LEAVE_RANGE;
        assert_eq!(click_on_creature(None, 5, limit), ClickOnCreature::Target);
        assert_eq!(click_on_creature(None, 5, limit + 0.1), ClickOnCreature::WalkOnly);
    }

    #[test]
    fn the_two_button_scheme_acts_on_a_right_click_and_selects_with_the_left_button() {
        // Default scheme: the left button's flags are the gesture (untouched).
        // Two-button: a held left button neither walks nor steers, it selects on
        // the press; the right button's click is the one-frame action.
        let mut held = PointerSample { pressed: true, just_pressed: true, just_released: false, ..default() };
        assert!(two_button_buttons(&mut held, false, true), "the left press selects");
        assert!(!held.pressed && !held.just_pressed && !held.just_released, "no walk, no steer");
        let mut idle = PointerSample { pressed: true, ..default() };
        assert!(!two_button_buttons(&mut idle, true, false));
        assert!(idle.just_pressed && !idle.pressed, "the right click is one action frame");
        // That action walks to the ground point like the default scheme's click,
        // and never turns into a held steer.
        let mut controls = CreatureControls::default();
        let ground = Vec3::new(4.0, 0.0, 6.0);
        let click = PointerSample { just_pressed: true, ground: Some(ground), ..default() };
        controls.apply_pointer(&click, 10.0, Vec3::ZERO);
        assert_eq!(controls.goal.map(|g| g.point), Some(ground));
        let later = PointerSample { ground: Some(ground), ..default() };
        controls.apply_pointer(&later, 11.0, Vec3::ZERO);
        assert!(!controls.held_move);
    }

    #[test]
    fn a_click_walks_to_the_point_a_double_click_targets_and_a_hold_steers() {
        let mut controls = CreatureControls::default();
        let avatar = Vec3::ZERO;
        // Single click on the ground: a goal with the original stop distance.
        let (sample, now) = click(10.0);
        assert_eq!(controls.apply_pointer(&sample, now, avatar), None);
        let goal = controls.goal.expect("click goal");
        close(goal.point, Vec3::new(5.0, 0.0, -3.0));
        assert_eq!(goal.stop, GOAL_STOP_DISTANCE);
        // Releasing a short click keeps the goal.
        let release = PointerSample {
            just_released: true,
            ..default()
        };
        controls.apply_pointer(&release, 10.1, avatar);
        assert!(controls.goal.is_some() && !controls.held_move);
        // A second click on a creature within 400 ms of the first is a double
        // click: it targets the creature and walks up to it.
        let creature = PointerSample {
            pressed: true,
            just_pressed: true,
            creature: Some((321, Vec3::new(8.0, 0.4, 0.0))),
            ground: Some(Vec3::new(8.0, 0.0, 0.0)),
            ..default()
        };
        assert_eq!(controls.apply_pointer(&creature, 10.3, avatar), Some(321));
        assert_eq!(controls.goal.unwrap().stop, CREATURE_APPROACH_STOP);
        controls.apply_pointer(&release, 10.35, avatar);
        // Two clicks 500 ms apart are two single clicks (no target)...
        assert_eq!(controls.apply_pointer(&creature, 10.8, avatar), None);
        controls.apply_pointer(&release, 10.85, avatar);
        assert_eq!(controls.apply_pointer(&creature, 11.3, avatar), None);
        controls.apply_pointer(&release, 11.35, avatar);
        // ...a quick third one doubles the second...
        assert_eq!(controls.apply_pointer(&creature, 11.6, avatar), Some(321));
        controls.apply_pointer(&release, 11.65, avatar);
        // ...and after a double click the clock starts over.
        assert_eq!(controls.apply_pointer(&creature, 11.7, avatar), None);
        controls.apply_pointer(&release, 11.72, avatar);

        // Holding for 700 ms steers continuously toward the cursor, 10 ahead.
        let mut controls = CreatureControls::default();
        let down = PointerSample {
            pressed: true,
            just_pressed: true,
            ground: Some(Vec3::new(0.0, 0.0, -2.0)),
            ..default()
        };
        controls.apply_pointer(&down, 20.0, avatar);
        let hold = PointerSample {
            pressed: true,
            ground: Some(Vec3::new(3.0, 0.0, 0.0)),
            ..default()
        };
        controls.apply_pointer(&hold, 20.5, avatar);
        assert!(!controls.held_move, "not yet 700 ms");
        controls.apply_pointer(&hold, 20.75, avatar);
        assert!(controls.held_move);
        close(
            controls.goal.unwrap().point,
            Vec3::new(MOUSE_MOVE_GOAL_DISTANCE, 0.0, 0.0),
        );
        // The cursor inside the minimum radius stops the steering...
        let near = PointerSample {
            pressed: true,
            ground: Some(Vec3::new(0.3, 0.0, 0.0)),
            ..default()
        };
        controls.apply_pointer(&near, 20.8, avatar);
        assert!(controls.goal.is_none() && controls.held_move);
        // ...and releasing a held move stops it.
        controls.apply_pointer(&hold, 20.9, avatar);
        assert!(controls.goal.is_some());
        controls.apply_pointer(&release, 21.0, avatar);
        assert!(controls.goal.is_none() && !controls.held_move);
    }

    #[test]
    fn a_double_clicked_far_creature_is_engaged_on_arrival_and_forgotten_when_the_goal_ends() {
        let mut controls = CreatureControls::default();
        let mut encounter = Encounter::default();
        let far = Some(Vec3::new(0.0, 0.4, -15.0));
        controls.pending_engage = Some(9);
        controls.goal = Some(MoveGoal {
            point: Vec3::new(0.0, 0.0, -12.0),
            stop: CREATURE_APPROACH_STOP,
        });
        // Still far: nothing is targeted yet.
        controls.resolve_pending(&mut encounter, Vec3::ZERO, far);
        assert_eq!(encounter.target, None);
        assert_eq!(controls.pending_engage, Some(9));
        // Within targeting range: targeted and engaged once.
        controls.resolve_pending(&mut encounter, Vec3::new(0.0, 0.0, -10.0), far);
        assert_eq!(encounter.target, Some(9));
        assert_eq!(controls.target_request, TargetRequest::Current);
        assert_eq!(controls.pending_engage, None);
        // The goal ending (the keys took over) forgets it while still far.
        encounter.target = None;
        controls.pending_engage = Some(9);
        controls.goal = None;
        controls.resolve_pending(&mut encounter, Vec3::ZERO, far);
        assert_eq!(controls.pending_engage, None);
        assert_eq!(encounter.target, None);
        // A creature that is gone is forgotten.
        controls.pending_engage = Some(9);
        controls.goal = Some(MoveGoal {
            point: Vec3::ZERO,
            stop: 0.4,
        });
        controls.resolve_pending(&mut encounter, Vec3::ZERO, None);
        assert_eq!(controls.pending_engage, None);
        // F and Esc take over from a pending double click.
        controls.pending_engage = Some(9);
        let mut input = ButtonInput::default();
        input.press(KeyCode::KeyF);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert_eq!(controls.pending_engage, None);
    }

    #[test]
    fn clicks_over_the_interface_are_ignored_and_far_goals_are_limited() {
        let mut controls = CreatureControls::default();
        let (mut sample, now) = click(1.0);
        sample.over_ui = true;
        controls.apply_pointer(&sample, now, Vec3::ZERO);
        assert!(controls.goal.is_none());
        // Beyond the goal distance limit the goal is pulled in along the line.
        let far = PointerSample {
            just_pressed: true,
            pressed: true,
            ground: Some(Vec3::new(0.0, 0.0, -200.0)),
            ..default()
        };
        controls.apply_pointer(&far, 2.0, Vec3::ZERO);
        let goal = controls.goal.unwrap().point;
        assert!(goal.z < 0.0 && goal.length() <= GOAL_DISTANCE_LIMIT + 1e-3);
    }

    #[test]
    fn the_hud_strip_and_minimap_corner_swallow_clicks() {
        let window = Vec2::new(1280.0, 900.0);
        assert!(
            over_hud_margin(Vec2::new(640.0, 880.0), window),
            "progress bar strip"
        );
        assert!(
            over_hud_margin(Vec2::new(100.0, 700.0), window),
            "minimap corner"
        );
        assert!(!over_hud_margin(Vec2::new(640.0, 400.0), window));
        assert!(
            !over_hud_margin(Vec2::new(400.0, 700.0), window),
            "open ground beside the corner"
        );
        assert!(!over_hud_margin(Vec2::new(100.0, 500.0), window));
    }

    #[test]
    fn creature_picking_takes_the_nearest_body_on_the_ray() {
        let bodies = [
            (1, Vec3::new(0.0, 0.0, -10.0), 1.0),
            (2, Vec3::new(0.3, 0.0, -5.0), 1.0),
            (3, Vec3::new(4.0, 0.0, -5.0), 1.0),
            (4, Vec3::new(0.0, 0.0, 5.0), 1.0),
        ];
        let hit = pick_creature(Vec3::ZERO, Vec3::NEG_Z, bodies);
        assert_eq!(
            hit.map(|h| h.0),
            Some(2),
            "nearest body wins, off-ray and behind are ignored"
        );
        assert!(pick_creature(Vec3::ZERO, Vec3::Y, bodies).is_none());
    }

    #[test]
    fn tab_and_the_hud_keys_pick_the_stance_and_social_stance_engages_a_target() {
        let mut controls = CreatureControls::default();
        let mut encounter = Encounter::default();
        assert!(controls.tray_social && encounter.mode == EncounterMode::Social);
        let mut input = ButtonInput::default();
        input.press(KeyCode::Tab);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert_eq!(encounter.mode, EncounterMode::Combat);
        assert!(!controls.tray_social);
        input.reset_all();
        input.press(KeyCode::Tab);
        encounter.target = Some(7);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert_eq!(encounter.mode, EncounterMode::Social);
        assert_eq!(
            controls.target_request,
            TargetRequest::Current,
            "a social stance engages the target"
        );
        // The HUD's keys select their own stance (idempotent).
        input.reset_all();
        input.press(KEY_STANCE_COMBAT);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert_eq!(encounter.mode, EncounterMode::Combat);
        assert_eq!(controls.target_request, TargetRequest::None);
        input.reset_all();
        input.press(KEY_STANCE_COMBAT);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert_eq!(encounter.mode, EncounterMode::Combat);
        input.reset_all();
        input.press(KEY_STANCE_SOCIAL);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert!(controls.tray_social);
        // Requests last one frame.
        input.reset_all();
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert_eq!(controls.target_request, TargetRequest::None);
    }

    #[test]
    fn f_targets_nearest_esc_untargets_without_opening_the_menu_and_l_k_p_set_flags() {
        let mut controls = CreatureControls::default();
        let mut encounter = Encounter::default();
        let mut input = ButtonInput::default();
        input.press(KeyCode::KeyF);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert_eq!(controls.target_request, TargetRequest::Nearest);
        assert_eq!(
            encounter.mode,
            EncounterMode::Social,
            "F does not change the stance"
        );
        // Esc with no target is left for the pause menu.
        input.reset_all();
        input.press(KeyCode::Escape);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert!(input.just_pressed(KeyCode::Escape));
        // Esc with a target clears it and consumes the press.
        encounter.target = Some(5);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert_eq!(encounter.target, None);
        assert!(!input.just_pressed(KeyCode::Escape));
        input.reset_all();
        input.press(KeyCode::KeyL);
        input.press(KeyCode::KeyK);
        input.press(KEY_RECRUIT);
        input.press(KEY_PACK_STANCE);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert!(!controls.missions_visible && controls.drop_requested && controls.recruit_request && controls.pack_stance_request);
        input.reset_all();
        input.press(KeyCode::KeyL);
        apply_keys(&mut controls, &mut encounter, &mut input);
        assert!(controls.missions_visible);
        assert!(!controls.drop_requested && !controls.recruit_request && !controls.pack_stance_request);
    }

    // --- through the real schedule ------------------------------------------

    fn creature_app() -> App {
        let mut app = crate::runtime::headless_app(crate::game::RunProgress::default());
        app.world_mut()
            .resource_mut::<crate::game::RunProgress>()
            .enter_stage(GameStage::Creature);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Creature);
        app.update();
        app.update();
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

    fn player_pose(app: &mut App) -> Transform {
        let world = app.world_mut();
        *world
            .query_filtered::<&Transform, With<Player>>()
            .single(world)
            .unwrap()
    }

    #[test]
    fn schedule_w_walks_a_d_turn_and_q_e_strafe_relative_to_the_creature() {
        let mut app = creature_app();
        let start = player_pose(&mut app);
        let heading = |t: &Transform| t.rotation.to_euler(EulerRot::YXZ).0;
        // W walks along the facing, whatever the camera does.
        app.world_mut()
            .resource_mut::<crate::play_camera::PlayCamera>()
            .rig_mut()
            .orbit_by_pixels(Vec2::new(300.0, 0.0));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        for _ in 0..30 {
            app.update();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyW);
        let walked = player_pose(&mut app);
        let delta = walked.translation - start.translation;
        let delta_flat = Vec3::new(delta.x, 0.0, delta.z);
        assert!(delta_flat.length() > 0.5, "the avatar walked: {delta_flat}");
        let facing = start.rotation * Vec3::NEG_Z;
        assert!(
            delta_flat
                .normalize()
                .dot(Vec3::new(facing.x, 0.0, facing.z).normalize())
                > 0.999
        );
        assert!(
            (heading(&walked) - heading(&start)).abs() < 1e-4,
            "W does not turn the creature"
        );

        // A turns left (positive yaw) without moving; D turns it back.
        let before = player_pose(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyA);
        for _ in 0..30 {
            app.update();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyA);
        let turned = player_pose(&mut app);
        let radians = wrap(heading(&turned) - heading(&before));
        assert!(
            radians > 1.0,
            "A turned left by {radians} rad in half a second"
        );
        assert!(
            Vec3::new(
                turned.translation.x - before.translation.x,
                0.0,
                turned.translation.z - before.translation.z
            )
            .length()
                < 1e-4,
            "turning in place does not walk"
        );

        // E strafes right: perpendicular to the facing.
        let before = player_pose(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyE);
        for _ in 0..30 {
            app.update();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyE);
        let after = player_pose(&mut app);
        let strafe = Vec3::new(
            after.translation.x - before.translation.x,
            0.0,
            after.translation.z - before.translation.z,
        );
        let right = before.rotation * Vec3::X;
        assert!(
            strafe.length() > 0.5
                && strafe
                    .normalize()
                    .dot(Vec3::new(right.x, 0.0, right.z).normalize())
                    > 0.999,
            "{strafe}"
        );
        assert!((heading(&after) - heading(&before)).abs() < 1e-4);
    }

    #[test]
    fn schedule_tab_f_esc_r_and_the_click_goal() {
        let mut app = creature_app();
        // Tab toggles the stance the HUD reads.
        assert!(app.world().resource::<CreatureControls>().tray_social);
        tap(&mut app, KeyCode::Tab);
        assert!(!app.world().resource::<CreatureControls>().tray_social);
        assert_eq!(
            app.world().resource::<Encounter>().mode,
            EncounterMode::Combat
        );
        tap(&mut app, KeyCode::Tab);
        assert!(app.world().resource::<CreatureControls>().tray_social);

        // F targets a creature next to the player; Esc drops it without the menu.
        let (id, entity) = {
            let world = app.world_mut();
            let (e, id) = world
                .query_filtered::<(Entity, &WorldId), (With<Npc>, Without<Player>)>()
                .iter(world)
                .next()
                .map(|(e, id)| (e, id.0))
                .expect("a nest member");
            (id, e)
        };
        let player_at = player_pose(&mut app).translation;
        {
            let world = app.world_mut();
            let mut npcs = world.query_filtered::<(&WorldId, &mut Transform), With<Npc>>();
            for (wid, mut t) in npcs.iter_mut(world) {
                t.translation = if wid.0 == id {
                    player_at + Vec3::new(0.0, 0.0, -2.0)
                } else {
                    Vec3::new(100.0, 0.4, 100.0)
                };
            }
        }
        tap(&mut app, KeyCode::KeyF);
        assert_eq!(app.world().resource::<Encounter>().target, Some(id));
        assert_eq!(
            app.world().resource::<CreatureControls>().targeted,
            Some(entity)
        );
        tap(&mut app, KeyCode::Escape);
        assert_eq!(app.world().resource::<Encounter>().target, None);
        assert!(
            !app.world().resource::<crate::pause_menu::PauseMenu>().open,
            "Esc untargeted instead"
        );
        assert_eq!(app.world().resource::<CreatureControls>().targeted, None);
        tap(&mut app, KeyCode::Escape);
        assert!(
            app.world().resource::<crate::pause_menu::PauseMenu>().open,
            "without a target Esc pauses"
        );
        tap(&mut app, KeyCode::Escape);
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);

        // R autoruns forward without a held key.
        let before = player_pose(&mut app).translation;
        tap(&mut app, KeyCode::KeyR);
        for _ in 0..30 {
            app.update();
        }
        let after = player_pose(&mut app).translation;
        assert!(app.world().resource::<CreatureControls>().autorun);
        assert!(Vec3::new(after.x - before.x, 0.0, after.z - before.z).length() > 0.5);
        tap(&mut app, KeyCode::KeyR);
        assert!(!app.world().resource::<CreatureControls>().autorun);

        // A click on the ground makes the avatar walk there and stop.
        let here = player_pose(&mut app).translation;
        let goal = Vec3::new(here.x + 3.0, 0.0, here.z + 4.0);
        app.world_mut().resource_mut::<PointerOverride>().0 = Some(PointerSample {
            pressed: true,
            just_pressed: true,
            ground: Some(goal),
            ..default()
        });
        app.update();
        app.world_mut().resource_mut::<PointerOverride>().0 = Some(PointerSample {
            just_released: true,
            ..default()
        });
        app.update();
        app.world_mut().resource_mut::<PointerOverride>().0 = None;
        assert!(app.world().resource::<CreatureControls>().goal.is_some());
        for _ in 0..900 {
            app.update();
            if app.world().resource::<CreatureControls>().goal.is_none() {
                break;
            }
        }
        assert!(
            app.world().resource::<CreatureControls>().goal.is_none(),
            "the goal was reached"
        );
        let end = player_pose(&mut app).translation;
        let remaining = Vec3::new(end.x - goal.x, 0.0, end.z - goal.z).length();
        assert!(
            remaining < GOAL_STOP_DISTANCE + 0.1,
            "stopped {remaining} from the goal"
        );
    }
}
