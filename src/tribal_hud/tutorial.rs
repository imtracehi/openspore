//! The Tribal tutorial manager: what the retail tribe game listener (the
//! message handler 0xCD81F0, `ShowTutorial` 0xCD6B00 and the hint chain
//! 0xCD62B0, object fields +0x198 / +0x1A4 / +0x394 = skipped / forced /
//! goal manager) does with callouts, hints and the `TribeTutorial_*` goals.
//! Pure rules; `guide.rs` feeds them the game's events and shows the result.
//! Read from the march2017 code image (`notes\tribal-flow-notes.md` T11 has
//! the evidence); VERIFIED = read in code, ADAPTED = OpenSpore choice.
//!
//! * `TRG_Begin` stopped (0xCD8419 branch, the cinematic name is checked):
//!   with the Tutorials option (0x04EA96CB) on the game is paused (token
//!   0x04BF38A7), the goal `TribeTutorial_Accessorize` ("Outfit": visit the
//!   Tribal Planner) is added (0xB707A0) and the callouts `Opening`,
//!   `ProgressBar`, `FirstGoals` are shown (the last carries the dismiss
//!   callback of mode 1). `Callout_TRG_Posse` has no code site at all.
//! * Dismissing `FirstGoals` (callback 0xCD6980, mode 1): resume, then with
//!   Game Hints (option 0x05B5BB5E) on the tutorial locks the UI (+0x390,
//!   input lock) and points at the planner (effect 0x1A2CC3F0): the forced
//!   step. Closing the planner (UI event 0xB2699147) while the tutorial is
//!   forced runs `ShowTutorial(0x064BE5FF)`: the callouts `TutorialMode` and
//!   `UICameraControls` (mode 2) and the UI unlocks. Dismissing
//!   `UICameraControls` shows the first chain hint (`trg-camzoom`).
//! * The forced hint chain is `0xCD62B0(step)`; each step is only taken while
//!   the hint of the step before is the one on screen (the cascade in
//!   `ShowTutorial` clears every later step flag, so the order is enforced):
//!
//!   | step (event) | id | the hint shown next |
//!   | --- | --- | --- |
//!   | zoom (UI buttons 0xCC9CED, wheel 0xCD76B0) | 0x064C010F | `trg-camrotate` (`TRG-CamRotate-MAC` when app prop 0x061B67B6) |
//!   | rotate (UI buttons 0xCC9CC6, mouse 0xCD3292) | 0x53203528 | `trg-camtranslate` |
//!   | translate (0xCD31A3) | 0x83861976 | `trg-minimapuse` |
//!   | minimap click (0xE0BBF9, 0xE0BC5B) | 0x064D44F9 | `trg-selectmemberfromui` |
//!   | select from the tribe list (0xCCB74A) | 0x064E70B3 | `trg-selectbox` |
//!   | select with a box (drag end 0xCD08AD) | 0x5AC72D7C | the diet goal + `trg-tribeeat*` |
//!
//! * Goals (`0xB707A0` add, `0xB708A0` complete; the goal props are
//!   `TribeTutorial_*` of `CreatureGoalCards`, parent `Tribe_Goal_Template`,
//!   props `priority`, `goalValue` and three texts): Outfit (added after
//!   `TRG_Begin`, done when the tribe wears outfit points, 0xCD2187), the diet
//!   goal `TribeTutorial_EatingCarnivore` / `Herbivore` / `Omnivore` (added
//!   by the last chain step, done by the food flag 0x14363974 at food >= 40),
//!   `Mating` (the populate card: added with the food flag when the tribe is
//!   below its member cap and no baby was ever born, done by the first baby,
//!   0xCD764C), `Conquer` (the "Interact" card: added when the first NPC
//!   tribe is created, 0xCC6CF9, done when a tribe's state turns to 5,
//!   0xC2F8B0) and `Domesticate` (added at the first member command on a wild
//!   animal, 0xCD1427, done by the first tamed animal, 0xDB5FAB). The goals
//!   `BuyATool`, `GrabTool` and `Select` exist as props but nothing in the
//!   code ever adds them.
//! * `TRG_FirstBaby` stopped -> `BabyPosse` (0xCD85D7); `TRG_PRE_FirstTribeAppears`
//!   stopped -> `ChieftainWithPowers` / `ChieftainNoPowers` by cPlayer +0x1100
//!   (0xCD86B1 / 0xCD86E6); both with Tutorials on and the game paused.
//! * With the Tutorials option off nothing of the opening, the chain or the
//!   Outfit / diet / populate goals exists (the 0xCDF24A path sets every flag).
//!
//! ADAPTED: the flags are per campaign (retail: profile flags, key {id, 0,
//! 0xAA9A8ED7}); the UI lock and the planner highlight of the forced step are
//! not built; the select steps are "a member selected" and "two members
//! selected" (retail: a click on the tribe list, the end of a selection box);
//! the 40 food of the food flag is the default of the tuning (prop
//! 0xB79E0065); the `priority` of the goal props is not read (the cards stay
//! in the order the goals were added).

