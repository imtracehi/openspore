//! The retail Tribal-stage HUD: spawns the SPUI layouts the original game
//! loads for the Tribal stage and binds them to [`TribalHudState`].
//!
//! * `GlobalUITribe3-6`: the bottom bar (food readout, the three-segment
//!   progress bar of tribes allied or conquered with its five tribe icons,
//!   History / Evolve, Sporepedia / collections / pause) and the social /
//!   attack stance buttons with their three-slot tool trays.
//! * `GlobalUIOptions-2`: the swirl orb (options menu), shared with the Cell
//!   and Creature stages (`cell_hud`).
//! * `TRGMissionCard`: the goal cards at the top left ([`guide`]).
//! * `PosseBar` / `PosseItemTribe` / `PosseItemTribeBaby`: the tribe list, and
//!   `Rollover_TribeHut` (Add Baby / Tribal Planner), `Rollover_TribeCitizen`,
//!   `Rollover_TribeTool` and `RolloverTribe2`: the cards over what the
//!   pointer rests on ([`world`]).
//! * `TribeToolsCategory`: the tool planner, one swatch per tool ([`palette`]).
//! * the retail callout dialogs and hints (`Callout_TRG_*`, `trg-*`) through
//!   the generic machinery of `cell_hud::dialogs` ([`guide`]).
//!
//! The layouts carry all geometry and art. This module decides which windows
//! are shown, fills their text and meters, and turns button clicks into the
//! keys and palette selections the Tribal controls already use (stance = `C`,
//! build or equip the highlighted tool = `B`, add a baby = `R`). The Tribal
//! gameplay modules (`tribal`, `tribal_rts`, `tribal_tools`,
//! `tribal_livestock`) are not edited: [`publish_state`] and
//! [`world::publish_world`] fill the [`TribalHudState`] contract from their
//! resources and components each frame, and every rule the HUD displays (the
//! price of a member or of a tool, the bar's segment ends) is read in one
//! place, [`rules`].
//!
//! Control ids and roles come from the layouts and their tooltips (see
//! `notes\tribal-hud-notes.md`). Where the retail code's behaviour is not
//! documented the binding is marked `ADAPTED`.

use bevy::prelude::*;

pub mod click;
pub mod cursor;
pub mod guide;
pub mod palette;
pub mod portrait;
pub mod social;
pub mod tutorial;
pub mod world;

use crate::{
    asset_catalog::ResourceIndex,
    cell_contract::CellGoalCard,
    cell_hud::ProgressColors,
    creature_hud::Diet,
    dbpf::ResourceKey,
    game::{Health, Player, RunProgress, TribeMate},
    persistence::WorldId,
    spui::{
        model::{ImageRef, WinId},
        solve::{Rect, RootPolicy},
        view::{self, SpuiAssets, SpuiPointer, SpuiState},
    },
    stages::GameStage,
    tribal::{self, TribalCampaign, TribalPowerKind},
    tribal_rts::{Selected, TribalRts, TribeUnit, UnitTask},
    tribal_tools::{self as tools, ToolKind},
};

pub const MAIN_LAYOUT: &str = "GlobalUITribe3-6";
pub const CARD_LAYOUT: &str = "TRGMissionCard";
pub const OPTIONS_LAYOUT: &str = crate::cell_hud::OPTIONS_LAYOUT;

/// Control ids (roles from the layouts and their tooltips). The list is the
/// reference of the layout; some ids are only checked by the tests.
#[allow(dead_code)]
pub mod id {
    // GlobalUITribe3-6: bottom bar.
    /// The food number, on the panel with the "Food Collected" tooltip, and the
    /// three-frame food icon beside it (hidden in the layout).
    pub const FOOD_TEXT: u32 = 0xABCD_1001;
    pub const FOOD_PANEL: u32 = 0x051B_6DB0;
    pub const FOOD_ICON: u32 = 0x04D9_9151;
    pub const BAR: u32 = 0x051B_5478;
    /// The three progress segments: tribe level 1, 2 and 3 (1, 2 and 2 tribes).
    pub const SEGMENTS: [u32; 3] = [0x051B_54C0, 0x051B_54C1, 0x051B_54C2];
    /// Two-frame tribe icon (dark, lit) of each of the five tribes.
    pub const TRIBE_ICONS: [u32; 5] = [0x0559_4A00, 0x0559_4A01, 0x0559_4A02, 0x0559_4A03, 0x0559_4A04];
    /// Target of `Callout_TRG_ProgressBar`.
    pub const BAR_POINTER: u32 = 0x0672_0386;
    /// Evolve (hidden until the bar is full), its glow along the bar and the
    /// History button it replaces.
    pub const EVOLVE: u32 = 0x0645_4B47;
    pub const EVOLVE_GLOW: u32 = 0x0652_A628;
    pub const HISTORY: u32 = 0x0448_27B8;
    pub const PAUSE_TOGGLE: u32 = 0xD305_CA84;
    pub const SPOREPEDIA: u32 = 0x0624_4660;
    pub const COLLECTIONS: u32 = 0x0624_4208;
    // Stance buttons (toggles of group 1) and the two three-slot tool trays.
    pub const STANCE_SOCIAL: u32 = 0x04C6_1B50;
    pub const STANCE_ATTACK: u32 = 0x04C6_1DD9;
    pub const SOCIAL_TRAY: u32 = 0x053E_FDC8;
    pub const ATTACK_TRAY: u32 = 0x053E_FDC0;
    /// The three tool buttons of each tray, left to right.
    pub const SOCIAL_SLOTS: [u32; 3] = [0x0101_2060, 0x0101_2061, 0x0101_2062];
    pub const ATTACK_SLOTS: [u32; 3] = [0x0101_2050, 0x0101_2051, 0x0101_2052];
    /// The key label ("1".."3") on every slot button.
    pub const SLOT_KEY_LABEL: u32 = 0x0628_40D8;
    /// The chieftain's two consequence ability slots on the stance slab (the
    /// toggles, their group and the locked variants the retail code shows
    /// before the Cell and the Creature stage were completed).
    pub const SUPERPOWER_GROUP: u32 = 0x05FF_7280;
    pub const POWERS: [u32; 2] = [0x0101_2040, 0x0101_2041];
    pub const POWERS_LOCKED: [u32; 2] = [0x0636_EE00, 0x0636_EE01];
    // The planet map panel (the picture is `land_minimap`'s).
    pub const MAP_TOGGLE: u32 = 0x0562_A0F7;
    pub const MAP_HOLDER: u32 = 0x0190_722E;
}

/// ADAPTED key of the Evolve button, as the Creature and the Civ stages (the
/// stage ends on it, with the play style card of the final relationships).
pub const ADVANCE_KEY: KeyCode = crate::creature_hud::ADVANCE_KEY;

/// Elements of the progress bar that carry no control id: the two-frame icons
/// at the end of the first and the second segment (tribe level reached).
const LEVEL_ICON_ELEMENTS: [usize; 2] = [169, 190];
/// Em size of the food caption (ADAPTED, as the DNA caption of the Cell and
/// Creature HUDs: the 13 px high caption window shows ~16 px digits).
const FOOD_TEXT_PX: f32 = 16.0;
/// Dimmed icon colour while an ability recharges (ADAPTED: the retail icon
/// plays a radial recharge animation, as the Creature trays).
const COOLDOWN_TINT: u32 = 0x6680_8080;

/// Which layout a spawned root is.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TribalHudRoot {
    Main,
    Orb,
    /// Goal card number `n` (top left, stacked).
    Card(u8),
    /// The tribe list header and list item `n` / growing egg `n`.
    Posse,
    PosseItem(u8),
    PosseBaby(u8),
    /// Cards over the hut, a tribe member, a tool rack and a rival tribe.
    Hut,
    Citizen,
    Tool,
    Tribe,
    /// The tool planner drawer (with a tile per tool).
    Planner,
    /// Hint bar and callout dialog.
    Hint,
    Callout,
    /// The social game's bar (`SocialBar`, [`social`]).
    SocialBar,
}

/// True while the SPUI Tribal HUD is shown; the legacy Tribal HUD and the
/// pause panel step aside.
#[derive(Resource, Default)]
pub struct TribalHudActive(pub bool);

/// Interface state that belongs to the HUD, not to the game.
#[derive(Resource, Default, Debug)]
pub struct TribalHudUi {
    /// The tool planner (`TribeToolsCategory`) is open.
    pub planner_open: bool,
    /// Members picked on the tribe list so far (the tutorial's select step,
    /// retail 0xCCB74A).
    pub list_picks: u32,
    /// A retail hint the HUD itself wants to show for a few seconds (the Add
    /// Baby button without enough food), with the seconds it has left.
    pub hint: Option<(String, f32)>,
    /// Screen rectangle of the hut card of the last frame (the card stays
    /// while the pointer is over it, so that its buttons can be reached).
    pub hut_card: Option<Rect>,
}

/// Social stance (friendly commands: tame, perform) or attack stance
/// (hunt, besiege). Retail: the selected members use their social or their
/// attack tool; here the stance is the campaign's `friendly_stance`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stance {
    Social,
    #[default]
    Attack,
}

/// State of a tool in the tray and the planner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolState {
    /// More tribes must be settled before the shack can be built.
    Locked,
    Available,
    /// The shack is built: members can take the tool.
    Built,
}

/// One tool of a tray or of the planner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolEntry {
    pub kind: ToolKind,
    pub state: ToolState,
    /// Food price of the shack.
    pub price: f32,
    /// The tool highlighted in the palette (`T`, a click): `B` builds or
    /// hands it out.
    pub highlighted: bool,
    /// Tribes that must be settled to unlock the tool.
    pub tier: usize,
}

/// Everything the Tribal HUD displays, filled by [`publish_state`] (and by
/// [`world`] / [`guide`] for their parts) so the HUD never reads simulation
/// internals itself.
#[derive(Resource, Clone, Debug, Default)]
pub struct TribalHudState {
    /// The tribe's food (the number beside the progress bar).
    pub food: i32,
    /// Tribes allied or conquered and the number that ends the stage.
    pub tribes_done: usize,
    pub tribes_goal: usize,
    /// Fill of the three progress segments, `0.0..=1.0`.
    pub level_fill: [f32; 3],
    /// Progress segments completed (the lit tribe level icons).
    pub level: u8,
    /// The progress bar is full: Evolve replaces History.
    pub evolve_visible: bool,
    /// Living members (without the chieftain), eggs growing, and the hut's
    /// capacity for both.
    pub members: usize,
    pub eggs: usize,
    pub member_cap: usize,
    /// Food price of a new member.
    pub baby_price: i32,
    pub stance: Stance,
    /// The chieftain's consequence abilities: the one of the Cell stage, then
    /// the one of the Creature stage.
    pub powers: Vec<PowerSlot>,
    /// The chieftain is alive (his abilities need him).
    pub chieftain_up: bool,
    /// The three social and the three attack tools of the trays.
    pub social: Vec<ToolEntry>,
    pub attack: Vec<ToolEntry>,
    /// Every tool of the planner, in its three sections.
    pub palette: Vec<ToolEntry>,
    pub planner_open: bool,
    /// Members (chieftain included) the player has selected.
    pub selected: usize,
    /// Diet of the player's species (the gather / hunt goal text).
    pub diet: Diet,
    /// The options menu is open (the game is paused).
    pub paused: bool,
    /// Goal cards in display order (retail strings, see [`guide::goal_cards`]).
    pub goal_cards: Vec<CellGoalCard>,
    /// Callouts to show, front first (`Callout_TRG_Opening`, ...).
    pub callouts: Vec<String>,
    /// Active hint: a retail hint id (`trg-gather`) or `msg:<text>`.
    pub hint: Option<String>,
    /// The tribe list: the chieftain first, then the members, and the eggs.
    pub list: Vec<world::ListMember>,
    pub babies: Vec<f32>,
    /// The card over what the pointer rests on, and the hut card.
    pub card: Option<world::Card>,
    pub hut: Option<world::HutCard>,
    /// The social game's bar while a song runs (or has just ended).
    pub social_bar: Option<social::SocialBarView>,
}

