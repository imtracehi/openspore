//! Goal cards, hints and callouts of the Tribal HUD, all from the retail data:
//! the goal cards are the installed `TRGMissionCard` layout filled with strings
//! of the `TribeGame` locale table, the hints are the `trg-*` property files
//! (texts of the `HintsTRG` table) shown on the `hint` layout and the callouts
//! are the `Callout_TRG_*` property files shown on the callout dialog layout
//! (the generic machinery of `cell_hud::dialogs`).
//!
//! The tutorial rules (the opening callouts, the forced hint chain and the
//! `TribeTutorial_*` goals) are the retail tribe game's, `tutorial.rs`; this
//! module feeds them the game's events. Every hint goes through the retail
//! hint manager (`hint_manager`, one `HintMachine` with the `trg-*` property
//! files): the chain and the event hints are `ShowHint` calls at the places
//! the retail code makes them, the conditional hints are answered by the
//! Tribe branch of the condition checker 0xE07E50 ([`Conditions`]). What is
//! still ADAPTED (the village goals, the inputs of a few conditions) is marked
//! as such.

use std::collections::HashMap;

use bevy::prelude::*;

use super::{
    tutorial::{self, CameraSample, Stage, Step, TutorialGoal},
    TribalHudActive, TribalHudRoot, TribalHudState, TribalHudUi,
};
use crate::{
    asset_catalog::ResourceIndex,
    cell_contract::CellGoalCard,
    cell_hud::dialogs::{self, CalloutDef, HintDef},
    creature_hud::Diet,
    game::RunProgress,
    hint_manager::{cond as shared, HintBar, HintMachine},
    spui::{
        model::{ImageRef, WinId},
        solve::Rect,
        view::{SpuiAssets, SpuiState},
    },
    stages::GameStage,
    tribal_controls::{CitizenAction, TribalCamera, TribalControls},
    tribal_tools::ToolKind,
};

/// Goal cards shown at once (retail stacks the current goals).
pub const MAX_CARDS: usize = 3;

/// Control ids of `TRGMissionCard` (the same as the Cell's `CLGMissionCard`).
#[allow(dead_code)]
pub mod card {
    pub const ROOT: u32 = 0x055E_BC68;
    pub const KIND: u32 = 0x0523_50C8;
    pub const COUNT: u32 = 0x740E_A83E;
    pub const COUNT_BACKDROP: u32 = 0x055D_3E68;
    pub const HEADLINE: u32 = 0x0523_5140;
    pub const DESCRIPTION: u32 = 0x74CB_0C14;
    pub const PANEL: u32 = 0x54BF_5692;
    pub const GROUP: u32 = 0x74BF_56AE;
}

/// ADAPTED: where the first card's root sits (its header edge matches the
/// Cell card's) and the gap between stacked cards.
const CARD_X: f32 = 5.0;
const CARD_Y: f32 = 15.0;
const CARD_GAP: f32 = 4.0;

/// Locale ids of the goal strings (`TribeGame`, from the installed table). A
/// goal is three consecutive ids: the card kind, then the two texts; the table
/// author put the long explanation and the short instruction in either order,
/// the constants below name them by what they are.
mod text {
    macro_rules! loc {
        ($id:literal) => {
            concat!("loc:TribeGame:", $id)
        };
    }
    // Diet goals: kind, instruction (headline), explanation (body).
    pub const HUNT: [&str; 3] = [loc!("055407bd"), loc!("055407bf"), loc!("055407be")];
    pub const HUNT_OR_GATHER: [&str; 3] = [loc!("055407c2"), loc!("055407c4"), loc!("055407c3")];
    pub const GATHER: [&str; 3] = [loc!("055407c6"), loc!("055407c8"), loc!("055407c7")];
    pub const POPULATE: [&str; 3] = [loc!("056cea63"), loc!("056cea65"), loc!("056cea64")];
    /// `TribeTutorial_Accessorize`: "Outfit" (the planner).
    pub const OUTFIT: [&str; 3] = [loc!("05935b28"), loc!("05935b2a"), loc!("05935b29")];
    /// `TribeTutorial_Conquer`.
    pub const INTERACT: [&str; 3] = [loc!("0567cbee"), loc!("0567cbf0"), loc!("0567cbef")];
    /// `TribeTutorial_Domesticate`.
    pub const DOMESTICATE: [&str; 3] = [loc!("05baa454"), loc!("05baa456"), loc!("05baa455")];
    /// "Ally" (`06E1F026`) with the instruction and explanation of the music goal
    /// and "Conquer" (`06E1F025`) with those of the attack goal; both mention
    /// the village by `~npc0_name_village~`.
    pub const ALLY: [&str; 3] = [loc!("06e1f026"), loc!("5519033b"), loc!("5519033a")];
    pub const CONQUER: [&str; 3] = [loc!("06e1f025"), loc!("055a6249"), loc!("055a6248")];
}

/// One village as the goal selection sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VillageGoal {
    pub relations: f32,
    pub huts: u32,
    pub resolved: bool,
}

/// What the goal selection looks at.
#[derive(Clone, Debug, Default)]
pub struct GoalInputs {
    pub diet: Diet,
    pub food: i32,
    pub livestock: usize,
    /// The `TribeTutorial_*` goals the goal manager holds (added and not
    /// completed), in the order they were added.
    pub tutorial: Vec<TutorialGoal>,
    pub villages: Vec<VillageGoal>,
}

/// A goal of the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// `TribeTutorial_Accessorize`: visit the planner.
    Outfit,
    /// `TribeTutorial_Eating*`: hunt, gather, or both, by diet.
    Diet,
    /// `TribeTutorial_Mating`: add babies to the tribe.
    Populate,
    /// `TribeTutorial_Conquer`: gift, impress or attack another tribe.
    Interact,
    /// A village that has started to like the tribe.
    Ally(usize),
    /// A village whose huts are being destroyed.
    Conquer(usize),
    /// `TribeTutorial_Domesticate`.
    Domesticate,
}

impl From<TutorialGoal> for Goal {
    fn from(goal: TutorialGoal) -> Self {
        match goal {
            TutorialGoal::Outfit => Goal::Outfit,
            TutorialGoal::Diet => Goal::Diet,
            TutorialGoal::Populate => Goal::Populate,
            TutorialGoal::Interact => Goal::Interact,
            TutorialGoal::Domesticate => Goal::Domesticate,
        }
    }
}

/// The goals for the current situation, first goal first. The tutorial goals
/// are the retail goal manager's (`tutorial.rs`: which goal is added when and
/// what completes it); the cards of a village that has started to like the
/// tribe or whose huts fall are ADAPTED (the retail per-tribe goals are not
/// decoded), they follow the tutorial goals.
pub fn goals(g: &GoalInputs) -> Vec<Goal> {
    let mut goals: Vec<Goal> = g.tutorial.iter().map(|t| Goal::from(*t)).collect();
    for (i, v) in g.villages.iter().enumerate() {
        if v.resolved {
            continue;
        }
        if v.huts < crate::tribal::HUTS_PER_VILLAGE {
            goals.push(Goal::Conquer(i));
        } else if v.relations > 0.0 {
            goals.push(Goal::Ally(i));
        }
    }
    goals.truncate(MAX_CARDS);
    goals
}

/// The card of a goal.
pub fn goal_card(goal: Goal, g: &GoalInputs) -> CellGoalCard {
    let strings = |s: [&str; 3]| (s[0].to_string(), s[1].to_string(), s[2].to_string());
    let village = |v: usize, s: [&str; 3]| {
        let name = super::world::village_name(v);
        let with = |s: &str| format!("{s}|npc0_name_village={name}");
        (s[0].to_string(), with(s[1]), with(s[2]))
    };
    let (kind, title, body) = match goal {
        Goal::Outfit => strings(text::OUTFIT),
        Goal::Diet => strings(match g.diet {
            Diet::Carnivore => text::HUNT,
            Diet::Omnivore => text::HUNT_OR_GATHER,
            Diet::Herbivore => text::GATHER,
        }),
        Goal::Populate => strings(text::POPULATE),
        Goal::Interact => strings(text::INTERACT),
        Goal::Ally(v) => village(v, text::ALLY),
        Goal::Conquer(v) => village(v, text::CONQUER),
        Goal::Domesticate => strings(text::DOMESTICATE),
    };
    // Only the goals whose prop has a `goalValue` (the diet goals: 50,
    // `Domesticate`: 1) count. ADAPTED: what the diet count counts (the food
    // in the stock) is not decoded; the goal itself ends at 40 food.
    let progress = match goal {
        Goal::Diet => Some((g.food.clamp(0, tutorial::DIET_GOAL_VALUE), tutorial::DIET_GOAL_VALUE)),
        Goal::Domesticate => Some((
            (g.livestock as i32).min(tutorial::DOMESTICATE_GOAL_VALUE),
            tutorial::DOMESTICATE_GOAL_VALUE,
        )),
        _ => None,
    };
    CellGoalCard { kind, progress, title, body }
}

