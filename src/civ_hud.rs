//! The retail Civilization-stage HUD: spawns the SPUI layouts the original
//! game loads for the Civ stage and binds them to [`CivHudState`].
//!
//! * `GlobalUICiv3-3` (the original name of `GlobalUICivilization~`): the
//!   Sporebucks readout, the city-control progress bar (ten city icons in two
//!   segments, the air-vehicle and space-travel milestones), the four
//!   consequence ability buttons (Cell, Creature, Tribe and the Civ
//!   superweapon), the vehicle stance toggles, the planet minimap with the
//!   nations' communication buttons, History/Sporepedia/collection/pause and
//!   Advance to Space.
//! * `GlobalUIOptions-2`: the swirl orb (options menu), shared with the Cell
//!   stage (`cell_hud`).
//! * `CVGMissionCard`: the goal cards at the top left ([`guide`]).
//! * `RolloverCivCity`, `Rollover_CivVehicle`, `Rollover_Building` and
//!   `CivSpiceMeter`: the cards of the city, vehicle, building and spice
//!   geyser under the pointer, plus the minimap picture ([`world`]).
//! * `CommScreen-3`: the communication screen with a
//!   rival nation ([`comm`]).
//! * the retail callout dialogs and hints (`CalloutCVG*`, `cvg-*`), through
//!   the generic machinery of `cell_hud::dialogs` ([`guide`]).
//!
//! The layouts carry all geometry and art. This module only decides which
//! windows are shown, fills their text and meters, and turns button clicks into
//! the keys and commands the Civilization modules already have, so those
//! modules are not touched: [`publish_state`] fills the [`CivHudState`]
//! contract from `RunProgress` (the campaign) and the live vehicles each
//! frame; nothing writes back except the purchases and diplomacy clicks of
//! the city card and the communication screen, which call the public
//! functions of `civ` / `civ_rts` the keyboard commands call.
//!
//! Control ids and roles come from the layouts and their tooltips (see
//! `notes\civ-hud-notes.md`). Where the retail code's behaviour is not
//! documented the binding is marked `ADAPTED`.

use bevy::prelude::*;

pub mod comm;
pub mod fleet;
pub mod guide;
pub mod sim;
pub mod world;

use crate::{
    asset_catalog::ResourceIndex,
    cell_contract::CellGoalCard,
    cell_hud::ProgressColors,
    civ::{self, Approach, CivCampaign},
    civ_rts::CivUnit,
    game::RunProgress,
    spui::{
        model::{ImageRef, WinId},
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

pub const MAIN_LAYOUT: &str = "GlobalUICiv3-3";
pub const CARD_LAYOUT: &str = "CVGMissionCard";
pub const OPTIONS_LAYOUT: &str = crate::cell_hud::OPTIONS_LAYOUT;

/// Control ids of `GlobalUICiv3-3` (roles from the layout and its tooltips).
/// The list is the reference of the layout; some ids are only checked by the
/// tests.
#[allow(dead_code)]
pub mod id {
    // Bottom bar.
    /// Sporebucks readout: panel and text ("$99,999"; the tooltip "Sporebucks" is on the text window).
    pub const SPICE_PANEL: u32 = 0x051B_6DB0;
    pub const SPICE_TEXT: u32 = 0xABCD_1001;
    /// City-control progress bar and its two segments (cities 1-4: up to the
    /// air-vehicle milestone; cities 5-10: up to space travel).
    pub const BAR: u32 = 0x051B_5478;
    pub const SEGMENTS: [u32; 2] = [0x051B_54C0, 0x051B_54C1];
    /// One two-frame city icon per city, in order: four in the first
    /// segment, six in the second.
    pub const CITY_ICONS: [u32; 10] = [
        0x0681_BEE0,
        0x0681_BEE1,
        0x0681_BEE2,
        0x0681_BEE3,
        0x0681_BEE4,
        0x0681_BEE5,
        0x0681_BEE6,
        0x0681_BEE7,
        0x0681_BEE8,
        0x0681_BEE9,
    ];
    /// Icons per segment (the layout has four and six).
    pub const SEGMENT_ICONS: [usize; 2] = [4, 6];
    /// Milestones on the bar: air vehicles (tooltip "Control 4 cities to
    /// invent air vehicles") and space travel.
    pub const MILESTONE_AIR: u32 = 0x0643_D8B3;
    pub const MILESTONE_SPACE: u32 = 0x0643_D8DA;
    pub const ADVANCE: u32 = 0x0645_3746;
    pub const ADVANCE_GLOW: u32 = 0x0652_A628;
    pub const HISTORY: u32 = 0x0448_27B8;
    pub const PAUSE_TOGGLE: u32 = 0xD305_CA84;
    pub const SPOREPEDIA: u32 = 0x0624_4660;
    pub const COLLECTIONS: u32 = 0x0624_4208;
    // Consequence abilities: Cell, Creature, Tribe and the Civ superweapon.
    pub const POWERS: [u32; 4] = [0x05A5_5B10, 0x05A5_5B20, 0x05A5_5B30, 0x05A5_5B40];
    /// Locked variants ("Complete Cell Stage to unlock this ability", ...).
    pub const POWERS_LOCKED: [u32; 3] = [0x05A5_5B11, 0x05A5_5B21, 0x05A5_5B31];
    /// Blinking glow of a ready ability (a child of its button).
    pub const POWER_GLOWS: [u32; 4] = [0x064E_D160, 0x064E_D161, 0x064E_D162, 0x064E_D163];
    pub const POWER_FRAME: u32 = 0x04C5_AC08;
    // Vehicle stance toggles (red crosshair, blue shield) and their panel.
    pub const STANCE_AGGRESSIVE: u32 = 0x04C6_1DD8;
    pub const STANCE_STAND: u32 = 0x04C6_1B50;
    pub const STANCE_PANEL: u32 = 0x04C5_AC28;
    // Planet minimap.
    pub const MAP_TOGGLE: u32 = 0x0562_A0F7;
    pub const MAP_NAME: u32 = 0x0562_A2DA;
    pub const MAP_NAME_GROUP: u32 = 0x063B_F17D;
    pub const MAP_FRAME: u32 = 0x053F_4AD8;
    pub const MAP_HOLDER: u32 = 0x0190_722E;
    pub const MAP_HIDE: u32 = 0x0562_A141;
    pub const MAP_ZOOM_IN: u32 = 0x04CA_B570;
    pub const MAP_ZOOM_OUT: u32 = 0x04CA_B571;
    /// Rotate buttons (left, right).
    pub const MAP_ROTATE: [u32; 2] = [0x04CA_B56E, 0x04CA_B56F];
    /// Communication buttons of the nations, beside the minimap (panel
    /// `COMM_PANEL`) and in the compact strip that replaces them while the
    /// minimap is hidden (`COMM_STRIP`). Both panels use the same ids.
    pub const COMM_PANEL: u32 = 0x0469_77FF;
    pub const COMM_STRIP: u32 = 0x055D_626F;
    pub const COMM_BUTTONS: [u32; 4] = [0x0465_5FD5, 0x0465_5FD6, 0x0465_5FD7, 0x0465_5FD8];
    /// Relationship face (five frames) and mood icon of a comm button.
    pub const COMM_FACE: u32 = 0x046A_DF86;
    pub const COMM_EMOTION: u32 = 0x055D_5429;
    /// The group hidden while the communication screen is open.
    pub const BAR_GROUP: u32 = 0x063B_F17C;
    pub const COMM_SHOW_TOOLTIP: u32 = 0x0615_FFA7;
}

/// ADAPTED key of the Advance button (the stage ends on it once the planet is
/// unified, as the other stages do with F4).
pub const ADVANCE_KEY: KeyCode = KeyCode::F4;
/// Keys of the four consequence ability buttons (the existing Civ controls:
/// 7 Cell, 8 Creature, 9 Tribe, 0 superweapon).
pub const POWER_KEYS: [KeyCode; 4] = [KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9, KeyCode::Digit0];
/// Camera keys of the minimap frame's buttons (retail trigger config
/// TriggerConfigTribeGame, RE V6: `=` `-` zoom, `,` `.` rotate).
pub const ZOOM_KEYS: [KeyCode; 2] = [KeyCode::Equal, KeyCode::Minus];
pub const ROTATE_KEYS: [KeyCode; 2] = [KeyCode::Comma, KeyCode::Period];
/// Em size of the Sporebucks caption (the style says 14 pt but the 13 px high
/// caption window shows ~16 px digits, as in the Cell and Creature HUD).
const SPICE_TEXT_PX: f32 = 16.0;

/// Which layout a spawned root is.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CivHudRoot {
    Main,
    Orb,
    /// Goal card number `n` (top left, stacked).
    Card(u8),
    /// The four world cards.
    CityCard,
    VehicleCard,
    BuildingCard,
    SpiceCard,
    /// The communication screen and its relationship breakdown.
    Comm,
    /// Item `n` of the fleet list (right edge).
    FleetItem(u8),
    /// Hint bar and callout dialog.
    Hint,
    Callout,
}

/// True while the SPUI Civ HUD is shown; the legacy Civ HUD and the pause
/// panel step aside.
#[derive(Resource, Default)]
pub struct CivHudActive(pub bool);

/// HUD-only state that the game does not keep: the minimap's hide toggle, the
/// city card the player clicked on and the open communication screen. (The
/// stance toggles keep nothing here: a click sets the stance of the selected
/// vehicles and the buttons show the stances of the selection, 0xE05850 /
/// 0xE05910.)
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct CivHudLocal {
    /// The planet minimap is shown (the Hide toggle).
    pub minimap_open: bool,
    /// The city whose card (with its buy buttons) stays open.
    pub pinned_city: Option<usize>,
    /// The nation the communication screen is open with.
    pub comm: Option<comm::CommSession>,
    /// A click on the communication screen waiting for `comm::apply_comm_clicks`.
    pub comm_click: Option<comm::CommClick>,
}

impl Default for CivHudLocal {
    fn default() -> Self {
        Self { minimap_open: true, pinned_city: None, comm: None, comm_click: None }
    }
}

/// Which consequence ability a button is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerClass {
    Cell,
    Creature,
    Tribal,
    Superweapon,
}