use serde::{Deserialize, Serialize};

use crate::creature_hud::Diet;

/// Callout names (`Callout_TRG_*`, property group 0x3629F036).
pub const OPENING: [&str; 3] = ["Callout_TRG_Opening", "Callout_TRG_ProgressBar", "Callout_TRG_FirstGoals"];
pub const FIRST_GOALS: &str = "Callout_TRG_FirstGoals";
pub const MODE_CALLOUTS: [&str; 2] = ["Callout_TRG_TutorialMode", "Callout_TRG_UICameraControls"];
pub const CAMERA_CONTROLS: &str = "Callout_TRG_UICameraControls";

/// The hints of the chain (`HintsTRG`).
pub const HINT_ZOOM: &str = "trg-camzoom";
pub const HINT_ROTATE: &str = "trg-camrotate";
/// The Mac id replaces the rotate hint when app property 0x061B67B6 is set
/// (0xCD3FE0); OpenSpore runs on Windows.
#[allow(dead_code)]
pub const HINT_ROTATE_MAC: &str = "TRG-CamRotate-MAC";
pub const HINT_TRANSLATE: &str = "trg-camtranslate";
pub const HINT_MINIMAP: &str = "trg-minimapuse";
pub const HINT_SELECT_LIST: &str = "trg-selectmemberfromui";
pub const HINT_SELECT_BOX: &str = "trg-selectbox";

/// Every hint the chain shows, the diet goal's three included: the Tutorials
/// option going off hides them (0xCDF24A).
pub const CHAIN_HINTS: [&str; 9] = [
    HINT_ZOOM,
    HINT_ROTATE,
    HINT_TRANSLATE,
    HINT_MINIMAP,
    HINT_SELECT_LIST,
    HINT_SELECT_BOX,
    "trg-tribeeatomni",
    "trg-tribeeatherbi",
    "trg-tribeeatcarni",
];

/// The hint of the diet goal's first appearance (0xCD6477 / 0xCD6498 /
/// 0xCD64C3: omnivore, herbivore, carnivore).
pub fn eating_hint(diet: Diet) -> &'static str {
    match diet {
        Diet::Omnivore => "trg-tribeeatomni",
        Diet::Herbivore => "trg-tribeeatherbi",
        Diet::Carnivore => "trg-tribeeatcarni",
    }
}

/// Food at which the food flag (0xC91B00, `ShowTutorial(0x14363974)`) fires:
/// the default of the tuning property 0xB79E0065.
pub const FOOD_FLAG_FOOD: i32 = 40;
/// `goalValue` of the diet goals (prop 0x6B15B5CB = 50) and of `Domesticate`
/// (1): the number on the goal card.
pub const DIET_GOAL_VALUE: i32 = 50;
pub const DOMESTICATE_GOAL_VALUE: i32 = 1;

/// How far the camera's goal has to move (the same units as the rig: distance
/// and focus in world units, azimuth in radians) before the tutorial counts it
/// as the player's camera input. ADAPTED: the retail events are the input
/// messages and the camera UI buttons themselves.
pub const ZOOM_EPSILON: f32 = 0.05;
pub const ROTATE_EPSILON: f32 = 0.005;
pub const PAN_EPSILON: f32 = 0.05;