/// The goal cards for the current situation.
pub fn goal_cards(g: &GoalInputs) -> Vec<CellGoalCard> {
    goals(g).into_iter().map(|goal| goal_card(goal, g)).collect()
}

/// Replace `~name~` placeholders of a retail string.
pub fn substitute(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in values {
        out = out.replace(&format!("~{name}~"), value);
    }
    out
}

/// Resolve `loc:<Table>:<hex id>` references (one per line) to locale text;
/// anything else is literal text. A reference may carry placeholder values,
/// `loc:Table:id|name=value|name2=value2`, which replace `~name~` in it.
pub fn resolve(assets: &mut SpuiAssets, index: &ResourceIndex, text: &str) -> String {
    text.lines()
        .map(|line| {
            let mut parts = line.split('|');
            let head = parts.next().unwrap_or("");
            let Some((table, hex)) = head.strip_prefix("loc:").and_then(|r| r.split_once(':')) else {
                return line.to_string();
            };
            let value = u32::from_str_radix(hex.trim_start_matches("0x"), 16)
                .ok()
                .and_then(|id| assets.localized(index, crate::world_objects::name_id(table), id))
                .unwrap_or_default();
            let pairs: Vec<(&str, &str)> = parts.filter_map(|p| p.split_once('=')).collect();
            substitute(&value, &pairs)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Bottom edge of a card's panel on screen (cards stack and the tribe list
/// hangs below them).
pub fn card_bottom(ui: &SpuiState) -> f32 {
    ui.find(card::PANEL).filter(|w| ui.visible(*w)).map_or(0.0, |w| ui.rect(w).y2)
}

/// Stack the headline and the description inside the panel and size the panel
/// to them. ADAPTED like the Cell card: the retail code sizes these windows to
/// their text (`IText::GetTextBounds`).
fn fit_card(ui: &mut SpuiState) {
    let (Some(panel), Some(group), Some(head), Some(desc)) =
        (ui.find(card::PANEL), ui.find(card::GROUP), ui.find(card::HEADLINE), ui.find(card::DESCRIPTION))
    else {
        return;
    };
    let area = |ui: &SpuiState, w: WinId| ui.layout.window(w).area;
    let (a_panel, a_group, a_head, a_desc) = (area(ui, panel), area(ui, group), area(ui, head), area(ui, desc));
    let height = |ui: &SpuiState, w: WinId| ui.text_extent(w).map_or(0.0, |e| e.y);
    // One 12 px headline line is 15 px high; the description wraps freely.
    let head_h = height(ui, head).max(15.0);
    let desc_h = height(ui, desc);
    // The authored headline sits 13 px below the panel top in this layout (11
    // in the Cell card, where the matched retail offset is 6).
    const PAD_TOP: f32 = 10.0;
    const PAD_BOTTOM: f32 = 8.0;
    let head_top = a_panel[1] + PAD_TOP;
    let desc_top = head_top + head_h;
    ui.set_area(head, [a_head[0], head_top, a_head[2], head_top + head_h]);
    ui.set_area(group, [a_group[0], a_group[1], a_group[2], desc_top + desc_h + 2.0]);
    let rel = desc_top - a_group[1];
    ui.set_area(desc, [a_desc[0], rel, a_desc[2], rel + desc_h]);
    ui.set_area(panel, [a_panel[0], a_panel[1], a_panel[2], desc_top + desc_h + PAD_BOTTOM]);
}

/// Fill one goal card and put its root at `(CARD_X, y)`.
fn bind_card(ui: &mut SpuiState, goal: &CellGoalCard, y: f32, assets: &mut SpuiAssets, index: &ResourceIndex) {
    let Some(root) = ui.find(card::ROOT) else {
        return;
    };
    ui.set_visible(root, true);
    let authored = ui.layout.window(root).area;
    ui.set_area(root, [CARD_X, y, CARD_X + authored[2] - authored[0], y + authored[3] - authored[1]]);
    let kind = resolve(assets, index, &goal.kind);
    let headline = resolve(assets, index, &goal.title);
    let description = resolve(assets, index, &goal.body);
    for (control, value) in [(card::KIND, kind), (card::HEADLINE, headline), (card::DESCRIPTION, description)] {
        if let Some(w) = ui.find(control) {
            ui.set_text(w, value);
        }
    }
    fit_card(ui);
    let count = goal.progress.map(|(a, b)| format!("{a}/{b}")).unwrap_or_default();
    if let Some(w) = ui.find(card::COUNT) {
        ui.set_text(w, count);
    }
    if let Some(w) = ui.find(card::COUNT_BACKDROP) {
        ui.set_visible(w, goal.progress.is_some());
    }
}

/// How long an encounter message (the ADAPTED text line) stays on the hint
/// bar, and how long the HUD keeps a button hint asked for. VERIFIED: the retail
/// event hints (`trg-notenoughfood*`, `trg-social*`, `trg-giftwrongtarget`,
/// `trg-chieftainunavailable`, `trg-superpowerlocked`) are `Triggered` props with
/// 0x0522DE0B = 5 s; the hint manager (0x67C410) fades one in for 1 s (state 4),
/// shows it 5 s at full alpha (state 5) and then fades it out for 2 s. The first
/// second is the fade-in, so the wanted window is 1 + 5 s.
pub const MESSAGE_SECONDS: f32 = 6.0;

/// Condition names the Tribe branch of the checker answers (0xE08664..0xE08BB2,
/// VERIFIED); the generic ones (`trg`, `UserBusy`, `Inactive`, `InPlanner`) are
/// in [`crate::hint_manager::cond`]. Each is a profile flag unless said
/// otherwise (the tutorial's own flags, set by its steps, `tutorial.rs`).
#[allow(dead_code)]
pub mod cond {
    /// Flag 0x064C010F: the camera was zoomed (unnamed in the registry).
    pub const ZOOM_CAMERA: u32 = 0x2757_37B5;
    /// Flag 0x53203528.
    pub const ROTATE_CAMERA: u32 = 0x97B1_6DD9;
    /// Flag 0x83861976.
    pub const MOVE_CAMERA: u32 = 0xD8F7_23F7;
    /// Flag 0x064D44F9.
    pub const MINIMAP_USE: u32 = 0xC2EA_D12B;
    /// Flag 0x064E70B3.
    pub const SELECT_MEMBER_FROM_UI: u32 = 0x35A0_1971;
    /// Flag 0x5AC72D7C.
    pub const SELECT_BOX: u32 = 0x75B7_CD30;
    /// 0xE087B2: a citizen's current action is the diet's food action
    /// ([`super::eating_started`]); unnamed in the registry.
    pub const EATING: u32 = 0xBD71_3587;
    /// 0xE08A9B: some member's health is below `MotiveCritical` (0xC0BCF0:
    /// health fraction x `MotiveMax` < 10).
    pub const NEEDS_HEALING: u32 = 0xFC5F_4372;
    /// 0xE08A54: the tribe owns a tool of type 9 (healing rods, 0xC8F870).
    pub const HAS_HEALING: u32 = 0x5FB5_4387;
    /// 0xE08A7E: `0xCD7070(9)` on the tribe game object, unnamed in the registry.
    pub const HEALING_UNLOCKED: u32 = 0x7F13_DD2C;
}

/// What the Tribe branch of the checker answers (the inputs the guide reads
/// from the game each frame). `Inactive` and `UserBusy` are constant false in
/// this mode (0xE07F97, 0xE08678), so the timed goal hints (`trg-gather`,
/// `-hunt`, `-fish`, `-gift`, `-tools`, `-equip`, `-mate`: all list `Inactive`
/// in list A) can never leave the first state in the shipped game; the manager
/// keeps them waiting by itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct Conditions {
    pub zoomed: bool,
    pub rotated: bool,
    pub panned: bool,
    pub minimap: bool,
    pub selected_from_list: bool,
    pub selected_box: bool,
    pub eating: bool,
    pub needs_healing: bool,
    pub has_healing: bool,
    pub healing_unlocked: bool,
    pub in_planner: bool,
}

impl Conditions {
    pub fn eval(&self, name: u32) -> bool {
        match name {
            shared::TRG => true,
            cond::ZOOM_CAMERA => self.zoomed,
            cond::ROTATE_CAMERA => self.rotated,
            cond::MOVE_CAMERA => self.panned,
            cond::MINIMAP_USE => self.minimap,
            cond::SELECT_MEMBER_FROM_UI => self.selected_from_list,
            cond::SELECT_BOX => self.selected_box,
            cond::EATING => self.eating,
            cond::NEEDS_HEALING => self.needs_healing,
            cond::HAS_HEALING => self.has_healing,
            cond::HEALING_UNLOCKED => self.healing_unlocked,
            shared::IN_PLANNER => self.in_planner,
            // `UserBusy`, `Inactive` and every name this mode does not know.
            _ => false,
        }
    }

    /// The chain's step flags from the tutorial's stage (the flags are set by
    /// the step events, `TribalTutorial::step`).
    pub fn from_tutorial(tut: &tutorial::TribalTutorial) -> Self {
        Self {
            zoomed: tut.step_done(Step::Zoom),
            rotated: tut.step_done(Step::Rotate),
            panned: tut.step_done(Step::Translate),
            minimap: tut.step_done(Step::Minimap),
            selected_from_list: tut.step_done(Step::SelectList),
            selected_box: tut.step_done(Step::SelectBox),
            ..default()
        }
    }
}

/// Whether a tribe member is doing what the diet goal's hint (`trg-tribeeat*`,
/// shown by the last step of the chain) asks for. VERIFIED: that hint is a
/// `Triggered` prop with no display time (0x0522DE0B = 0) whose completion
/// condition is `0xBD713587` (checker 0xE087B2): it ends when any citizen's
/// current action is Fish (any diet), Gather (herbivorous mouth) or Hunt /
/// GatherMeat (carnivorous mouth); until then the hint stays. ADAPTED: the
/// members' task labels stand in for the actions (a gathering member counts for
/// every diet, as OpenSpore lets any diet gather; "fighting" is the hunt).
pub fn eating_started(list: &[super::world::ListMember], diet: Diet) -> bool {
    let carnivorous = matches!(diet, Diet::Carnivore | Diet::Omnivore);
    list.iter().any(|m| match m.task {
        "fishing" | "gathering" => true,
        "fighting" => carnivorous,
        _ => false,
    })
}

/// `NeedsHealing` (0xE08A9B): some member is too hurt (0xC0BCF0: the health
/// meter value over its maximum times `MotiveMax` (100) is below
/// `MotiveCritical` (10)).
pub fn needs_healing(list: &[super::world::ListMember]) -> bool {
    list.iter().any(|m| m.health * MOTIVE_MAX < MOTIVE_CRITICAL)
}
const MOTIVE_MAX: f32 = 100.0;
const MOTIVE_CRITICAL: f32 = 10.0;

/// Retail hint for a campaign message, if its meaning matches one.
pub fn retail_hint_for(message: &str) -> Option<&'static str> {
    // The hints the right click shows (`click::Hint`, tribal_click).
    if let Some(hint) = super::click::Hint::from_message(message) {
        return Some(hint.name());
    }
    let lower = message.to_ascii_lowercase();
    if lower.contains("food") && (lower.contains("gift") || lower.contains("give")) && lower.contains("not enough") {
        return Some("trg-notenoughfoodforgift");
    }
    if lower.contains("not enough food") && (lower.contains("feed") || lower.contains("animal")) {
        return Some("trg-notenoughfoodtofeedanimal");
    }
    if lower.contains("chieftain") && (lower.contains("unavailable") || lower.contains("dead")) {
        return Some("trg-chieftainunavailable");
    }
    None
}

/// The text line of a campaign message that has no retail hint. ADAPTED: the
/// message line is the rewrite's feedback channel ("Built Maracas (25 food)."),
/// retail has no such line; it only shows while the manager has no hint of its
/// own on the bar.
pub fn message_hint(message: &str, age: f32) -> Option<String> {
    (!message.is_empty() && age < MESSAGE_SECONDS && retail_hint_for(message).is_none())
        .then(|| format!("msg:{message}"))
}

/// The first egg: the tribe list shows the babies.
pub const BABY_CALLOUT: &str = "Callout_TRG_BabyPosse";
/// The first time the chieftain is selected (the retail text names the
/// consequence abilities).
pub const CHIEFTAIN_CALLOUT: &str = "Callout_TRG_ChieftainWithPowers";
/// "Congratulations", then the Evolve button, once every tribe is settled.
pub const FINAL_CALLOUTS: [&str; 2] = ["Callout_TRG_LastTribeConquered", "Callout_TRG_TransitionToCiv"];
/// Every Tribal hint of the install (`HintsTRG`): the manager's list.
pub const ALL_HINTS: [&str; 29] = [
    "TRG-CamRotate-MAC",
    "trg-allies",
    "trg-camrotate",
    "trg-camtranslate",
    "trg-camzoom",
    "trg-chieftainunavailable",
    "trg-equip",
    "trg-fish",
    "trg-gather",
    "trg-gift",
    "trg-giftwrongtarget",
    "trg-healing",
    "trg-hunt",
    "trg-mate",
    "trg-minimapuse",
    "trg-notenoughfoodforbaby",
    "trg-notenoughfoodforgift",
    "trg-notenoughfoodtofeedanimal",
    "trg-selectbox",
    "trg-selectmemberfromui",
    "trg-socialneedchieftain",
    "trg-socialnoaudience",
    "trg-socialtutorial",
    "trg-socialwrongtarget",
    "trg-superpowerlocked",
    "trg-tools",
    "trg-tribeeatcarni",
    "trg-tribeeatherbi",
    "trg-tribeeatomni",
];
/// Every callout of the stage, for the tests that check what the game says.
#[cfg(test)]
const ALL_CALLOUTS: [&str; 12] = [
    "Callout_TRG_BabyPosse",
    "Callout_TRG_ChieftainNoPowers",
    "Callout_TRG_ChieftainWithPowers",
    "Callout_TRG_FirstGoals",
    "Callout_TRG_LastTribeConquered",
    "Callout_TRG_Opening",
    "Callout_TRG_Posse",
    "Callout_TRG_ProgressBar",
    "Callout_TRG_TransitionToCiv",
    "Callout_TRG_TutorialMode",
    "Callout_TRG_TutorialModeEnd",
    "Callout_TRG_UICameraControls",
];
/// ADAPTED: seconds after the HUD appears before the introduction starts.
const CALLOUT_DELAY: f32 = 1.5;

/// Flags of the capture demo (`--tribal-hud-*`).
#[derive(Clone, Debug, Default)]
pub struct GuideFlags {
    /// No introduction callouts and no tutorial hints.
    pub quiet: bool,
    pub callout: Option<String>,
    pub hint: Option<String>,
}

impl GuideFlags {
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned());
        Self {
            quiet: args.iter().any(|a| a == "--tribal-hud-quiet"),
            callout: value("--tribal-hud-callout"),
            hint: value("--tribal-hud-hint"),
        }
    }
}