/// One of the four consequence ability buttons.
#[derive(Clone, Debug, PartialEq)]
pub struct PowerSlot {
    pub class: PowerClass,
    /// Registered name of the retail power file (group `0x449505AF`):
    /// `cvg_clg_meat`, `cvg_crg_social`, `cvg_trg_attackwin`, ...
    pub prop: &'static str,
    /// The ability's name in the rewrite's enums (fallback for the tooltip).
    pub name: &'static str,
    /// The ability exists (Cell/Creature/Tribe: the stage's power was resolved;
    /// superweapon: the nation has grown enough and it was not launched yet).
    pub unlocked: bool,
    /// Seconds left of the cooldown and its total.
    pub cooldown: f32,
    pub cooldown_total: f32,
    /// What using it costs (the superweapon: 12,000 per city still to take).
    pub cost: f32,
    /// Why it cannot be used now (`CivCampaign::use_power`'s own answer:
    /// recharging, too expensive, locked), if it cannot.
    pub refused: Option<String>,
}

impl PowerSlot {
    pub fn ready(&self) -> bool {
        self.unlocked && self.refused.is_none()
    }
}

/// Retail power file for each ability of the rewrite's enums. The Cell
/// stage's diet picks the Cell power (carnivore, herbivore, omnivore), the
/// Creature stage's social outcome the Creature power and the Tribal
/// stage's the Tribal one; the superweapon is the Tribal "win" power of the
/// nation's doctrine (ICBM, Fanatical Uprising, Global Merger).
pub fn cell_prop(p: civ::CivCellPower) -> &'static str {
    match p {
        civ::CivCellPower::Invulnerability => "cvg_clg_meat",
        civ::CivCellPower::HealingAura => "cvg_clg_plant",
        civ::CivCellPower::StaticBomb => "cvg_clg_mix",
    }
}

pub fn creature_prop(p: civ::CivCreaturePower) -> &'static str {
    match p {
        civ::CivCreaturePower::MightyBomb => "cvg_crg_attack",
        civ::CivCreaturePower::DiploDervish => "cvg_crg_social",
        civ::CivCreaturePower::BribeBomb => "cvg_crg_mix",
    }
}

pub fn tribal_prop(p: civ::CivTribalPower) -> &'static str {
    match p {
        civ::CivTribalPower::GadgetBomb => "cvg_trg_attack",
        civ::CivTribalPower::BlackCloud => "cvg_trg_social",
        civ::CivTribalPower::AdBlitz => "cvg_trg_mix",
    }
}

pub fn superweapon_prop(p: civ::CivSuperweapon) -> &'static str {
    match p {
        civ::CivSuperweapon::Icbm => "cvg_trg_attackwin",
        civ::CivSuperweapon::FanaticalUprising => "cvg_trg_socialwin",
        civ::CivSuperweapon::GlobalMerger => "cvg_trg_mixwin",
    }
}

/// The four ability buttons from the campaign.
pub fn power_slots(c: &CivCampaign, money: f32) -> [PowerSlot; 4] {
    let cell = c.cell_power.unwrap_or(civ::CivCellPower::StaticBomb);
    let creature = c.creature_power.unwrap_or(civ::CivCreaturePower::BribeBomb);
    let tribal = c.tribal_power.unwrap_or(civ::CivTribalPower::AdBlitz);
    let weapon = civ::resolve_superweapon(c.approach);
    // Costs, recharge times and what blocks a use all come from the rules.
    let refusal = |slot| sim::power_refusal(c, slot, money);
    [
        PowerSlot {
            class: PowerClass::Cell,
            prop: cell_prop(cell),
            name: cell.name(),
            unlocked: c.power_unlocked(civ::PowerSlot::Cell),
            cooldown: c.power_cell_cooldown,
            cooldown_total: cell.cooldown(),
            cost: cell.cost(),
            refused: refusal(civ::PowerSlot::Cell),
        },
        PowerSlot {
            class: PowerClass::Creature,
            prop: creature_prop(creature),
            name: creature.name(),
            unlocked: c.power_unlocked(civ::PowerSlot::Creature),
            cooldown: c.power_creature_cooldown,
            cooldown_total: creature.cooldown(),
            cost: creature.cost(),
            refused: refusal(civ::PowerSlot::Creature),
        },
        PowerSlot {
            class: PowerClass::Tribal,
            prop: tribal_prop(tribal),
            name: tribal.name(),
            unlocked: c.power_unlocked(civ::PowerSlot::Tribal),
            cooldown: c.power_tribal_cooldown,
            cooldown_total: tribal.cooldown(),
            cost: tribal.cost(),
            refused: refusal(civ::PowerSlot::Tribal),
        },
        PowerSlot {
            class: PowerClass::Superweapon,
            prop: superweapon_prop(weapon),
            name: weapon.name(),
            unlocked: c.superweapon_unlocked(),
            cooldown: c.superweapon_cooldown,
            cooldown_total: weapon.cooldown(),
            cost: weapon.cost(c.rivals_left()),
            refused: refusal(civ::PowerSlot::Superweapon),
        },
    ]
}

/// A rival nation as the communication buttons show it.
#[derive(Clone, Debug, PartialEq)]
pub struct Rival {
    /// Campaign city index (also the nation's index).
    pub nation: usize,
    pub name: String,
    pub kind: Approach,
    pub at_war: bool,
    /// A trade route to the nation is open.
    pub trade: bool,
    /// Relationship with the player, `-1.0` (hate) to `1.0` (love).
    pub relation: f32,
}

/// Legacy stand-in from before the relationship ledger (`civ_rules`): the HUD
/// no longer calls it, every face now comes from the attitude level of the
/// ledger (`sim::nation_relation`, 0xBD8220 / 0xD00E60). Kept for its tests.
#[allow(dead_code)]
pub fn relation_of(at_war: bool, trade: bool, bought: f32) -> f32 {
    if at_war {
        -0.9
    } else if trade {
        (0.4 + 0.5 * bought).clamp(0.4, 0.9)
    } else {
        0.0
    }
}

/// Frame of the five-face relationship icon: angry (red), unhappy (orange),
/// neutral (yellow), content (blue), happy (green).
pub fn relation_frame(relation: f32) -> u32 {
    match relation {
        r if r <= -0.6 => 0,
        r if r <= -0.2 => 1,
        r if r < 0.2 => 2,
        r if r < 0.6 => 3,
        _ => 4,
    }
}

/// Sporebucks as the readout writes them: `$1,234`.
pub fn money_text(spice: f32) -> String {
    let n = if spice.is_finite() { spice.max(0.0).floor() as u64 } else { 0 };
    let digits = n.to_string();
    let mut out = String::from("$");
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Fill of the two progress segments and the number of lit city icons in
/// each. The first segment runs from no city to the air-vehicle milestone
/// (`air` cities), the second from there to the stage goal (`goal`); retail
/// has 4 and 10, the rewrite's numbers come from the campaign.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProgressFill {
    pub segments: [f32; 2],
    pub lit: [usize; 2],
}

pub fn progress_fill(owned: usize, air: usize, goal: usize) -> ProgressFill {
    let goal = goal.max(2);
    let air = air.clamp(1, goal - 1);
    let owned = owned.min(goal);
    let first = (owned as f32 / air as f32).clamp(0.0, 1.0);
    let second = (owned.saturating_sub(air) as f32 / (goal - air) as f32).clamp(0.0, 1.0);
    let lit = |fill: f32, icons: usize| ((fill * icons as f32) + 1e-3).floor() as usize;
    ProgressFill {
        segments: [first, second],
        lit: [lit(first, id::SEGMENT_ICONS[0]).min(id::SEGMENT_ICONS[0]), lit(second, id::SEGMENT_ICONS[1]).min(id::SEGMENT_ICONS[1])],
    }
}

