//! Goal cards, hints and callouts of the Civ HUD, all from the retail data:
//! the goal cards are the installed `CVGMissionCard` layout filled with
//! strings of the `CivGame` locale table, the hints are the `cvg-*` property
//! files (texts of `HintsCVG`) shown on the `hint` layout and the callouts are
//! the `CalloutCVG*` property files shown on the callout dialog layout (the
//! generic machinery of `cell_hud::dialogs`). The retail consequence ability
//! files (`cvg_clg_meat`, ...) are read here too.
//!
//! Every hint goes through the retail hint manager (`hint_manager`, one
//! `HintMachine` with the `cvg-*` property files): the forced tutorial's four
//! hints are `ShowHint` / `HideHint` calls at the steps of the retail chain
//! (0xCF9D00, 0xCFAAF0), the conditional ones (a city under attack, an unhappy
//! city, a damaged vehicle, the vehicle class hints, the building planner)
//! are answered by the Civ branch of the condition checker 0xE07E50
//! ([`Conditions`]). What the retail goal manager decides (which goal is
//! current) and the order of the callouts is ADAPTED from the state the Civ
//! gameplay modules expose and marked as such.
//!
//! The goal cards are the goals the retail goal manager holds (`civ_goals`, the
//! chain 0xCF9D00 steps 6..12 and the completions of 0xCFAAF0); while a forced
//! tutorial hint or a callout waits the game is held like the retail pause
//! (token 0x064BEB65 / 0x04BF38A7), see [`world_runs`].

use std::collections::HashMap;

use bevy::prelude::*;