/// The goals of the `TribeTutorial_*` props.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TutorialGoal {
    /// `TribeTutorial_Accessorize` (card "Outfit").
    Outfit,
    /// `TribeTutorial_EatingCarnivore` / `Herbivore` / `Omnivore`.
    Diet,
    /// `TribeTutorial_Mating` (card "Populate").
    Populate,
    /// `TribeTutorial_Conquer` (card "Interact").
    Interact,
    /// `TribeTutorial_Domesticate`.
    Domesticate,
}

/// Where the forced tutorial stands (the step flags and +0x1A4 / +0x198).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    /// The stage has not started its tutorial yet.
    #[default]
    Waiting,
    /// The opening callouts are up.
    Opening,
    /// `FirstGoals` was dismissed: the planner is the forced step.
    Planner,
    /// `TutorialMode` and `UICameraControls` are up.
    Callouts,
    Zoom,
    Rotate,
    Translate,
    Minimap,
    SelectList,
    SelectBox,
    /// The chain is over (or there never was one: Tutorials off, a stage
    /// entered from somewhere other than the Creature stage).
    Done,
}

/// One step of the chain (the event that moves it on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Zoom,
    Rotate,
    Translate,
    Minimap,
    SelectList,
    SelectBox,
}

/// Flags of [`TribalTutorial::flags`] (each a retail profile flag).
pub mod flag {
    /// The food flag 0x14363974 fired.
    pub const FOOD: u32 = 1 << 0;
    /// A baby was born (`ShowTutorial(Mating)`, 0xCD764C).
    pub const BORN: u32 = 1 << 1;
    /// The first NPC tribe was created (the `Conquer` goal was added).
    pub const TRIBE: u32 = 1 << 2;
    /// The first wild animal command added `Domesticate`.
    pub const TAME_TRIED: u32 = 1 << 3;
    /// `BabyPosse` was shown after the first baby (`TRG_FirstBaby` stopped).
    pub const BABY_CALLOUT: u32 = 1 << 4;
    /// `ChieftainWithPowers` was shown when the first tribe appeared
    /// (`TRG_PRE_FirstTribeAppears` stopped).
    pub const CHIEFTAIN_CALLOUT: u32 = 1 << 5;
}

/// The saved tutorial state (`TribalCampaign::tutorial`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TribalTutorial {
    #[serde(default)]
    pub stage: Stage,
    /// Goals added so far, in the order they were added.
    #[serde(default)]
    pub added: Vec<TutorialGoal>,
    #[serde(default)]
    pub done: Vec<TutorialGoal>,
    #[serde(default)]
    pub flags: u32,
}

impl TribalTutorial {
    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    /// The chain is running (a forced step or a chain hint is pending).
    pub fn forced(&self) -> bool {
        !matches!(self.stage, Stage::Waiting | Stage::Done)
    }

    /// The goals on the cards: added and not completed, in the order added.
    pub fn goals(&self) -> Vec<TutorialGoal> {
        self.added.iter().copied().filter(|g| !self.done.contains(g)).collect()
    }

    /// `AddGoal` (0xB707A0): a goal that was ever added is not added again.
    pub fn add(&mut self, goal: TutorialGoal) {
        if !self.added.contains(&goal) {
            self.added.push(goal);
        }
    }

    /// `CompleteGoal` (0xB708A0).
    pub fn complete(&mut self, goal: TutorialGoal) {
        if self.added.contains(&goal) && !self.done.contains(&goal) {
            self.done.push(goal);
        }
    }