/// Everything the Civ HUD displays, filled by [`publish_state`] (and by
/// [`world`], [`guide`] and [`comm`] for their parts) so the HUD never reads
/// simulation internals itself.
#[derive(Resource, Clone, Debug, Default)]
pub struct CivHudState {
    /// Sporebucks (the stockpile).
    pub spice: f32,
    /// Cities the nation controls, the cities that end the stage, and the
    /// city counts that unlock air and sea vehicles.
    pub owned: usize,
    pub goal_cities: usize,
    pub air_cities: usize,
    pub sea_cities: usize,
    pub air_unlocked: bool,
    pub sea_unlocked: bool,
    /// The whole planet is the nation's: Advance replaces History.
    pub advance_visible: bool,
    pub powers: [Option<PowerSlot>; 4],
    /// The stance toggles (0xE05910): the red Aggressive button is checked while
    /// a selected vehicle is aggressive, the blue Stand Ground one while a
    /// selected vehicle holds its ground (both when the selection is mixed,
    /// neither with nothing selected).
    pub stance_aggressive: bool,
    pub stance_stand: bool,
    /// The home planet's name (the minimap header).
    pub planet_name: String,
    /// The ability button whose tool is armed (it shows checked until fired or dropped).
    pub armed: Option<usize>,
    pub rivals: Vec<Rival>,
    /// The options menu is open (the game is paused).
    pub paused: bool,
    /// The capital's doctrine (what the nation's cities build).
    pub doctrine: Approach,
    /// Last line of feedback of the campaign (the hint bar shows it).
    pub message: String,
    /// Goal cards in display order (retail strings, see [`guide::goal_cards`]).
    pub goal_cards: Vec<CellGoalCard>,
    /// Callouts to show, front first (`CalloutCVGIntro`, ...).
    pub callouts: Vec<String>,
    /// Active hint: a retail hint id (`cvg-selectunit`) or `msg:<text>`.
    pub hint: Option<String>,
    /// The cards and the minimap picture of the world.
    pub world: world::WorldState,
    /// The player's vehicles (the first [`fleet::MAX_ITEMS`]) and their number.
    pub fleet: Vec<fleet::FleetItem>,
    pub fleet_total: usize,
    /// The communication screen, when open.
    pub comm: Option<comm::CommState>,
}

/// Seconds-per-cycle income in spice per minute, as the retail HUD shows its
/// city rows ("99999/min").
pub fn per_minute(per_cycle: f32) -> f32 {
    per_cycle * 60.0 / civ::INCOME_PERIOD
}

pub struct CivHudPlugin;

impl Plugin for CivHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CivHudState>()
            .init_resource::<CivHudActive>()
            .init_resource::<CivHudLocal>()
            .init_resource::<guide::GuideState>()
            .init_resource::<guide::GuideCache>()
            .init_resource::<guide::PowerDefs>()
            .init_resource::<fleet::FleetIcons>()
            .init_resource::<world::MinimapState>()
            .init_resource::<comm::CommCache>()
            .init_resource::<crate::civ_comm::CivComm>()
            .add_systems(
                Update,
                (
                    publish_state,
                    fleet::publish_fleet,
                    world::minimap_pointer,
                    world::publish_world,
                    guide::publish_guide,
                    comm::publish_comm,
                    demo_state,
                    manage,
                    guide::manage_cards,
                    fleet::manage_fleet,
                    comm::manage_comm,
                    bind,
                    world::bind_world,
                    guide::bind_guide,
                    fleet::bind_fleet,
                    comm::bind_comm,
                )
                    .chain()
                    .before(view::sync)
                    .after(view::input),
            )
            // The minimap picture sits over the holder window once the HUD
            // has been laid out.
            .add_systems(Update, (world::paint_minimap, world::place_minimap).chain().after(view::sync).after(manage))
            // The conversation's response rows (0xDD3E60) over the comm
            // screen, and the clicks they and its buttons queue.
            .add_systems(Update, (comm::manage_rows, comm::update_rows).chain().after(view::sync))
            .add_systems(Update, comm::apply_comm_clicks.before(comm::publish_comm).after(view::input))
            // Clicks recorded by the SPUI input of the last frame become key
            // presses and commands before the controls (and then the
            // simulation) read the keyboard: ahead of the first system of the
            // controls chain.
            .add_systems(
                Update,
                (actions, fleet::fleet_clicks)
                    .in_set(crate::GameSystems::Controls)
                    .before(crate::player_hud::actions)
                    .run_if(in_state(GameStage::Civilization)),
            );
    }
}

/// Flags of the capture demo.
struct DemoFlags {
    on: bool,
    paused: bool,
    /// Open the communication screen with this nation.
    comm: Option<usize>,
    /// Keep this city's card (with the buy buttons) open.
    pin: Option<usize>,
    /// Start with the minimap hidden (the compact comm strip shows).
    map_hidden: bool,
}

impl DemoFlags {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let number = |flag: &str| {
            args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<usize>().ok())
        };
        Self {
            on: has("--civ-hud-demo"),
            paused: has("--civ-hud-paused"),
            comm: number("--civ-hud-comm"),
            pin: number("--civ-hud-pin"),
            map_hidden: has("--civ-hud-map-hidden"),
        }
    }
}

/// `--civ-hud-demo` (captures): replace what the game published with a
/// mid-game HUD state so that every part of the layouts can be looked at:
/// partly filled progress bar, abilities with a cooldown, four nations.
/// `--civ-hud-paused` also opens the options menu, `--civ-hud-comm N` opens
/// the communication screen with nation N and `--civ-hud-pin N` keeps the
/// card of city N open (these two work on the real state too).
fn demo_state(
    stage: Res<State<GameStage>>,
    mut demo: Local<Option<DemoFlags>>,
    mut hud: ResMut<CivHudState>,
    mut local: ResMut<CivHudLocal>,
) {
    let flags = demo.get_or_insert_with(DemoFlags::from_args);
    if *stage.get() != GameStage::Civilization {
        return;
    }
    if flags.map_hidden {
        local.minimap_open = false;
    }
    if let (Some(n), true) = (flags.pin, local.pinned_city.is_none()) {
        local.pinned_city = Some(n);
    }
    if let (Some(n), true) = (flags.comm, local.comm.is_none()) {
        local.comm = Some(comm::CommSession { nation: n, menu: comm::CommMenu::Root });
    }
    if !flags.on {
        return;
    }
    hud.spice = 12_345.0;
    hud.owned = 3;
    hud.goal_cities = civ::CITY_COUNT;
    hud.air_cities = civ::AIR_UNLOCK_CITIES;
    hud.sea_cities = civ::SEA_UNLOCK_CITIES;
    hud.air_unlocked = true;
    hud.sea_unlocked = true;
    let slot = |class, prop, name, cooldown: f32, total, cost| PowerSlot {
        class,
        prop,
        name,
        unlocked: true,
        cooldown,
        cooldown_total: total,
        cost,
        refused: (cooldown > 0.0).then(|| format!("{name} recharging ({cooldown:.0}s)")),
    };
    hud.powers = [
        Some(slot(PowerClass::Cell, "cvg_clg_meat", "Invulnerability", 0.0, 60.0, 2000.0)),
        Some(slot(PowerClass::Creature, "cvg_crg_social", "Diplo Dervish", 42.0, 180.0, 4000.0)),
        Some(slot(PowerClass::Tribal, "cvg_trg_mix", "Ad Blitz", 0.0, 300.0, 16000.0)),
        Some(slot(PowerClass::Superweapon, "cvg_trg_attackwin", "ICBM", 0.0, 0.0, 24000.0)),
    ];
    hud.rivals = (1..civ::CITY_COUNT)
        .map(|nation| Rival {
            nation,
            name: civ::city_label(nation),
            kind: civ::RIVAL_KINDS[(nation - 1) % civ::RIVAL_COUNT],
            at_war: nation == 1,
            trade: nation == 3,
            relation: match nation {
                1 => -0.9,
                3 => 0.7,
                2 => 0.3,
                _ => 0.0,
            },
        })
        .collect();
    hud.paused = flags.paused;
}