/// Timers and the callout queue of the guide.
#[derive(Resource, Default)]
pub struct GuideState {
    flags: Option<GuideFlags>,
    /// Seconds the HUD has been shown in this visit of the stage.
    pub elapsed: f32,
    message: String,
    message_age: f32,
    started: bool,
    /// The introduction is still being read, and when it ended.
    intro_active: bool,
    intro_end: Option<f32>,
    /// The first goal and how long it has been the first.
    goal: Option<(Goal, f32)>,
    /// Tribes done in the previous frame (the last one shows the final
    /// callouts once).
    last_done: usize,
    queue: Vec<String>,
    /// The callout the last dismissal removed (the tutorial's callbacks).
    last_dismissed: Option<String>,
    /// What the previous frame looked like (the tutorial's events are changes).
    prev_planner: bool,
    prev_members: usize,
    prev_appeared: Option<usize>,
    prev_selected: usize,
    prev_camera: Option<CameraSample>,
    prev_minimap: u32,
    prev_list_picks: u32,
    /// The retail hint manager with the `trg-*` hints (loaded once per visit),
    /// the times it cleared the bar at once, the chain hint it was last asked
    /// for and the button hint of the last frame (a new one is a `ShowHint`).
    machine: Option<HintMachine>,
    hint_cuts: u32,
    shown_chain: Option<&'static str>,
    prev_button: Option<(String, f32)>,
}

impl GuideState {
    /// Forget everything about the visit (the stage was left).
    fn reset(&mut self) {
        let flags = self.flags.take();
        *self = Self { flags, ..default() };
    }

    /// Dismiss the front callout.
    pub fn dismiss(&mut self) {
        if !self.queue.is_empty() {
            self.last_dismissed = Some(self.queue.remove(0));
        }
    }
}

/// Loaded callout and hint definitions and the hint bar's fade.
#[derive(Resource, Default)]
pub struct GuideCache {
    callouts: HashMap<String, Option<CalloutDef>>,
    hints: HashMap<String, Option<HintDef>>,
    /// The retail fade of the shown hint (1 s in, 2 s out, `HintFade`).
    hint_bar: HintBar,
}

/// The goal inputs from the HUD state and the campaign.
pub fn goal_inputs(hud: &TribalHudState, run: &RunProgress) -> GoalInputs {
    let campaign = &run.tribal_campaign;
    GoalInputs {
        diet: hud.diet,
        food: hud.food,
        livestock: campaign.livestock.len(),
        tutorial: campaign.tutorial.goals(),
        // Only the tribes in view (they appear in village order, so the
        // indices stay aligned).
        villages: campaign
            .villages
            .iter()
            .filter(|v| v.appeared)
            .map(|v| VillageGoal { relations: v.relations, huts: v.huts, resolved: v.status.is_resolved() })
            .collect(),
    }
}