/// One of the chieftain's consequence abilities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerSlot {
    pub kind: TribalPowerKind,
    /// Seconds until it can be used again and the full recharge time.
    pub cooldown: f32,
    pub total: f32,
}

/// The keys of the two abilities (Antigravity's 0.81 bindings: the Cell
/// stage's, then the Creature stage's) and the labels the tooltips show.
pub const POWER_KEYS: [KeyCode; 2] = [KeyCode::KeyY, KeyCode::KeyU];
pub const POWER_KEY_LABELS: [&str; 2] = ["Y", "U"];
/// Locale table of the superpower names and descriptions (`9A5ACF10`).
pub const POWER_TABLE: u32 = 0x9A5A_CF10;

/// The locale ids `(name, description)` of an ability in [`POWER_TABLE`]
/// (the `consequenceSuperPowerName` / `...Description` of the `trg_*` property
/// files, see `notes\space-re\consequence_superpowers.txt`).
pub fn power_text_ids(kind: TribalPowerKind) -> (u32, u32) {
    match kind {
        TribalPowerKind::Traps => (0x0002_0025, 0x0002_0026),
        TribalPowerKind::RefreshingStorm => (0x0001_0027, 0x0001_0028),
        TribalPowerKind::FlyingFish => (0x0001_0029, 0x0001_002A),
        TribalPowerKind::FireBombs => (0x0001_002B, 0x0001_002C),
        TribalPowerKind::Fireworks => (0x0001_002D, 0x0001_002E),
        TribalPowerKind::Beastmaster => (0x0001_002F, 0x0001_0030),
    }
}

/// The installed picture of an ability (`consequenceSuperPowerIcon` of its
/// `trg_*` property file, group `consequence~`).
pub fn power_icon(kind: TribalPowerKind) -> ResourceKey {
    let instance_id = match kind {
        TribalPowerKind::Traps => 0xBD79_0A03,
        TribalPowerKind::RefreshingStorm => 0xC638_8A15,
        TribalPowerKind::FlyingFish => 0x5F49_756A,
        TribalPowerKind::FireBombs => 0xC2EB_A868,
        TribalPowerKind::Fireworks => 0xFE06_0A8D,
        TribalPowerKind::Beastmaster => 0x0FB0_60F8,
    };
    ResourceKey { type_id: crate::ui_art::UI_PNG_TYPE, group_id: POWER_ICON_GROUP, instance_id }
}

/// Group of the `consequence~` icons.
pub const POWER_ICON_GROUP: u32 = 0xD509_FFBF;

/// Tooltip of an ability button: name with its key, the description, and the
/// seconds left of the recharge.
pub fn power_tooltip(name: &str, description: &str, key: &str, cooldown: f32) -> String {
    let mut text = format!("{name} [{key}]\n{description}");
    if cooldown > 0.0 {
        text.push_str(&format!("\nReady in {:.0} s", cooldown.ceil()));
    }
    text
}

/// The rules the HUD displays, read from the game in one place so that the
/// parallel rules work (`tribal_rules`: member price, tool prices, tribe
/// levels) is a change of these bodies only. MERGE with `tribal-rules`:
/// `member_price` -> `tribal_rules::MEMBER_PRICE`, `level_totals` ->
/// `GOAL_PROGRESS_UPGRADES` + `NUM_TRIBES_TO_CONQUER`, `tool_price` ->
/// `tribal_rules::tool_price`.
pub mod rules {
    use super::*;

    /// Food a new member costs (`R` near the hut).
    pub fn member_price() -> f32 {
        tribal::RECRUIT_FOOD
    }

    /// Food a tool's shack costs.
    pub fn tool_price(kind: ToolKind) -> f32 {
        tools::info(kind).price
    }

    /// Food a gift to a rival tribe costs (`G`).
    pub fn gift_price() -> f32 {
        tribal::GIFT_FOOD
    }

    /// Food feeding a wild animal to tame it costs.
    pub fn feed_price() -> f32 {
        crate::tribal_rts::livestock::TAME_FOOD
    }

    /// Tribes that end the stage.
    pub fn tribes_goal() -> usize {
        tribal::RIVAL_VILLAGES
    }

    /// Tribes completed at the end of each progress segment. Retail: the tribe
    /// levels at 1 and 3 (`goalProgressUpgrades` [1, 3], RE T1) and the 5 of
    /// `numTribesToConquer`; the segments of the layout are 190:382:370 px
    /// wide, which is 1:2:2.
    pub fn level_totals() -> [usize; 3] {
        [1, 3, tribes_goal()]
    }
}

/// Fill of the three progress segments for `done` tribes. Segment `i` fills
/// while the count runs from the previous segment's total to its own.
pub fn level_fills(done: usize, totals: &[usize; 3]) -> [f32; 3] {
    let mut fills = [0.0; 3];
    let mut low = 0usize;
    for (fill, &high) in fills.iter_mut().zip(totals) {
        let span = high.saturating_sub(low).max(1) as f32;
        *fill = (done.saturating_sub(low) as f32 / span).clamp(0.0, 1.0);
        low = high;
    }
    fills
}

/// Number of completely filled segments (the tribe level icons that light).
pub fn lit_levels(done: usize, totals: &[usize; 3]) -> u8 {
    totals.iter().filter(|&&total| done >= total).count() as u8
}

/// Frame of a tribe icon: lit once that many tribes are done.
pub fn tribe_icon_frame(index: usize, done: usize) -> u32 {
    u32::from(index < done)
}

/// The installed PNG of a tool's palette icon.
pub fn tool_icon(kind: ToolKind) -> Option<ResourceKey> {
    let (group, name) = tools::info(kind).icon?;
    Some(ResourceKey {
        type_id: crate::ui_art::UI_PNG_TYPE,
        group_id: group,
        instance_id: crate::world_objects::name_id(name),
    })
}

/// The three tools of each tray, in the retail slot order (`social1..3` horn,
/// maraca, didgeridoo; `attack1..3` torch, axe, spear).
pub const SOCIAL_TOOLS: [ToolKind; 3] = [ToolKind::WoodenHorns, ToolKind::Maracas, ToolKind::Aerophones];
pub const ATTACK_TOOLS: [ToolKind; 3] = [ToolKind::SpikedClubs, ToolKind::StoneAxes, ToolKind::ThrowingSpears];

/// The planner's sections, in the order of the layout's three category
/// boxes: "Food and Healing", "Instruments", "Weapons". The camp fire is not
/// listed: the tribe has it from the start of the stage (`TRG_Begin` 0xCD4400
/// binds `TribeFire` to `GetToolByType(10)`), it is not a purchase.
pub const PLANNER_SECTIONS: [&[ToolKind]; 3] = [
    &[ToolKind::FruitSticks, ToolKind::FishingSpears, ToolKind::HealingRods],
    &SOCIAL_TOOLS,
    &ATTACK_TOOLS,
];

/// A tool's state from the campaign: built shack, buildable, or locked.
pub fn tool_entry(kind: ToolKind, campaign: &TribalCampaign, highlighted: ToolKind) -> ToolEntry {
    let resolved = campaign.resolved_count();
    let state = if campaign.has_shack(kind) {
        ToolState::Built
    } else if tools::is_unlocked(kind, resolved) {
        ToolState::Available
    } else {
        ToolState::Locked
    };
    ToolEntry { kind, state, price: rules::tool_price(kind), highlighted: highlighted == kind, tier: tools::info(kind).tier }
}

/// Tooltip of a tool button: name, price or state, and what it does.
pub fn tool_tooltip(entry: &ToolEntry) -> String {
    let info = tools::info(entry.kind);
    let state = match entry.state {
        ToolState::Built => "Built: click to hand it to the selected members".to_string(),
        ToolState::Available => format!("{:.0} Food: click to build the shack", entry.price),
        ToolState::Locked => format!(
            "Settle {} tribe{} to unlock",
            entry.tier,
            if entry.tier == 1 { "" } else { "s" }
        ),
    };
    format!("{}\n{}\n{}", info.name, state, info.effect)
}

/// The stance the campaign is in (`C` toggles it).
pub fn stance_of(campaign: &TribalCampaign) -> Stance {
    if campaign.friendly_stance {
        Stance::Social
    } else {
        Stance::Attack
    }
}

/// The virtual keys of the Tribal controls of `tribal-ctrl`
/// (`tribal_controls::KEY_SELECT_TRIBE` and friends): the tribe list's "Select
/// All" button, and the two stance buttons, which select and never toggle.
/// MERGE: with `tribal_controls.rs` in the tree, set [`CONTROLS_KEYS`] and
/// replace these constants by the module's.
pub const KEY_SELECT_TRIBE: KeyCode = KeyCode::F15;
pub const KEY_STANCE_SOCIAL: KeyCode = KeyCode::F16;
pub const KEY_STANCE_ATTACK: KeyCode = KeyCode::F17;
/// `tribal-ctrl` has the keys above and has freed `Q` (it strafes) and `C` is
/// only a toggle. Off in this copy, which does not have `tribal_controls.rs`:
/// the HUD then presses the keys the legacy `tribal_input` reads (`Q` selects
/// the tribe, `C` toggles the stance, so a stance click presses it only when
/// it changes the stance).
pub const CONTROLS_KEYS: bool = true;

/// The key a click on the "Select All" button presses.
pub fn select_tribe_key() -> KeyCode {
    if CONTROLS_KEYS {
        KEY_SELECT_TRIBE
    } else {
        KeyCode::KeyQ
    }
}

/// The key a click on a stance button presses, if any.
pub fn stance_key(current: Stance, clicked: Stance) -> Option<KeyCode> {
    if CONTROLS_KEYS {
        Some(if clicked == Stance::Social { KEY_STANCE_SOCIAL } else { KEY_STANCE_ATTACK })
    } else {
        stance_click_needs_key(current, clicked).then_some(KeyCode::KeyC)
    }
}

/// Held state of the four camera buttons of the map panel (what
/// `tribal_controls::HudCamera` takes). Nothing is held while the panel is
/// hidden ([`CAMERA_BUTTONS`] off). The field names follow the order of
/// [`CAMERA_BUTTON_IDS`] (`rotate_left` = 0x04CAB56F, `rotate_right` =
/// 0x04CAB56E); which way each turns is `tribal_controls::HudCamera::as_keys`
/// (VERIFIED by tribal-audit-a: 0xCC9B30 / 0xB14800).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TribalHudCamera {
    pub zoom_in: bool,
    pub zoom_out: bool,
    pub rotate_left: bool,
    pub rotate_right: bool,
}