/// Fill [`CivHudState`] from the campaign and the live vehicles.
fn publish_state(
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    pause: Res<crate::pause_menu::PauseMenu>,
    rts: Option<Res<crate::civ_rts::RtsState>>,
    selected: Query<Option<&crate::civ_controls::CivStance>, (With<crate::civ_rts::Selected>, With<CivUnit>)>,
    originals: Option<Res<crate::Originals>>,
    mut hud: ResMut<CivHudState>,
) {
    if *stage.get() != GameStage::Civilization {
        return;
    }
    let c = &run.civ_campaign;
    let (aggressive, stand) = stance_flags(selected.iter().map(|s| s.copied().unwrap_or_default()));
    hud.stance_aggressive = aggressive;
    hud.stance_stand = stand;
    // The minimap header names the home planet like the Creature, Tribal and
    // Space HUDs do (`land_minimap::planet_name`).
    if hud.planet_name.is_empty() {
        if let Some(originals) = originals {
            hud.planet_name = crate::land_minimap::planet_name(&originals);
        }
    }
    hud.armed = rts.as_ref().and_then(|r| r.armed).map(|slot| match slot {
        civ::PowerSlot::Cell => 0,
        civ::PowerSlot::Creature => 1,
        civ::PowerSlot::Tribal => 2,
        civ::PowerSlot::Superweapon => 3,
    });
    hud.spice = sim::money(&run);
    hud.owned = c.owned_count();
    hud.goal_cities = sim::goal_cities();
    hud.air_cities = sim::air_cities();
    hud.sea_cities = sim::sea_cities();
    hud.air_unlocked = c.air_unlocked();
    hud.sea_unlocked = c.sea_unlocked();
    hud.advance_visible = c.is_unified();
    hud.powers = power_slots(c, sim::money(&run)).map(Some);
    hud.doctrine = c.approach;
    hud.message = c.message.clone();
    hud.rivals = (1..c.cities.len().min(civ::CITY_COUNT))
        .map(|nation| {
            let city = &c.cities[nation];
            Rival {
                nation,
                name: civ::city_label(nation),
                kind: city.kind,
                at_war: c.at_war(nation),
                trade: city.trade,
                relation: sim::nation_relation(c, nation),
            }
        })
        // A taken city is no longer a nation of its own.
        .filter(|r| c.nation_alive(r.nation))
        .collect();
    hud.paused = pause.open;
}

/// VERIFIED (0xE05910, the selection update of the stance toggles): the
/// Aggressive button is checked when some selected vehicle's stance is 0 (attack) and
/// the Stand Ground one when some selected vehicle's stance is 1 (defence); with
/// nothing selected both are off.
pub fn stance_flags(stances: impl Iterator<Item = crate::civ_controls::CivStance>) -> (bool, bool) {
    use crate::civ_controls::CivStance;
    let (mut aggressive, mut stand) = (false, false);
    for stance in stances {
        match stance {
            CivStance::Aggressive => aggressive = true,
            CivStance::StandGround => stand = true,
        }
    }
    (aggressive, stand)
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
    mut active: ResMut<CivHudActive>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &CivHudRoot)>,
    planner: Res<crate::civ_planner::CivPlanner>,
) {
    // The City Planner screen (`civ_planner`) replaces the HUD while open.
    let want = *stage.get() == GameStage::Civilization && !editor.open && !options.diagnostics && !planner.is_open();
    if !want {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        // The legacy Civ HUD stays hidden under the planner.
        active.0 = *stage.get() == GameStage::Civilization && planner.is_open() && !editor.open && !options.diagnostics;
        return;
    }
    if roots.iter().any(|(_, r)| *r == CivHudRoot::Main) {
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
    let spawn = |commands: &mut Commands, assets: &mut SpuiAssets, name: &str, kind, policy, z| {
        if let Some(layout) = assets.layout(index, name) {
            let root = view::spawn(commands, layout, policy, Some(camera), z);
            commands.entity(root).insert(kind);
        }
    };
    // The world cards first so that the bottom bar stays above them.
    spawn(&mut commands, &mut assets, world::BUILDING_LAYOUT, CivHudRoot::BuildingCard, RootPolicy::Authored, 97);
    spawn(&mut commands, &mut assets, world::VEHICLE_LAYOUT, CivHudRoot::VehicleCard, RootPolicy::Authored, 97);
    spawn(&mut commands, &mut assets, world::SPICE_LAYOUT, CivHudRoot::SpiceCard, RootPolicy::Authored, 97);
    spawn(&mut commands, &mut assets, world::CITY_LAYOUT, CivHudRoot::CityCard, RootPolicy::Authored, 98);
    let root = view::spawn(&mut commands, main, RootPolicy::Fit, Some(camera), 100);
    commands.entity(root).insert(CivHudRoot::Main);
    spawn(&mut commands, &mut assets, OPTIONS_LAYOUT, CivHudRoot::Orb, RootPolicy::Fit, 101);
    // The hint bar and the callout dialog stay spawned (hidden) so that their
    // pop-in effects run when they are shown.
    spawn(&mut commands, &mut assets, crate::cell_hud::dialogs::HINT_LAYOUT, CivHudRoot::Hint, RootPolicy::Fit, 102);
    spawn(
        &mut commands,
        &mut assets,
        crate::cell_hud::dialogs::CALLOUT_LAYOUT,
        CivHudRoot::Callout,
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

/// The image of a whole installed PNG, ready for `set_image` / `set_icon`
/// (`None` when it cannot be read).
pub(crate) fn picture(
    key: crate::dbpf::ResourceKey,
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) -> Option<ImageRef> {
    assets.texture(index, images, key).map(|_| ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None })
}

/// Tooltip of a consequence ability: its retail name, its description, its
/// price and, while it cannot be used, the rules' reason.
pub fn power_tooltip(name: &str, description: &str, cost: f32, refused: Option<&str>) -> String {
    let mut text = name.to_string();
    if !description.is_empty() {
        text.push('\n');
        text.push_str(description);
    }
    text.push('\n');
    text.push_str(&money_text(cost));
    if let Some(reason) = refused {
        text.push('\n');
        text.push_str(reason);
    }
    text
}

/// Icon colour while an ability recharges. VERIFIED (0xE060A0, the ability button update): the
/// retail code only sets the button's visible flag (a power is bound), its enabled flag
/// (0xBF8B40, "can be used") and the blink; it never tints the icon, so a recharging ability
/// is the plain icon of a disabled button (white: no tint), without the blink.
const COOLDOWN_TINT: u32 = 0xFFFF_FFFF;

/// Bind the four consequence ability buttons.
#[allow(clippy::too_many_arguments)]
fn bind_powers(
    ui: &mut SpuiState,
    hud: &CivHudState,
    defs: &mut guide::PowerDefs,
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) {
    for (i, slot) in hud.powers.iter().enumerate() {
        let (Some(button), Some(glow)) = (window(ui, id::POWERS[i]), window(ui, id::POWER_GLOWS[i])) else {
            continue;
        };
        let locked = id::POWERS_LOCKED.get(i).and_then(|c| window(ui, *c));
        let Some(slot) = slot else {
            ui.set_visible(button, false);
            if let Some(l) = locked {
                ui.set_visible(l, false);
            }
            continue;
        };
        // The three stage abilities show their locked variant until their
        // stage is completed; the superweapon is simply absent until the
        // nation has grown enough.
        let show_button = slot.unlocked;
        ui.set_visible(button, show_button);
        if let Some(l) = locked {
            ui.set_visible(l, !slot.unlocked);
        }
        if !show_button {
            continue;
        }
        let def = defs.get(slot.prop, assets, index);
        let icon = def.icon.and_then(|key| picture(key, assets, images, index));
        ui.set_icon(button, icon);
        ui.set_tint(button, if slot.ready() { 0xFFFF_FFFF } else { COOLDOWN_TINT });
        ui.set_enabled(button, slot.ready());
        // A press arms the tool (`civ_target`): the toggle stays checked until
        // the tool is fired or dropped.
        ui.set_selected(button, hud.armed == Some(i));
        ui.set_visible(glow, slot.ready());
        let name = if def.name.is_empty() { slot.name } else { def.name.as_str() };
        ui.set_tooltip(button, Some(power_tooltip(name, &def.description, slot.cost, slot.refused.as_deref())));
    }
    // The frame behind the four buttons appears with the first of them.
    let any = hud.powers.iter().flatten().any(|s| s.unlocked);
    set_visible(ui, id::POWER_FRAME, any);
}

/// Tooltip of the air and space milestones: the retail strings with the
/// rewrite's city counts (the retail data has 4 and 10).
pub fn milestone_text(retail: &str, retail_count: usize, count: usize) -> String {
    retail.replacen(&retail_count.to_string(), &count.to_string(), 1)
}

#[allow(clippy::too_many_arguments)]
fn bind_main(
    ui: &mut SpuiState,
    hud: &CivHudState,
    local: &CivHudLocal,
    colors: &ProgressColors,
    defs: &mut guide::PowerDefs,
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) {
    let comm_open = hud.comm.is_some();
    if let Some(w) = window(ui, id::SPICE_TEXT) {
        ui.set_text(w, money_text(hud.spice));
        ui.set_text_size(w, Some(SPICE_TEXT_PX));
    }
    if let Some(w) = window(ui, id::PAUSE_TOGGLE) {
        ui.set_selected(w, hud.paused);
    }
    // Advance replaces History once the planet is the nation's.
    for control in [id::ADVANCE, id::ADVANCE_GLOW] {
        set_visible(ui, control, hud.advance_visible);
    }
    set_visible(ui, id::HISTORY, !hud.advance_visible);
    // Progress: the nation's cities along two segments.
    let fill = progress_fill(hud.owned, hud.air_cities, hud.goal_cities);
    for (i, control) in id::SEGMENTS.iter().enumerate() {
        let Some(w) = window(ui, *control) else {
            continue;
        };
        ui.set_shade(w, colors.right);
        ui.set_overlay(w, (fill.segments[i] > 0.0).then_some((fill.segments[i], colors.left)));
    }
    for (i, control) in id::CITY_ICONS.iter().enumerate() {
        let (segment, at) = if i < id::SEGMENT_ICONS[0] { (0, i) } else { (1, i - id::SEGMENT_ICONS[0]) };
        if let Some(w) = window(ui, *control) {
            ui.set_frame(w, Some(u32::from(at < fill.lit[segment])));
        }
    }
    if let Some(w) = window(ui, id::MILESTONE_AIR) {
        let text = ui_text(assets, index, "tooltips", 0x04CA_C3C5);
        ui.set_tooltip(w, Some(milestone_text(&text, 4, hud.air_cities)).filter(|_| !hud.air_unlocked));
    }
    if let Some(w) = window(ui, id::MILESTONE_SPACE) {
        let text = ui_text(assets, index, "tooltips", 0xF47C_278D);
        ui.set_tooltip(w, Some(milestone_text(&text, 10, hud.goal_cities)));
    }
    bind_powers(ui, hud, defs, assets, images, index);
    // Vehicle stance toggles.
    if let Some(w) = window(ui, id::STANCE_AGGRESSIVE) {
        ui.set_selected(w, hud.stance_aggressive);
    }
    if let Some(w) = window(ui, id::STANCE_STAND) {
        ui.set_selected(w, hud.stance_stand);
    }
    // The communication screen takes the HUD's upper parts away (the retail
    // update hides the group `063BF17C`/`063BF17D` while it is open).
    let shown = !comm_open;
    for control in [id::BAR_GROUP, id::MAP_NAME_GROUP] {
        set_visible(ui, control, shown);
    }
    for control in id::POWERS.iter().chain(&id::POWERS_LOCKED).chain(&[id::STANCE_AGGRESSIVE, id::STANCE_STAND, id::STANCE_PANEL]) {
        if !shown {
            set_visible(ui, *control, false);
        }
    }
    if !shown {
        set_visible(ui, id::POWER_FRAME, false);
    }
    world::bind_minimap_frame(ui, hud, local);
}

/// A string of a locale table by its name.
pub fn ui_text(assets: &mut SpuiAssets, index: &ResourceIndex, table: &str, id: u32) -> String {
    assets.localized(index, crate::world_objects::name_id(table), id).unwrap_or_default()
}

/// Push [`CivHudState`] into the layouts.
#[allow(clippy::too_many_arguments)]
fn bind(
    hud: Res<CivHudState>,
    local: Res<CivHudLocal>,
    originals: Res<crate::Originals>,
    colors: Res<ProgressColors>,
    mut defs: ResMut<guide::PowerDefs>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut roots: Query<(&mut SpuiState, &CivHudRoot)>,
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
            CivHudRoot::Main => bind_main(&mut ui, &hud, &local, &colors, &mut defs, &mut assets, &mut images, index),
            // The orb and its menu are the Cell stage's (the menu is shown
            // while the game is paused through it).
            CivHudRoot::Orb => crate::cell_hud::bind_orb(
                &mut ui,
                &crate::cell_contract::CellHudState { paused: hud.paused, ..default() },
            ),
            _ => {}
        }
    }
}