/// Fill the goal cards, hint and callouts of [`TribalHudState`].
#[allow(clippy::too_many_arguments)]
pub fn publish_guide(
    time: Res<Time>,
    stage: Res<State<GameStage>>,
    active: Res<TribalHudActive>,
    mut run: ResMut<RunProgress>,
    keys: Res<ButtonInput<KeyCode>>,
    ui_state: Res<TribalHudUi>,
    controls: Option<Res<TribalControls>>,
    camera: Option<Res<TribalCamera>>,
    originals: Option<Res<crate::Originals>>,
    mut guide: ResMut<GuideState>,
    mut hud: ResMut<TribalHudState>,
) {
    if *stage.get() != GameStage::Tribal {
        // The visit is over: the next one starts the introduction again.
        guide.reset();
    }
    if *stage.get() != GameStage::Tribal || !active.0 {
        // No HUD (the editor is open, or the layouts are not available): the
        // guide waits where it is and shows nothing.
        hud.callouts.clear();
        hud.hint = None;
        hud.goal_cards.clear();
        return;
    }
    let flags = guide.flags.get_or_insert_with(GuideFlags::from_args).clone();
    let dt = time.delta_secs();
    // Settings > Game > Tutorials / Game Hints (`front_end::settings`).
    let tutorials = crate::front_end::settings::tutorials_enabled();
    let hints = crate::front_end::settings::hints_enabled();
    let mut tut = run.tribal_campaign.tutorial.clone();
    let campaign = &run.tribal_campaign;
    guide.elapsed += dt;
    let new_message = guide.message != campaign.message;
    if new_message {
        guide.message = campaign.message.clone();
        guide.message_age = 0.0;
    } else {
        guide.message_age += dt;
    }
    // The opening (0xCD8419, `TRG_Begin` stopped): a stage entered fresh gets
    // it once; a stage saved in the middle of a callout shows it again.
    let fresh = hud.tribes_done == 0
        && campaign.births.is_empty()
        && campaign.livestock.is_empty()
        && hud.members <= crate::tribal::START_POPULATION_CAP
        && campaign.shacks.iter().all(|k| *k == ToolKind::Campfire);
    if !guide.started && guide.elapsed >= CALLOUT_DELAY {
        guide.started = true;
        if let Some(name) = &flags.callout {
            guide.queue = vec![name.clone()];
        } else if flags.quiet {
            tut.begin(false, false);
        } else {
            let mut opening = tut.begin(tutorials, fresh);
            if opening.is_empty() && tutorials {
                opening = match tut.stage {
                    Stage::Opening => tutorial::OPENING.to_vec(),
                    Stage::Callouts => tutorial::MODE_CALLOUTS.to_vec(),
                    _ => Vec::new(),
                };
            }
            if !opening.is_empty() {
                guide.queue = opening.iter().map(|n| n.to_string()).collect();
                guide.intro_active = true;
            }
        }
    }
    if guide.intro_active && guide.queue.is_empty() {
        guide.intro_active = false;
        guide.intro_end = Some(guide.elapsed);
    }
    // The dismiss callbacks (0xCD6980) and the planner closing (0xB2699147).
    if let Some(name) = guide.last_dismissed.take() {
        tut.dismissed(&name, hints);
    }
    // (The options menu closes the planner too: that is no visit.)
    if guide.prev_planner && !hud.planner_open && !hud.paused && tutorials {
        let callouts = tut.planner_closed();
        guide.queue.extend(callouts.iter().map(|n| n.to_string()));
    }
    guide.prev_planner = hud.planner_open;
    if !tutorials {
        // The callout manager drops every callout and the chain ends (0x67A566,
        // 0xCDF24A).
        tut.switched_off();
        if flags.callout.is_none() {
            guide.queue.clear();
        }
    }
    // The chain's events: the camera moved by the player, a minimap click, a
    // pick from the tribe list, a selection of several members.
    let sample = camera.as_deref().map(|c| CameraSample {
        distance: c.rig.goal_distance(),
        azimuth: c.rig.goal_azimuth(),
        focus: c.rig.goal_focus().to_array(),
    });
    let minimap = controls.as_deref().map_or(0, |c| c.minimap_gotos);
    let following = camera.as_deref().is_some_and(|c| c.rig.is_following());
    let mut events: Vec<Step> = Vec::new();
    if let (Some(now), Some(before)) = (sample, guide.prev_camera) {
        let e = tutorial::Events::from_frames(before, now, minimap != guide.prev_minimap || following);
        for (hit, step) in [(e.zoom, Step::Zoom), (e.rotate, Step::Rotate), (e.translate, Step::Translate)] {
            if hit {
                events.push(step);
            }
        }
    }
    if minimap != guide.prev_minimap {
        events.push(Step::Minimap);
    }
    if ui_state.list_picks != guide.prev_list_picks {
        events.push(Step::SelectList);
    }
    // ADAPTED: the end of a selection box is "two or more members selected".
    if hud.selected >= 2 && guide.prev_selected < 2 {
        events.push(Step::SelectBox);
    }
    guide.prev_camera = sample;
    guide.prev_minimap = minimap;
    guide.prev_list_picks = ui_state.list_picks;
    guide.prev_selected = hud.selected;
    // The retail hint manager with the `trg-*` hints (loaded once per visit).
    if guide.machine.is_none() {
        if let Some(originals) = originals.as_deref().filter(|o| !o.resource_index.records.is_empty()) {
            guide.machine = Some(HintMachine::load(&originals.resource_index, &ALL_HINTS));
        }
    }
    if tut.forced() && guide.queue.is_empty() && tutorials && hints {
        let before = tut.stage;
        for step in events {
            // The last step adds the diet goal and shows its hint
            // (`ShowHint(trg-tribeeat*)`, 0xCD647D / 0xCD649E / 0xCD64C9); it ends
            // when a member does what it says (condition C, [`eating_started`]).
            if let Some(hint) = tut.step(step, hud.diet) {
                if let Some(machine) = guide.machine.as_mut() {
                    machine.show(hint);
                }
            }
            if tut.stage != before {
                break;
            }
        }
    }
    // The goal events (the sites of `tutorial.rs`).
    if hud.food >= tutorial::FOOD_FLAG_FOOD {
        tut.food_flag(hud.members, hud.member_cap, tutorials && hints);
    }
    // The first baby (`TRG_FirstBaby`) and the first NPC tribe
    // (`TRG_PRE_FirstTribeAppears`) of this visit.
    let baby = guide.prev_members > 0 && hud.members > guide.prev_members;
    if baby {
        tut.baby_born();
    }
    guide.prev_members = hud.members;
    let appeared = campaign.appeared_count();
    let first_tribe = guide.prev_appeared == Some(0) && appeared >= 1;
    guide.prev_appeared = Some(appeared);
    if run.tribe.iter().any(|m| m.outfit_definition.is_some()) {
        tut.outfit_worn();
    }
    if appeared >= 1 {
        tut.tribe_created();
    }
    if hud.tribes_done >= 1 {
        tut.tribe_settled();
    }
    if controls
        .as_deref()
        .is_some_and(|c| c.issued.iter().any(|i| matches!(i.action, CitizenAction::Tame | CitizenAction::FeedWild)))
    {
        tut.animal_command();
    }
    if !campaign.livestock.is_empty() {
        tut.animal_tamed();
    }
    // Later callouts, once the introduction has been offered (all of them need
    // Tutorials, 0xCD8419): the babies in the list when the first baby is born
    // (`TRG_FirstBaby` stopped), the chieftain with his abilities when the first
    // tribe has appeared (`TRG_PRE_FirstTribeAppears` stopped; OpenSpore's
    // chieftain always has both consequence powers), the end of the stage.
    if guide.started && !flags.quiet && tutorials {
        if baby && !tut.has(tutorial::flag::BABY_CALLOUT) {
            tut.flags |= tutorial::flag::BABY_CALLOUT;
            guide.queue.push(BABY_CALLOUT.to_string());
        }
        if first_tribe && !tut.has(tutorial::flag::CHIEFTAIN_CALLOUT) {
            tut.flags |= tutorial::flag::CHIEFTAIN_CALLOUT;
            guide.queue.push(CHIEFTAIN_CALLOUT.to_string());
        }
        if hud.tribes_done >= hud.tribes_goal && hud.tribes_goal > 0 && guide.last_done < hud.tribes_goal {
            guide.queue.extend(FINAL_CALLOUTS.iter().map(|n| n.to_string()));
        }
    }
    if tut != run.tribal_campaign.tutorial {
        run.tribal_campaign.tutorial = tut.clone();
    }
    guide.last_done = hud.tribes_done;
    if keys.just_pressed(crate::cell_flow::DISMISS_KEY) {
        guide.dismiss();
    }
    hud.callouts = guide.queue.clone();
    // Goals.
    let inputs = goal_inputs(&hud, &run);
    let current = goals(&inputs);
    hud.goal_cards = current.iter().map(|g| goal_card(*g, &inputs)).collect();
    match (current.first(), guide.goal) {
        (Some(first), Some((goal, age))) if *first == goal => guide.goal = Some((goal, age + dt)),
        (Some(first), _) => guide.goal = Some((*first, 0.0)),
        (None, _) => guide.goal = None,
    }
    // Hints. The chain hint of the step the player has to take is a `ShowHint`
    // when the step begins (0xCD62B0 after each event); an event hint (the right
    // click errors, the Add Baby button without food, the chieftain down) is one
    // at the place the retail code calls it (`retail_hint_for`, `ui_state.hint`).
    let chain = tut.chain_hint().filter(|_| tutorials && hints);
    let shown_chain = guide.shown_chain;
    if let Some(machine) = guide.machine.as_mut() {
        if chain != shown_chain {
            if let Some(id) = chain {
                machine.show(id);
            }
        }
        if !(tutorials && hints) {
            // 0xCDF24A: the chain is over; its hints go with it.
            for id in tutorial::CHAIN_HINTS {
                machine.complete(id, true);
            }
        }
    }
    guide.shown_chain = chain;
    let button = ui_state.hint.clone();
    let fresh_button = match (&button, &guide.prev_button) {
        (Some((id, left)), Some((prev, before))) => id != prev || left > before,
        (Some(_), None) => true,
        _ => false,
    };
    if fresh_button {
        if let (Some((id, _)), Some(machine)) = (&button, guide.machine.as_mut()) {
            machine.show(id);
        }
    }
    guide.prev_button = button;
    if new_message {
        if let (Some(id), Some(machine)) = (retail_hint_for(&guide.message), guide.machine.as_mut()) {
            machine.show(id);
        }
    }
    // The social game's hint `trg-socialtutorial` (Triggered, 30 s, `always`):
    // `ShowHint` when the game starts while the profile flag 0x157BDF98 is unset
    // (0xDA325E) and again from a thought bubble that follows more than one
    // round without an order (0xDA213F); `HideHint` with done set by the first
    // order (0xDA31A6) and by the end of the game (0xD9FFB5).
    let social = std::mem::take(&mut run.bypass_change_detection().tribal_campaign.social_events.0);
    if let Some(machine) = guide.machine.as_mut() {
        for event in social {
            match event {
                crate::tribal::SocialEvent::Started { seen: false } | crate::tribal::SocialEvent::Reminder => {
                    machine.show("trg-socialtutorial")
                }
                crate::tribal::SocialEvent::Started { seen: true } => {}
                crate::tribal::SocialEvent::FirstOrder | crate::tribal::SocialEvent::Ended => {
                    machine.complete("trg-socialtutorial", true)
                }
            }
        }
    }
    let callout_open = !hud.callouts.is_empty();
    let conditions = Conditions {
        eating: eating_started(&hud.list, hud.diet),
        needs_healing: needs_healing(&hud.list),
        has_healing: run.tribal_campaign.shacks.contains(&ToolKind::HealingRods),
        // ADAPTED: 0xCD7070(9) asks the tribe game object about the tool type 9
        // (true when the tutorial is skipped; the rest of the test is a table the
        // tutorial fills); the rewrite answers: once the forced chain is over.
        healing_unlocked: !tut.forced(),
        in_planner: hud.planner_open,
        ..Conditions::from_tutorial(&tut)
    };
    if !callout_open {
        // The game is held while a callout is up: the hints wait.
        if let Some(machine) = guide.machine.as_mut() {
            machine.update(dt, &|name| conditions.eval(name), hints);
        }
    }
    guide.hint_cuts = guide.machine.as_ref().map_or(0, HintMachine::cuts);
    let manager_hint = guide.machine.as_ref().and_then(|m| m.visible()).map(str::to_string);
    hud.hint = match &flags.hint {
        Some(id) => Some(id.clone()),
        None if callout_open => None,
        None if flags.quiet => manager_hint,
        // Settings > Game > Game Hints: the manager already selects only the
        // hints that ignore it (`always`); the text line stays off.
        None if !hints => manager_hint,
        None => manager_hint.or_else(|| message_hint(&guide.message, guide.message_age)),
    };
}