use super::{CivHudActive, CivHudRoot, CivHudState};
use crate::{
    asset_catalog::ResourceIndex,
    cell_contract::CellGoalCard,
    cell_hud::dialogs::{self, CalloutDef, HintDef},
    civ::{self, Approach, HOME_INDEX},
    civ_goals::{self, CatchUp, ChainInputs, Goal, Observed, Seen},
    civ_controls::CivCamera,
    civ_planner::CivPlanner,
    civ_rts::{CivUnit, Selected},
    dbpf::ResourceKey,
    game::RunProgress,
    hint_manager::{cond as shared, HintBar, HintMachine},
    spui::{
        model::{ImageRef, WinId},
        solve::{Rect, RootPolicy},
        view::{self, SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

/// Goal cards shown at once (retail stacks the current goals).
pub const MAX_CARDS: usize = 3;

/// Control ids of `CVGMissionCard` (the same as the Cell's `CLGMissionCard`).
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
    /// Collapse button of a card and the anchor window the Civ callouts
    /// point at.
    pub const BUTTON: u32 = 0x0539_7388;
    pub const ANCHOR: u32 = 0x0641_6A79;
}

/// ADAPTED: where the first card's root sits (its header edge matches the
/// Cell card's) and the gap between stacked cards.
const CARD_X: f32 = 5.0;
const CARD_Y: f32 = 15.0;
const CARD_GAP: f32 = 4.0;

/// Locale ids of the goal strings (`CivGame`, from the installed table).
mod text {
    pub const CLAIM_KIND: &str = "loc:CivGame:058b486a";
    pub const CLAIM_BODY: &str = "loc:CivGame:058b486b";
    pub const CLAIM_HEADLINE: &str = "loc:CivGame:058b486c";
    pub const PROFIT_KIND: &str = "loc:CivGame:058b71b4";
    pub const PROFIT_BODY: &str = "loc:CivGame:058b71b5";
    pub const PROFIT_HEADLINE: &str = "loc:CivGame:058b71b6";
    pub const HAPPY_KIND: &str = "loc:CivGame:058c75fd";
    pub const HAPPY_BODY: &str = "loc:CivGame:058c75fe";
    pub const HAPPY_HEADLINE: &str = "loc:CivGame:058c75ff";
    pub const SEA_KIND: &str = "loc:CivGame:058c7642";
    pub const SEA_BODY: &str = "loc:CivGame:058c7643";
    pub const SEA_HEADLINE: &str = "loc:CivGame:058c7644";
    pub const AIR_KIND: &str = "loc:CivGame:058c765b";
    pub const AIR_BODY: &str = "loc:CivGame:058c765c";
    pub const AIR_HEADLINE: &str = "loc:CivGame:058c765d";
    /// Capture / convert / buy an opposing city: kind, body, headline.
    pub const MILITARY: [&str; 3] = ["loc:CivGame:058c9722", "loc:CivGame:058c9723", "loc:CivGame:058c9724"];
    pub const RELIGIOUS: [&str; 3] = ["loc:CivGame:058c968c", "loc:CivGame:058c968d", "loc:CivGame:058c968e"];
    pub const ECONOMIC: [&str; 3] = ["loc:CivGame:058c96a2", "loc:CivGame:058c96a3", "loc:CivGame:058c96a4"];
    pub const ALL_KIND: &str = "loc:CivGame:0590c228";
    pub const ALL_BODY: &str = "loc:CivGame:0590c229";
    pub const ALL_HEADLINE: &str = "loc:CivGame:0590c22a";
}

/// The card of a goal: its three retail strings (kind, body, headline). None of
/// the nine `CivTutorial_*` props has a `goalValue` and nothing updates a Civ
/// goal's progress, so no card carries a count (VERIFIED, `civ_goals`).
pub fn goal_card(goal: Goal) -> CellGoalCard {
    let s = match goal {
        Goal::CaptureNode => [text::CLAIM_KIND, text::CLAIM_BODY, text::CLAIM_HEADLINE],
        Goal::ProfitableCity => [text::PROFIT_KIND, text::PROFIT_BODY, text::PROFIT_HEADLINE],
        Goal::HappyCity => [text::HAPPY_KIND, text::HAPPY_BODY, text::HAPPY_HEADLINE],
        Goal::SeaUnit => [text::SEA_KIND, text::SEA_BODY, text::SEA_HEADLINE],
        Goal::AirUnit => [text::AIR_KIND, text::AIR_BODY, text::AIR_HEADLINE],
        Goal::MilitaryCapture => text::MILITARY,
        Goal::ReligionCapture => text::RELIGIOUS,
        Goal::EconomicCapture => text::ECONOMIC,
        Goal::CaptureAll => [text::ALL_KIND, text::ALL_BODY, text::ALL_HEADLINE],
    };
    CellGoalCard { kind: s[0].into(), progress: None, title: s[2].into(), body: s[1].into() }
}

/// The goal cards for the goals on the manager's list, first added first (the
/// order of the retail chain, `civ_goals`). ADAPTED: at most [`MAX_CARDS`] are
/// stacked on screen (the chain never holds more than three: a sea goal, an air
/// goal and one capture goal).
pub fn goal_cards(active: &[Goal]) -> Vec<CellGoalCard> {
    active.iter().take(MAX_CARDS).map(|g| goal_card(*g)).collect()
}

/// What the campaign has done so far, for a campaign that is under way (see
/// [`civ_goals::GoalChain::catch_up`]).
fn catch_up_of(campaign: &civ::CivCampaign, units: &Query<(&CivUnit, Has<Selected>)>) -> CatchUp {
    let owned = || campaign.cities.iter().filter(|c| c.owned());
    let has = |domain: civ::VehicleDomain| units.iter().any(|(u, _)| u.nation == HOME_INDEX && u.domain == domain);
    CatchUp {
        derricks: campaign.derricks(HOME_INDEX),
        designed: campaign.cities.get(HOME_INDEX).is_some_and(|c| c.buildings > 0),
        happy: owned().any(|c| c.happiness() >= civ_goals::HAPPY_MOOD),
        sea_vehicle: has(civ::VehicleDomain::Sea),
        air_vehicle: has(civ::VehicleDomain::Air),
        owned: campaign.owned_count(),
        capital: campaign.approach,
        tech: campaign.vehicle_tech_level(),
    }
}

/// The counters the goal manager's events are read from (`civ_goals::Seen`).
fn observed_of(campaign: &civ::CivCampaign, units: &Query<(&CivUnit, Has<Selected>)>) -> Observed {
    let count = |domain: civ::VehicleDomain| {
        units.iter().filter(|(u, _)| u.nation == HOME_INDEX && u.domain == domain && u.hp > 0.0).count() as u32
    };
    Observed {
        has_city: campaign.owned_count() > 0,
        seen: Seen {
            derricks: campaign.derricks(HOME_INDEX) as u32,
            sea: count(civ::VehicleDomain::Sea),
            air: count(civ::VehicleDomain::Air),
            military: campaign.captures.military,
            religious: campaign.captures.religious,
            economic: campaign.captures.economic,
            tech: campaign.vehicle_tech_level(),
        },
    }
}

/// Resolve `loc:<Table>:<hex id>` references (one per line) to locale text;
/// anything else is literal text.
pub fn resolve(assets: &mut SpuiAssets, index: &ResourceIndex, text: &str) -> String {
    text.lines()
        .map(|line| match line.strip_prefix("loc:").and_then(|r| r.split_once(':')) {
            Some((table, hex)) => u32::from_str_radix(hex.trim_start_matches("0x"), 16)
                .ok()
                .and_then(|id| assets.localized(index, crate::world_objects::name_id(table), id))
                .unwrap_or_default(),
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Bottom edge of a card's panel on screen (cards stack).
pub fn card_bottom(ui: &SpuiState) -> f32 {
    ui.find(card::PANEL).map_or(0.0, |w| ui.rect(w).y2)
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
    let head_h = height(ui, head).max(15.0);
    let desc_h = height(ui, desc);
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

/// Fill one mission card and put its root at `(CARD_X, y)`.
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

/// Spawn and despawn the goal card roots (one `CVGMissionCard` each).
pub fn manage_cards(
    mut commands: Commands,
    active: Res<CivHudActive>,
    hud: Res<CivHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &CivHudRoot)>,
) {
    if !active.0 {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let index = &originals.resource_index;
    let want = hud.goal_cards.len().min(MAX_CARDS);
    for (entity, kind) in &roots {
        if matches!(kind, CivHudRoot::Card(n) if *n as usize >= want) {
            commands.entity(entity).despawn();
        }
    }
    for n in 0..want {
        let kind = CivHudRoot::Card(n as u8);
        if !roots.iter().any(|(_, k)| *k == kind) {
            if let Some(layout) = assets.layout(index, super::CARD_LAYOUT) {
                let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 99);
                commands.entity(root).insert(kind);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Consequence ability definitions
// ---------------------------------------------------------------------------

/// Group of the consequence power property files and the property ids of the
/// name, description and icon (installed data, see `notes\civ-hud-notes.md`).
const POWER_GROUP: u32 = 0x4495_05AF;
const PROP_POWER_NAME: u32 = 0xD4D9_59E0;
const PROP_POWER_DESCRIPTION: u32 = 0xD4D9_59E1;
const PROP_POWER_ICON: u32 = 0xD4D9_59E2;

/// What a consequence ability file says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerDef {
    pub name: String,
    pub description: String,
    pub icon: Option<ResourceKey>,
}

/// The loaded ability definitions.
#[derive(Resource, Default)]
pub struct PowerDefs {
    map: HashMap<&'static str, PowerDef>,
}

impl PowerDefs {
    pub fn get(&mut self, prop: &'static str, assets: &mut SpuiAssets, index: &ResourceIndex) -> &PowerDef {
        self.map.entry(prop).or_insert_with(|| read_power(index, assets, prop).unwrap_or_default())
    }
}

fn text_of(props: &crate::prop::PropertyList, id: u32, index: &ResourceIndex, assets: &mut SpuiAssets) -> String {
    props
        .0
        .get(&id)
        .and_then(|p| p.text())
        .and_then(|(table, text_id, _)| (table != 0).then(|| assets.localized(index, table, text_id)).flatten())
        .unwrap_or_default()
}

/// Read a consequence ability file by its registered name (`cvg_clg_meat`).
pub fn read_power(index: &ResourceIndex, assets: &mut SpuiAssets, name: &str) -> Option<PowerDef> {
    let key = ResourceKey {
        group_id: POWER_GROUP,
        instance_id: crate::world_objects::name_id(name),
        type_id: crate::asset_catalog::PROP,
    };
    let props = index.properties(key).ok()?;
    Some(PowerDef {
        name: text_of(&props, PROP_POWER_NAME, index, assets),
        description: text_of(&props, PROP_POWER_DESCRIPTION, index, assets),
        icon: props.0.get(&PROP_POWER_ICON).and_then(|p| p.key()),
    })
}

// ---------------------------------------------------------------------------
// Hints and callouts
// ---------------------------------------------------------------------------

/// How long a campaign message (the ADAPTED text line) stays on the hint bar, seconds.
pub const MESSAGE_SECONDS: f32 = 4.0;
/// The forced tutorial of the retail Civ start (VERIFIED, 0xCF9D00 steps 1-4
/// and the completions of 0xCFAAF0, ids 0x064E666F / 73 / 76 / 79): four hints,
/// each `ShowHint` when its step begins (0xCF9DEA, 0xCF9E3D, 0xCF9E90; the first
/// by 0xCF6F90) and `HideHint` by the step's completion (0xCFAB6F..0xCFAE11):
/// `Triggered`, no display time, no condition, shown until the player does
/// what it says. `cvg-moveunit` is only ever hidden (0xCFAB8A), never shown:
/// no code site asks for it. Retail also holds the game paused (token
/// 0x064BEB65) while one waits: [`world_runs`] holds the Civ simulation.
pub const FORCED_HINTS: [&str; 4] = ["cvg-camrotate", "cvg-camtranslate", "cvg-camzoom", "cvg-selectunit"];
/// Every Civ hint of the install (`HintsCVG`): the manager's list. The
/// conditional ones are answered by [`Conditions`]; `cvg-buyunit` has no
/// condition and no code site (never shown).
pub const ALL_HINTS: [&str; 15] = [
    "cvg-camrotate",
    "cvg-camtranslate",
    "cvg-camzoom",
    "cvg-selectunit",
    "cvg-moveunit",
    "cvg-cityattacked",
    "cvg-cityunhappy",
    "cvg-healvehicle",
    "cvg-military",
    "cvg-religious",
    "cvg-economic",
    "cvg-plannerbuildings",
    "cvg-harvester",
    "cvg-mine",
    "cvg-buyunit",
];
/// ADAPTED thresholds of "the player rotated / panned / zoomed the camera"
/// (retail raises the completions from the camera's own input events): radians
/// of azimuth or pitch, world units of the anchor, a fraction of the distance.
const ROTATE_DONE: f32 = 0.03;
const PAN_DONE: f32 = 3.0;
const ZOOM_DONE: f32 = 0.02;
/// Seconds after damage during which a city counts as under attack.
const ATTACK_WINDOW: f32 = 3.0;
/// Hit-point share under which a vehicle counts as damaged. ADAPTED: retail's
/// `OwnsDamagedVehicle` is a byte of the civilization (+0x430) the vehicle
/// code sets; the threshold is not decoded.
const DAMAGED_VEHICLE: f32 = 0.4;
/// `HasMoney` (0xE084BE): the civilization's money (+0x98) is above 100.0
/// (the float 0x13EC4B0).
const MONEY_LEVEL: f32 = 100.0;

/// Condition names the Civ branch of the checker answers (0xE081B1..0xE0865A,
/// VERIFIED); the generic ones (`cvg`, `UserBusy`, `InPlanner`) are in
/// [`crate::hint_manager::cond`].
#[allow(dead_code)]
pub mod cond {
    /// 0xE08283: the player's city exists and is unhappy (0xBD7F40).
    pub const HAS_UNHAPPY_CITY: u32 = 0x8788_C629;
    /// 0xE082B7: the player's city exists and is not unhappy.
    pub const HAS_HAPPY_CITY: u32 = 0x70CA_6ADA;
    /// 0xE08259: the city hall is on screen (0xBD9C90, the building's `+0x34` object).
    pub const CITY_HALL_VISIBLE: u32 = 0xD436_D975;
    /// 0xE082EB: some object of the mine list (0xAE5FB0) is on screen.
    pub const MINE_VISIBLE: u32 = 0xB6DF_BFF2;
    /// Constant false in the Civ mode (0xE081F9 -> 0xE08B8B): no harvesters.
    pub const HARVESTER: u32 = 0x556B_B739;
    /// 0xE081FF: the harvester window is visible (0xCF7EA0).
    pub const HARVESTER_VISIBLE: u32 = 0xEC28_A285;
    pub const HARVESTER_SELECTED: u32 = 0xF302_FA1E;
    /// The civilization's byte `+0x430` / `+0x431`.
    pub const OWNS_DAMAGED_VEHICLE: u32 = 0x5FD1_F4B5;
    pub const HAS_HEALED_VEHICLE: u32 = 0xEF0B_A57A;
    /// 0xBF0E30(civilization, purpose, -1, -1) > 0: selected vehicles of the
    /// purpose 0 (military) / 1 (religious) / 2 (economic).
    pub const MILITARY_SELECTED: u32 = 0xC480_A83C;
    pub const RELIGIOUS_SELECTED: u32 = 0x14C8_9726;
    pub const ECONOMIC_SELECTED: u32 = 0x8777_5C72;
    /// 0xE084BE: the civilization's money is above 100.
    pub const HAS_MONEY: u32 = 0xB815_D50D;
    /// Profile flags: 0x52DA1F4 (a trade order was given, 0xCF38BC), 0x0566493F
    /// (the building planner was entered, 0xD1393F), 0x52DA1F6 (a military
    /// order was given at a target, 0xCF3582 / 0xCF427F), 0x52DA1F2 (a vehicle
    /// was sent to a spice geyser, 0xCF3EEB).
    pub const TRADE_ROUTE: u32 = 0x9153_1C40;
    pub const ENTERED_BUILDING_PLANNER: u32 = 0x79DE_641E;
    pub const PLAYER_ATTACKED_CITY: u32 = 0xDE91_118C;
    pub const MINED: u32 = 0xD69A_DAD0;
    /// 0xE0854D: a city of the player is attacked (0xC00600 with 0 / 1) and
    /// its `+0x34` object (0xBD9C90) is not visible.
    pub const OFFSCREEN_CITY_ATTACKED: u32 = 0xF6F4_B4ED;
    /// 0xE085FB: the player owns exactly one city.
    pub const ONE_PLAYER_CITY: u32 = 0xFF24_A90D;
    /// 0xE0862D: more than one city has appeared (slots - hidden > 1).
    pub const FIRST_NPC_CITY: u32 = 0xD349_682D;
}

/// What the Civ branch of the checker answers (the guide reads the inputs
/// from the campaign each frame).
#[derive(Clone, Copy, Debug, Default)]
pub struct Conditions {
    pub has_unhappy_city: bool,
    pub has_happy_city: bool,
    pub city_hall_visible: bool,
    pub offscreen_city_attacked: bool,
    pub owns_damaged_vehicle: bool,
    pub has_healed_vehicle: bool,
    pub military_selected: bool,
    pub religious_selected: bool,
    pub economic_selected: bool,
    pub has_money: bool,
    pub trade_route: bool,
    pub entered_building_planner: bool,
    pub player_attacked_city: bool,
    pub mined: bool,
    pub one_player_city: bool,
    pub first_npc_city: bool,
    pub in_planner: bool,
}

impl Conditions {
    pub fn eval(&self, name: u32) -> bool {
        match name {
            shared::CVG => true,
            cond::HAS_UNHAPPY_CITY => self.has_unhappy_city,
            cond::HAS_HAPPY_CITY => self.has_happy_city,
            cond::CITY_HALL_VISIBLE => self.city_hall_visible,
            cond::OFFSCREEN_CITY_ATTACKED => self.offscreen_city_attacked,
            cond::OWNS_DAMAGED_VEHICLE => self.owns_damaged_vehicle,
            cond::HAS_HEALED_VEHICLE => self.has_healed_vehicle,
            cond::MILITARY_SELECTED => self.military_selected,
            cond::RELIGIOUS_SELECTED => self.religious_selected,
            cond::ECONOMIC_SELECTED => self.economic_selected,
            cond::HAS_MONEY => self.has_money,
            cond::TRADE_ROUTE => self.trade_route,
            cond::ENTERED_BUILDING_PLANNER => self.entered_building_planner,
            cond::PLAYER_ATTACKED_CITY => self.player_attacked_city,
            cond::MINED => self.mined,
            cond::ONE_PLAYER_CITY => self.one_player_city,
            cond::FIRST_NPC_CITY => self.first_npc_city,
            shared::IN_PLANNER => self.in_planner,
            // `UserBusy` (constant false in this mode), `harvester` (constant
            // false: the shipped game has no harvesters, so `cvg-harvester` and
            // `cvg-mine` never show), `Inactive` and every name this mode does
            // not know.
            _ => false,
        }
    }
}

/// The text line of a campaign message. ADAPTED: the message line is the
/// rewrite's feedback channel ("Fleet sent against C2"), retail has no such
/// line; it only shows while the manager has no hint of its own on the bar.
pub fn message_hint(message: &str, age: f32) -> Option<String> {
    (!message.is_empty() && age < MESSAGE_SECONDS).then(|| format!("msg:{message}"))
}

/// A world point is on screen: not behind the planet's limb and inside the
/// viewport (ADAPTED stand-in for the retail building's visibility test,
/// 0xBD9C90 -> `+0x34` object vtable `+0x48`).
pub fn on_screen(shape: &crate::civ_planet::Shape, camera: &Camera, transform: &GlobalTransform, p: Vec3) -> bool {
    if !shape.visible_from(transform.translation(), p) {
        return false;
    }
    match (camera.world_to_viewport(transform, p), camera.logical_viewport_size()) {
        (Ok(at), Some(size)) => at.x >= 0.0 && at.y >= 0.0 && at.x <= size.x && at.y <= size.y,
        _ => false,
    }
}

/// The retail introduction (VERIFIED, message 0x06270D2F of 0xCFE105, sent when
/// `TRG2CVG_CivStart` ends): welcome, progress bar, then the forced tutorial's
/// start card (callout keys 0x01581B18 / B24 / B30); the game is paused under
/// them and Tutorials off skips all three.
pub const OPENING_CALLOUTS: [&str; 3] = ["CalloutCVGIntro", "CalloutCVGProgressBar", "CalloutCVGForcedTutorialStart"];
/// The goals callout: retail adds the first goal (`CivTutorial_CaptureNode`)
/// and shows it (0xCF9D00 step 6, callout key 0x01581B78) once the vehicle
/// cinematic `CVG_SendVehicleToCommodity` has ended.
pub const CLAIM_CALLOUT: &str = "CalloutCVGGoalClaimNode";
/// 0xCF9D00 step 7 (VERIFIED, key 0x01581B54): once the first goal is done
/// the next goal (`ProfitableCity`) is added and, once, the game pauses on the
/// minimap callout about spice.
pub const SPICE_CALLOUT: &str = "CalloutCVGMiniMapSpice";
/// The communications panel, when the first nation shows up.
pub const COMM_CALLOUT: &str = "CalloutCVGMiniMapCommButtons";
/// The consequence abilities, after the first new city appeared.
pub const SUPERWEAPON_CALLOUT: &str = "CalloutCVGInitialSuperweapon";
/// Sea and air vehicles invented.
pub const SEA_CALLOUT: &str = "CalloutCVGSea";
pub const AIR_CALLOUT: &str = "CalloutCVGAir";
/// The planet is won: congratulations, then the Advance button.
pub const FINAL_CALLOUTS: [&str; 2] = ["CalloutCVGWin", "CalloutCVGTransitionButton"];
/// ADAPTED: seconds after the HUD appears before the introduction starts.
const CALLOUT_DELAY: f32 = 1.5;
/// A campaign younger than this many seconds is a new nation and gets the
/// introduction.
const FRESH_CLOCK: f32 = 10.0;

/// Flags of the capture demo (`--civ-hud-*`).
#[derive(Clone, Debug, Default)]
pub struct GuideFlags {
    /// No introduction callouts and no first-minute hints.
    pub quiet: bool,
    pub callout: Option<String>,
    pub hint: Option<String>,
}

impl GuideFlags {
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned());
        Self {
            quiet: args.iter().any(|a| a == "--civ-hud-quiet"),
            callout: value("--civ-hud-callout"),
            hint: value("--civ-hud-hint"),
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
    intro_active: bool,
    intro_end: Option<f32>,
    comm_shown: bool,
    weapon_shown: bool,
    sea_shown: bool,
    air_shown: bool,
    final_shown: bool,
    /// Forced tutorial steps done (0..=4) and the camera at the step's start.
    forced: usize,
    forced_base: Option<(f32, f32, f32, Vec3)>,
    /// The retail hint manager with the `cvg-*` hints (loaded once per visit),
    /// the times it cleared the bar at once and the forced step it was last
    /// asked to show.
    machine: Option<HintMachine>,
    hint_cuts: u32,
    shown_step: Option<usize>,
    /// The sticky profile flags the conditions read ([`cond::ENTERED_BUILDING_PLANNER`],
    /// `PlayerAttackedCity`, `TradeRoute`, `mined`, `HasHealedVehicle`) and the
    /// damaged-vehicle answer of the last frame.
    entered_planner: bool,
    /// The city the City Planner was editing last frame (its exit judges the city,
    /// `civ_goals::GoalChain::planner_exit`).
    planner_prev: Option<usize>,
    attacked_city: bool,
    trade_route: bool,
    mined: bool,
    healed_vehicle: bool,
    damaged_before: bool,
    queue: Vec<String>,
}

impl GuideState {
    /// The forced tutorial (rotate, pan, zoom, select) is finished: the
    /// vehicle cinematic follows (0xCF9D00 step 5).
    pub fn forced_complete(&self) -> bool {
        self.forced >= FORCED_HINTS.len()
    }

    /// A visit whose four forced steps are done (tests of the vehicle
    /// cinematic's trigger).
    #[cfg(test)]
    pub fn with_forced_done() -> Self {
        Self { forced: FORCED_HINTS.len(), ..default() }
    }

    /// The retail game is paused while a forced tutorial hint waits (token
    /// 0x064BEB65, 0xCF9D00 steps 1..4, resumed by 0xCFAAF0) and while a callout is
    /// open (tokens 0x04BF38A7 / 0x064BEB65, resumed by the dismiss callbacks).
    pub fn holds_world(&self) -> bool {
        !self.queue.is_empty() || self.shown_step.is_some()
    }

    /// Forget everything about the visit (the stage was left).
    fn reset(&mut self) {
        let flags = self.flags.take();
        *self = Self { flags, ..default() };
    }

    /// Dismiss the front callout.
    pub fn dismiss(&mut self) {
        if !self.queue.is_empty() {
            self.queue.remove(0);
        }
    }
}

/// Run condition of the Civ simulation that a held game stops (the vehicles'
/// AI, movement, combat and the campaign clock): false while the forced tutorial
/// or a callout holds the game. The camera, the selection and the HUD keep
/// working, as under the retail pause (`GameTimeManager::Pause` stops game time
/// only).
pub fn world_runs(guide: Option<Res<GuideState>>) -> bool {
    !guide.is_some_and(|g| g.holds_world())
}

/// Loaded callout and hint definitions and the hint bar's fade.
#[derive(Resource, Default)]
pub struct GuideCache {
    callouts: HashMap<String, Option<CalloutDef>>,
    hints: HashMap<String, Option<HintDef>>,
    /// The retail fade of the shown hint (1 s in, 2 s out, `HintFade`).
    hint_bar: HintBar,
}

/// Fill the goal cards, hint and callouts of [`CivHudState`].
#[allow(clippy::too_many_arguments)]
pub fn publish_guide(
    time: Res<Time>,
    stage: Res<State<GameStage>>,
    active: Res<CivHudActive>,
    mut run: ResMut<RunProgress>,
    keys: Res<ButtonInput<KeyCode>>,
    units: Query<(&CivUnit, Has<Selected>)>,
    camera: Option<Res<CivCamera>>,
    planner: Res<CivPlanner>,
    originals: Option<Res<crate::Originals>>,
    view: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    mut guide: ResMut<GuideState>,
    mut hud: ResMut<CivHudState>,
) {
    if *stage.get() != GameStage::Civilization {
        // The visit is over: the next one starts the introduction again.
        guide.reset();
    }
    // `EnteredBuildingPlanner` (flag 0x0566493F, 0xD1393F): the HUD hides under
    // the planner, so the flag is read before the early return. ADAPTED: any
    // planner visit sets it.
    if *stage.get() == GameStage::Civilization && planner.is_open() {
        guide.entered_planner = true;
    }
    // The City Planner was left (message 0xB2699149, 0xCF5923): the planned
    // city is judged for `ProfitableCity` / `HappyCity` (civ_goals).
    let planner_now = (*stage.get() == GameStage::Civilization).then_some(planner.city).flatten();
    if let Some(left) = guide.planner_prev.filter(|_| planner_now.is_none()) {
        if let Some(city) = run.civ_campaign.cities.get(left) {
            let model = city.model();
            let income = crate::civ_rules::city_income(model.money, city.partying(), city.rioting(), model.happiness());
            let mut goals = run.civ_campaign.goals.clone();
            goals.planner_exit(city.owned(), income, model.happiness());
            if goals != run.civ_campaign.goals {
                run.civ_campaign.goals = goals;
            }
        }
    }
    guide.planner_prev = planner_now;
    if *stage.get() != GameStage::Civilization || !active.0 {
        hud.callouts.clear();
        hud.hint = None;
        hud.goal_cards.clear();
        return;
    }
    let flags = guide.flags.get_or_insert_with(GuideFlags::from_args).clone();
    let dt = time.delta_secs();
    guide.elapsed += dt;
    let mut goals = run.civ_campaign.goals.clone();
    let campaign = &run.civ_campaign;
    // The retail hint manager with the `cvg-*` hints (loaded once per visit).
    if guide.machine.is_none() {
        if let Some(originals) = originals.as_deref().filter(|o| !o.resource_index.records.is_empty()) {
            guide.machine = Some(HintMachine::load(&originals.resource_index, &ALL_HINTS));
        }
    }
    if guide.message != campaign.message {
        guide.message = campaign.message.clone();
        guide.message_age = 0.0;
    } else {
        guide.message_age += dt;
    }
    // Callouts: a new nation gets the introduction once per visit.
    let fresh = campaign.clock < FRESH_CLOCK && hud.owned <= 1;
    if !guide.started && guide.elapsed >= CALLOUT_DELAY {
        guide.started = true;
        if let Some(name) = &flags.callout {
            guide.queue = vec![name.clone()];
        } else if fresh && !flags.quiet && crate::front_end::settings::tutorials_enabled() {
            // Retail shows the three cards only with Tutorials on (0xCFE105).
            guide.queue = OPENING_CALLOUTS.iter().map(|n| n.to_string()).collect();
            guide.intro_active = true;
        }
    }
    if guide.intro_active && guide.queue.is_empty() {
        guide.intro_active = false;
        guide.intro_end = Some(guide.elapsed);
    }
    // The forced tutorial (0xCF9D00 steps 1-4): the hint of the current step
    // stays until the player does it; the next starts from the camera as it
    // is then.
    let selected = units.iter().any(|(u, s)| s && u.nation == HOME_INDEX);
    let tutorials = crate::front_end::settings::tutorials_enabled();
    if guide.intro_end.is_some() && tutorials && !guide.forced_complete() && guide.queue.is_empty() {
        if let Some(cam) = camera.as_deref() {
            let now = (cam.azimuth, cam.pitch, cam.goal_distance, cam.anchor);
            let (azimuth, pitch, distance, anchor) = *guide.forced_base.get_or_insert(now);
            let done = match guide.forced {
                0 => (now.0 - azimuth).abs() > ROTATE_DONE || (now.1 - pitch).abs() > ROTATE_DONE,
                1 => now.3.distance(anchor) > PAN_DONE,
                2 => distance > 0.0 && (now.2 / distance - 1.0).abs() > ZOOM_DONE,
                _ => selected,
            };
            if done {
                guide.forced += 1;
                guide.forced_base = Some(now);
            }
        }
    }
    // Later callouts, once the introduction has been offered.
    if guide.started && !flags.quiet && flags.callout.is_none() {
        if !guide.intro_active && !guide.comm_shown && !hud.rivals.is_empty() && guide.intro_end.is_some() {
            guide.comm_shown = true;
            guide.queue.push(COMM_CALLOUT.to_string());
        }
        // 0xCFDA95 (VERIFIED): when `CVG_PRE_NewCityAppears` ends, with
        // Tutorials on and one of the Cell powers (0x0756D422 / 0x045CDF1A /
        // 0xC0617B2F) in the civilization, the abilities callout (key
        // 0x01581B60, "Consequence Abilities") opens. It was tied to the
        // superweapon becoming available.
        let weapon = hud.powers[0].as_ref().is_some_and(|p| p.unlocked)
            && tutorials
            && run.cinematics_played.iter().any(|n| n == "CVG_PRE_NewCityAppears");
        if weapon && !guide.weapon_shown {
            guide.weapon_shown = true;
            guide.queue.push(SUPERWEAPON_CALLOUT.to_string());
        }
        if hud.sea_unlocked && !guide.sea_shown && guide.intro_end.is_some() {
            guide.sea_shown = true;
            guide.queue.push(SEA_CALLOUT.to_string());
        }
        if hud.air_unlocked && !guide.air_shown && guide.intro_end.is_some() {
            guide.air_shown = true;
            guide.queue.push(AIR_CALLOUT.to_string());
        }
        if hud.advance_visible && !guide.final_shown {
            guide.final_shown = true;
            guide.queue.extend(FINAL_CALLOUTS.iter().map(|n| n.to_string()));
        }
    }
    // The goal manager (`civ_goals`): the chain 0xCF9D00 steps 6..12 only run
    // with the Tutorials option on. It opens after the forced steps and the vehicle
    // cinematic (steps 1..5), at once for a campaign that is already under way
    // (retail keeps the step flags in the profile) and for the captures' quiet mode.
    let observed = observed_of(campaign, &units);
    if tutorials {
        if !goals.open() {
            let under_way = guide.started && !fresh && !flags.quiet;
            let after_forced = guide.forced_complete()
                && run.cinematics_played.iter().any(|n| n == "CVG_SendVehicleToCommodity");
            if under_way {
                goals.observe(&observed, Some(&catch_up_of(campaign, &units)));
            } else if after_forced || flags.quiet {
                goals.observe(&observed, None);
                goals.open_chain();
            }
        } else {
            goals.observe(&observed, None);
        }
        if goals.wants_advance() {
            let step = goals.advance(&ChainInputs {
                sea_available: hud.sea_unlocked,
                air_available: hud.air_unlocked,
                appeared: (0..campaign.cities.len()).filter(|&i| campaign.present(i)).count(),
                capital: campaign.approach,
            });
            if !flags.quiet && flags.callout.is_none() {
                // Step 6: the goals callout (once); step 7: the minimap spice callout (once).
                if step.claim_callout {
                    guide.queue.push(CLAIM_CALLOUT.to_string());
                }
                if step.spice_callout {
                    guide.queue.push(SPICE_CALLOUT.to_string());
                }
            }
        }
    }
    if keys.just_pressed(crate::cell_flow::DISMISS_KEY) {
        guide.dismiss();
    }
    hud.callouts = guide.queue.clone();
    // Hints. The forced chain: each step `ShowHint`s its hint when it begins and
    // `HideHint`s it (at once, no fade) when the player has done it (0xCF9D00 /
    // 0xCFAAF0); the rest are conditional hints of the manager.
    let want_step = (guide.intro_end.is_some() && tutorials && !guide.forced_complete()).then_some(guide.forced);
    let shown_step = guide.shown_step;
    if want_step != shown_step {
        if let Some(machine) = guide.machine.as_mut() {
            if let Some(done) = shown_step.and_then(|i| FORCED_HINTS.get(i)) {
                machine.complete(done, true);
            }
            if let Some(next) = want_step.and_then(|i| FORCED_HINTS.get(i)) {
                machine.show(next);
            }
        }
        guide.shown_step = want_step;
    }
    let owned: Vec<&civ::CityState> = campaign.cities.iter().filter(|c| c.owned()).collect();
    let shape = campaign.shape();
    let visible = |p: [f32; 3]| view.single().is_ok_and(|(camera, t)| on_screen(&shape, camera, t, Vec3::from(p)));
    let home = |u: &CivUnit| u.nation == HOME_INDEX && u.hp > 0.0;
    let damaged = units.iter().any(|(u, _)| home(u) && u.hp < u.max_hp * DAMAGED_VEHICLE);
    let unhappy = owned.iter().any(|c| c.rioting());
    let selected = |kind: Approach| units.iter().any(|(u, s)| s && home(u) && u.kind == kind);
    // The sticky flags: set by the player's orders (the retail target handlers
    // 0xCF3582 / 0xCF38BC / 0xCF3EEB), a damaged vehicle that is whole again
    // (`HasHealedVehicle`). ADAPTED: read off the vehicles' standing orders.
    for (u, _) in units.iter().filter(|(u, _)| home(u)) {
        match u.order {
            civ::UnitOrder::City(i) if !campaign.cities.get(i).is_some_and(|c| c.owned()) => match u.kind {
                Approach::Economic => guide.trade_route = true,
                _ => guide.attacked_city = true,
            },
            civ::UnitOrder::Geyser(_) => guide.mined = true,
            _ => {}
        }
        if u.trade_home.is_some() {
            guide.trade_route = true;
        }
    }
    guide.mined |= campaign.derricks(HOME_INDEX) >= 2;
    if guide.damaged_before && !damaged {
        guide.healed_vehicle = true;
    }
    guide.damaged_before = damaged;
    let conditions = Conditions {
        has_unhappy_city: unhappy,
        has_happy_city: !unhappy && !owned.is_empty(),
        city_hall_visible: campaign.cities.get(HOME_INDEX).is_some_and(|c| visible(c.position)),
        offscreen_city_attacked: owned
            .iter()
            .any(|c| c.calm < ATTACK_WINDOW && c.hp < c.max_hp() - 1.0 && !visible(c.position)),
        owns_damaged_vehicle: damaged,
        has_healed_vehicle: guide.healed_vehicle,
        military_selected: selected(Approach::Military),
        religious_selected: selected(Approach::Religious),
        economic_selected: selected(Approach::Economic),
        has_money: hud.spice > MONEY_LEVEL,
        trade_route: guide.trade_route,
        entered_building_planner: guide.entered_planner,
        player_attacked_city: guide.attacked_city,
        mined: guide.mined,
        one_player_city: owned.len() == 1,
        first_npc_city: campaign.cities.len() > 1 && (0..campaign.cities.len()).filter(|&i| campaign.present(i)).count() > 1,
        in_planner: false,
    };
    let callout_open = !hud.callouts.is_empty();
    let hints_on = crate::front_end::settings::hints_enabled();
    if !callout_open {
        // The game is held while a callout is up: the hints wait.
        if let Some(machine) = guide.machine.as_mut() {
            machine.update(dt, &|name| conditions.eval(name), hints_on);
        }
    }
    guide.hint_cuts = guide.machine.as_ref().map_or(0, HintMachine::cuts);
    let manager_hint = guide.machine.as_ref().and_then(|m| m.visible()).map(str::to_string);
    hud.hint = match &flags.hint {
        Some(id) => Some(id.clone()),
        None if flags.quiet || callout_open => None,
        // Settings > Game > Game Hints: the manager selects only the hints that
        // ignore it (`always`: the forced chain); the text line stays off.
        None if !hints_on => manager_hint,
        None => manager_hint.or_else(|| message_hint(&guide.message, guide.message_age)),
    };
    // Goals: the manager's list (no goals without the Tutorials option).
    hud.goal_cards = if tutorials { goal_cards(goals.active()) } else { Vec::new() };
    if goals != run.civ_campaign.goals {
        run.civ_campaign.goals = goals;
    }
}

/// The rectangle of a window of the Main or first Card layouts (callout
/// targets), if it is shown.
fn target_rect(roots: &Query<(&mut SpuiState, &CivHudRoot)>, control: u32) -> Option<Rect> {
    roots.iter().find_map(|(ui, kind)| {
        if !ui.ready || !matches!(kind, CivHudRoot::Main | CivHudRoot::Card(0)) {
            return None;
        }
        ui.find(control).filter(|w| ui.visible(*w)).map(|w| ui.rect(w))
    })
}

/// Bind the mission cards, the hint bar and the callout dialog.
#[allow(clippy::too_many_arguments)]
pub fn bind_guide(
    time: Res<Time>,
    hud: Res<CivHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<GuideCache>,
    mut guide: ResMut<GuideState>,
    mut roots: Query<(&mut SpuiState, &CivHudRoot)>,
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
    // Hint: the retail fade (1 s in, 2 s out) towards the published hint.
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
    let icon = hint_def.as_ref().and_then(|d| d.icon).and_then(|key| {
        assets.texture(index, &mut images, key).map(|_| ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None })
    });
    let alpha = cache.hint_bar.alpha();
    // Cards stack downwards by their measured height.
    let mut next_y = CARD_Y;
    let mut cards: Vec<(u8, f32)> = roots
        .iter()
        .filter_map(|(_, kind)| match kind {
            CivHudRoot::Card(n) => Some((*n, 0.0)),
            _ => None,
        })
        .collect();
    cards.sort_by_key(|(n, _)| *n);
    for (n, y) in &mut cards {
        *y = next_y;
        let height = roots.iter().find(|(_, kind)| **kind == CivHudRoot::Card(*n)).map_or(0.0, |(ui, _)| {
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
            CivHudRoot::Card(n) => {
                if hud.comm.is_some() {
                    // The communication screen covers the cards' corner.
                    if let Some(root) = ui.find(card::ROOT) {
                        ui.set_visible(root, false);
                    }
                } else if let (Some(goal), Some((_, y))) =
                    (hud.goal_cards.get(*n as usize), cards.iter().find(|(c, _)| c == n))
                {
                    bind_card(&mut ui, goal, *y, &mut assets, index);
                }
            }
            CivHudRoot::Callout => {
                dialogs::bind_callout(&mut ui, def.as_ref(), target, 0.5);
                for click in ui.take_clicks() {
                    if ui.layout.window(click).control_id == dialogs::callout::OK {
                        dismiss = true;
                    }
                }
            }
            CivHudRoot::Hint => dialogs::bind_hint(&mut ui, hint_def.as_ref(), alpha, icon.clone()),
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

    const ALL_GOALS: [Goal; 9] = [
        Goal::CaptureNode,
        Goal::ProfitableCity,
        Goal::HappyCity,
        Goal::SeaUnit,
        Goal::AirUnit,
        Goal::MilitaryCapture,
        Goal::EconomicCapture,
        Goal::ReligionCapture,
        Goal::CaptureAll,
    ];

    #[test]
    fn a_goal_card_is_its_goals_three_retail_strings_without_a_count() {
        for goal in ALL_GOALS {
            let card = goal_card(goal);
            assert!(card.progress.is_none(), "{goal:?}: no Civ goal prop has a goalValue");
        }
        let node = goal_card(Goal::CaptureNode);
        assert_eq!((node.kind.as_str(), node.title.as_str()), (text::CLAIM_KIND, text::CLAIM_HEADLINE));
        assert_eq!(node.body, text::CLAIM_BODY);
        let all = goal_card(Goal::CaptureAll);
        assert_eq!((all.kind.as_str(), all.title.as_str()), (text::ALL_KIND, text::ALL_HEADLINE));
        // The capture goals are by type: capture, convert, buy an opposing city.
        assert_eq!(goal_card(Goal::MilitaryCapture).title, text::MILITARY[2]);
        assert_eq!(goal_card(Goal::ReligionCapture).title, text::RELIGIOUS[2]);
        assert_eq!(goal_card(Goal::EconomicCapture).body, text::ECONOMIC[1]);
    }

    #[test]
    fn the_cards_follow_the_managers_list_in_the_order_the_goals_were_added() {
        assert!(goal_cards(&[]).is_empty());
        let cards = goal_cards(&[Goal::MilitaryCapture, Goal::SeaUnit, Goal::AirUnit]);
        assert_eq!(kinds(&cards), [text::MILITARY[0], text::SEA_KIND, text::AIR_KIND]);
        // Never more than the stack holds.
        assert_eq!(goal_cards(&ALL_GOALS).len(), MAX_CARDS);
    }

    #[test]
    fn a_held_game_is_a_forced_hint_or_an_open_callout() {
        let idle = GuideState::default();
        assert!(!idle.holds_world());
        let forced = GuideState { shown_step: Some(0), ..default() };
        assert!(forced.holds_world());
        let callout = GuideState { queue: vec!["CalloutCVGIntro".into()], ..default() };
        assert!(callout.holds_world());
        // The run condition reads the same.
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.insert_resource(callout);
        assert!(!app.world_mut().run_system_once(world_runs).unwrap());
        app.insert_resource(idle);
        assert!(app.world_mut().run_system_once(world_runs).unwrap());
        // Without the resource (a headless app) the world runs.
        assert!(App::new().world_mut().run_system_once(world_runs).unwrap());
    }

    use crate::hint_manager::{HintRule, KIND_NORMAL};

    /// A chain hint as installed: Triggered, no condition, no display time, `always`.
    fn chain_rule(id: &str) -> HintRule {
        HintRule { priority: 1, always: true, ..HintRule::new(id) }
    }

    fn run(m: &mut HintMachine, seconds: f32, c: Conditions) {
        for _ in 0..(seconds * 10.0).round() as u32 {
            m.update(0.1, &|name| c.eval(name), true);
        }
    }

    #[test]
    fn the_forced_tutorial_hints_follow_the_retail_steps() {
        // 0xCF9D00 `ShowHint`s the step's hint; the step's completion (0xCFAAF0)
        // `HideHint`s it at once and the chain shows the next.
        let mut m = HintMachine::new(FORCED_HINTS.iter().map(|id| chain_rule(id)).collect());
        run(&mut m, 5.0, Conditions::default());
        assert_eq!(m.visible(), None, "nothing is shown before the explicit call");
        for (step, id) in FORCED_HINTS.iter().enumerate() {
            m.show(id);
            run(&mut m, 0.2, Conditions::default());
            assert_eq!(m.visible(), Some(*id), "step {step}");
            // No display time and no condition: it stays until the step is done.
            run(&mut m, 120.0, Conditions::default());
            assert_eq!(m.visible(), Some(*id), "step {step} stays");
            let cuts = m.cuts();
            m.complete(id, true);
            assert_eq!((m.visible(), m.cuts()), (None, cuts + 1), "step {step} is cleared at once");
        }
        assert!(GuideState { forced: 4, ..default() }.forced_complete());
        assert!(!GuideState { forced: 3, ..default() }.forced_complete());
        // The chain hints also show with Game Hints off (`always`).
        let mut off = HintMachine::new(vec![chain_rule("cvg-camrotate")]);
        off.show("cvg-camrotate");
        off.update(0.1, &|_| false, false);
        assert_eq!(off.visible(), Some("cvg-camrotate"));
    }

    fn cityattacked() -> HintRule {
        // `cvg-cityattacked`: A [cvg, OffScreenPlayerCityAttacked], 2 s + 1 s, shown 5 s, Normal.
        HintRule {
            all_of: vec![shared::CVG, cond::OFFSCREEN_CITY_ATTACKED],
            first_delay: 2.0,
            second_delay: 1.0,
            display: 5.0,
            priority: 1,
            kind: KIND_NORMAL,
            ..HintRule::new("cvg-cityattacked")
        }
    }

    #[test]
    fn a_city_under_attack_off_screen_is_announced_after_its_delays_and_repeats() {
        let mut m = HintMachine::new(vec![cityattacked()]);
        let attacked = Conditions { offscreen_city_attacked: true, ..default() };
        run(&mut m, 2.0, attacked);
        assert_eq!(m.visible(), None, "2 s + 1 s first");
        run(&mut m, 2.0, attacked);
        assert_eq!(m.visible(), Some("cvg-cityattacked"));
        // 1 s in, 5 s shown, 2 s out; a Normal hint then starts waiting again.
        run(&mut m, 6.5, attacked);
        assert_eq!((m.visible(), m.state("cvg-cityattacked")), (None, 7));
        run(&mut m, 2.2, attacked);
        assert_eq!(m.state("cvg-cityattacked"), 1);
        // The attack ending before the second check cancels it.
        let mut quiet = HintMachine::new(vec![cityattacked()]);
        run(&mut quiet, 2.5, attacked);
        assert_eq!(quiet.state("cvg-cityattacked"), 2);
        run(&mut quiet, 1.5, default());
        assert_eq!((quiet.state("cvg-cityattacked"), quiet.visible()), (1, None));
    }

    #[test]
    fn the_unhappy_city_hint_ends_when_the_city_is_happy_again() {
        // `cvg-cityunhappy`: A [cvg, OnePlayerCity, HasUnhappyCity], B [UserBusy,
        // InPlanner], C [HasHappyCity], 2 s + 1 s, shown 10 s, Normal.
        let rule = HintRule {
            all_of: vec![shared::CVG, cond::ONE_PLAYER_CITY, cond::HAS_UNHAPPY_CITY],
            none_of: vec![shared::USER_BUSY, shared::IN_PLANNER],
            until: vec![cond::HAS_HAPPY_CITY],
            first_delay: 2.0,
            second_delay: 1.0,
            display: 10.0,
            priority: 1,
            kind: KIND_NORMAL,
            ..HintRule::new("cvg-cityunhappy")
        };
        let mut m = HintMachine::new(vec![rule]);
        let unhappy = Conditions { one_player_city: true, has_unhappy_city: true, ..default() };
        run(&mut m, 4.5, unhappy);
        assert_eq!(m.visible(), Some("cvg-cityunhappy"));
        let happy = Conditions { one_player_city: true, has_happy_city: true, ..default() };
        run(&mut m, 1.5, happy);
        assert_eq!(m.state("cvg-cityunhappy"), 6, "condition C ends it");
        // A second city takes the hint's first condition away.
        let mut two = HintMachine::new(vec![HintRule {
            all_of: vec![shared::CVG, cond::ONE_PLAYER_CITY, cond::HAS_UNHAPPY_CITY],
            first_delay: 2.0,
            ..HintRule::new("cvg-cityunhappy")
        }]);
        run(&mut two, 30.0, Conditions { has_unhappy_city: true, ..default() });
        assert_eq!(two.visible(), None);
    }

    #[test]
    fn the_planner_hint_needs_money_and_a_mined_geyser_and_waits_thirty_seconds() {
        // `cvg-plannerbuildings`: A [cvg, CityHallVisible, HasMoney, mined], B
        // [EnteredBuildingPlanner, UserBusy], 30 s + 15 s, shown 10 s, Normal.
        let rule = HintRule {
            all_of: vec![shared::CVG, cond::CITY_HALL_VISIBLE, cond::HAS_MONEY, cond::MINED],
            none_of: vec![cond::ENTERED_BUILDING_PLANNER, shared::USER_BUSY],
            until: vec![cond::ENTERED_BUILDING_PLANNER],
            first_delay: 30.0,
            second_delay: 15.0,
            display: 10.0,
            priority: 1,
            kind: KIND_NORMAL,
            ..HintRule::new("cvg-plannerbuildings")
        };
        let ready = Conditions { city_hall_visible: true, has_money: true, mined: true, ..default() };
        let mut m = HintMachine::new(vec![rule.clone()]);
        run(&mut m, 44.0, ready);
        assert_eq!(m.visible(), None);
        run(&mut m, 2.0, ready);
        assert_eq!(m.visible(), Some("cvg-plannerbuildings"));
        let mut poor = HintMachine::new(vec![rule]);
        run(&mut poor, 200.0, Conditions { has_money: false, ..ready });
        assert_eq!(poor.visible(), None);
    }

    #[test]
    fn the_checker_answers_the_civ_conditions() {
        for (id, name) in [
            (cond::HAS_UNHAPPY_CITY, "HasUnhappyCity"),
            (cond::HAS_HAPPY_CITY, "HasHappyCity"),
            (cond::CITY_HALL_VISIBLE, "CityHallVisible"),
            (cond::HARVESTER, "harvester"),
            (cond::OWNS_DAMAGED_VEHICLE, "OwnsDamagedVehicle"),
            (cond::HAS_HEALED_VEHICLE, "HasHealedVehicle"),
            (cond::MILITARY_SELECTED, "MilitaryVehiclesSelected"),
            (cond::RELIGIOUS_SELECTED, "ReligiousVehiclesSelected"),
            (cond::ECONOMIC_SELECTED, "EconomicVehiclesSelected"),
            (cond::HAS_MONEY, "HasMoney"),
            (cond::TRADE_ROUTE, "TradeRoute"),
            (cond::ENTERED_BUILDING_PLANNER, "EnteredBuildingPlanner"),
            (cond::PLAYER_ATTACKED_CITY, "PlayerAttackedCity"),
            (cond::MINED, "mined"),
            (cond::OFFSCREEN_CITY_ATTACKED, "OffScreenPlayerCityAttacked"),
            (cond::ONE_PLAYER_CITY, "OnePlayerCity"),
            (cond::FIRST_NPC_CITY, "FirstNPCCityHasAppeared"),
        ] {
            assert_eq!(id, crate::world_objects::name_id(name), "{name}");
        }
        let all = Conditions {
            has_unhappy_city: true,
            has_happy_city: true,
            city_hall_visible: true,
            offscreen_city_attacked: true,
            owns_damaged_vehicle: true,
            has_healed_vehicle: true,
            military_selected: true,
            religious_selected: true,
            economic_selected: true,
            has_money: true,
            trade_route: true,
            entered_building_planner: true,
            player_attacked_city: true,
            mined: true,
            one_player_city: true,
            first_npc_city: true,
            in_planner: true,
        };
        assert!(all.eval(shared::CVG) && all.eval(cond::MINED) && all.eval(shared::IN_PLANNER));
        // `UserBusy` and `Inactive` are constant false, `harvester` too in this
        // mode, and so is every name the Civ branch does not know.
        assert!(!all.eval(shared::USER_BUSY) && !all.eval(shared::INACTIVE) && !all.eval(cond::HARVESTER));
        assert!(!all.eval(shared::TRG) && !all.eval(0x1234_5678));
    }

    #[test]
    fn a_message_is_a_text_line_for_a_few_seconds() {
        assert_eq!(message_hint("Fleet sent against C2", 0.0).as_deref(), Some("msg:Fleet sent against C2"));
        assert_eq!(message_hint("Fleet sent against C2", MESSAGE_SECONDS + 0.1), None);
        assert_eq!(message_hint("", 0.0), None);
    }

    #[test]
    fn installed_hint_rules_match_the_decode() {
        let Some((index, _)) = installed() else {
            return;
        };
        let read = |id: &str| HintRule::read(&index, id).unwrap_or_else(|| panic!("{id}"));
        // The forced chain: Triggered, no condition, no display time, `always`, priority 1.
        for id in FORCED_HINTS {
            let rule = read(id);
            assert_eq!((rule.kind, rule.display, rule.priority, rule.always), (crate::hint_manager::KIND_TRIGGERED, 0.0, 1, true), "{id}");
            assert!(rule.all_of.is_empty() && rule.until.is_empty(), "{id}");
        }
        let attacked = read("cvg-cityattacked");
        assert_eq!(attacked.all_of, [shared::CVG, cond::OFFSCREEN_CITY_ATTACKED]);
        assert_eq!((attacked.first_delay, attacked.second_delay, attacked.display, attacked.kind), (2.0, 1.0, 5.0, KIND_NORMAL));
        let unhappy = read("cvg-cityunhappy");
        assert_eq!(unhappy.all_of, [shared::CVG, cond::ONE_PLAYER_CITY, cond::HAS_UNHAPPY_CITY]);
        assert_eq!(unhappy.none_of, [shared::USER_BUSY, shared::IN_PLANNER]);
        assert_eq!(unhappy.until, [cond::HAS_HAPPY_CITY]);
        let heal = read("cvg-healvehicle");
        assert_eq!(heal.all_of, [shared::CVG, cond::OWNS_DAMAGED_VEHICLE]);
        assert_eq!((heal.none_of.clone(), heal.until.clone()), (vec![cond::HAS_HEALED_VEHICLE], vec![cond::HAS_HEALED_VEHICLE]));
        assert_eq!((heal.first_delay, heal.second_delay, heal.display), (3.0, 8.0, 5.0));
        // The harvester hints list `harvester`, constant false in the Civ mode.
        for id in ["cvg-harvester", "cvg-mine"] {
            assert!(read(id).all_of.contains(&cond::HARVESTER), "{id}");
        }
        // Every hint of the list exists.
        for id in ALL_HINTS {
            assert!(HintRule::read(&index, id).is_some(), "{id}");
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

    // ---- installed data --------------------------------------------------

    fn installed() -> Option<(ResourceIndex, SpuiAssets)> {
        let install = crate::spore_install::discover().ok()?;
        let index = ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then(|| (index, SpuiAssets::default()))
    }

    #[test]
    fn installed_goal_strings_and_card_windows_exist() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let mut strings: Vec<String> = Vec::new();
        for goal in ALL_GOALS {
            let card = goal_card(goal);
            strings.extend([card.kind, card.title, card.body]);
        }
        for s in [
            text::SEA_KIND, text::SEA_BODY, text::SEA_HEADLINE, text::AIR_KIND, text::AIR_BODY, text::AIR_HEADLINE,
            text::HAPPY_KIND, text::HAPPY_BODY, text::HAPPY_HEADLINE,
        ] {
            strings.push(s.to_string());
        }
        for s in strings {
            assert!(!resolve(&mut assets, &index, &s).is_empty(), "{s}");
        }
        let layout = assets.layout(&index, super::super::CARD_LAYOUT).expect("CVGMissionCard");
        for control in [
            card::ROOT, card::KIND, card::COUNT, card::COUNT_BACKDROP, card::HEADLINE, card::DESCRIPTION, card::PANEL, card::GROUP,
            card::BUTTON, card::ANCHOR,
        ] {
            assert!(layout.find(control).is_some(), "card window {control:08X}");
        }
        assert_eq!(resolve(&mut assets, &index, text::ALL_HEADLINE), "Capture All Cities.");
        assert_eq!(resolve(&mut assets, &index, text::CLAIM_HEADLINE), "Claim a Spice Geyser.");
        assert_eq!(resolve(&mut assets, &index, text::HAPPY_HEADLINE), "Design a Happy City.");
        assert_eq!(resolve(&mut assets, &index, text::RELIGIOUS[2]), "Convert an Opposing City.");
    }

    #[test]
    fn installed_civ_callouts_and_hints_decode() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let intro = dialogs::read_callout(&index, &mut assets, "CalloutCVGIntro").expect("intro");
        assert_eq!(intro.title, "Welcome to Civilization");
        assert!(intro.text.starts_with("You now control the vehicles"));
        assert!(intro.targets.is_empty());
        let bar = dialogs::read_callout(&index, &mut assets, "CalloutCVGProgressBar").expect("progress bar");
        assert_eq!(bar.title, "Making Progress");
        assert_eq!(bar.targets, vec![super::super::id::BAR]);
        let forced = dialogs::read_callout(&index, &mut assets, "CalloutCVGForcedTutorialStart").expect("forced tutorial start");
        assert!(!forced.title.is_empty() && !forced.text.is_empty());
        let goals = dialogs::read_callout(&index, &mut assets, CLAIM_CALLOUT).expect("goals");
        assert_eq!(goals.title, "Goals");
        assert_eq!(goals.targets, vec![card::ANCHOR, card::PANEL, card::BUTTON]);
        let comm = dialogs::read_callout(&index, &mut assets, COMM_CALLOUT).expect("comm");
        assert_eq!(comm.title, "Communications Panel");
        assert_eq!(comm.targets, vec![0x065C_C9D8]);
        let weapon = dialogs::read_callout(&index, &mut assets, SUPERWEAPON_CALLOUT).expect("weapon");
        assert_eq!(weapon.title, "Consequence Abilities");
        assert_eq!(weapon.targets, vec![super::super::id::POWER_FRAME]);
        let advance = dialogs::read_callout(&index, &mut assets, FINAL_CALLOUTS[1]).expect("advance");
        assert_eq!(advance.title, "Advance to Space");
        assert_eq!(advance.targets, vec![super::super::id::ADVANCE]);
        let win = dialogs::read_callout(&index, &mut assets, FINAL_CALLOUTS[0]).expect("win");
        assert_eq!(win.title, "Congratulations!");
        for name in OPENING_CALLOUTS.iter().chain(&FINAL_CALLOUTS).chain(&[COMM_CALLOUT, SUPERWEAPON_CALLOUT, SEA_CALLOUT, AIR_CALLOUT, SPICE_CALLOUT]) {
            assert!(dialogs::read_callout(&index, &mut assets, name).is_some(), "{name}");
        }
        for id in [
            "cvg-camrotate", "cvg-camtranslate", "cvg-camzoom", "cvg-selectunit", "cvg-moveunit", "cvg-cityattacked", "cvg-cityunhappy", "cvg-healvehicle",
        ] {
            let hint = dialogs::read_hint(&index, &mut assets, id).unwrap_or_else(|| panic!("{id}"));
            assert!(!hint.before.is_empty(), "{id} has its retail text");
        }
        let attacked = dialogs::read_hint(&index, &mut assets, "cvg-cityattacked").unwrap();
        assert_eq!(attacked.before, "One of your cities is under attack");
        // The targets of the callouts exist in the layouts they live in.
        let main = assets.layout(&index, super::super::MAIN_LAYOUT).unwrap();
        for control in [super::super::id::BAR, super::super::id::POWER_FRAME, super::super::id::ADVANCE, 0x065C_C9D8] {
            assert!(main.find(control).is_some(), "callout target {control:08X}");
        }
        let card_layout = assets.layout(&index, super::super::CARD_LAYOUT).unwrap();
        for control in [card::ANCHOR, card::PANEL, card::BUTTON] {
            assert!(card_layout.find(control).is_some(), "callout target {control:08X}");
        }
    }

    #[test]
    fn the_twelve_ability_icons_are_pictures() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let def = read_power(&index, &mut assets, "cvg_clg_meat").expect("power file");
        assert_eq!(def.name, "Invulnerability");
        assert!(def.description.contains("impervious"));
        assert_eq!(def.icon.map(|k| k.type_id), Some(crate::ui_art::UI_PNG_TYPE));
        let bomb = read_power(&index, &mut assets, "cvg_trg_attackwin").expect("superweapon file");
        assert_eq!(bomb.name, "ICBM");
        assert!(read_power(&index, &mut assets, "no_such_power").is_none());
    }
}
