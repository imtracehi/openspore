//! The Settings > Control page options, read by the stages.
//!
//! Retail keeps five bool config options (`IConfigManager`, all `$Off` in
//! `ConfigManager.txt`): `OptionCellControls` 0x0679B833, `OptionCreatureControls`
//! 0x0679B85E, `OptionTribeControls` 0x0679B868, `OptionCivControls` 0x0679B873,
//! `OptionSpaceControls` 0x0679B880. The Control page's check box of each stage
//! window (0x0679B630) sets one of them and sends message 0x0679C40D; the stage's
//! message handler then reads the option again (VERIFIED, march2017 image):
//!
//! | stage | readers | what the option selects |
//! | --- | --- | --- |
//! | Cell | 0xE50710 in `MovePlayerToMousePosition` 0xE5B790, 0xE57FFB | `steer = option \|\| lmb \|\| rmb`; the "hold the mouse" hint window 0x933103EA fades in only while the option is OFF |
//! | Creature | 0xD43902 (message handler of the Creature mode: 0x679C40D while in mode 0x1654C01) and 0xD43F6A, both call 0xD36940(option) | input state machine `UIMachineCreature` (off) or `UIMachineCreatureTwoButton` (on) |
//! | Tribe | 0xCD796F (mode enter), 0xCD8F21 (message), both call 0xCD1820(option) | `UIMachineTribe` plus `UIMachineLeftRightTranslate` (off) or `UIMachineTribeRotate` (on) |
//! | Civ | 0xCF73A1 (mode enter), 0xCFE281 (message, mode 0x1654C04), both call 0xCF4D60(option) | `UIMachineCityCiv` plus `UIMachineCivTranslate` (off) or `UIMachineCivRotate` (on) |
//! | Space | 0xFE2BF6 (mode enter), 0xFE3A13, 0x10168D4, all call 0xFE1A10(option) | trigger file `TriggerConfigSpaceGame` (off) or `TriggerConfigSpaceFlight` (on) |
//!
//! The `UIMachine*` files are installed text (PatchData `024A4F5A!<name>.024A0E52`,
//! dumped with `tools\findart.py <dir> "^UIMachine"`). The differences that
//! matter for play are summarised on each reader below. Each stage reads the
//! live value every frame, which is what the retail message handler achieves by
//! rebuilding the machine when the check box is clicked.

use bevy::prelude::*;

/// `-LongMouseMove 16` of every `UIMachine*` right-button transition: the
/// cursor has to travel this many pixels from where the button went down
/// before the press counts as a drag (VERIFIED in the installed files).
pub const LONG_MOUSE_MOVE: f32 = 16.0;

/// The Control page options, in the page's stage order. The property ids and
/// machine names are the data of the table above (checked by the tests); the
/// stages read the option through the functions below.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageOption {
    Cell,
    Creature,
    Tribe,
    Civ,
    Space,
}

#[allow(dead_code)]
impl StageOption {
    pub const ALL: [StageOption; 5] = [Self::Cell, Self::Creature, Self::Tribe, Self::Civ, Self::Space];

    /// Index into `UserSettings::stage_controls` and the page's stage windows.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The config property id (`OptionCellControls` ..).
    pub fn property(self) -> u32 {
        match self {
            Self::Cell => 0x0679_B833,
            Self::Creature => 0x0679_B85E,
            Self::Tribe => 0x0679_B868,
            Self::Civ => 0x0679_B873,
            Self::Space => 0x0679_B880,
        }
    }

    /// The installed resources the stage loads for the option off / on (the
    /// retail reader's string constants; Cell has no machine).
    pub fn machines(self, on: bool) -> &'static [&'static str] {
        match (self, on) {
            (Self::Cell, _) => &[],
            (Self::Creature, false) => &["UIMachineCreature"],
            (Self::Creature, true) => &["UIMachineCreatureTwoButton"],
            (Self::Tribe, false) => &["UIMachineTribe", "UIMachineLeftRightTranslate"],
            (Self::Tribe, true) => &["UIMachineTribe", "UIMachineTribeRotate"],
            (Self::Civ, false) => &["UIMachineCityCiv", "UIMachineCivTranslate"],
            (Self::Civ, true) => &["UIMachineCityCiv", "UIMachineCivRotate"],
            (Self::Space, false) => &["TriggerConfigSpaceGame"],
            (Self::Space, true) => &["TriggerConfigSpaceFlight"],
        }
    }

    /// The live value (`front_end::settings::stage_control_option`).
    pub fn is_on(self) -> bool {
        crate::front_end::settings::stage_control_option(self.index())
    }
}