    /// `TRG_Begin` stopped (or the HUD's first moment, ADAPTED): the opening of
    /// a stage entered fresh from the Creature stage with Tutorials on (0xCD8419);
    /// any other entry skips the tutorial (the flag 0x056D1871 of 0xCDE8F6).
    /// Returns the callouts to queue.
    pub fn begin(&mut self, tutorials: bool, fresh: bool) -> Vec<&'static str> {
        if self.stage != Stage::Waiting {
            return Vec::new();
        }
        if !(tutorials && fresh) {
            self.stage = Stage::Done;
            return Vec::new();
        }
        self.stage = Stage::Opening;
        self.add(TutorialGoal::Outfit);
        OPENING.to_vec()
    }

    /// A callout was dismissed: the callbacks of 0xCD6980. `hints` is Game Hints.
    pub fn dismissed(&mut self, name: &str, hints: bool) {
        match (self.stage, name) {
            // Mode 1: resume, then (with Game Hints) the forced planner step.
            (Stage::Opening, FIRST_GOALS) => {
                self.stage = if hints { Stage::Planner } else { Stage::Done };
            }
            // Mode 2: resume and show the first hint of the chain.
            (Stage::Callouts, CAMERA_CONTROLS) => {
                self.stage = if hints { Stage::Zoom } else { Stage::Done };
            }
            _ => {}
        }
    }

    /// The Tribal Planner was closed (UI event 0xB2699147) with the tutorial
    /// forced: `ShowTutorial(0x064BE5FF)`. Returns the callouts to queue.
    pub fn planner_closed(&mut self) -> Vec<&'static str> {
        if self.stage != Stage::Planner {
            return Vec::new();
        }
        self.stage = Stage::Callouts;
        MODE_CALLOUTS.to_vec()
    }

    /// Whether the flag of a chain step is set (the checker's `ZoomCamera`,
    /// `RotateCamera`, ... answers, 0xE08703..0xE08780): the chain moved past
    /// the step. ADAPTED: the flags are the chain's progress (retail: profile
    /// flags the step events set, which also persist across games).
    pub fn step_done(&self, step: Step) -> bool {
        let order = |stage: Stage| match stage {
            Stage::Waiting => 0,
            Stage::Opening => 1,
            Stage::Planner => 2,
            Stage::Callouts => 3,
            Stage::Zoom => 4,
            Stage::Rotate => 5,
            Stage::Translate => 6,
            Stage::Minimap => 7,
            Stage::SelectList => 8,
            Stage::SelectBox => 9,
            Stage::Done => 10,
        };
        let step = match step {
            Step::Zoom => Stage::Zoom,
            Step::Rotate => Stage::Rotate,
            Step::Translate => Stage::Translate,
            Step::Minimap => Stage::Minimap,
            Step::SelectList => Stage::SelectList,
            Step::SelectBox => Stage::SelectBox,
        };
        order(self.stage) > order(step)
    }

    /// The hint of the chain on screen (the step the player has to take).
    pub fn chain_hint(&self) -> Option<&'static str> {
        match self.stage {
            Stage::Zoom => Some(HINT_ZOOM),
            Stage::Rotate => Some(HINT_ROTATE),
            Stage::Translate => Some(HINT_TRANSLATE),
            Stage::Minimap => Some(HINT_MINIMAP),
            Stage::SelectList => Some(HINT_SELECT_LIST),
            Stage::SelectBox => Some(HINT_SELECT_BOX),
            _ => None,
        }
    }

    /// A step event (`ShowTutorial(step)` then 0xCD62B0): it moves the chain
    /// only when it is the step the current hint asks for. The last step adds
    /// the diet goal and returns the hint that comes with it.
    pub fn step(&mut self, step: Step, diet: Diet) -> Option<&'static str> {
        let (expected, next) = match step {
            Step::Zoom => (Stage::Zoom, Stage::Rotate),
            Step::Rotate => (Stage::Rotate, Stage::Translate),
            Step::Translate => (Stage::Translate, Stage::Minimap),
            Step::Minimap => (Stage::Minimap, Stage::SelectList),
            Step::SelectList => (Stage::SelectList, Stage::SelectBox),
            Step::SelectBox => (Stage::SelectBox, Stage::Done),
        };
        if self.stage != expected {
            return None;
        }
        self.stage = next;
        if next == Stage::Done {
            self.add(TutorialGoal::Diet);
            return Some(eating_hint(diet));
        }
        None
    }

    /// The Tutorials option was switched off: the 0xCDF24A path ends the chain.
    pub fn switched_off(&mut self) {
        if self.forced() {
            self.stage = Stage::Done;
        }
    }

    /// The food flag (0xC91B00 -> `ShowTutorial(0x14363974)`): the diet goals
    /// are done, and a tribe under its member cap that never had a baby gets
    /// the populate goal (0xCD62B0). `tutorials_and_hints` gates the second
    /// part (the chain function only runs with both options on).
    pub fn food_flag(&mut self, members: usize, cap: usize, tutorials_and_hints: bool) {
        if self.has(flag::FOOD) {
            return;
        }
        self.flags |= flag::FOOD;
        self.complete(TutorialGoal::Diet);
        if tutorials_and_hints && members < cap && !self.has(flag::BORN) {
            self.add(TutorialGoal::Populate);
        }
    }

    /// The first baby (0xCD764C).
    pub fn baby_born(&mut self) {
        self.flags |= flag::BORN;
        self.complete(TutorialGoal::Populate);
    }

    /// The first NPC tribe was created (0xCC6CF9): the Interact card.
    pub fn tribe_created(&mut self) {
        if !self.has(flag::TRIBE) {
            self.flags |= flag::TRIBE;
            self.add(TutorialGoal::Interact);
        }
    }

    /// A tribe turned allied or conquered (0xC2F8B0).
    pub fn tribe_settled(&mut self) {
        self.complete(TutorialGoal::Interact);
    }

    /// The first command on a wild animal (0xCD1427).
    pub fn animal_command(&mut self) {
        if !self.has(flag::TAME_TRIED) {
            self.flags |= flag::TAME_TRIED;
            self.add(TutorialGoal::Domesticate);
        }
    }

    /// The first tamed animal (0xDB5FAB).
    pub fn animal_tamed(&mut self) {
        self.complete(TutorialGoal::Domesticate);
    }

    /// The tribe wears outfit points (0xCD2187).
    pub fn outfit_worn(&mut self) {
        self.complete(TutorialGoal::Outfit);
    }
}