/// The held camera buttons of the main layout.
pub fn held_camera_buttons(ui: &SpuiState) -> TribalHudCamera {
    let held = |control: u32| {
        ui.find(control).is_some_and(|w| ui.visible(w) && ui.states[w] & crate::spui::ids::state::CLICKED != 0)
    };
    TribalHudCamera {
        zoom_in: held(CAMERA_BUTTON_IDS[0]),
        zoom_out: held(CAMERA_BUTTON_IDS[1]),
        rotate_left: held(CAMERA_BUTTON_IDS[2]),
        rotate_right: held(CAMERA_BUTTON_IDS[3]),
    }
}

/// Zoom in, zoom out, rotate left, rotate right (`04CAB570`, `04CAB571`,
/// `04CAB56F`, `04CAB56E`).
pub const CAMERA_BUTTON_IDS: [u32; 4] = [0x04CA_B570, 0x04CA_B571, 0x04CA_B56F, 0x04CA_B56E];

/// Publish the held camera buttons every frame.
fn publish_camera_buttons(
    roots: Query<(&SpuiState, &TribalHudRoot)>,
    mut camera: ResMut<TribalHudCamera>,
    controls: Option<ResMut<crate::tribal_controls::TribalControls>>,
) {
    let held = roots
        .iter()
        .find(|(ui, kind)| **kind == TribalHudRoot::Main && ui.ready)
        .map(|(ui, _)| held_camera_buttons(ui))
        .unwrap_or_default();
    if *camera != held {
        *camera = held;
    }
    // The Tribal controls read the held buttons from `TribalControls::hud`.
    if let Some(mut controls) = controls {
        let hud = crate::tribal_controls::HudCamera {
            zoom_in: held.zoom_in,
            zoom_out: held.zoom_out,
            rotate_left: held.rotate_left,
            rotate_right: held.rotate_right,
        };
        if controls.hud != hud {
            controls.hud = hud;
        }
    }
}

/// Whether a click on a stance button has to press the stance key: the
/// campaign only has a toggle, so the key is pressed when the clicked stance
/// is not the current one.
pub fn stance_click_needs_key(current: Stance, clicked: Stance) -> bool {
    current != clicked
}

pub struct TribalHudPlugin;

impl Plugin for TribalHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(cursor::TribalCursorPlugin)
            .init_resource::<TribalHudState>()
            .init_resource::<TribalHudActive>()
            .init_resource::<TribalHudUi>()
            .init_resource::<guide::GuideState>()
            .init_resource::<guide::GuideCache>()
            .init_resource::<portrait::PortraitState>()
            .init_resource::<palette::PlannerLayout>()
            .init_resource::<TribalHudCamera>()
            .add_systems(
                Update,
                (
                    publish_state,
                    social::publish_social,
                    world::publish_world,
                    guide::publish_guide,
                    demo_state,
                    manage,
                    world::manage_dynamic,
                    palette::manage_swatches,
                    bind,
                    world::bind_world,
                    palette::bind_palette,
                    guide::bind_guide,
                    legacy_step_aside,
                    publish_camera_buttons,
                )
                    .chain()
                    .before(view::sync)
                    .after(view::input),
            )
            // The village's thought bubbles and answers while a song runs.
            .add_systems(Update, social::audience.run_if(in_state(GameStage::Tribal)))
            // The member portraits follow the chieftain; their pictures sit over
            // the portrait windows once the HUD has been laid out.
            .add_systems(Update, (portrait::update, portrait::place).chain().after(view::sync).after(manage))
            // Clicks recorded by the SPUI input of the last frame become key
            // presses before the controls (and then the simulation) read the
            // keyboard: ahead of the first system of the controls chain. The
            // presses over the HUD are kept from the world (`tribal_input`).
            .add_systems(
                Update,
                (swallow_world_clicks, actions, world::list_clicks)
                    .chain()
                    .in_set(crate::GameSystems::Controls)
                    .before(crate::player_hud::actions)
                    .run_if(in_state(GameStage::Tribal)),
            );
    }
}

/// Flags of the capture demo.
struct DemoFlags {
    on: bool,
    paused: bool,
    planner: bool,
    hut: bool,
    /// Every tribe done: the bar is full and Evolve shows.
    evolve: bool,
    /// `--tribal-hud-card citizen|tool|tribe`: which card hangs over the world.
    card: Option<String>,
}

impl DemoFlags {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        Self {
            on: has("--tribal-hud-demo"),
            paused: has("--tribal-hud-paused"),
            planner: has("--tribal-hud-planner"),
            hut: has("--tribal-hud-hut"),
            evolve: has("--tribal-hud-evolve"),
            card: args.iter().position(|a| a == "--tribal-hud-card").and_then(|i| args.get(i + 1).cloned()),
        }
    }
}

/// `--tribal-hud-demo` (captures): replace what the game published with a
/// mid-game HUD state so that every part of the layouts can be looked at:
/// two tribes done, tools in the trays, members and eggs in the list and the
/// cards over a member, a tool rack, a rival tribe and the hut.
/// `--tribal-hud-paused` also opens the options menu, `--tribal-hud-planner`
/// opens the tool planner and `--tribal-hud-hut` shows the hut card.
fn demo_state(
    stage: Res<State<GameStage>>,
    mut demo: Local<Option<DemoFlags>>,
    mut ui_state: ResMut<TribalHudUi>,
    mut hud: ResMut<TribalHudState>,
) {
    let flags = demo.get_or_insert_with(DemoFlags::from_args);
    if !flags.on || *stage.get() != GameStage::Tribal {
        return;
    }
    let totals = rules::level_totals();
    let done = if flags.evolve { rules::tribes_goal() } else { 2 };
    hud.food = 135;
    hud.tribes_done = done;
    hud.tribes_goal = rules::tribes_goal();
    hud.level_fill = level_fills(done, &totals);
    hud.level = lit_levels(done, &totals);
    hud.evolve_visible = flags.evolve;
    hud.chieftain_up = true;
    hud.members = 5;
    hud.eggs = 1;
    hud.member_cap = 8;
    hud.baby_price = rules::member_price() as i32;
    hud.stance = Stance::Social;
    let entry = |kind: ToolKind, state: ToolState, highlighted: bool| ToolEntry {
        kind,
        state,
        price: rules::tool_price(kind),
        highlighted,
        tier: tools::info(kind).tier,
    };
    hud.social = vec![
        entry(ToolKind::WoodenHorns, ToolState::Built, false),
        entry(ToolKind::Maracas, ToolState::Built, true),
        entry(ToolKind::Aerophones, ToolState::Locked, false),
    ];
    hud.attack = vec![
        entry(ToolKind::SpikedClubs, ToolState::Built, false),
        entry(ToolKind::StoneAxes, ToolState::Available, false),
        entry(ToolKind::ThrowingSpears, ToolState::Locked, false),
    ];
    hud.palette = PLANNER_SECTIONS
        .iter()
        .flat_map(|section| section.iter())
        .map(|&kind| {
            let state = match kind {
                ToolKind::WoodenHorns | ToolKind::Maracas | ToolKind::SpikedClubs | ToolKind::FruitSticks => ToolState::Built,
                k if tools::info(k).tier > 2 => ToolState::Locked,
                ToolKind::Aerophones | ToolKind::ThrowingSpears | ToolKind::HealingRods => ToolState::Locked,
                _ => ToolState::Available,
            };
            entry(kind, state, kind == ToolKind::Maracas)
        })
        .collect();
    ui_state.planner_open |= flags.planner;
    hud.planner_open = ui_state.planner_open;
    hud.selected = 2;
    hud.diet = Diet::Omnivore;
    hud.paused = flags.paused;
    hud.list = vec![
        world::ListMember { name: "Chieftain".into(), health: 1.0, energy: 1.0, selected: false, tool: None, task: "idle", chieftain: true, entity: Entity::PLACEHOLDER },
        world::ListMember { name: "Hornist".into(), health: 0.9, energy: 0.7, selected: true, tool: Some(ToolKind::WoodenHorns), task: "gathering", chieftain: false, entity: Entity::PLACEHOLDER },
        world::ListMember { name: "Maracan".into(), health: 0.5, energy: 0.3, selected: true, tool: Some(ToolKind::Maracas), task: "fishing", chieftain: false, entity: Entity::PLACEHOLDER },
        world::ListMember { name: "Farmer".into(), health: 1.0, energy: 0.95, selected: false, tool: Some(ToolKind::FruitSticks), task: "gathering", chieftain: false, entity: Entity::PLACEHOLDER },
        world::ListMember { name: "Tribe member".into(), health: 0.25, energy: 0.1, selected: false, tool: None, task: "eating / resting", chieftain: false, entity: Entity::PLACEHOLDER },
        world::ListMember { name: "Axeling".into(), health: 0.8, energy: 0.8, selected: false, tool: Some(ToolKind::StoneAxes), task: "following", chieftain: false, entity: Entity::PLACEHOLDER },
    ];
    hud.babies = vec![0.4];
    hud.card = Some(match flags.card.as_deref() {
        Some("tool") => world::Card {
            kind: world::CardKind::Tool,
            name: tools::info(ToolKind::Maracas).name.into(),
            health: None,
            face: 4,
            count: world::role_glyph(Some(ToolKind::Maracas)).map(|f| f as i32),
            screen: Vec2::new(760.0, 360.0),
        },
        Some("tribe") => world::Card {
            kind: world::CardKind::Tribe,
            name: world::village_name(1),
            health: None,
            face: 2,
            count: Some(6),
            screen: Vec2::new(760.0, 360.0),
        },
        _ => world::Card {
            kind: world::CardKind::Citizen,
            name: "Maracan".into(),
            health: Some((0.5, 50)),
            face: 4,
            count: world::role_glyph(Some(ToolKind::Maracas)).map(|f| f as i32),
            screen: Vec2::new(760.0, 360.0),
        },
    });
    if flags.hut {
        hud.hut = Some(world::HutCard { members: 5, eggs: 1, cap: 8, food: 135, price: hud.baby_price, mood: 4, screen: Vec2::new(640.0, 300.0) });
    }
}