/// Button clicks become the keyboard actions and commands the Civ modules
/// already have. Runs before the simulation (`Controls`) on the clicks the
/// SPUI input recorded in the previous frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn actions(
    hud: Res<CivHudState>,
    mut local: ResMut<CivHudLocal>,
    mut roots: Query<(&mut SpuiState, &CivHudRoot)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<crate::player_hud::InjectedKeys>,
    mut run: ResMut<RunProgress>,
    mut pause: ResMut<crate::pause_menu::PauseMenu>,
    mut front_menu: MessageWriter<crate::front_end::MenuCommand>,
    mut spawns: ResMut<crate::civ_rts::UnitSpawns>,
    units: Query<&CivUnit>,
    mut planner: ResMut<crate::civ_planner::CivPlanner>,
    selected: Query<Entity, (With<crate::civ_rts::Selected>, With<CivUnit>)>,
    mut commands: Commands,
) {
    use crate::cell_hud::id as menu;
    // A key the HUD presses stays down for the frame; `player_hud::release`
    // lets it go at the end of it.
    let mut press = |keys: &mut ButtonInput<KeyCode>, key: KeyCode| {
        if !keys.pressed(key) {
            keys.press(key);
            injected.push(key);
        }
    };
    for (mut ui, kind) in &mut roots {
        // The clicks of the other roots are handled by `world`, `comm` and
        // `guide`.
        if !matches!(kind, CivHudRoot::Main | CivHudRoot::Orb) {
            continue;
        }
        // Zoom and rotate are held: their key stays down while the button is
        // pressed.
        if *kind == CivHudRoot::Main {
            let held = |ui: &SpuiState, control: u32| {
                ui.find(control).is_some_and(|w| ui.states[w] & crate::spui::ids::state::CLICKED != 0)
            };
            for (control, key) in [(id::MAP_ZOOM_IN, ZOOM_KEYS[0]), (id::MAP_ZOOM_OUT, ZOOM_KEYS[1])]
                .into_iter()
                .chain([(id::MAP_ROTATE[0], ROTATE_KEYS[0]), (id::MAP_ROTATE[1], ROTATE_KEYS[1])])
            {
                if held(&ui, control) {
                    press(&mut keys, key);
                }
            }
        }
        for click in ui.take_clicks() {
            let control = ui.layout.window(click).control_id;
            // The options menu and Sporepedia are the front end's.
            if let Some(command) = crate::front_end::menu_command(control) {
                front_menu.write(command);
                continue;
            }
            match control {
                id::ADVANCE => press(&mut keys, ADVANCE_KEY),
                // VERIFIED (0xE05C45 -> 0xE05850): the stance buttons set the stance
                // of every selected vehicle (0xC9FC00(stance, silent 0): stance 0
                // attack plays its voice, 1 defence); the voices module hears the change.
                id::STANCE_AGGRESSIVE | id::STANCE_STAND => {
                    let stance = if control == id::STANCE_AGGRESSIVE {
                        crate::civ_controls::CivStance::Aggressive
                    } else {
                        crate::civ_controls::CivStance::StandGround
                    };
                    for entity in &selected {
                        commands.entity(entity).insert(stance);
                    }
                }
                id::MAP_HIDE => local.minimap_open = !local.minimap_open,
                // The pause menu is opened directly.
                id::PAUSE_TOGGLE | menu::OPTIONS_ORB => pause.open = !pause.open,
                c => {
                    if let Some(slot) = id::POWERS.iter().position(|v| *v == c) {
                        if hud.powers[slot].as_ref().is_some_and(PowerSlot::ready) {
                            press(&mut keys, POWER_KEYS[slot]);
                        }
                    } else if let Some(slot) = id::COMM_BUTTONS.iter().position(|v| *v == c) {
                        if let Some(rival) = hud.rivals.get(slot) {
                            local.comm = Some(comm::CommSession { nation: rival.nation, menu: comm::CommMenu::Root });
                        }
                    }
                    // Sporepedia, My Collections and History have no
                    // counterpart in the rewrite yet; the minimap's own
                    // zoom and rotate buttons are handled above.
                }
            }
        }
    }
    // City card purchases and the communication screen.
    world::card_actions(&hud, &mut local, &mut roots, &mut planner, &mut run, &mut spawns, &units);
    comm::comm_actions(&hud, &mut local, &mut roots, &mut run);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_has_a_dollar_sign_and_thousands_separators() {
        assert_eq!(money_text(0.0), "$0");
        assert_eq!(money_text(999.9), "$999");
        assert_eq!(money_text(1000.0), "$1,000");
        assert_eq!(money_text(12_345.6), "$12,345");
        assert_eq!(money_text(1_234_567.0), "$1,234,567");
        assert_eq!(money_text(-5.0), "$0");
        assert_eq!(money_text(f32::NAN), "$0");
    }

    #[test]
    fn progress_runs_through_two_segments() {
        // Retail numbers: the first segment ends at 4 cities, the bar at 10.
        let at = |owned| progress_fill(owned, 4, 10);
        assert_eq!(at(0), ProgressFill { segments: [0.0, 0.0], lit: [0, 0] });
        assert_eq!(at(1).lit, [1, 0]);
        assert_eq!(at(4), ProgressFill { segments: [1.0, 0.0], lit: [4, 0] });
        assert_eq!(at(7).lit, [4, 3]);
        assert_eq!(at(10), ProgressFill { segments: [1.0, 1.0], lit: [4, 6] });
        assert_eq!(at(50).lit, [4, 6], "more cities than the bar holds");
        // A smaller planet (the campaign now has retail's ten): five cities,
        // aircraft at three.
        let own = |owned| progress_fill(owned, 3, 5);
        assert_eq!(own(1).segments[0], 1.0 / 3.0);
        assert_eq!(own(1).lit, [1, 0]);
        assert_eq!(own(3).lit, [4, 0]);
        assert_eq!(own(4).segments, [1.0, 0.5]);
        assert_eq!(own(4).lit, [4, 3]);
        assert_eq!(own(5).lit, [4, 6]);
        // Degenerate goals never divide by zero.
        assert_eq!(progress_fill(0, 0, 0).segments, [0.0, 0.0]);
        assert_eq!(progress_fill(3, 9, 1).segments[0], 1.0);
    }

    #[test]
    fn relations_pick_one_of_the_five_faces() {
        assert_eq!(relation_frame(-1.0), 0);
        assert_eq!(relation_frame(-0.4), 1);
        assert_eq!(relation_frame(0.0), 2);
        assert_eq!(relation_frame(0.4), 3);
        assert_eq!(relation_frame(1.0), 4);
        let faces: std::collections::HashSet<_> = [-1.0, -0.4, 0.0, 0.4, 1.0].map(relation_frame).into_iter().collect();
        assert_eq!(faces.len(), 5);
    }

    #[test]
    fn a_war_is_hostile_and_a_trade_route_friendly() {
        assert!(relation_of(true, true, 1.0) < -0.5, "war wins over trade");
        assert_eq!(relation_frame(relation_of(true, false, 0.0)), 0);
        assert_eq!(relation_frame(relation_of(false, false, 0.0)), 2);
        assert!(relation_of(false, true, 0.0) > 0.0);
        assert!(relation_of(false, true, 1.0) > relation_of(false, true, 0.0), "more bought, warmer");
        assert!(relation_frame(relation_of(false, true, 1.0)) >= 3);
    }

    /// A campaign whose stage powers have been resolved (the campaign update
    /// does it once a frame in play).
    fn campaign_with_powers() -> CivCampaign {
        let mut c = CivCampaign::default();
        c.ensure_cities();
        let (cell, creature, tribal) = civ::resolve_consequence_powers(0.0, 0.0, 0, 0, 0, 0);
        c.cell_power = Some(cell);
        c.creature_power = Some(creature);
        c.tribal_power = Some(tribal);
        c
    }

    #[test]
    fn the_four_buttons_follow_the_campaign_and_press_the_power_keys() {
        let mut c = campaign_with_powers();
        let rich = 1.0e6;
        // Two cities: the Cell and Creature abilities are open (VERIFIED 0xBF2510: 1 / 2 / 4 cities).
        c.cities[1].owner = civ::CityOwner::Player;
        let slots = power_slots(&c, rich);
        assert_eq!(slots.clone().map(|s| s.class), [PowerClass::Cell, PowerClass::Creature, PowerClass::Tribal, PowerClass::Superweapon]);
        assert!(slots[0].ready() && slots[1].ready());
        assert!(!slots[2].unlocked && !slots[2].ready(), "the Tribal ability needs four cities");
        assert!(!slots[3].unlocked && !slots[3].ready(), "the superweapon needs a grown nation");
        // The prices are the rules' (retail 2000 / 4000 / 16000).
        assert_eq!(slots[0].cost, c.cell_power.unwrap().cost());
        assert_eq!(slots[1].cost, c.creature_power.unwrap().cost());
        assert_eq!(slots[2].cost, c.tribal_power.unwrap().cost());
        assert!(slots[0].cost >= 2000.0 && slots[2].cost > slots[1].cost);
        // Too poor: unlocked but refused with the rule's message.
        let poor = power_slots(&c, 100.0);
        assert!(poor[0].unlocked && !poor[0].ready());
        assert!(poor[0].refused.as_deref().is_some_and(|m| m.contains("costs")), "{:?}", poor[0].refused);
        // A grown nation (six cities, VERIFIED 0xBEFBA0): the superweapon costs
        // per city still to take.
        for i in 1..6 {
            c.cities[i].owner = civ::CityOwner::Player;
        }
        let grown = power_slots(&c, rich);
        assert!(grown[3].unlocked && grown[3].ready(), "{:?}", grown[3].refused);
        assert_eq!(grown[3].cost, civ::resolve_superweapon(c.approach).cost(c.rivals_left()));
        c.power_creature_cooldown = 12.0;
        let slots = power_slots(&c, rich);
        assert!(!slots[1].ready() && slots[1].cooldown == 12.0);
        assert!(slots[1].refused.as_deref().is_some_and(|m| m.contains("recharging")));
        // The keys are the ones the Civ controls read (7 8 9 0).
        assert_eq!(POWER_KEYS, [KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9, KeyCode::Digit0]);
    }

    #[test]
    fn powers_map_to_retail_power_files() {
        use civ::*;
        let props = [
            cell_prop(CivCellPower::Invulnerability),
            cell_prop(CivCellPower::HealingAura),
            cell_prop(CivCellPower::StaticBomb),
            creature_prop(CivCreaturePower::MightyBomb),
            creature_prop(CivCreaturePower::DiploDervish),
            creature_prop(CivCreaturePower::BribeBomb),
            tribal_prop(CivTribalPower::GadgetBomb),
            tribal_prop(CivTribalPower::BlackCloud),
            tribal_prop(CivTribalPower::AdBlitz),
            superweapon_prop(CivSuperweapon::Icbm),
            superweapon_prop(CivSuperweapon::FanaticalUprising),
            superweapon_prop(CivSuperweapon::GlobalMerger),
        ];
        let unique: std::collections::HashSet<_> = props.iter().collect();
        assert_eq!(unique.len(), 12, "twelve distinct retail powers");
        assert!(props.iter().all(|p| p.starts_with("cvg_")));
    }

    #[test]
    fn the_superweapon_is_the_doctrines_win_power() {
        let mut c = CivCampaign::default();
        c.approach = Approach::Religious;
        assert_eq!(power_slots(&c, 0.0)[3].prop, "cvg_trg_socialwin");
        c.approach = Approach::Economic;
        assert_eq!(power_slots(&c, 0.0)[3].prop, "cvg_trg_mixwin");
        c.approach = Approach::Military;
        assert_eq!(power_slots(&c, 0.0)[3].prop, "cvg_trg_attackwin");
    }

    #[test]
    fn ability_tooltips_name_describe_and_count_down() {
        assert_eq!(power_tooltip("Mighty Bomb", "Hits everything.", 4000.0, None), "Mighty Bomb\nHits everything.\n$4,000");
        assert_eq!(
            power_tooltip("Mighty Bomb", "Hits everything.", 4000.0, Some("Mighty Bomb recharging (42s)")),
            "Mighty Bomb\nHits everything.\n$4,000\nMighty Bomb recharging (42s)"
        );
        assert_eq!(power_tooltip("Mighty Bomb", "", 0.0, None), "Mighty Bomb\n$0");
    }

    #[test]
    fn milestone_tooltips_carry_the_rewrites_city_counts() {
        assert_eq!(
            milestone_text("Control 4 cities to invent air vehicles", 4, 3),
            "Control 3 cities to invent air vehicles"
        );
        assert_eq!(
            milestone_text("Control 10 cities to invent space travel", 10, 5),
            "Control 5 cities to invent space travel"
        );
    }

    #[test]
    fn per_minute_scales_the_income_cycle() {
        // One cycle's income, taken every INCOME_PERIOD seconds, per minute.
        assert!((per_minute(civ::INCOME_PERIOD) - 60.0).abs() < 1e-4);
        assert!((per_minute(40.0) - 40.0 * 60.0 / civ::INCOME_PERIOD).abs() < 1e-3);
        assert_eq!(per_minute(0.0), 0.0);
    }

    // ---- installed data and bindings ---------------------------------

    fn installed() -> Option<(ResourceIndex, SpuiAssets)> {
        let install = crate::spore_install::discover().ok()?;
        let index = ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then(|| (index, SpuiAssets::default()))
    }

    #[test]
    fn installed_main_layout_has_every_control_the_binding_uses() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, MAIN_LAYOUT).expect("GlobalUICiv3-3");
        let single = [
            id::SPICE_PANEL,
            id::SPICE_TEXT,
            id::BAR,
            id::MILESTONE_AIR,
            id::MILESTONE_SPACE,
            id::ADVANCE,
            id::ADVANCE_GLOW,
            id::HISTORY,
            id::PAUSE_TOGGLE,
            id::SPOREPEDIA,
            id::COLLECTIONS,
            id::POWER_FRAME,
            id::STANCE_AGGRESSIVE,
            id::STANCE_STAND,
            id::STANCE_PANEL,
            id::MAP_TOGGLE,
            id::MAP_NAME,
            id::MAP_NAME_GROUP,
            id::MAP_FRAME,
            id::MAP_HOLDER,
            id::MAP_HIDE,
            id::MAP_ZOOM_IN,
            id::MAP_ZOOM_OUT,
            id::COMM_PANEL,
            id::COMM_STRIP,
            id::BAR_GROUP,
        ];
        for control in single
            .iter()
            .chain(&id::SEGMENTS)
            .chain(&id::CITY_ICONS)
            .chain(&id::POWERS)
            .chain(&id::POWERS_LOCKED)
            .chain(&id::POWER_GLOWS)
            .chain(&id::MAP_ROTATE)
            .chain(&id::COMM_BUTTONS)
        {
            assert!(layout.find(*control).is_some(), "window {control:08X}");
        }
        // Each communication button exists in both panels.
        for control in id::COMM_BUTTONS {
            assert_eq!(layout.find_all(control).len(), 2, "{control:08X} in the panel and the strip");
        }
        // The city icons are split four and six over the two segments.
        let parent_of = |control: u32| {
            let mut at = layout.find(control);
            while let Some(w) = at {
                let c = layout.window(w).control_id;
                if id::SEGMENTS.contains(&c) {
                    return c;
                }
                at = layout.window(w).parent;
            }
            0
        };
        let first = id::CITY_ICONS.iter().filter(|c| parent_of(**c) == id::SEGMENTS[0]).count();
        let second = id::CITY_ICONS.iter().filter(|c| parent_of(**c) == id::SEGMENTS[1]).count();
        assert_eq!([first, second], id::SEGMENT_ICONS);
        // The glow of an ability is a child of its button.
        for (button, glow) in id::POWERS.iter().zip(&id::POWER_GLOWS) {
            let b = layout.find(*button).unwrap();
            let g = layout.find(*glow).unwrap();
            assert_eq!(layout.window(g).parent, Some(b));
        }
    }

    #[test]
    fn installed_tooltips_are_the_retail_strings() {
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
        assert_eq!(tip(&mut assets, id::SPICE_TEXT).as_deref(), Some("Sporebucks"));
        assert_eq!(tip(&mut assets, id::STANCE_AGGRESSIVE).as_deref(), Some("Aggressive"));
        assert_eq!(tip(&mut assets, id::STANCE_STAND).as_deref(), Some("Stand Ground"));
        assert_eq!(tip(&mut assets, id::MILESTONE_AIR).as_deref(), Some("Control 4 cities to invent air vehicles"));
        assert_eq!(tip(&mut assets, id::MILESTONE_SPACE).as_deref(), Some("Control 10 cities to invent space travel"));
        assert_eq!(tip(&mut assets, id::POWERS[0]).as_deref(), Some("Consequence Abilities"));
        assert_eq!(tip(&mut assets, id::POWERS_LOCKED[0]).as_deref(), Some("Complete Cell Stage to unlock this ability"));
        assert_eq!(tip(&mut assets, id::POWERS_LOCKED[2]).as_deref(), Some("Complete Tribe Stage to unlock this ability."));
        assert_eq!(tip(&mut assets, id::ADVANCE).as_deref(), Some("Advance to Space"));
    }

    fn demo_hud() -> CivHudState {
        let mut c = campaign_with_powers();
        c.cities[1].owner = civ::CityOwner::Player;
        c.cities[2].owner = civ::CityOwner::Player;
        c.cities[3].owner = civ::CityOwner::Player;
        // The superweapon button needs six cities.
        c.cities[4].owner = civ::CityOwner::Player;
        c.cities[5].owner = civ::CityOwner::Player;
        c.power_creature_cooldown = 20.0;
        CivHudState {
            spice: 120_000.0,
            owned: 4,
            goal_cities: 5,
            air_cities: 3,
            sea_cities: 2,
            air_unlocked: true,
            powers: power_slots(&c, 120_000.0).map(Some),
            ..default()
        }
    }

    #[test]
    fn the_main_layout_shows_the_published_state() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, MAIN_LAYOUT).unwrap();
        let mut images = Assets::<Image>::default();
        let mut ui = SpuiState::new(layout, RootPolicy::Fit);
        let mut hud = demo_hud();
        let local = CivHudLocal::default();
        let colors = ProgressColors::default();
        let mut defs = guide::PowerDefs::default();
        bind_main(&mut ui, &hud, &local, &colors, &mut defs, &mut assets, &mut images, &index);
        let w = |ui: &SpuiState, control: u32| ui.find(control).unwrap();
        // Money, History until the planet is won.
        assert_eq!(ui.captions[w(&ui, id::SPICE_TEXT)].as_deref(), Some("$120,000"));
        assert!(ui.visible(w(&ui, id::HISTORY)) && !ui.visible(w(&ui, id::ADVANCE)));
        // Progress: four of five cities with aircraft at three: the first
        // segment is full with its four icons lit, the second is half full
        // (three of its six icons).
        assert_eq!(ui.overlays[w(&ui, id::SEGMENTS[0])].map(|o| o.0), Some(1.0));
        assert_eq!(ui.overlays[w(&ui, id::SEGMENTS[1])].map(|o| o.0), Some(0.5));
        for i in 0..7 {
            assert_eq!(ui.frame_overrides[w(&ui, id::CITY_ICONS[i])], Some(1), "icon {i}");
        }
        for i in 7..10 {
            assert_eq!(ui.frame_overrides[w(&ui, id::CITY_ICONS[i])], Some(0), "icon {i}");
        }
        // The air milestone is reached (no tooltip needed), the end one counts five cities.
        assert!(ui.tooltip_overrides[w(&ui, id::MILESTONE_AIR)].is_none());
        assert_eq!(
            ui.tooltip_overrides[w(&ui, id::MILESTONE_SPACE)].as_deref(),
            Some("Control 5 cities to invent space travel")
        );
        // Abilities: Cell and Tribal ready (glow), Creature recharging (no glow, dimmed, disabled),
        // superweapon unlocked by six cities.
        for slot in [0, 2, 3] {
            assert!(ui.visible(w(&ui, id::POWERS[slot])));
            assert!(ui.flags[w(&ui, id::POWERS[slot])] & crate::spui::ids::flag::ENABLED != 0, "slot {slot}");
            assert!(ui.visible(w(&ui, id::POWER_GLOWS[slot])), "ready glows: slot {slot}");
        }
        assert!(ui.flags[w(&ui, id::POWERS[1])] & crate::spui::ids::flag::ENABLED == 0);
        assert!(!ui.visible(w(&ui, id::POWER_GLOWS[1])));
        assert_eq!(ui.tints[w(&ui, id::POWERS[1])], COOLDOWN_TINT);
        let tip = ui.tooltip_overrides[w(&ui, id::POWERS[1])].clone().unwrap();
        assert!(tip.contains("$4,000") || tip.contains('$'), "the price is shown: {tip}");
        // The stance toggles show the selection's stances: nothing selected, both off.
        assert!(ui.states[w(&ui, id::STANCE_AGGRESSIVE)] & crate::spui::ids::state::SELECTED == 0);
        assert!(ui.states[w(&ui, id::STANCE_STAND)] & crate::spui::ids::state::SELECTED == 0);
        // A planet won: Advance replaces History. A locked superweapon disappears.
        hud.advance_visible = true;
        hud.powers[3].as_mut().unwrap().unlocked = false;
        bind_main(&mut ui, &hud, &local, &colors, &mut defs, &mut assets, &mut images, &index);
        assert!(ui.visible(w(&ui, id::ADVANCE)) && ui.visible(w(&ui, id::ADVANCE_GLOW)) && !ui.visible(w(&ui, id::HISTORY)));
        assert!(!ui.visible(w(&ui, id::POWERS[3])));
        // A locked stage ability shows its locked variant instead.
        hud.powers[0].as_mut().unwrap().unlocked = false;
        bind_main(&mut ui, &hud, &local, &colors, &mut defs, &mut assets, &mut images, &index);
        assert!(!ui.visible(w(&ui, id::POWERS[0])) && ui.visible(w(&ui, id::POWERS_LOCKED[0])));
        assert!(ui.visible(w(&ui, id::POWERS_LOCKED[1]) ) == false);
        // The communication screen hides the HUD's upper parts.
        hud.comm = Some(comm::CommState::default());
        bind_main(&mut ui, &hud, &local, &colors, &mut defs, &mut assets, &mut images, &index);
        assert!(!ui.visible(w(&ui, id::BAR_GROUP)) && !ui.visible(w(&ui, id::POWERS[1])));
        assert!(!ui.visible(w(&ui, id::STANCE_PANEL)));
    }

    #[test]
    fn installed_power_files_name_the_twelve_powers() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let mut defs = guide::PowerDefs::default();
        let mut names = Vec::new();
        for prop in [
            "cvg_clg_meat", "cvg_clg_plant", "cvg_clg_mix", "cvg_crg_attack", "cvg_crg_social", "cvg_crg_mix",
            "cvg_trg_attack", "cvg_trg_social", "cvg_trg_mix", "cvg_trg_attackwin", "cvg_trg_socialwin", "cvg_trg_mixwin",
        ] {
            let def = defs.get(prop, &mut assets, &index);
            assert!(!def.name.is_empty(), "{prop} has a retail name");
            assert!(!def.description.is_empty(), "{prop} has a retail description");
            let icon = def.icon.unwrap_or_else(|| panic!("{prop} has an icon"));
            assert!(index.read(icon).is_ok(), "{prop}: the icon {icon:?} is installed");
            names.push(def.name.clone());
        }
        // The rewrite's names are the retail ones.
        for slot in [
            civ::CivCellPower::Invulnerability.name(),
            civ::CivCellPower::HealingAura.name(),
            civ::CivCellPower::StaticBomb.name(),
            civ::CivCreaturePower::MightyBomb.name(),
            civ::CivCreaturePower::DiploDervish.name(),
            civ::CivCreaturePower::BribeBomb.name(),
            civ::CivTribalPower::GadgetBomb.name(),
            civ::CivTribalPower::BlackCloud.name(),
            civ::CivTribalPower::AdBlitz.name(),
            civ::CivSuperweapon::Icbm.name(),
            civ::CivSuperweapon::FanaticalUprising.name(),
            civ::CivSuperweapon::GlobalMerger.name(),
        ] {
            assert!(names.iter().any(|n| n.eq_ignore_ascii_case(slot)), "{slot} is a retail name: {names:?}");
        }
    }

    fn civ_app() -> App {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        app.update();
        app.update();
        app
    }

    #[test]
    fn the_published_state_follows_the_campaign() {
        let mut app = civ_app();
        app.update();
        let hud = app.world().resource::<CivHudState>();
        assert_eq!(hud.owned, 1);
        assert_eq!(hud.goal_cities, civ::CITY_COUNT);
        assert!(hud.spice > 0.0, "the opening purse");
        assert_eq!(hud.rivals.len(), civ::RIVAL_COUNT);
        assert!(hud.powers.iter().all(|p| p.is_some()));
        assert!(!hud.advance_visible && !hud.paused);
        assert!(hud.comm.is_none());
    }

    #[test]
    fn a_taken_city_leaves_the_comm_panel_and_moves_the_bar() {
        let mut app = civ_app();
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            for i in 2..civ::CITY_COUNT {
                run.civ_campaign.cities[i].owner = civ::CityOwner::Player;
            }
        }
        app.update();
        let hud = app.world().resource::<CivHudState>();
        assert_eq!(hud.owned, civ::CITY_COUNT - 1);
        assert_eq!(hud.rivals.iter().map(|r| r.nation).collect::<Vec<_>>(), vec![1]);
        assert!(hud.powers[3].as_ref().unwrap().unlocked, "a grown nation has the superweapon");
        assert!(hud.air_unlocked);
    }

    /// A root with the installed main layout, to click on.
    fn with_main_layout(app: &mut App) -> Option<Entity> {
        let (index, mut assets) = installed()?;
        let layout = assets.layout(&index, MAIN_LAYOUT)?;
        Some(app.world_mut().spawn((CivHudRoot::Main, SpuiState::new(layout, RootPolicy::Fit))).id())
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

    /// Keys the simulation sees as just pressed.
    #[derive(Resource, Default)]
    struct SeenKeys(Vec<KeyCode>);

    fn watch_keys(app: &mut App) {
        app.init_resource::<SeenKeys>().add_systems(
            Update,
            (|keys: Res<ButtonInput<KeyCode>>, mut seen: ResMut<SeenKeys>| {
                for key in [KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9, KeyCode::Digit0, KeyCode::F4] {
                    if keys.just_pressed(key) {
                        seen.0.push(key);
                    }
                }
            })
            .in_set(crate::GameSystems::Simulation),
        );
    }

    #[test]
    fn button_clicks_reach_the_simulation_as_fresh_key_presses() {
        let mut app = civ_app();
        watch_keys(&mut app);
        // The abilities cost money (retail 2000 and up): a full purse.
        app.world_mut().resource_mut::<RunProgress>().spice = 1.0e6;
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        app.update();
        app.update();
        click(&mut app, root, id::POWERS[0]);
        click(&mut app, root, id::ADVANCE);
        let seen = &app.world().resource::<SeenKeys>().0;
        assert_eq!(seen, &[KeyCode::Digit7, KeyCode::F4]);
        // The injected key was let go again.
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Digit7));
    }

    #[test]
    fn a_unified_planet_shows_advance_and_its_click_ends_the_stage() {
        let mut app = civ_app();
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            for city in run.civ_campaign.cities.iter_mut() {
                city.owner = crate::civ::CityOwner::Player;
            }
            assert!(run.civ_campaign.is_unified());
        }
        // The planet is the nation's: the button shows and the stage waits.
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(*app.world().resource::<State<GameStage>>().get(), GameStage::Civilization);
        assert!(app.world().resource::<CivHudState>().advance_visible);
        click(&mut app, root, id::ADVANCE);
        app.update();
        assert_eq!(*app.world().resource::<State<GameStage>>().get(), GameStage::Space);
    }

    #[test]
    fn a_recharging_ability_does_not_press_its_key() {
        let mut app = civ_app();
        watch_keys(&mut app);
        app.world_mut().resource_mut::<RunProgress>().civ_campaign.power_creature_cooldown = 30.0;
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        app.update();
        click(&mut app, root, id::POWERS[1]);
        assert!(app.world().resource::<SeenKeys>().0.is_empty());
    }

    #[test]
    fn the_stance_toggles_and_the_pause_button_work() {
        let mut app = civ_app();
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        // Nothing selected: neither toggle is checked and a click changes nothing.
        app.update();
        assert!(!app.world().resource::<CivHudState>().stance_aggressive);
        assert!(!app.world().resource::<CivHudState>().stance_stand);
        // A selected vehicle: the buttons set its stance and show it.
        let unit = CivUnit::from_state(&crate::civ::UnitState {
            id: 41,
            nation: crate::civ::HOME_INDEX,
            kind: Approach::Military,
            ..Default::default()
        });
        let e = app.world_mut().spawn((unit, crate::civ_rts::Selected)).id();
        click(&mut app, root, id::STANCE_AGGRESSIVE);
        app.update();
        let stance_of = |app: &App| app.world().get::<crate::civ_controls::CivStance>(e).copied();
        assert_eq!(stance_of(&app), Some(crate::civ_controls::CivStance::Aggressive));
        assert!(app.world().resource::<CivHudState>().stance_aggressive);
        assert!(!app.world().resource::<CivHudState>().stance_stand);
        click(&mut app, root, id::STANCE_STAND);
        app.update();
        assert_eq!(stance_of(&app), Some(crate::civ_controls::CivStance::StandGround));
        assert!(!app.world().resource::<CivHudState>().stance_aggressive);
        assert!(app.world().resource::<CivHudState>().stance_stand);
        let both = [crate::civ_controls::CivStance::StandGround, crate::civ_controls::CivStance::Aggressive];
        assert_eq!(stance_flags(both.into_iter()), (true, true));
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
        click(&mut app, root, id::PAUSE_TOGGLE);
        assert!(app.world().resource::<crate::pause_menu::PauseMenu>().open);
        click(&mut app, root, id::PAUSE_TOGGLE);
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
    }

    #[test]
    fn the_hide_toggle_collapses_the_minimap_and_the_comm_buttons_open_the_screen() {
        let mut app = civ_app();
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        app.update();
        assert!(app.world().resource::<CivHudLocal>().minimap_open);
        click(&mut app, root, id::MAP_HIDE);
        assert!(!app.world().resource::<CivHudLocal>().minimap_open);
        click(&mut app, root, id::MAP_HIDE);
        assert!(app.world().resource::<CivHudLocal>().minimap_open);
        // The second comm button is the second rival.
        click(&mut app, root, id::COMM_BUTTONS[1]);
        let session = app.world().resource::<CivHudLocal>().comm;
        assert_eq!(session.map(|s| s.nation), Some(2));
    }
}