/// Creature: `UIMachineCreatureTwoButton`. The left button only selects
/// (`CommandSelect`); the right button acts (`CommandMove` on terrain,
/// `CommandAnyVerb` on anything else) when it comes up without having moved
/// [`LONG_MOUSE_MOVE`] pixels or joined the left button, and turns the camera
/// when it did (the middle button always turns it). Off is `UIMachineCreature`:
/// the left button acts (a hold keeps steering), the right and middle buttons
/// turn the camera from the moment they go down.
pub fn creature_two_button() -> bool {
    StageOption::Creature.is_on()
}

/// Tribe: `UIMachineTribeRotate`: dragging with the right button turns the
/// camera (`TIMRotateCamera`); off (`UIMachineLeftRightTranslate`) the same drag
/// pans it (`TIMDragCamera`). A right click acts in both.
pub fn tribe_rotate_on_right() -> bool {
    StageOption::Tribe.is_on()
}

/// Civ: `UIMachineCivRotate`: a right-button drag turns the camera
/// (`CommandRotateCamera`); off (`UIMachineCivTranslate`) it pans it
/// (`CommandDragCamera`).
pub fn civ_rotate_on_right() -> bool {
    StageOption::Civ.is_on()
}

/// Space: `TriggerConfigSpaceFlight` (right and middle buttons drag the camera,
/// WASD fly relative to it); off `TriggerConfigSpaceGame` (the right button
/// flies toward the cursor).
pub fn space_flight() -> bool {
    StageOption::Space.is_on()
}

/// One frame of the right-button gesture of the two-button Creature scheme
/// (and any scheme with a rotate-on-drag right button), the `CRGRightDown ..
/// CRGBothToRight` states of the installed state machine, folded into what a
/// stage needs: is the camera turning, did the press end as a click.
#[derive(Clone, Copy, Debug, Default)]
pub struct RightGesture {
    down: bool,
    /// Net cursor displacement since the press (pixels).
    displacement: Vec2,
    /// A drag past [`LONG_MOUSE_MOVE`] or the left button joined: the press is a
    /// camera turn until the right button comes up (`CRGRightDrag`,
    /// `CRGBothToRight`).
    turning: bool,
    /// The press began over the interface (it belongs to the interface).
    over_ui: bool,
}

/// What [`RightGesture::update`] resolved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RightFrame {
    /// The camera turns this frame.
    pub rotating: bool,
    /// The right button came up and the press was a click.
    pub clicked: bool,
}

impl RightGesture {
    /// Feed one frame: the buttons' state, whether the right button went down
    /// this frame (and over the interface), and the cursor's motion.
    pub fn update(&mut self, left: bool, right: bool, right_just_pressed: bool, over_ui: bool, delta: Vec2) -> RightFrame {
        let mut frame = RightFrame::default();
        if right_just_pressed {
            *self = Self { down: true, over_ui, ..default() };
        }
        if !right {
            // The button came up (or was never down).
            if self.down && !self.turning && !self.over_ui {
                frame.clicked = true;
            }
            *self = Self::default();
            return frame;
        }
        if !self.down {
            // The right button is held from before this gesture began to
            // count (the pause menu closed over it): it is not a click.
            self.down = true;
            self.over_ui = true;
        }
        if delta.is_finite() {
            self.displacement += delta;
        }
        if left || (!self.over_ui && self.displacement.length() > LONG_MOUSE_MOVE) {
            self.turning = true;
        }
        // Both buttons turn the camera even over the interface.
        frame.rotating = left || (self.turning && !self.over_ui);
        frame
    }
}

/// Tests that change a live option take this lock for as long as they hold
/// the option, and restore it to off afterwards; tests that depend on an option
/// being off take it too ([`hold_option`]). The options are process-wide
/// (`front_end::settings`), the test harness runs tests in parallel.
#[cfg(test)]
pub struct OptionGuard {
    stage: usize,
    _lock: std::sync::MutexGuard<'static, ()>,
}