/// The rectangle of a window of the Main, Card or Posse layouts (callout
/// targets), if it is shown.
fn target_rect(roots: &Query<(&mut SpuiState, &TribalHudRoot)>, control: u32) -> Option<Rect> {
    roots.iter().find_map(|(ui, kind)| {
        if !ui.ready || !matches!(kind, TribalHudRoot::Main | TribalHudRoot::Card(0) | TribalHudRoot::Posse) {
            return None;
        }
        ui.find(control).filter(|w| ui.visible(*w)).map(|w| ui.rect(w))
    })
}

/// Bind the goal cards, the hint bar and the callout dialog.
#[allow(clippy::too_many_arguments)]
pub fn bind_guide(
    time: Res<Time>,
    hud: Res<TribalHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<GuideCache>,
    mut guide: ResMut<GuideState>,
    mut roots: Query<(&mut SpuiState, &TribalHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    // Callout: definition and the rectangle of its target window.
    let callout_name = hud.callouts.first().cloned();
    if let Some(name) = &callout_name {
        if !cache.callouts.contains_key(name) {
            let def = dialogs::read_callout(index, &mut assets, name);
            cache.callouts.insert(name.clone(), def);
        }
    }
    let def = callout_name.as_ref().and_then(|n| cache.callouts.get(n)).cloned().flatten();
    let target = def.as_ref().and_then(|d| d.targets.first()).and_then(|control| target_rect(&roots, *control));
    // Hint: the retail fade (`cHintManager::Update` 0x67C410: 1 s in, then full
    // alpha, 2 s out once the hint ends) towards the published hint.
    let dt = time.delta_secs();
    cache.hint_bar.step(hud.hint.as_deref(), guide.hint_cuts, dt);
    let hint_def = cache.hint_bar.shown().map(str::to_string).and_then(|id| {
        if let Some(text) = id.strip_prefix("msg:") {
            return Some(HintDef { before: text.to_string(), ..default() });
        }
        if !cache.hints.contains_key(&id) {
            let def = dialogs::read_hint(index, &mut assets, &id);
            cache.hints.insert(id.clone(), def);
        }
        cache.hints.get(&id).cloned().flatten()
    });
    // The retail hint texts carry placeholders the code fills in.
    let (baby, gift, feed) = (
        hud.baby_price.to_string(),
        super::rules::gift_price().ceil().to_string(),
        super::rules::feed_price().ceil().to_string(),
    );
    let hint_def = hint_def.map(|mut def| {
        let values = [
            ("tribe_food_required_for_baby", baby.as_str()),
            ("tribe_food_required_for_gift", gift.as_str()),
            ("tribe_food_required_to_feed_wild_animal", feed.as_str()),
        ];
        def.before = substitute(&def.before, &values);
        def.after = substitute(&def.after, &values);
        def
    });
    let icon = hint_def.as_ref().and_then(|d| d.icon).and_then(|key| {
        assets.texture(index, &mut images, key).map(|_| ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None })
    });
    let alpha = cache.hint_bar.alpha();
    // Cards stack downwards by their measured height.
    let mut next_y = CARD_Y;
    let mut cards: Vec<(u8, f32)> = roots
        .iter()
        .filter_map(|(_, kind)| match kind {
            TribalHudRoot::Card(n) => Some((*n, 0.0)),
            _ => None,
        })
        .collect();
    cards.sort_by_key(|(n, _)| *n);
    for (n, y) in &mut cards {
        *y = next_y;
        let height = roots.iter().find(|(_, kind)| **kind == TribalHudRoot::Card(*n)).map_or(0.0, |(ui, _)| {
            if ui.ready {
                card_bottom(ui) - ui.rect(ui.find(card::ROOT).unwrap_or(0)).y1
            } else {
                0.0
            }
        });
        next_y += height.max(60.0) + CARD_GAP;
    }
    let mut dismiss = false;
    for (mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match kind {
            TribalHudRoot::Card(n) => {
                if hud.planner_open {
                    // The planner drawer covers the goal cards.
                    if let Some(w) = ui.find(card::ROOT) {
                        ui.set_visible(w, false);
                    }
                } else if let (Some(goal), Some((_, y))) =
                    (hud.goal_cards.get(*n as usize), cards.iter().find(|(c, _)| c == n))
                {
                    bind_card(&mut ui, goal, *y, &mut assets, index);
                }
            }
            TribalHudRoot::Callout => {
                dialogs::bind_callout(&mut ui, def.as_ref(), target, 0.5);
                for click in ui.take_clicks() {
                    if ui.layout.window(click).control_id == dialogs::callout::OK {
                        dismiss = true;
                    }
                }
            }
            TribalHudRoot::Hint => dialogs::bind_hint(&mut ui, hint_def.as_ref(), alpha, icon.clone()),
            _ => {}
        }
    }
    if dismiss {
        guide.dismiss();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(cards: &[CellGoalCard]) -> Vec<&str> {
        cards.iter().map(|c| c.kind.as_str()).collect()
    }

    fn start() -> GoalInputs {
        GoalInputs {
            diet: Diet::Herbivore,
            food: 15,
            villages: vec![VillageGoal { relations: 0.0, huts: crate::tribal::HUTS_PER_VILLAGE, resolved: false }; 5],
            ..default()
        }
    }

    #[test]
    fn the_eating_hint_ends_when_a_member_gets_food() {
        use super::super::world::ListMember;
        let member = |task: &'static str| ListMember {
            entity: Entity::PLACEHOLDER,
            name: String::new(),
            health: 1.0,
            energy: 1.0,
            selected: false,
            tool: None,
            task,
            chieftain: false,
        };
        let idle = [member("idle"), member("moving")];
        assert!(!eating_started(&idle, Diet::Omnivore));
        // Fishing counts for every diet, gathering as OpenSpore lets every diet
        // gather, the hunt only with a carnivorous mouth.
        for diet in [Diet::Herbivore, Diet::Carnivore, Diet::Omnivore] {
            assert!(eating_started(&[member("fishing")], diet));
            assert!(eating_started(&[member("idle"), member("gathering")], diet));
        }
        assert!(!eating_started(&[member("fighting")], Diet::Herbivore));
        assert!(eating_started(&[member("fighting")], Diet::Carnivore));
        assert!(eating_started(&[member("fighting")], Diet::Omnivore));
    }

    #[test]
    fn a_new_tribe_has_only_the_outfit_goal() {
        // The retail goal manager is empty until the opening adds `Accessorize`.
        let mut g = start();
        assert!(goals(&g).is_empty());
        g.tutorial = vec![TutorialGoal::Outfit];
        assert_eq!(goals(&g), [Goal::Outfit]);
        let cards = goal_cards(&g);
        assert_eq!(kinds(&cards), ["loc:TribeGame:05935b28"]);
        // Outfit has no `goalValue`: no count on the card.
        assert_eq!(cards[0].progress, None);
        assert_eq!(cards[0].title, "loc:TribeGame:05935b2a");
        assert_eq!(cards[0].body, "loc:TribeGame:05935b29");
    }

    #[test]
    fn the_diet_picks_the_text_and_counts_the_food() {
        let mut g = start();
        g.food = 3;
        g.tutorial = vec![TutorialGoal::Diet];
        assert_eq!(goals(&g), [Goal::Diet]);
        let card = |diet| {
            let g = GoalInputs { diet, ..g.clone() };
            goal_card(Goal::Diet, &g)
        };
        assert_eq!(card(Diet::Carnivore).kind, "loc:TribeGame:055407bd");
        assert_eq!(card(Diet::Omnivore).kind, "loc:TribeGame:055407c2");
        assert_eq!(card(Diet::Herbivore).kind, "loc:TribeGame:055407c6");
        // The headline is the short instruction, the body the explanation.
        assert_eq!(card(Diet::Carnivore).title, "loc:TribeGame:055407bf");
        assert_eq!(card(Diet::Carnivore).body, "loc:TribeGame:055407be");
        // The prop's `goalValue` is 50.
        assert_eq!(card(Diet::Herbivore).progress, Some((3, 50)));
        g.food = 90;
        assert_eq!(goal_card(Goal::Diet, &g).progress, Some((50, 50)));
    }

    #[test]
    fn the_goal_cards_follow_the_goal_managers_order() {
        let mut g = start();
        g.tutorial = vec![TutorialGoal::Populate, TutorialGoal::Interact, TutorialGoal::Domesticate];
        assert_eq!(goals(&g), [Goal::Populate, Goal::Interact, Goal::Domesticate]);
        let cards = goal_cards(&g);
        assert_eq!(kinds(&cards), ["loc:TribeGame:056cea63", "loc:TribeGame:0567cbee", "loc:TribeGame:05baa454"]);
        // Populate has no `goalValue` (the old target of five was invented).
        assert_eq!(cards[0].progress, None);
        assert_eq!(cards[2].progress, Some((0, 1)));
        g.livestock = 2;
        assert_eq!(goal_card(Goal::Domesticate, &g).progress, Some((1, 1)));
        // The village cards follow the tutorial goals.
        g.tutorial = vec![TutorialGoal::Interact];
        g.villages[2].relations = 40.0;
        g.villages[3].huts = 1;
        assert_eq!(goals(&g), [Goal::Interact, Goal::Ally(2), Goal::Conquer(3)]);
    }

    #[test]
    fn villages_goals_name_the_village_and_resolved_villages_are_left_alone() {
        let mut g = start();
        g.villages[1].relations = 30.0;
        g.villages[0].resolved = true;
        g.villages[4].huts = 2;
        g.villages[4].resolved = true;
        assert_eq!(goals(&g), [Goal::Ally(1)]);
        let card = goal_card(Goal::Ally(1), &g);
        assert_eq!(card.kind, "loc:TribeGame:06e1f026");
        assert!(card.title.ends_with("|npc0_name_village=Tribe 2"), "{}", card.title);
        assert!(card.body.contains("loc:TribeGame:5519033a"));
        let conquer = goal_card(Goal::Conquer(3), &g);
        assert_eq!(conquer.kind, "loc:TribeGame:06e1f025");
        assert!(conquer.title.ends_with("Tribe 4"));
    }

    #[test]
    fn the_stack_is_bounded() {
        let mut g = start();
        g.tutorial =
            vec![TutorialGoal::Outfit, TutorialGoal::Diet, TutorialGoal::Populate, TutorialGoal::Interact];
        assert_eq!(goals(&g).len(), MAX_CARDS);
        assert_eq!(goals(&g), [Goal::Outfit, Goal::Diet, Goal::Populate]);
        for v in &mut g.villages {
            v.resolved = true;
        }
        g.tutorial.clear();
        assert!(goals(&g).is_empty());
    }

    #[test]
    fn placeholders_are_replaced_in_retail_strings() {
        assert_eq!(substitute("Ally the ~npc0_name_village~", &[("npc0_name_village", "Tribe 2")]), "Ally the Tribe 2");
        assert_eq!(substitute("none", &[("x", "y")]), "none");
        assert_eq!(
            substitute("Add Baby (~tribe_food_required_for_baby~ Food)", &[("tribe_food_required_for_baby", "40")]),
            "Add Baby (40 Food)"
        );
    }

    use crate::hint_manager::{HintRule, KIND_ONESHOT, KIND_TRIGGERED};

    /// The chain pair as installed: Triggered, until the step's flag, priority 100.
    fn chain_rule(id: &str, until: u32) -> HintRule {
        HintRule { until: vec![until], priority: 100, ..HintRule::new(id) }
    }

    fn run(m: &mut HintMachine, seconds: f32, c: Conditions) {
        for _ in 0..(seconds * 10.0).round() as u32 {
            m.update(0.1, &|name| c.eval(name), true);
        }
    }

    #[test]
    fn the_chain_hint_stays_until_the_step_is_taken_and_the_next_replaces_it_at_once() {
        let mut m = HintMachine::new(vec![
            chain_rule("trg-camzoom", cond::ZOOM_CAMERA),
            chain_rule("trg-camrotate", cond::ROTATE_CAMERA),
        ]);
        let mut tut = tutorial::TribalTutorial { stage: Stage::Zoom, ..default() };
        // 0xCD62B0 after `UICameraControls`: ShowHint(camzoom). No timer, no window.
        m.show(tut.chain_hint().unwrap());
        run(&mut m, 200.0, Conditions::from_tutorial(&tut));
        assert_eq!(m.visible(), Some("trg-camzoom"));
        // The zoom event: the chain shows the next hint, which takes the bar at once
        // (0x67C7F0 demotes and clears the one on it) with a new 1 s fade in.
        let cuts = m.cuts();
        assert_eq!(tut.step(Step::Zoom, Diet::Herbivore), None);
        m.show(tut.chain_hint().unwrap());
        run(&mut m, 0.1, Conditions::from_tutorial(&tut));
        assert_eq!((m.visible(), m.cuts()), (Some("trg-camrotate"), cuts + 1));
        assert_eq!(m.state("trg-camzoom"), 0, "a Triggered hint that was replaced is idle");
        // A step that is already done when its hint is asked for is skipped (state 3 -> 0).
        let mut skipped = HintMachine::new(vec![chain_rule("trg-camrotate", cond::ROTATE_CAMERA)]);
        skipped.show("trg-camrotate");
        let done = tutorial::TribalTutorial { stage: Stage::Translate, ..default() };
        run(&mut skipped, 0.2, Conditions::from_tutorial(&done));
        assert_eq!((skipped.visible(), skipped.state("trg-camrotate")), (None, 0));
    }

    #[test]
    fn the_last_step_shows_the_eating_hint_until_a_member_eats() {
        let eating = HintRule { until: vec![cond::EATING], priority: 100, ..HintRule::new("trg-tribeeatherbi") };
        let mut m = HintMachine::new(vec![eating]);
        m.show("trg-tribeeatherbi");
        run(&mut m, 60.0, Conditions::default());
        assert_eq!(m.visible(), Some("trg-tribeeatherbi"), "display time 0: it stays");
        run(&mut m, 1.5, Conditions { eating: true, ..default() });
        assert_eq!(m.state("trg-tribeeatherbi"), 6, "condition C: fade out");
        run(&mut m, 2.5, Conditions { eating: true, ..default() });
        assert_eq!((m.visible(), m.state("trg-tribeeatherbi")), (None, 0));
    }

    #[test]
    fn an_event_hint_interrupts_the_chain_and_runs_its_five_seconds() {
        let error = HintRule { display: 5.0, priority: 100, always: true, ..HintRule::new("trg-notenoughfoodforgift") };
        let mut m = HintMachine::new(vec![chain_rule("trg-camzoom", cond::ZOOM_CAMERA), error]);
        m.show("trg-camzoom");
        run(&mut m, 3.0, Conditions::default());
        m.show("trg-notenoughfoodforgift");
        run(&mut m, 0.1, Conditions::default());
        assert_eq!(m.visible(), Some("trg-notenoughfoodforgift"));
        // 1 s in, 5 s shown, 2 s out: `MESSAGE_SECONDS` covers the shown part.
        run(&mut m, 6.0, Conditions::default());
        assert_eq!(m.visible(), Some("trg-notenoughfoodforgift"));
        run(&mut m, 2.5, Conditions::default());
        assert_eq!(m.visible(), None);
        // Retail never re-shows the chain hint the error hint displaced (state 0):
        // the next step event shows the following one.
        run(&mut m, 30.0, Conditions::default());
        assert_eq!(m.visible(), None);
    }

    fn healing_rule() -> HintRule {
        // `trg-healing` as installed: A [trg, NeedsHealing, 7F13DD2C], B [HasHealing,
        // UserBusy, InPlanner], C [HasHealing], 3 s + 10 s, shown 10 s, Oneshot.
        HintRule {
            all_of: vec![shared::TRG, cond::NEEDS_HEALING, cond::HEALING_UNLOCKED],
            none_of: vec![cond::HAS_HEALING, shared::USER_BUSY, shared::IN_PLANNER],
            until: vec![cond::HAS_HEALING],
            first_delay: 3.0,
            second_delay: 10.0,
            display: 10.0,
            priority: 1,
            kind: KIND_ONESHOT,
            ..HintRule::new("trg-healing")
        }
    }

    #[test]
    fn the_healing_hint_is_the_one_timed_hint_the_shipped_game_shows() {
        let hurt = Conditions { needs_healing: true, healing_unlocked: true, ..default() };
        let mut m = HintMachine::new(vec![healing_rule()]);
        run(&mut m, 12.0, hurt);
        assert_eq!(m.visible(), None, "3 s + 10 s of a member being hurt first");
        run(&mut m, 2.0, hurt);
        assert_eq!(m.visible(), Some("trg-healing"));
        // The planner being open keeps it waiting (list B), a healing tool ends it (C).
        let mut open = HintMachine::new(vec![healing_rule()]);
        run(&mut open, 60.0, Conditions { in_planner: true, ..hurt });
        assert_eq!(open.visible(), None);
        run(&mut m, 1.0, Conditions { has_healing: true, ..hurt });
        assert_eq!(m.state("trg-healing"), 6);
        run(&mut m, 2.5, Conditions { has_healing: true, ..hurt });
        assert_eq!(m.state("trg-healing"), 0, "a Oneshot hint whose condition was met is done");
        // Nobody hurt: nothing.
        let mut well = HintMachine::new(vec![healing_rule()]);
        run(&mut well, 100.0, Conditions { healing_unlocked: true, ..default() });
        assert_eq!(well.visible(), None);
    }

    #[test]
    fn the_checker_answers_the_tribe_conditions() {
        for (id, name) in [
            (cond::ZOOM_CAMERA, "275737B5"),
            (cond::ROTATE_CAMERA, "RotateCamera"),
            (cond::MOVE_CAMERA, "moveCamera"),
            (cond::MINIMAP_USE, "MinimapUse"),
            (cond::SELECT_MEMBER_FROM_UI, "SelectMemberFromUI"),
            (cond::SELECT_BOX, "SelectBox"),
            (cond::NEEDS_HEALING, "NeedsHealing"),
            (cond::HAS_HEALING, "HasHealing"),
        ] {
            if name != "275737B5" {
                assert_eq!(id, crate::world_objects::name_id(name), "{name}");
            }
        }
        let all = Conditions {
            zoomed: true,
            rotated: true,
            panned: true,
            minimap: true,
            selected_from_list: true,
            selected_box: true,
            eating: true,
            needs_healing: true,
            has_healing: true,
            healing_unlocked: true,
            in_planner: true,
        };
        assert!(all.eval(shared::TRG) && all.eval(cond::ROTATE_CAMERA) && all.eval(shared::IN_PLANNER));
        // `Inactive` and `UserBusy` are constant false (0xE07F97, 0xE08678), and
        // so is every name the Tribe branch does not know.
        assert!(!all.eval(shared::INACTIVE) && !all.eval(shared::USER_BUSY) && !all.eval(shared::CRG));
        assert!(!all.eval(0x1234_5678));
        // The chain's flags follow its stage.
        let at = |stage| Conditions::from_tutorial(&tutorial::TribalTutorial { stage, ..default() });
        assert!(!at(Stage::Zoom).zoomed && at(Stage::Rotate).zoomed && !at(Stage::Rotate).rotated);
        assert!(at(Stage::SelectBox).selected_from_list && !at(Stage::SelectBox).selected_box);
        assert!(at(Stage::Done).selected_box);
        assert!(!at(Stage::Opening).zoomed);
    }

    #[test]
    fn a_member_below_a_tenth_of_its_health_needs_healing() {
        use super::super::world::ListMember;
        let member = |health: f32| ListMember {
            entity: Entity::PLACEHOLDER,
            name: String::new(),
            health,
            energy: 1.0,
            selected: false,
            tool: None,
            task: "idle",
            chieftain: false,
        };
        assert!(!needs_healing(&[member(1.0), member(0.1)]), "10 of 100 is not below MotiveCritical");
        assert!(needs_healing(&[member(1.0), member(0.099)]));
        assert!(!needs_healing(&[]));
    }

    #[test]
    fn campaign_messages_without_a_retail_hint_are_the_adapted_text_line() {
        assert_eq!(message_hint("Built Maracas (25 food).", 0.0).as_deref(), Some("msg:Built Maracas (25 food)."));
        assert_eq!(message_hint("Built Maracas (25 food).", MESSAGE_SECONDS + 0.1), None);
        assert_eq!(message_hint("", 0.0), None);
        // A message that has a retail hint raises that hint instead.
        assert_eq!(message_hint("Not enough food for a gift.", 0.0), None);
        assert_eq!(retail_hint_for("Not enough food for a gift."), Some("trg-notenoughfoodforgift"));
    }

    #[test]
    fn known_messages_use_the_retail_hint() {
        assert_eq!(retail_hint_for("Not enough food for a gift."), Some("trg-notenoughfoodforgift"));
        assert_eq!(retail_hint_for("Not enough food to feed the animal."), Some("trg-notenoughfoodtofeedanimal"));
        assert_eq!(retail_hint_for("The chieftain is unavailable."), Some("trg-chieftainunavailable"));
        assert_eq!(retail_hint_for("Built Maracas."), None);
    }

    #[test]
    fn installed_hint_rules_match_the_decode() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let read = |id: &str| HintRule::read(&index, id).unwrap_or_else(|| panic!("{id}"));
        // The timed goal hints list `Inactive`; the checker answers it with false.
        for id in ["trg-gather", "trg-hunt", "trg-fish", "trg-gift", "trg-tools", "trg-equip", "trg-mate"] {
            let rule = read(id);
            assert!(rule.all_of.contains(&shared::INACTIVE), "{id}");
            assert_ne!(rule.kind, KIND_TRIGGERED, "{id}");
        }
        // `trg-healing` does not: it is live in the shipped game.
        let healing = read("trg-healing");
        assert_eq!(healing.all_of, [shared::TRG, cond::NEEDS_HEALING, cond::HEALING_UNLOCKED]);
        assert_eq!(healing.none_of, [cond::HAS_HEALING, shared::USER_BUSY, shared::IN_PLANNER]);
        assert_eq!(healing.until, [cond::HAS_HEALING]);
        assert_eq!((healing.first_delay, healing.second_delay, healing.display), (3.0, 10.0, 10.0));
        assert_eq!((healing.kind, healing.priority, healing.always), (KIND_ONESHOT, 1, false));
        // The chain: Triggered, until its flag, no display time, priority 100.
        for (id, until) in [
            ("trg-camzoom", cond::ZOOM_CAMERA),
            ("trg-camrotate", cond::ROTATE_CAMERA),
            ("trg-camtranslate", cond::MOVE_CAMERA),
            ("trg-minimapuse", cond::MINIMAP_USE),
            ("trg-selectmemberfromui", cond::SELECT_MEMBER_FROM_UI),
            ("trg-selectbox", cond::SELECT_BOX),
            ("trg-tribeeatcarni", cond::EATING),
        ] {
            let rule = read(id);
            assert_eq!((rule.until.clone(), rule.display, rule.priority, rule.kind), (vec![until], 0.0, 100, KIND_TRIGGERED), "{id}");
        }
        // The event hints: 5 s (the allies hint 2 s), priority 100, shown with Game Hints off.
        for hint in super::super::click::Hint::ALL {
            let rule = read(hint.name());
            assert_eq!((rule.priority, rule.always, rule.kind), (100, true, KIND_TRIGGERED), "{hint:?}");
            assert!(rule.display == 5.0 || rule.display == 2.0, "{hint:?}");
        }
        // `Inactive` is the only thing that keeps a timed goal hint from showing: with
        // everything else in its list A true (and nothing of B / C) it waits for ever;
        // were `Inactive` true it would come up after its two delays.
        for id in ["trg-gather", "trg-hunt", "trg-fish", "trg-gift", "trg-tools", "trg-equip", "trg-mate"] {
            let rule = read(id);
            let list = rule.all_of.clone();
            let mut dead = HintMachine::new(vec![rule.clone()]);
            let mut alive = HintMachine::new(vec![rule.clone()]);
            for _ in 0..4000 {
                dead.update(0.1, &|n| n != shared::INACTIVE && list.contains(&n), true);
                alive.update(0.1, &|n| list.contains(&n), true);
            }
            assert_eq!((dead.visible(), dead.state(id)), (None, 1), "{id} waits for ever");
            assert!(alive.state(id) != 1, "{id} would run with `Inactive` true");
        }
    }

    #[test]
    fn dismissing_pops_the_front_callout() {
        let mut guide = GuideState { queue: vec!["a".into(), "b".into()], ..default() };
        guide.dismiss();
        assert_eq!(guide.queue, ["b"]);
        guide.dismiss();
        guide.dismiss();
        assert!(guide.queue.is_empty());
    }

    #[test]
    fn installed_goal_strings_and_card_windows_exist() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        let mut g = start();
        g.villages[1].relations = 10.0;
        g.villages[2].huts = 1;
        for goal in [
            Goal::Outfit,
            Goal::Diet,
            Goal::Populate,
            Goal::Interact,
            Goal::Ally(1),
            Goal::Conquer(2),
            Goal::Domesticate,
        ] {
            for diet in [Diet::Herbivore, Diet::Carnivore, Diet::Omnivore] {
                g.diet = diet;
                let card = goal_card(goal, &g);
                for s in [&card.kind, &card.title, &card.body] {
                    let text = resolve(&mut assets, &index, s);
                    assert!(!text.is_empty(), "{goal:?} {s}");
                    assert!(!text.contains("~npc0_name_village~"), "{goal:?} {text}");
                }
            }
        }
        let ally = goal_card(Goal::Ally(1), &g);
        assert_eq!(resolve(&mut assets, &index, &ally.title), "Ally the Tribe 2");
        assert_eq!(resolve(&mut assets, &index, &ally.kind), "Ally");
        let populate = goal_card(Goal::Populate, &g);
        assert_eq!(resolve(&mut assets, &index, &populate.title), "Click on your hut and add a baby.");
        assert_eq!(resolve(&mut assets, &index, &populate.kind), "Populate");
        let outfit = goal_card(Goal::Outfit, &g);
        assert_eq!(resolve(&mut assets, &index, &outfit.kind), "Outfit");
        assert_eq!(resolve(&mut assets, &index, &outfit.title), "Visit the Tribal Planner to customize your outfit.");
        assert_eq!(
            resolve(&mut assets, &index, &outfit.body),
            "Outfits help your tribe in different ways, choose an outfit that matches your strategy."
        );
        let layout = assets.layout(&index, super::super::CARD_LAYOUT).expect("TRGMissionCard");
        for control in [
            card::ROOT,
            card::KIND,
            card::COUNT,
            card::COUNT_BACKDROP,
            card::HEADLINE,
            card::DESCRIPTION,
            card::PANEL,
            card::GROUP,
        ] {
            assert!(layout.find(control).is_some(), "card window {control:08X}");
        }
    }

    /// `--ignored --nocapture`: every Tribal hint and callout text as the game resolves it.
    #[test]
    #[ignore]
    fn dump_installed_tribal_texts() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = ResourceIndex::load(&install.packages);
        let mut assets = SpuiAssets::default();
        for id in ALL_HINTS {
            println!("hint {id}: {:?}", dialogs::read_hint(&index, &mut assets, id));
        }
        for name in ALL_CALLOUTS {
            println!("callout {name}: {:?}", dialogs::read_callout(&index, &mut assets, name));
        }
    }

    #[test]
    fn installed_tribal_callouts_and_hints_decode() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        let opening = dialogs::read_callout(&index, &mut assets, "Callout_TRG_Opening").expect("opening");
        assert_eq!(opening.title, "You've formed a Tribe!");
        assert!(opening.text.starts_with("You now command a burgeoning tribe of three."));
        assert!(opening.targets.is_empty());
        let bar = dialogs::read_callout(&index, &mut assets, "Callout_TRG_ProgressBar").expect("progress bar");
        assert_eq!(bar.title, "Making Progress");
        // The arrow points at the marker's pointer, the whole bar is lit.
        assert_eq!(bar.targets, vec![super::super::id::BAR_POINTER, super::super::id::BAR]);
        let posse = dialogs::read_callout(&index, &mut assets, "Callout_TRG_Posse").expect("posse");
        assert_eq!(posse.title, "You Should Know...");
        assert_eq!(posse.targets, vec![super::super::world::id::POSSE_ROOT]);
        let goals = dialogs::read_callout(&index, &mut assets, "Callout_TRG_FirstGoals").expect("goals");
        assert_eq!(goals.title, "Goals");
        assert_eq!(goals.targets, vec![card::PANEL]);
        let chief = dialogs::read_callout(&index, &mut assets, CHIEFTAIN_CALLOUT).expect("chieftain");
        assert_eq!(chief.title, "Tribal Chieftain");
        assert_eq!(chief.targets, vec![super::super::world::id::POSSE_ROOT, super::super::id::SUPERPOWER_GROUP]);
        let evolve = dialogs::read_callout(&index, &mut assets, FINAL_CALLOUTS[1]).unwrap();
        assert_eq!(evolve.title, "Advance to Civilization");
        assert_eq!(evolve.targets, vec![super::super::id::EVOLVE]);
        let last = dialogs::read_callout(&index, &mut assets, FINAL_CALLOUTS[0]).unwrap();
        assert_eq!(last.title, "Congratulations!");
        assert!(last.targets.is_empty());
        for name in tutorial::OPENING.iter().chain(&FINAL_CALLOUTS).chain(&[BABY_CALLOUT, CHIEFTAIN_CALLOUT]) {
            assert!(dialogs::read_callout(&index, &mut assets, name).is_some(), "{name}");
        }
        let low = dialogs::read_hint(&index, &mut assets, "trg-notenoughfoodforbaby").expect("baby");
        assert_eq!(low.before, "You need at least ~tribe_food_required_for_baby~ food to make a baby.");
        for id in [
            "trg-gather",
            "trg-hunt",
            "trg-tools",
            "trg-equip",
            "trg-gift",
            "trg-notenoughfoodforgift",
            "trg-notenoughfoodtofeedanimal",
            "trg-chieftainunavailable",
        ] {
            let def = dialogs::read_hint(&index, &mut assets, id).unwrap_or_else(|| panic!("{id}"));
            assert!(!def.before.is_empty(), "{id}");
        }
        let gather = dialogs::read_hint(&index, &mut assets, "trg-gather").unwrap();
        assert_eq!(gather.before, "Send a tribe member to a fruit tree to harvest fruit");
        // The placeholders the HUD fills in are the only ones in the hints.
        for id in ALL_HINTS {
            let def = dialogs::read_hint(&index, &mut assets, id).unwrap_or_else(|| panic!("{id}"));
            let rest = substitute(
                &def.before,
                &[
                    ("tribe_food_required_for_baby", "1"),
                    ("tribe_food_required_for_gift", "1"),
                    ("tribe_food_required_to_feed_wild_animal", "1"),
                ],
            );
            assert!(!rest.contains('~'), "{id}: {rest}");
        }
        let chief = dialogs::read_hint(&index, &mut assets, "trg-chieftainunavailable").unwrap();
        assert_eq!(chief.before, "Your Chieftain is currently unavailable.");
        // The targets of the callouts exist in the layouts they live in.
        let main = assets.layout(&index, super::super::MAIN_LAYOUT).unwrap();
        for control in [super::super::id::BAR_POINTER, super::super::id::BAR, super::super::id::SUPERPOWER_GROUP, super::super::id::EVOLVE] {
            assert!(main.find(control).is_some(), "callout target {control:08X}");
        }
    }
}