/// Camera and selection changes of one frame, read by [`Events::from_frames`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraSample {
    /// The rig's goal distance, azimuth and focus.
    pub distance: f32,
    pub azimuth: f32,
    pub focus: [f32; 3],
}

/// The step events a frame produced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    pub zoom: bool,
    pub rotate: bool,
    pub translate: bool,
}

impl Events {
    /// Which camera channels the player moved between two frames. `jump`: a
    /// minimap click or a follow moved the focus (that is no camera pan).
    pub fn from_frames(before: CameraSample, now: CameraSample, jump: bool) -> Self {
        let pan = (0..3).map(|i| (now.focus[i] - before.focus[i]).abs()).fold(0.0, f32::max);
        Self {
            zoom: (now.distance - before.distance).abs() > ZOOM_EPSILON,
            rotate: (now.azimuth - before.azimuth).abs() > ROTATE_EPSILON,
            translate: !jump && pan > PAN_EPSILON,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> TribalTutorial {
        let mut t = TribalTutorial::default();
        assert_eq!(t.begin(true, true), OPENING);
        t
    }

    #[test]
    fn the_opening_is_three_callouts_and_the_outfit_goal() {
        let t = running();
        assert_eq!(t.stage, Stage::Opening);
        assert_eq!(t.goals(), [TutorialGoal::Outfit]);
        assert!(!OPENING.contains(&"Callout_TRG_Posse"), "the posse callout has no retail code site");
    }

    #[test]
    fn tutorials_off_or_a_returning_stage_has_no_tutorial_at_all() {
        let mut t = TribalTutorial::default();
        assert!(t.begin(false, true).is_empty());
        assert_eq!(t.stage, Stage::Done);
        assert!(t.goals().is_empty());
        let mut t = TribalTutorial::default();
        assert!(t.begin(true, false).is_empty());
        assert_eq!(t.stage, Stage::Done);
        // The opening happens once.
        let mut t = running();
        assert!(t.begin(true, true).is_empty());
    }

    #[test]
    fn the_forced_steps_run_in_the_retail_order() {
        let mut t = running();
        // Only the last callout of the opening starts the forced planner step.
        t.dismissed("Callout_TRG_Opening", true);
        assert_eq!(t.stage, Stage::Opening);
        t.dismissed(FIRST_GOALS, true);
        assert_eq!(t.stage, Stage::Planner);
        // Nothing else moves it on: no camera hint before the planner closed.
        assert_eq!(t.step(Step::Zoom, Diet::Herbivore), None);
        assert_eq!(t.stage, Stage::Planner);
        assert_eq!(t.planner_closed(), MODE_CALLOUTS);
        assert_eq!(t.stage, Stage::Callouts);
        assert!(t.planner_closed().is_empty());
        t.dismissed("Callout_TRG_TutorialMode", true);
        assert_eq!(t.stage, Stage::Callouts);
        t.dismissed(CAMERA_CONTROLS, true);
        assert_eq!(t.chain_hint(), Some("trg-camzoom"));
        // Out of order events are ignored: the hint on screen decides.
        assert_eq!(t.step(Step::Rotate, Diet::Herbivore), None);
        assert_eq!(t.chain_hint(), Some("trg-camzoom"));
        let order = [
            (Step::Zoom, "trg-camrotate"),
            (Step::Rotate, "trg-camtranslate"),
            (Step::Translate, "trg-minimapuse"),
            (Step::Minimap, "trg-selectmemberfromui"),
            (Step::SelectList, "trg-selectbox"),
        ];
        for (step, next) in order {
            assert_eq!(t.step(step, Diet::Herbivore), None);
            assert_eq!(t.chain_hint(), Some(next));
        }
        // The last step ends the chain and brings the diet goal with its hint.
        assert_eq!(t.step(Step::SelectBox, Diet::Herbivore), Some("trg-tribeeatherbi"));
        assert_eq!(t.stage, Stage::Done);
        assert_eq!(t.chain_hint(), None);
        assert_eq!(t.goals(), [TutorialGoal::Outfit, TutorialGoal::Diet]);
    }

    #[test]
    fn game_hints_off_stops_the_forced_chain() {
        let mut t = running();
        t.dismissed(FIRST_GOALS, false);
        assert_eq!(t.stage, Stage::Done, "the callback of mode 1 needs Game Hints");
        let mut t = running();
        t.dismissed(FIRST_GOALS, true);
        t.planner_closed();
        t.dismissed(CAMERA_CONTROLS, false);
        assert_eq!(t.stage, Stage::Done, "mode 2 shows the first hint only with Game Hints");
        assert_eq!(t.chain_hint(), None);
    }

    #[test]
    fn the_diet_hint_follows_the_diet() {
        assert_eq!(eating_hint(Diet::Carnivore), "trg-tribeeatcarni");
        assert_eq!(eating_hint(Diet::Omnivore), "trg-tribeeatomni");
        assert_eq!(eating_hint(Diet::Herbivore), "trg-tribeeatherbi");
    }

    #[test]
    fn the_food_flag_ends_the_diet_goal_and_asks_for_babies_once() {
        let mut t = running();
        t.add(TutorialGoal::Diet);
        t.food_flag(3, 6, true);
        assert_eq!(t.goals(), [TutorialGoal::Outfit, TutorialGoal::Populate]);
        // The flag fires once.
        t.baby_born();
        t.food_flag(3, 6, true);
        assert_eq!(t.goals(), [TutorialGoal::Outfit]);
        // A full tribe, a tribe that already had a baby or the options off: no card.
        let mut full = TribalTutorial::default();
        full.food_flag(6, 6, true);
        assert!(full.goals().is_empty());
        let mut veteran = TribalTutorial { flags: flag::BORN, ..Default::default() };
        veteran.food_flag(3, 6, true);
        assert!(veteran.goals().is_empty());
        let mut quiet = TribalTutorial::default();
        quiet.food_flag(3, 6, false);
        assert!(quiet.goals().is_empty());
    }

    #[test]
    fn interact_and_domesticate_come_with_their_events_and_never_return() {
        let mut t = TribalTutorial::default();
        t.tribe_created();
        t.tribe_created();
        t.animal_command();
        assert_eq!(t.goals(), [TutorialGoal::Interact, TutorialGoal::Domesticate]);
        t.tribe_settled();
        t.animal_tamed();
        assert!(t.goals().is_empty());
        t.tribe_created();
        t.animal_command();
        assert!(t.goals().is_empty(), "a goal that was done is not added again");
        // Completing a goal nobody added does nothing.
        let mut t = TribalTutorial::default();
        t.baby_born();
        t.outfit_worn();
        assert!(t.done.is_empty());
    }

    #[test]
    fn the_buy_and_grab_goals_are_never_added() {
        // `BuyATool`, `GrabTool` and `Select` exist in the data and nothing
        // adds them: the enum has no such variant.
        let all = [
            TutorialGoal::Outfit,
            TutorialGoal::Diet,
            TutorialGoal::Populate,
            TutorialGoal::Interact,
            TutorialGoal::Domesticate,
        ];
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn turning_tutorials_off_ends_the_chain() {
        let mut t = running();
        t.dismissed(FIRST_GOALS, true);
        t.switched_off();
        assert_eq!(t.stage, Stage::Done);
        let mut done = TribalTutorial::default();
        done.switched_off();
        assert_eq!(done.stage, Stage::Waiting, "a stage that has not begun is left alone");
    }

    #[test]
    fn camera_events_need_real_movement() {
        let a = CameraSample { distance: 70.0, azimuth: 0.0, focus: [1.0, 0.0, 2.0] };
        assert_eq!(Events::from_frames(a, a, false), Events::default());
        let zoomed = CameraSample { distance: 60.0, ..a };
        assert_eq!(Events::from_frames(a, zoomed, false), Events { zoom: true, ..Default::default() });
        let rotated = CameraSample { azimuth: 0.2, ..a };
        assert_eq!(Events::from_frames(a, rotated, false), Events { rotate: true, ..Default::default() });
        let moved = CameraSample { focus: [5.0, 0.0, 2.0], ..a };
        assert_eq!(Events::from_frames(a, moved, false), Events { translate: true, ..Default::default() });
        assert_eq!(Events::from_frames(a, moved, true), Events::default(), "a minimap jump is not a pan");
    }

    #[test]
    fn the_state_round_trips_and_old_saves_load() {
        let mut t = running();
        t.dismissed(FIRST_GOALS, true);
        t.food_flag(3, 6, true);
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<TribalTutorial>(&json).unwrap(), t);
        assert_eq!(serde_json::from_str::<TribalTutorial>("{}").unwrap(), TribalTutorial::default());
    }

    #[test]
    fn every_callout_and_hint_of_the_tutorial_is_installed() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = crate::spui::view::SpuiAssets::default();
        for name in OPENING.iter().chain(&MODE_CALLOUTS) {
            assert!(crate::cell_hud::dialogs::read_callout(&index, &mut assets, name).is_some(), "{name}");
        }
        for id in [
            HINT_ZOOM,
            HINT_ROTATE,
            HINT_TRANSLATE,
            HINT_MINIMAP,
            HINT_SELECT_LIST,
            HINT_SELECT_BOX,
            eating_hint(Diet::Carnivore),
            eating_hint(Diet::Omnivore),
            eating_hint(Diet::Herbivore),
        ] {
            assert!(crate::cell_hud::dialogs::read_hint(&index, &mut assets, id).is_some(), "{id}");
        }
    }

    #[test]
    fn the_retail_ids_of_the_chain_are_the_names_of_the_hints() {
        use crate::world_objects::name_id;
        assert_eq!(name_id(HINT_ZOOM), 0x508F_CCB5);
        assert_eq!(name_id(HINT_TRANSLATE), 0x255C_3C16);
        assert_eq!(name_id(HINT_MINIMAP), 0xB300_9745);
        assert_eq!(name_id(HINT_SELECT_LIST), 0x2E57_3CCF);
        assert_eq!(name_id(HINT_SELECT_BOX), 0xB2A3_FE9E);
        assert_eq!(name_id(HINT_ROTATE_MAC), 0xA14F_59A7);
        assert_eq!(name_id("trg-tribeeatomni"), 0xF4FF_7B8A);
        assert_eq!(name_id("trg-tribeeatherbi"), 0xA15B_B8DD);
        assert_eq!(name_id("trg-tribeeatcarni"), 0xCCB2_843A);
        assert_eq!(name_id("TribeTutorial_Accessorize"), 0x5312_DA01);
        assert_eq!(name_id("TribeTutorial_Mating"), 0x11AD_E9CE);
        assert_eq!(name_id("TribeTutorial_Conquer"), 0xDCDF_2F2F);
        assert_eq!(name_id("TribeTutorial_Domesticate"), 0x84F0_37B0);
    }
}