/// Fill [`TribalHudState`] from the Tribal resources and components.
#[allow(clippy::too_many_arguments)]
fn publish_state(
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    rts: Option<Res<TribalRts>>,
    pause: Res<crate::pause_menu::PauseMenu>,
    keys: Res<ButtonInput<KeyCode>>,
    originals: Res<crate::Originals>,
    time: Res<Time>,
    mut ui_state: ResMut<TribalHudUi>,
    mut hud: ResMut<TribalHudState>,
) {
    if *stage.get() != GameStage::Tribal {
        ui_state.planner_open = false;
        ui_state.hut_card = None;
        ui_state.hint = None;
        return;
    }
    let campaign = &run.tribal_campaign;
    let totals = rules::level_totals();
    let done = campaign.resolved_count();
    hud.food = run.dna_this_stage.max(0.0).floor() as i32;
    hud.tribes_done = done;
    hud.tribes_goal = rules::tribes_goal();
    hud.level_fill = level_fills(done, &totals);
    hud.level = lit_levels(done, &totals);
    // The stage ends when the bar is full: Evolve replaces History.
    hud.evolve_visible = campaign.can_advance();
    hud.members = run.tribe.len();
    hud.eggs = campaign.pending_birth_count();
    hud.member_cap = campaign.population_cap();
    hud.baby_price = rules::member_price().ceil() as i32;
    hud.stance = stance_of(campaign);
    hud.powers = vec![
        PowerSlot { kind: campaign.cell_power(), cooldown: campaign.power_cell_cooldown, total: campaign.cell_power().cooldown() },
        PowerSlot {
            kind: campaign.creature_power(),
            cooldown: campaign.power_creature_cooldown,
            total: campaign.creature_power().cooldown(),
        },
    ];
    let highlighted = rts.as_deref().map_or(ToolKind::SpikedClubs, |r| r.palette);
    let entries = |kinds: &[ToolKind]| kinds.iter().map(|&k| tool_entry(k, campaign, highlighted)).collect::<Vec<_>>();
    hud.social = entries(&SOCIAL_TOOLS);
    hud.attack = entries(&ATTACK_TOOLS);
    hud.palette = PLANNER_SECTIONS.iter().flat_map(|section| entries(section)).collect();
    // `T` cycles the highlighted tool: show the planner so that the choice
    // can be seen (ADAPTED: retail opens the planner from the hut).
    if keys.just_pressed(tools_key()) {
        ui_state.planner_open = true;
    }
    hud.planner_open = ui_state.planner_open;
    hud.diet = crate::creature_hud::diet_of(&crate::creature_actions::player_abilities(&run, &originals));
    hud.paused = pause.open;
    // The planner drawer and the options menu do not share the screen.
    if pause.open {
        ui_state.planner_open = false;
        hud.planner_open = false;
    }
    if let Some((_, left)) = ui_state.hint.as_mut() {
        *left -= time.delta_secs();
    }
    if ui_state.hint.as_ref().is_some_and(|(_, left)| *left <= 0.0) {
        ui_state.hint = None;
    }
}

/// The key that cycles the highlighted tool (`tribal_input`).
fn tools_key() -> KeyCode {
    KeyCode::KeyT
}

/// Spawn or despawn the static HUD layouts depending on the stage.
#[allow(clippy::too_many_arguments)]
fn manage(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    editor: Res<crate::editor::EditorState>,
    options: Res<crate::HudOptions>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut active: ResMut<TribalHudActive>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &TribalHudRoot)>,
) {
    let want = *stage.get() == GameStage::Tribal && !editor.open && !options.diagnostics;
    if !want {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        active.0 = false;
        return;
    }
    if roots.iter().any(|(_, r)| *r == TribalHudRoot::Main) {
        return;
    }
    if originals.resource_index.records.is_empty() {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let index = &originals.resource_index;
    let Some(main) = assets.layout(index, MAIN_LAYOUT) else {
        return;
    };
    let spawn = |commands: &mut Commands, assets: &mut SpuiAssets, name: &str, kind: TribalHudRoot, policy: RootPolicy, z: i32| {
        if let Some(layout) = assets.layout(index, name) {
            // The flipbook strips of the cards carry no frame grid in the file.
            let layout = if matches!(
                kind,
                TribalHudRoot::Citizen | TribalHudRoot::Tool | TribalHudRoot::Tribe | TribalHudRoot::Hut
            ) {
                world::with_flipbooks(layout)
            } else {
                layout
            };
            let root = view::spawn(commands, layout, policy, Some(camera), z);
            commands.entity(root).insert(kind);
        }
    };
    // Back to front: the cards over the world, the list, the planner, then
    // the bottom bar.
    spawn(&mut commands, &mut assets, world::TOOL_LAYOUT, TribalHudRoot::Tool, RootPolicy::Authored, 90);
    spawn(&mut commands, &mut assets, world::TRIBE_LAYOUT, TribalHudRoot::Tribe, RootPolicy::Authored, 91);
    spawn(&mut commands, &mut assets, world::CITIZEN_LAYOUT, TribalHudRoot::Citizen, RootPolicy::Authored, 92);
    spawn(&mut commands, &mut assets, world::POSSE_LAYOUT, TribalHudRoot::Posse, RootPolicy::Authored, 95);
    // The social game's bar stays spawned (hidden) like the retail UI's, which
    // loads it once and shows it with the song (0xE31630, 0xE31120).
    spawn(&mut commands, &mut assets, social::LAYOUT, TribalHudRoot::SocialBar, RootPolicy::Fit, 97);
    spawn(&mut commands, &mut assets, world::HUT_LAYOUT, TribalHudRoot::Hut, RootPolicy::Authored, 99);
    let root = view::spawn(&mut commands, main, RootPolicy::Fit, Some(camera), 100);
    commands.entity(root).insert(TribalHudRoot::Main);
    spawn(&mut commands, &mut assets, OPTIONS_LAYOUT, TribalHudRoot::Orb, RootPolicy::Fit, 101);
    // The hint bar and the callout dialog stay spawned (hidden) so that their
    // pop-in effects run when they are shown.
    spawn(&mut commands, &mut assets, crate::cell_hud::dialogs::HINT_LAYOUT, TribalHudRoot::Hint, RootPolicy::Fit, 102);
    spawn(
        &mut commands,
        &mut assets,
        crate::cell_hud::dialogs::CALLOUT_LAYOUT,
        TribalHudRoot::Callout,
        RootPolicy::Authored,
        103,
    );
    active.0 = true;
}

pub(crate) fn window(state: &SpuiState, control: u32) -> Option<WinId> {
    state.find(control)
}

pub(crate) fn set_visible(ui: &mut SpuiState, control: u32, on: bool) {
    if let Some(w) = window(ui, control) {
        ui.set_visible(w, on);
    }
}

/// The window of a layout element that has no control id.
pub(crate) fn element_window(ui: &SpuiState, element: usize) -> Option<WinId> {
    ui.layout.windows.iter().find(|w| w.element == element).map(|w| w.id)
}

/// The image of a whole PNG, if its texture can be loaded.
pub(crate) fn png_image(
    key: ResourceKey,
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) -> Option<ImageRef> {
    assets.texture(index, images, key).map(|_| ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None })
}

/// Fill one tray: the tools whose shack is built show their installed icon in
/// the slot button (shown only then; the empty slots stay the layout's
/// placeholders), the highlighted tool looks selected.
fn bind_tray(
    ui: &mut SpuiState,
    controls: &[u32; 3],
    tools: &[ToolEntry],
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) {
    for (slot, &control) in controls.iter().enumerate() {
        let Some(button) = window(ui, control) else {
            continue;
        };
        let entry = tools.get(slot);
        let shown = entry.is_some_and(|e| e.state == ToolState::Built);
        ui.set_visible(button, shown);
        ui.set_enabled(button, shown);
        let icon = entry.and_then(|e| tool_icon(e.kind)).and_then(|key| png_image(key, assets, images, index));
        ui.set_icon(button, icon);
        ui.set_selected(button, entry.is_some_and(|e| e.highlighted));
        ui.set_tooltip(button, entry.filter(|_| shown).map(tool_tooltip));
    }
}

#[allow(clippy::too_many_arguments)]
fn bind_main(
    ui: &mut SpuiState,
    hud: &TribalHudState,
    colors: &ProgressColors,
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) {
    if let Some(w) = window(ui, id::FOOD_TEXT) {
        ui.set_text(w, hud.food.to_string());
        ui.set_text_size(w, Some(FOOD_TEXT_PX));
        // The caption stays where the layout puts it. VERIFIED (tribal-audit-b):
        // the three-frame food icon 0x04D99151 and its panel 0x051B6DB0 are named
        // by no instruction and no data table of the march2017 image (scan of
        // every 32-bit immediate and of .rdata / .data), so the retail game
        // never shows the icon the file hides: the bar's number (0xABCD1001, set
        // by the shared progress bar) stands alone at its authored place. (The
        // Cell / Creature code behaves alike for their DNA icon.)
    }
    if let Some(w) = window(ui, id::PAUSE_TOGGLE) {
        ui.set_selected(w, hud.paused);
    }
    // Evolve replaces History once the progress bar is full.
    for control in [id::EVOLVE, id::EVOLVE_GLOW] {
        set_visible(ui, control, hud.evolve_visible);
    }
    set_visible(ui, id::HISTORY, !hud.evolve_visible);
    // Progress: three segments (tribe levels), five tribe icons, the level
    // icons at the end of the first two segments.
    for (i, control) in id::SEGMENTS.iter().enumerate() {
        let Some(w) = window(ui, *control) else {
            continue;
        };
        let fill = hud.level_fill[i];
        ui.set_shade(w, colors.right);
        ui.set_overlay(w, (fill > 0.0).then_some((fill, colors.left)));
    }
    for (i, control) in id::TRIBE_ICONS.iter().enumerate() {
        if let Some(w) = window(ui, *control) {
            ui.set_frame(w, Some(tribe_icon_frame(i, hud.tribes_done)));
        }
    }
    for (i, element) in LEVEL_ICON_ELEMENTS.iter().enumerate() {
        if let Some(w) = element_window(ui, *element) {
            ui.set_frame(w, Some(u32::from(hud.level as usize > i)));
        }
    }
    // Stance buttons and trays.
    let social = hud.stance == Stance::Social;
    if let Some(w) = window(ui, id::STANCE_SOCIAL) {
        ui.set_selected(w, social);
    }
    if let Some(w) = window(ui, id::STANCE_ATTACK) {
        ui.set_selected(w, !social);
    }
    set_visible(ui, id::SOCIAL_TRAY, social);
    set_visible(ui, id::ATTACK_TRAY, !social);
    bind_tray(ui, &id::SOCIAL_SLOTS, &hud.social, assets, images, index);
    bind_tray(ui, &id::ATTACK_SLOTS, &hud.attack, assets, images, index);
    // The slot key labels ("1".."3", control 0x062840D8) keep their authored
    // visibility and text: VERIFIED the Tribal HUD code names the control nowhere
    // (its only users, 0x1077D78 / 0x1078A81 / 0x1079DF8, are the Space tool
    // buttons), so retail shows the digits on both trays. They are the digit
    // keys of the slots (a performance answers its songs with them).
    bind_powers(ui, hud, assets, images, index);
    // The planet map panel with the camera buttons (held state in
    // [`TribalHudCamera`]); its picture, name and Hide toggle are
    // `land_minimap`'s, which also turns the holder's clicks into
    // `TribalControls::goto_request` / `move_request`.
    set_visible(ui, id::MAP_TOGGLE, CAMERA_BUTTONS);
}

/// Show the planet map panel (with the camera buttons, held state in
/// [`TribalHudCamera`]) of the layout: the retail minimap (`land_minimap`)
/// replaces the legacy world map of `navigation`, and the buttons are wired to
/// the Tribal camera through `TribalControls::hud`.
pub const CAMERA_BUTTONS: bool = true;