/// An option is held by a test: another test app's first front-end frame publishes the
/// default settings (every option off, `UserSettings::publish`), which would race the
/// held value, so `publish` leaves the options alone while this is set.
#[cfg(test)]
static OPTION_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
pub fn option_held() -> bool {
    OPTION_HELD.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
pub fn hold_option(option: StageOption, on: bool) -> OptionGuard {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let lock = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    OPTION_HELD.store(true, std::sync::atomic::Ordering::SeqCst);
    crate::front_end::settings::set_stage_control_option(option.index(), on);
    OptionGuard { stage: option.index(), _lock: lock }
}

#[cfg(test)]
impl Drop for OptionGuard {
    fn drop(&mut self) {
        crate::front_end::settings::set_stage_control_option(self.stage, false);
        OPTION_HELD.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_name_the_retail_properties_and_machines() {
        let ids: Vec<u32> = StageOption::ALL.iter().map(|o| o.property()).collect();
        assert_eq!(ids, [0x0679_B833, 0x0679_B85E, 0x0679_B868, 0x0679_B873, 0x0679_B880]);
        assert!(StageOption::ALL.iter().enumerate().all(|(i, o)| o.index() == i));
        assert_eq!(StageOption::Creature.machines(false), ["UIMachineCreature"]);
        assert_eq!(StageOption::Creature.machines(true), ["UIMachineCreatureTwoButton"]);
        assert_eq!(StageOption::Tribe.machines(true)[1], "UIMachineTribeRotate");
        assert_eq!(StageOption::Tribe.machines(false)[1], "UIMachineLeftRightTranslate");
        assert_eq!(StageOption::Civ.machines(true), ["UIMachineCityCiv", "UIMachineCivRotate"]);
        assert_eq!(StageOption::Civ.machines(false), ["UIMachineCityCiv", "UIMachineCivTranslate"]);
        assert_eq!(StageOption::Space.machines(true), ["TriggerConfigSpaceFlight"]);
        assert!(StageOption::Cell.machines(true).is_empty());
    }

    fn press(g: &mut RightGesture, left: bool, over_ui: bool) -> RightFrame {
        g.update(left, true, true, over_ui, Vec2::ZERO)
    }

    #[test]
    fn a_still_right_press_is_a_click_when_it_comes_up() {
        let mut g = RightGesture::default();
        assert_eq!(press(&mut g, false, false), RightFrame::default());
        assert_eq!(g.update(false, true, false, false, Vec2::new(6.0, 5.0)), RightFrame::default());
        let up = g.update(false, false, false, false, Vec2::ZERO);
        assert!(up.clicked && !up.rotating);
        // Nothing is left of it afterwards.
        assert_eq!(g.update(false, false, false, false, Vec2::ZERO), RightFrame::default());
    }

    #[test]
    fn a_right_drag_past_sixteen_pixels_turns_the_camera_and_is_no_click() {
        let mut g = RightGesture::default();
        press(&mut g, false, false);
        // 15 pixels: still a press.
        assert!(!g.update(false, true, false, false, Vec2::new(15.0, 0.0)).rotating);
        // 17: a drag; it keeps turning even if the cursor comes back.
        assert!(g.update(false, true, false, false, Vec2::new(2.0, 0.0)).rotating);
        assert!(g.update(false, true, false, false, Vec2::new(-30.0, 0.0)).rotating);
        let up = g.update(false, false, false, false, Vec2::ZERO);
        assert!(!up.clicked && !up.rotating);
    }

    #[test]
    fn both_buttons_turn_the_camera_until_the_right_button_comes_up() {
        let mut g = RightGesture::default();
        // Left held first, then right goes down: BothDown.
        let f = press(&mut g, true, false);
        assert!(f.rotating);
        // Left comes up first: the turn goes on (CRGBothToRight) ...
        assert!(g.update(false, true, false, false, Vec2::ZERO).rotating);
        // ... until the right button comes up, and that is no click.
        let up = g.update(false, false, false, false, Vec2::ZERO);
        assert!(!up.rotating && !up.clicked);
        // Right first, then left joins.
        let mut g = RightGesture::default();
        press(&mut g, false, false);
        assert!(g.update(true, true, false, false, Vec2::ZERO).rotating);
    }

    #[test]
    fn a_press_over_the_interface_is_neither_a_click_nor_a_turn() {
        let mut g = RightGesture::default();
        press(&mut g, false, true);
        assert!(!g.update(false, true, false, true, Vec2::new(60.0, 0.0)).rotating);
        let up = g.update(false, false, false, true, Vec2::ZERO);
        assert!(!up.clicked);
        // A held button that was never seen going down is not a click either.
        let mut g = RightGesture::default();
        g.update(false, true, false, false, Vec2::ZERO);
        assert!(!g.update(false, false, false, false, Vec2::ZERO).clicked);
    }
}