/// Fill the two ability slots of the stance slab: the picture, the tooltip and
/// the recharge of the Cell-stage and of the Creature-stage ability. The locked
/// variants (shown by the retail code before those stages were completed) stay
/// hidden: both abilities are always available here.
fn bind_powers(
    ui: &mut SpuiState,
    hud: &TribalHudState,
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) {
    set_visible(ui, id::SUPERPOWER_GROUP, true);
    for control in id::POWERS_LOCKED {
        set_visible(ui, control, false);
    }
    for (slot, &control) in id::POWERS.iter().enumerate() {
        let Some(button) = window(ui, control) else {
            continue;
        };
        let power = hud.powers.get(slot);
        ui.set_visible(button, power.is_some());
        let Some(power) = power else {
            continue;
        };
        let key = power_icon(power.kind);
        let icon = assets.texture(index, images, key).map(|_| ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None });
        ui.set_icon(button, icon);
        // A click is a use, not a switch: the toggle never stays checked.
        ui.set_selected(button, false);
        ui.set_enabled(button, hud.chieftain_up);
        ui.set_tint(button, if power.cooldown > 0.0 || !hud.chieftain_up { COOLDOWN_TINT } else { 0xFFFF_FFFF });
        let (name_id, description_id) = power_text_ids(power.kind);
        let name = assets.localized(index, POWER_TABLE, name_id).unwrap_or_else(|| power.kind.name().to_string());
        let description =
            assets.localized(index, POWER_TABLE, description_id).unwrap_or_else(|| power.kind.description().to_string());
        ui.set_tooltip(button, Some(power_tooltip(&name, &description, POWER_KEY_LABELS[slot], power.cooldown)));
    }
}

/// Push [`TribalHudState`] into the layouts.
#[allow(clippy::too_many_arguments)]
fn bind(
    hud: Res<TribalHudState>,
    originals: Res<crate::Originals>,
    colors: Res<ProgressColors>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut roots: Query<(&mut SpuiState, &TribalHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    for (mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match kind {
            TribalHudRoot::Main => bind_main(&mut ui, &hud, &colors, &mut assets, &mut images, index),
            // The orb and its menu are the Cell stage's (the menu is shown
            // while the game is paused through it).
            TribalHudRoot::Orb => crate::cell_hud::bind_orb(
                &mut ui,
                &crate::cell_contract::CellHudState { paused: hud.paused, ..default() },
            ),
            TribalHudRoot::SocialBar => social::bind_bar(&mut ui, hud.social_bar.as_ref()),
            _ => {}
        }
    }
}

/// The legacy palette panel (`tribal_rts::TribalPanel`) steps aside while the
/// SPUI HUD is shown (its selection box stays: it is gameplay, not chrome; its
/// song prompt steps aside in `tribal_rts::update_panel`: the song's own picture
/// is the `SocialBar` and the villagers' bubbles, `social`).
fn legacy_step_aside(
    active: Res<TribalHudActive>,
    mut panels: Query<&mut Node, With<crate::tribal_rts::TribalPanel>>,
) {
    let want = if active.0 { Display::None } else { Display::Flex };
    for mut node in &mut panels {
        if node.display != want {
            node.display = want;
        }
    }
}

/// Screen rectangles (physical pixels) of the HUD parts that are not buttons
/// but still keep a press from reaching the world: the bottom bar, the tribe
/// list items, the planner and the hut card.
pub fn solid_rects(roots: &Query<(&SpuiState, &TribalHudRoot)>) -> Vec<Rect> {
    let mut rects = Vec::new();
    for (ui, kind) in roots.iter() {
        if !ui.ready {
            continue;
        }
        let live = ui.interactive();
        let mut add = |control: u32| {
            if let Some(w) = ui.find(control).filter(|w| live[*w]) {
                rects.push(ui.rect(w));
            }
        };
        match kind {
            TribalHudRoot::Main => {
                // Everything from the top of the bottom bar down.
                if let Some(w) = ui.find(id::BAR) {
                    let bar = ui.rect(w);
                    rects.push(Rect { x1: 0.0, y1: bar.y1 - 20.0, x2: ui.screen[0], y2: ui.screen[1] });
                }
                // The minimap's picture (the holder window).
                if let Some(w) = ui.find(id::MAP_HOLDER).filter(|w| live[*w]) {
                    rects.push(ui.rect(w));
                }
            }
            TribalHudRoot::PosseItem(_) | TribalHudRoot::PosseBaby(_) => add(world::id::ITEM_BODY),
            TribalHudRoot::Planner => add(palette::id::PANEL),
            TribalHudRoot::Hut => {
                add(world::id::HUT_BODY);
                add(world::id::HUT_BUTTONS);
            }
            _ => {}
        }
    }
    rects
}

/// A press over the HUD belongs to the HUD: the world (`tribal_rts::
/// tribal_input`, which only knows Bevy UI buttons) would otherwise also
/// select or command units behind the bar, the list or the planner. The press
/// is taken out of the mouse input before the simulation reads it; the SPUI
/// input reads the held button, not the press, so it is unaffected.
pub(crate) fn swallow_world_clicks(
    pointer: Res<SpuiPointer>,
    roots: Query<(&SpuiState, &TribalHudRoot)>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
) {
    let Some(cursor) = pointer.position else {
        return;
    };
    let over = pointer.over_ui || solid_rects(&roots).iter().any(|r| r.contains(cursor.x, cursor.y));
    if over {
        mouse.clear_just_pressed(MouseButton::Left);
        mouse.clear_just_pressed(MouseButton::Right);
    }
}

/// A key the HUD presses stays down for the frame; `player_hud::release` lets
/// it go at the end of it.
pub(crate) fn press(keys: &mut ButtonInput<KeyCode>, injected: &mut crate::player_hud::InjectedKeys, key: KeyCode) {
    if !keys.pressed(key) {
        keys.press(key);
        injected.push(key);
    }
}

/// Button clicks become the keyboard actions and palette selections the
/// Tribal controls already use. Runs before the simulation (`Controls`) on the
/// clicks the SPUI input recorded in the previous frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn actions(
    hud: Res<TribalHudState>,
    mut roots: Query<(&mut SpuiState, &TribalHudRoot)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<crate::player_hud::InjectedKeys>,
    mut run: ResMut<RunProgress>,
    rts: Option<ResMut<TribalRts>>,
    mut ui_state: ResMut<TribalHudUi>,
    mut pause: ResMut<crate::pause_menu::PauseMenu>,
    mut front_menu: MessageWriter<crate::front_end::MenuCommand>,
) {
    use crate::cell_hud::id as menu;
    let mut rts = rts;
    let mut select_tool = |kind: ToolKind, keys: &mut ButtonInput<KeyCode>, injected: &mut crate::player_hud::InjectedKeys| {
        // The palette click of the legacy HUD: highlight the tool, then `B`
        // builds its shack or hands it to the selected members.
        if let Some(rts) = rts.as_mut() {
            rts.palette = kind;
        }
        press(keys, injected, KeyCode::KeyB);
    };
    for (mut ui, kind) in &mut roots {
        if !matches!(
            kind,
            TribalHudRoot::Main | TribalHudRoot::Orb | TribalHudRoot::Hut | TribalHudRoot::Planner | TribalHudRoot::Posse
        ) {
            continue;
        }
        for click in ui.take_clicks() {
            let control = ui.layout.window(click).control_id;
            // The options menu and Sporepedia are the front end's.
            if let Some(command) = crate::front_end::menu_command(control) {
                front_menu.write(command);
                continue;
            }
            match control {
                id::STANCE_SOCIAL | id::STANCE_ATTACK => {
                    let clicked = if control == id::STANCE_SOCIAL { Stance::Social } else { Stance::Attack };
                    if let Some(key) = stance_key(stance_of(&run.tribal_campaign), clicked) {
                        press(&mut keys, &mut injected, key);
                    }
                }
                id::EVOLVE => {
                    // The Evolve button takes the tribe on: the stage transition
                    // of `tribal_campaign_update` (play style card included) runs
                    // on this key.
                    if run.tribal_campaign.can_advance() {
                        press(&mut keys, &mut injected, ADVANCE_KEY);
                    }
                }
                // The pause menu is opened directly: Esc is the pause key.
                id::PAUSE_TOGGLE | menu::OPTIONS_ORB => pause.open = !pause.open,
                world::id::HUT_ADD_BABY => {
                    let price = rules::member_price();
                    if run.dna_this_stage < price {
                        // The retail hint for a purchase without enough food.
                        ui_state.hint = Some((world::NOT_ENOUGH_FOOD_HINT.to_string(), guide::MESSAGE_SECONDS));
                    } else {
                        // ADAPTED: the keyboard control (`R`) only buys a member
                        // with the chieftain at home; the retail button works from
                        // the hut card. The same purchase (`TribalCampaign::recruit`
                        // and the food it costs), without the distance rule.
                        let members = run.tribe.len().min(tribal::MAX_MATES);
                        let food = run.dna_this_stage;
                        if run.tribal_campaign.recruit(food, members) {
                            run.dna_this_stage = (run.dna_this_stage - price).max(0.0);
                            run.request_save();
                        }
                    }
                }
                world::id::HUT_PLANNER => ui_state.planner_open = !ui_state.planner_open,
                // "Select All" of the tribe list: the whole tribe (`Q`).
                world::id::POSSE_SELECT_ALL => press(&mut keys, &mut injected, select_tribe_key()),
                c => {
                    if let Some(slot) = id::POWERS.iter().position(|v| *v == c) {
                        // The ability keys of the controls (Y / U): a click is a use.
                        if hud.chieftain_up {
                            press(&mut keys, &mut injected, POWER_KEYS[slot]);
                        } else {
                            ui_state.hint = Some((world::CHIEFTAIN_DOWN_HINT.to_string(), guide::MESSAGE_SECONDS));
                        }
                    // (The camera buttons are held, not clicked: `publish_camera_buttons`.)
                    } else if let Some(slot) = id::SOCIAL_SLOTS.iter().position(|v| *v == c) {
                        if let Some(entry) = hud.social.get(slot) {
                            select_tool(entry.kind, &mut keys, &mut injected);
                        }
                    } else if let Some(slot) = id::ATTACK_SLOTS.iter().position(|v| *v == c) {
                        if let Some(entry) = hud.attack.get(slot) {
                            select_tool(entry.kind, &mut keys, &mut injected);
                        }
                    } else if let Some(entry) = palette::tile_of(c).and_then(|i| hud.palette.get(i)) {
                        select_tool(entry.kind, &mut keys, &mut injected);
                    }
                    // Sporepedia, My Collections and History have no
                    // counterpart in the rewrite yet.
                }
            }
        }
    }
}

/// Members of the tribe and the chieftain, as the list and the cards need
/// them. (Kept here so that `world` stays about presentation.)
pub type MemberQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static TribeUnit, &'static Health, &'static UnitTask, &'static Transform, Option<&'static WorldId>, Option<&'static Selected>),
    (With<TribeMate>, Without<Player>),
>;

#[cfg(test)]
mod tests {
    use super::*;

    const TOTALS: [usize; 3] = [1, 3, 5];

    #[test]
    fn the_segments_fill_one_after_the_other_in_tribes() {
        assert_eq!(level_fills(0, &TOTALS), [0.0, 0.0, 0.0]);
        assert_eq!(level_fills(1, &TOTALS), [1.0, 0.0, 0.0]);
        assert_eq!(level_fills(2, &TOTALS), [1.0, 0.5, 0.0]);
        assert_eq!(level_fills(3, &TOTALS), [1.0, 1.0, 0.0]);
        assert_eq!(level_fills(4, &TOTALS), [1.0, 1.0, 0.5]);
        assert_eq!(level_fills(5, &TOTALS), [1.0, 1.0, 1.0]);
        assert_eq!(level_fills(9, &TOTALS), [1.0, 1.0, 1.0]);
        // A degenerate table does not divide by zero (every segment is complete).
        assert_eq!(level_fills(2, &[0, 0, 0]), [1.0, 1.0, 1.0]);
        assert_eq!(level_fills(0, &[0, 0, 0]), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn the_level_icons_light_when_their_segment_is_full() {
        assert_eq!(lit_levels(0, &TOTALS), 0);
        assert_eq!(lit_levels(1, &TOTALS), 1);
        assert_eq!(lit_levels(2, &TOTALS), 1);
        assert_eq!(lit_levels(3, &TOTALS), 2);
        assert_eq!(lit_levels(5, &TOTALS), 3);
    }

    #[test]
    fn tribe_icons_light_one_per_tribe_done() {
        let frames: Vec<u32> = (0..5).map(|i| tribe_icon_frame(i, 2)).collect();
        assert_eq!(frames, [1, 1, 0, 0, 0]);
        assert_eq!((0..5).map(|i| tribe_icon_frame(i, 0)).sum::<u32>(), 0);
        assert_eq!((0..5).map(|i| tribe_icon_frame(i, 9)).sum::<u32>(), 5);
    }

    #[test]
    fn the_bar_ends_at_the_tribes_that_end_the_stage() {
        let totals = rules::level_totals();
        assert_eq!(totals[0], 1);
        assert_eq!(totals[1], 3);
        assert_eq!(totals[2], tribal::RIVAL_VILLAGES);
        assert!(totals.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn every_tool_but_the_camp_fire_is_in_the_planner_once_and_the_trays_hold_six() {
        let mut all: Vec<ToolKind> = PLANNER_SECTIONS.iter().flat_map(|s| s.iter().copied()).collect();
        assert_eq!(all.len(), tools::TOOLS.len() - 1);
        assert!(!all.contains(&ToolKind::Campfire), "the camp fire is not bought");
        all.sort();
        all.dedup();
        assert_eq!(all.len(), tools::TOOLS.len() - 1, "no tool twice");
        for kind in SOCIAL_TOOLS {
            assert_eq!(tools::info(kind).class, tools::ToolClass::Social);
        }
        for kind in ATTACK_TOOLS {
            assert_eq!(tools::info(kind).class, tools::ToolClass::Attack);
        }
    }

    #[test]
    fn a_tool_is_built_available_or_locked() {
        let mut campaign = TribalCampaign::default();
        campaign.ensure_ready();
        // Retail starting tools (`tribeStartingTools` [3, 4]): the spear is open
        // at the start, the torch (spiked clubs) is a tier-2 tool.
        let locked = tool_entry(ToolKind::SpikedClubs, &campaign, ToolKind::ThrowingSpears);
        assert_eq!(locked.state, ToolState::Locked);
        assert_eq!(locked.tier, 2);
        let free = tool_entry(ToolKind::ThrowingSpears, &campaign, ToolKind::ThrowingSpears);
        assert_eq!(free.state, ToolState::Available);
        assert!(free.highlighted);
        assert_eq!(free.price, tools::info(ToolKind::ThrowingSpears).price);
        campaign.shacks.push(ToolKind::ThrowingSpears);
        assert_eq!(tool_entry(ToolKind::ThrowingSpears, &campaign, ToolKind::Maracas).state, ToolState::Built);
        for village in campaign.villages.iter_mut().take(2) {
            village.status = tribal::RivalStatus::Allied;
        }
        assert_eq!(tool_entry(ToolKind::SpikedClubs, &campaign, ToolKind::Maracas).state, ToolState::Available);
    }

    #[test]
    fn tooltips_name_the_tool_its_price_and_its_state() {
        let entry = |state| ToolEntry { kind: ToolKind::StoneAxes, state, price: 25.0, highlighted: false, tier: 1 };
        assert!(tool_tooltip(&entry(ToolState::Available)).contains("25 Food"));
        assert!(tool_tooltip(&entry(ToolState::Available)).starts_with("Stone Axes\n"));
        assert!(tool_tooltip(&entry(ToolState::Built)).contains("selected members"));
        assert!(tool_tooltip(&entry(ToolState::Locked)).contains("Settle 1 tribe to unlock"));
    }

    #[test]
    fn every_tool_with_a_picture_has_an_installed_icon_key() {
        for info in &tools::TOOLS {
            assert_eq!(tool_icon(info.kind).is_some(), info.icon.is_some(), "{:?}", info.kind);
        }
        let keys: std::collections::HashSet<_> =
            tools::TOOLS.iter().filter_map(|t| tool_icon(t.kind)).map(|k| k.instance_id).collect();
        assert_eq!(keys.len(), tools::TOOLS.iter().filter(|t| t.icon.is_some()).count(), "every icon is distinct");
    }

    #[test]
    fn a_stance_click_presses_the_key_only_when_it_changes_the_stance() {
        assert!(stance_click_needs_key(Stance::Attack, Stance::Social));
        assert!(stance_click_needs_key(Stance::Social, Stance::Attack));
        assert!(!stance_click_needs_key(Stance::Social, Stance::Social));
        assert!(!stance_click_needs_key(Stance::Attack, Stance::Attack));
        let mut campaign = TribalCampaign::default();
        assert_eq!(stance_of(&campaign), Stance::Attack);
        campaign.friendly_stance = true;
        assert_eq!(stance_of(&campaign), Stance::Social);
    }

    // ---- abilities ---------------------------------------------------------

    #[test]
    fn the_abilities_have_keys_texts_and_pictures() {
        use TribalPowerKind::*;
        let kinds = [Traps, RefreshingStorm, FlyingFish, FireBombs, Fireworks, Beastmaster];
        let mut icons = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for kind in kinds {
            assert!(icons.insert(power_icon(kind).instance_id), "{kind:?} has its own picture");
            let (name, description) = power_text_ids(kind);
            assert!(names.insert(name));
            assert_eq!(description, name + 1, "the description follows the name in the table");
        }
        // Antigravity's 0.81 bindings, which the buttons press.
        assert_eq!(POWER_KEYS, [KeyCode::KeyY, KeyCode::KeyU]);
        assert_eq!(POWER_KEY_LABELS, ["Y", "U"]);
        assert_eq!(power_tooltip("Traps", "Lay traps", "Y", 0.0), "Traps [Y]\nLay traps");
        assert_eq!(power_tooltip("Traps", "Lay traps", "Y", 12.2), "Traps [Y]\nLay traps\nReady in 13 s");
    }

    // ---- the installed main layout ------------------------------------------

    fn installed() -> Option<(ResourceIndex, SpuiAssets)> {
        let install = crate::spore_install::discover().ok()?;
        let index = ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then(|| (index, SpuiAssets::default()))
    }

    #[test]
    fn installed_ability_texts_and_pictures_are_retail() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        use TribalPowerKind::*;
        for kind in [Traps, RefreshingStorm, FlyingFish, FireBombs, Fireworks, Beastmaster] {
            let (name, description) = power_text_ids(kind);
            let name = assets.localized(&index, POWER_TABLE, name).unwrap_or_default();
            let description = assets.localized(&index, POWER_TABLE, description).unwrap_or_default();
            assert_eq!(name, kind.name(), "the rewrite already uses the retail names");
            assert!(description.starts_with("This ability allows the chieftain"), "{description}");
            assert!(index.records.contains_key(&power_icon(kind)), "{kind:?} picture");
        }
    }

    #[test]
    fn installed_main_layout_has_every_control_the_binding_uses() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, MAIN_LAYOUT).expect("GlobalUITribe3-6");
        let single = [
            id::FOOD_TEXT,
            id::FOOD_PANEL,
            id::FOOD_ICON,
            id::BAR,
            id::BAR_POINTER,
            id::EVOLVE,
            id::EVOLVE_GLOW,
            id::HISTORY,
            id::PAUSE_TOGGLE,
            id::SPOREPEDIA,
            id::COLLECTIONS,
            id::STANCE_SOCIAL,
            id::STANCE_ATTACK,
            id::SOCIAL_TRAY,
            id::ATTACK_TRAY,
            id::SLOT_KEY_LABEL,
            id::SUPERPOWER_GROUP,
            id::MAP_TOGGLE,
            id::MAP_HOLDER,
        ];
        for control in single
            .iter()
            .chain(&id::SEGMENTS)
            .chain(&id::TRIBE_ICONS)
            .chain(&id::SOCIAL_SLOTS)
            .chain(&id::ATTACK_SLOTS)
            .chain(&id::POWERS)
            .chain(&id::POWERS_LOCKED)
            .chain(&CAMERA_BUTTON_IDS)
        {
            assert!(layout.find(*control).is_some(), "window {control:08X}");
        }
        // The social tray holds the social slots, the attack tray the attack ones.
        let holder = |button: u32| {
            let mut at = layout.find(button);
            while let Some(w) = at {
                let control = layout.window(w).control_id;
                if control == id::SOCIAL_TRAY || control == id::ATTACK_TRAY {
                    return control;
                }
                at = layout.window(w).parent;
            }
            0
        };
        assert_eq!(holder(id::SOCIAL_SLOTS[0]), id::SOCIAL_TRAY);
        assert_eq!(holder(id::ATTACK_SLOTS[2]), id::ATTACK_TRAY);
        // The two ability toggles and their locked variants share the ability group.
        let group = layout.find(id::SUPERPOWER_GROUP).unwrap();
        for control in id::POWERS.iter().chain(&id::POWERS_LOCKED) {
            assert_eq!(layout.window(layout.find(*control).unwrap()).parent, Some(group), "{control:08X}");
        }
        // The tribe and level icons are two-frame flipbooks (dark, lit); the food icon has three.
        for control in id::TRIBE_ICONS {
            let a = layout.window(layout.find(control).unwrap()).anim().cloned().expect("animated icon");
            assert_eq!((a.rows * a.columns).max(a.frame_count), 2, "{control:08X}");
        }
        for element in LEVEL_ICON_ELEMENTS {
            let w = layout.windows.iter().find(|w| w.element == element).expect("level icon");
            let a = w.anim().cloned().expect("animated icon");
            assert_eq!((a.rows * a.columns).max(a.frame_count), 2, "element {element}");
        }
        let food = layout.window(layout.find(id::FOOD_ICON).unwrap()).anim().cloned().unwrap();
        assert_eq!((food.rows * food.columns).max(food.frame_count), 3);
    }

    #[test]
    fn installed_stance_and_bar_tooltips_are_the_retail_strings() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, MAIN_LAYOUT).unwrap();
        let tip = |assets: &mut SpuiAssets, control: u32| {
            let w = layout.find(control).unwrap();
            layout.window(w).procs.iter().find_map(|p| match p {
                crate::spui::model::WinProc::Tooltip { text: Some(text), .. } => Some(assets.text(&index, text)),
                _ => None,
            })
        };
        assert_eq!(tip(&mut assets, id::STANCE_SOCIAL).as_deref(), Some("Social Stance"));
        assert_eq!(tip(&mut assets, id::STANCE_ATTACK).as_deref(), Some("Combat Stance"));
        assert_eq!(tip(&mut assets, id::FOOD_TEXT).as_deref(), Some("Food Collected"));
        assert_eq!(tip(&mut assets, id::EVOLVE).as_deref(), Some("Advance to Civilization"));
        assert_eq!(tip(&mut assets, id::HISTORY).as_deref(), Some("History"));
        assert_eq!(tip(&mut assets, id::SEGMENTS[1]).as_deref(), Some("Conquer or ally with other tribes to progress"));
    }

    fn demo_hud() -> TribalHudState {
        let totals = rules::level_totals();
        let entry = |kind: ToolKind, state, highlighted| ToolEntry {
            kind,
            state,
            price: rules::tool_price(kind),
            highlighted,
            tier: tools::info(kind).tier,
        };
        TribalHudState {
            food: 135,
            tribes_done: 2,
            tribes_goal: 5,
            level_fill: level_fills(2, &totals),
            level: lit_levels(2, &totals),
            stance: Stance::Social,
            powers: vec![
                PowerSlot { kind: TribalPowerKind::Traps, cooldown: 0.0, total: 60.0 },
                PowerSlot { kind: TribalPowerKind::Fireworks, cooldown: 30.0, total: 90.0 },
            ],
            chieftain_up: true,
            social: vec![
                entry(ToolKind::WoodenHorns, ToolState::Built, false),
                entry(ToolKind::Maracas, ToolState::Built, true),
                entry(ToolKind::Aerophones, ToolState::Locked, false),
            ],
            attack: vec![entry(ToolKind::SpikedClubs, ToolState::Available, false)],
            diet: Diet::Carnivore,
            ..default()
        }
    }

    #[test]
    fn the_main_layout_shows_the_published_state() {
        use crate::spui::ids::{flag, state};
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, MAIN_LAYOUT).unwrap();
        let mut images = Assets::<Image>::default();
        let mut ui = SpuiState::new(layout, RootPolicy::Fit);
        let mut hud = demo_hud();
        let colors = ProgressColors::default();
        bind_main(&mut ui, &hud, &colors, &mut assets, &mut images, &index);
        let w = |ui: &SpuiState, control: u32| ui.find(control).unwrap();
        // Food number at its authored place; the retail code never touches the
        // (hidden) diet icon, so neither does the binding.
        assert_eq!(ui.captions[w(&ui, id::FOOD_TEXT)].as_deref(), Some("135"));
        assert_eq!(ui.frame_overrides[w(&ui, id::FOOD_ICON)], None);
        assert!(!ui.visible(w(&ui, id::FOOD_ICON)));
        let authored = ui.layout.window(w(&ui, id::FOOD_TEXT)).area;
        assert_eq!(ui.area(w(&ui, id::FOOD_TEXT)), authored);
        // The progress bar: segment fills, tribe icons, level icons; History until the bar is full.
        assert_eq!(ui.overlays[w(&ui, id::SEGMENTS[0])].map(|o| o.0), Some(1.0));
        assert_eq!(ui.overlays[w(&ui, id::SEGMENTS[1])].map(|o| o.0), Some(0.5));
        assert!(ui.overlays[w(&ui, id::SEGMENTS[2])].is_none());
        let frames: Vec<_> = id::TRIBE_ICONS.iter().map(|c| ui.frame_overrides[w(&ui, *c)]).collect();
        assert_eq!(frames, [Some(1), Some(1), Some(0), Some(0), Some(0)]);
        assert!(ui.visible(w(&ui, id::HISTORY)) && !ui.visible(w(&ui, id::EVOLVE)) && !ui.visible(w(&ui, id::EVOLVE_GLOW)));
        // The social stance: its tray and the social slots show their tools; locked tools stay empty.
        assert!(ui.visible(w(&ui, id::SOCIAL_TRAY)) && !ui.visible(w(&ui, id::ATTACK_TRAY)));
        assert!(ui.states[w(&ui, id::STANCE_SOCIAL)] & state::SELECTED != 0);
        assert!(ui.states[w(&ui, id::STANCE_ATTACK)] & state::SELECTED == 0);
        let slot = |ui: &SpuiState, n: usize| w(ui, id::SOCIAL_SLOTS[n]);
        assert!(ui.visible(slot(&ui, 0)) && ui.visible(slot(&ui, 1)) && !ui.visible(slot(&ui, 2)));
        assert!(ui.icon_overrides[slot(&ui, 0)].is_some());
        assert!(ui.states[slot(&ui, 1)] & state::SELECTED != 0, "the highlighted tool looks selected");
        assert!(ui.tooltip_overrides[slot(&ui, 1)].as_deref().is_some_and(|t| t.starts_with("Maracas\n")));
        // The ability slots: pictures, tooltips with the retail text and the key, the recharge dims.
        let power = |ui: &SpuiState, n: usize| w(ui, id::POWERS[n]);
        assert!(ui.visible(w(&ui, id::SUPERPOWER_GROUP)));
        assert!(ui.visible(power(&ui, 0)) && ui.visible(power(&ui, 1)));
        assert!(!ui.visible(w(&ui, id::POWERS_LOCKED[0])) && !ui.visible(w(&ui, id::POWERS_LOCKED[1])));
        assert!(ui.icon_overrides[power(&ui, 0)].is_some());
        assert_eq!(ui.tints[power(&ui, 0)], 0xFFFF_FFFF);
        assert_eq!(ui.tints[power(&ui, 1)], COOLDOWN_TINT);
        assert!(ui.tooltip_overrides[power(&ui, 0)].as_deref().is_some_and(|t| t.starts_with("Traps [Y]\nThis ability allows")));
        assert!(ui.tooltip_overrides[power(&ui, 1)].as_deref().is_some_and(|t| t.starts_with("Fireworks [U]") && t.ends_with("Ready in 30 s")));
        // Without the chieftain the abilities are out.
        hud.chieftain_up = false;
        bind_main(&mut ui, &hud, &colors, &mut assets, &mut images, &index);
        assert!(ui.flags[power(&ui, 0)] & flag::ENABLED == 0);
        assert_eq!(ui.tints[power(&ui, 0)], COOLDOWN_TINT);
        // The attack stance with a full bar: the other tray, Evolve instead of History.
        hud.stance = Stance::Attack;
        hud.evolve_visible = true;
        hud.chieftain_up = true;
        hud.tribes_done = 5;
        hud.level_fill = level_fills(5, &rules::level_totals());
        hud.level = 3;
        bind_main(&mut ui, &hud, &colors, &mut assets, &mut images, &index);
        assert!(!ui.visible(w(&ui, id::SOCIAL_TRAY)) && ui.visible(w(&ui, id::ATTACK_TRAY)));
        assert!(ui.visible(w(&ui, id::EVOLVE)) && ui.visible(w(&ui, id::EVOLVE_GLOW)) && !ui.visible(w(&ui, id::HISTORY)));
        assert_eq!(ui.overlays[w(&ui, id::SEGMENTS[2])].map(|o| o.0), Some(1.0));
        assert!(ui.flags[power(&ui, 0)] & flag::ENABLED != 0);
        // The map panel waits for the camera keys.
        assert_eq!(ui.visible(w(&ui, id::MAP_TOGGLE)), CAMERA_BUTTONS);
    }

    #[test]
    fn the_stance_and_select_all_buttons_press_the_keys_of_the_active_controls() {
        assert_eq!(select_tribe_key(), if CONTROLS_KEYS { KEY_SELECT_TRIBE } else { KeyCode::KeyQ });
        if CONTROLS_KEYS {
            // tribal-ctrl: the buttons select, they never toggle.
            assert_eq!(stance_key(Stance::Social, Stance::Social), Some(KEY_STANCE_SOCIAL));
            assert_eq!(stance_key(Stance::Social, Stance::Attack), Some(KEY_STANCE_ATTACK));
        } else {
            assert_eq!(stance_key(Stance::Attack, Stance::Social), Some(KeyCode::KeyC));
            assert_eq!(stance_key(Stance::Social, Stance::Attack), Some(KeyCode::KeyC));
            assert_eq!(stance_key(Stance::Social, Stance::Social), None);
        }
        // The virtual keys are keys nobody types.
        for key in [KEY_SELECT_TRIBE, KEY_STANCE_SOCIAL, KEY_STANCE_ATTACK] {
            assert!(matches!(key, KeyCode::F15 | KeyCode::F16 | KeyCode::F17));
        }
    }

    #[test]
    fn held_camera_buttons_are_read_from_the_layout() {
        use crate::spui::ids::state;
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, MAIN_LAYOUT).unwrap();
        let mut ui = SpuiState::new(layout, RootPolicy::Fit);
        assert_eq!(held_camera_buttons(&ui), TribalHudCamera::default());
        for (n, control) in CAMERA_BUTTON_IDS.iter().enumerate() {
            let w = ui.find(*control).unwrap();
            ui.states[w] |= state::CLICKED;
            let held = held_camera_buttons(&ui);
            let all = [held.zoom_in, held.zoom_out, held.rotate_left, held.rotate_right];
            assert_eq!(all.iter().filter(|h| **h).count(), n + 1, "{control:08X}");
            assert!(all[n]);
        }
    }

    // ---- clicks through the controls ---------------------------------------

    fn tribal_app() -> App {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<RunProgress>().enter_stage(GameStage::Tribal);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Tribal);
        app.update();
        app.update();
        app
    }

    /// A root with the installed main layout, to click on.
    fn with_main_layout(app: &mut App) -> Option<Entity> {
        let (index, mut assets) = installed()?;
        let layout = assets.layout(&index, MAIN_LAYOUT)?;
        Some(app.world_mut().spawn((TribalHudRoot::Main, SpuiState::new(layout, RootPolicy::Fit))).id())
    }

    fn click(app: &mut App, root: Entity, control: u32) {
        let mut ui = app.world_mut().get_mut::<SpuiState>(root).unwrap();
        let w = ui.find(control).unwrap();
        ui.clicks.push(w);
        app.update();
        // Nothing drives the keyboard in a headless app: start the next frame
        // with a clean state, like the input plugin does.
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    }

    /// Whether the headless app has the installed HUD (the tests that need the
    /// layouts return quietly without an install).
    fn hud_ready(app: &mut App) -> bool {
        for _ in 0..8 {
            app.update();
        }
        app.world().resource::<TribalHudActive>().0
    }

    fn run_until(app: &mut App, mut done: impl FnMut(&App) -> bool) -> bool {
        for _ in 0..400 {
            if done(app) {
                return true;
            }
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        }
        done(app)
    }

    #[test]
    fn the_guide_runs_the_retail_opening_the_forced_chain_and_the_goals() {
        use guide::GuideState;
        use tutorial::{Stage, TutorialGoal};
        let mut app = tribal_app();
        if !hud_ready(&mut app) {
            eprintln!("the guide test needs the installed HUD layouts: skipped");
            return;
        }
        // The opening: Tutorials is on in the settings. `TRG_Begin`'s three
        // callouts and the first goal card; the posse is not introduced.
        assert!(run_until(&mut app, |a| !a.world().resource::<TribalHudState>().callouts.is_empty()));
        {
            let hud = app.world().resource::<TribalHudState>();
            assert_eq!(hud.callouts, tutorial::OPENING);
            assert_eq!(hud.goal_cards.len(), 1);
            assert_eq!(hud.goal_cards[0].kind, "loc:TribeGame:05935b28", "the Outfit card");
            assert_eq!(app.world().resource::<RunProgress>().tribal_campaign.tutorial.stage, Stage::Opening);
            assert!(hud.hint.is_none(), "no hint under a callout");
        }
        let dismiss = |app: &mut App, n: usize| {
            for _ in 0..n {
                app.world_mut().resource_mut::<GuideState>().dismiss();
            }
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        };
        dismiss(&mut app, 3);
        let stage = |app: &App| app.world().resource::<RunProgress>().tribal_campaign.tutorial.stage;
        assert_eq!(stage(&app), Stage::Planner, "the forced step: the planner");
        // Camera input before the planner closed does not count.
        app.world_mut().resource_mut::<crate::tribal_controls::TribalCamera>().rig.pan_world(Vec3::new(8.0, 0.0, 0.0));
        app.update();
        assert_eq!(stage(&app), Stage::Planner);
        // Opening and closing the planner shows the tutorial mode callouts.
        app.world_mut().resource_mut::<TribalHudUi>().planner_open = true;
        app.update();
        app.world_mut().resource_mut::<TribalHudUi>().planner_open = false;
        app.update();
        assert_eq!(app.world().resource::<TribalHudState>().callouts, tutorial::MODE_CALLOUTS);
        assert_eq!(stage(&app), Stage::Callouts);
        dismiss(&mut app, 2);
        assert_eq!(stage(&app), Stage::Zoom);
        app.update();
        assert_eq!(app.world().resource::<TribalHudState>().hint.as_deref(), Some("trg-camzoom"));
        // The chain: each step only on the hint's turn.
        app.world_mut().resource_mut::<crate::tribal_controls::TribalCamera>().rig.orbit_by_pixels(Vec2::new(60.0, 0.0));
        app.update();
        assert_eq!(stage(&app), Stage::Zoom, "rotating is not the zoom step");
        app.world_mut().resource_mut::<crate::tribal_controls::TribalCamera>().rig.zoom_step(-1);
        app.update();
        assert_eq!(stage(&app), Stage::Rotate);
        app.world_mut().resource_mut::<crate::tribal_controls::TribalCamera>().rig.orbit_by_pixels(Vec2::new(60.0, 0.0));
        app.update();
        assert_eq!(stage(&app), Stage::Translate);
        app.world_mut().resource_mut::<crate::tribal_controls::TribalCamera>().rig.pan_world(Vec3::new(8.0, 0.0, 0.0));
        app.update();
        assert_eq!(stage(&app), Stage::Minimap);
        assert_eq!(app.world().resource::<TribalHudState>().hint.as_deref(), Some("trg-minimapuse"));
        app.world_mut().resource_mut::<crate::tribal_controls::TribalControls>().minimap_gotos += 1;
        app.update();
        assert_eq!(stage(&app), Stage::SelectList);
        app.world_mut().resource_mut::<TribalHudUi>().list_picks += 1;
        app.update();
        assert_eq!(stage(&app), Stage::SelectBox);
        assert_eq!(app.world().resource::<TribalHudState>().hint.as_deref(), Some("trg-selectbox"));
        let mates: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<TribeMate>>()
            .iter(app.world())
            .take(2)
            .collect();
        assert_eq!(mates.len(), 2);
        for mate in mates {
            app.world_mut().entity_mut(mate).insert(crate::tribal_rts::Selected);
        }
        app.update();
        assert_eq!(stage(&app), Stage::Done);
        {
            let run = app.world().resource::<RunProgress>();
            assert_eq!(run.tribal_campaign.tutorial.goals(), [TutorialGoal::Outfit, TutorialGoal::Diet]);
            let hud = app.world().resource::<TribalHudState>();
            assert_eq!(hud.goal_cards.len(), 2);
            assert!(hud.hint.as_deref().is_some_and(|h| h.starts_with("trg-tribeeat")), "{:?}", hud.hint);
        }
        // The food flag ends the diet goal and asks for babies.
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 45.0;
        app.update();
        let hud = app.world().resource::<TribalHudState>();
        assert_eq!(
            hud.goal_cards.iter().map(|c| c.kind.as_str()).collect::<Vec<_>>(),
            ["loc:TribeGame:05935b28", "loc:TribeGame:056cea63"]
        );
        // The first tribe to appear: the chieftain callout (once) and the Interact card.
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.villages[0].appeared = true;
        app.update();
        let hud = app.world().resource::<TribalHudState>();
        assert_eq!(hud.callouts, ["Callout_TRG_ChieftainWithPowers"]);
        assert_eq!(
            hud.goal_cards.iter().map(|c| c.kind.as_str()).collect::<Vec<_>>(),
            ["loc:TribeGame:05935b28", "loc:TribeGame:056cea63", "loc:TribeGame:0567cbee"]
        );
        app.world_mut().resource_mut::<GuideState>().dismiss();
        app.update();
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.villages[1].appeared = true;
        app.update();
        assert!(app.world().resource::<TribalHudState>().callouts.is_empty(), "the chieftain callout comes once");
    }

    #[test]
    fn the_camp_fire_stands_from_the_first_frame() {
        let mut app = tribal_app();
        app.update();
        let kinds: Vec<tools::ToolKind> =
            app.world_mut().query::<&tools::ToolShack>().iter(app.world()).map(|s| s.0).collect();
        assert_eq!(kinds, [tools::ToolKind::Campfire], "the fire pit of TRG_Begin is the only rack at the start");
        assert_eq!(app.world().resource::<RunProgress>().tribal_campaign.shacks, [tools::ToolKind::Campfire]);
        assert_eq!(app.world().resource::<TribalHudState>().food, 15, "the fire costs nothing");
    }

    #[test]
    fn the_published_state_follows_the_tribe() {
        let mut app = tribal_app();
        app.update();
        let hud = app.world().resource::<TribalHudState>();
        assert_eq!(hud.tribes_goal, rules::tribes_goal());
        assert_eq!(hud.tribes_done, 0);
        assert_eq!(hud.level_fill, [0.0; 3]);
        assert!(!hud.evolve_visible && !hud.paused && !hud.planner_open);
        assert_eq!(hud.social.len(), 3);
        assert_eq!(hud.attack.len(), 3);
        assert_eq!(hud.palette.len(), tools::TOOLS.len() - 1, "every tool but the camp fire is bought in the planner");
        assert_eq!(hud.powers.len(), 2);
        assert!(hud.chieftain_up, "the chieftain stands in the tribal scene");
        assert!(!hud.list.is_empty() && hud.list[0].chieftain, "the chieftain heads the list");
        assert!(hud.callouts.is_empty(), "no HUD, no callouts: the simulation is never held up");
        assert_eq!(hud.stance, Stance::Attack);
    }

    #[test]
    fn food_progress_and_the_pause_menu_are_published() {
        let mut app = tribal_app();
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 61.9;
        app.world_mut().resource_mut::<crate::pause_menu::PauseMenu>().open = true;
        app.world_mut().resource_mut::<TribalHudUi>().planner_open = true;
        app.update();
        let hud = app.world().resource::<TribalHudState>();
        assert_eq!(hud.food, 61, "the number is the food, rounded down");
        assert!(hud.paused);
        assert!(!hud.planner_open, "the planner and the options menu do not share the screen");
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            for village in run.tribal_campaign.villages.iter_mut().take(2) {
                village.status = tribal::RivalStatus::Allied;
            }
        }
        app.world_mut().resource_mut::<crate::pause_menu::PauseMenu>().open = false;
        app.update();
        let hud = app.world().resource::<TribalHudState>();
        assert_eq!(hud.tribes_done, 2);
        assert_eq!(hud.level_fill, level_fills(2, &rules::level_totals()));
        assert!(hud.level >= 1);
    }

    #[test]
    fn clicking_the_stance_buttons_switches_the_stance_through_the_key() {
        let mut app = tribal_app();
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        assert!(!app.world().resource::<RunProgress>().tribal_campaign.friendly_stance);
        click(&mut app, root, id::STANCE_SOCIAL);
        assert!(app.world().resource::<RunProgress>().tribal_campaign.friendly_stance);
        // Clicking the stance that is already on changes nothing.
        click(&mut app, root, id::STANCE_SOCIAL);
        assert!(app.world().resource::<RunProgress>().tribal_campaign.friendly_stance);
        click(&mut app, root, id::STANCE_ATTACK);
        assert!(!app.world().resource::<RunProgress>().tribal_campaign.friendly_stance);
        // The injected key was let go again.
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyC));
    }

    #[test]
    fn the_pause_toggle_opens_and_closes_the_options_menu() {
        let mut app = tribal_app();
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
        click(&mut app, root, id::PAUSE_TOGGLE);
        assert!(app.world().resource::<crate::pause_menu::PauseMenu>().open);
        click(&mut app, root, id::PAUSE_TOGGLE);
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
    }

    #[test]
    fn tool_slots_build_the_shack_and_the_ability_buttons_press_their_keys() {
        let mut app = tribal_app();
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        // The slot of a tool whose shack is not built is not on screen; the
        // click of a built one highlights it and builds / hands it out. Give the
        // tribe the food and the shack of the horns, then click its slot.
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.dna_this_stage = 500.0;
            run.tribal_campaign.shacks.push(ToolKind::WoodenHorns);
        }
        app.update();
        click(&mut app, root, id::SOCIAL_SLOTS[0]);
        assert_eq!(app.world().resource::<TribalRts>().palette, ToolKind::WoodenHorns);
        // The ability buttons press the keys of the controls: the Cell power on Y.
        let before = app.world().resource::<RunProgress>().tribal_campaign.power_cell_cooldown;
        assert_eq!(before, 0.0);
        click(&mut app, root, id::POWERS[0]);
        let run = app.world().resource::<RunProgress>();
        assert!(run.tribal_campaign.power_cell_cooldown > 0.0, "the ability was used");
    }

    #[test]
    fn evolve_waits_for_a_full_bar() {
        let mut app = tribal_app();
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        // Not full: the click does nothing.
        click(&mut app, root, id::EVOLVE);
        assert_eq!(*app.world().resource::<State<GameStage>>().get(), GameStage::Tribal);
    }

    #[test]
    fn the_united_tribes_wait_for_the_evolve_button_which_takes_the_tribe_to_civilization() {
        let mut app = tribal_app();
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.tribal_campaign.reveal_all();
            for village in run.tribal_campaign.villages.iter_mut() {
                village.status = tribal::RivalStatus::Allied;
                village.relations = 100.0;
            }
            assert!(run.tribal_campaign.can_advance());
        }
        // The stage no longer ends by itself: the button appears and waits.
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(*app.world().resource::<State<GameStage>>().get(), GameStage::Tribal);
        assert!(app.world().resource::<TribalHudState>().evolve_visible);
        assert!(app.world().resource::<RunProgress>().tribal_campaign.city_hall_card.is_none());
        // The click is the key the transition reads: the play style card is
        // chosen and the tribe goes on to Civilization.
        click(&mut app, root, id::EVOLVE);
        app.update();
        assert_eq!(*app.world().resource::<State<GameStage>>().get(), GameStage::Civilization);
        let campaign = &app.world().resource::<RunProgress>().tribal_campaign;
        let card = campaign.city_hall_card.expect("the card");
        // The click also stores the consequence trait and the play-style id
        // (0xCDBEA0), and a tribe that allied with all five earned the social one.
        assert_eq!(campaign.earned_trait, Some(card.trait_id()));
        assert_eq!(campaign.play_style_id, Some(card.card_id()));
        assert_eq!(card, crate::tribal_rules::PlayStyle::Religious);
        assert_eq!(campaign.earned_trait, Some(crate::world_objects::name_id("trg_earnedtraitsocial")));
    }
}
