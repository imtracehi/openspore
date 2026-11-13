//! The communication screen with a rival nation (`CommScreen-3`), opened from
//! the nations' buttons beside the minimap or by a rival's own call.
//!
//! The retail screen is the one the Space stage uses: a speech panel on top
//! (the speaker, the nation's name and mood, the line it says) and the
//! response rows the code lays into window 0x04ADC3BF (0xDD3E60: text rows
//! 18 high, 0xFFFFB400 / disabled 0xFF7F7F7F). The conversation is the
//! installed `civ_main_menu` (`civ_comm`, 0xCFF010 actions): the greeting by
//! attitude and city type, compliments, insults, gifts of 4000 / 2000 /
//! 1000, the city purchase haggle, demands, war warnings, surrender.
//!
//! The three option buttons on the left open the Civ flows that do not start
//! from the greeting: Start Trade Route (0xCEEF90: accepted from Cautious up,
//! with the retail accepted / rejected answer), Buy City (the haggle, only
//! with a trade route) and Give Gift (the gift menu). ADAPTED: their place
//! on the left buttons (the retail Civ entry points are on the city rollover
//! and the vehicles).

use bevy::prelude::*;

use super::{CivHudActive, CivHudLocal, CivHudRoot, CivHudState};
use crate::{
    civ::{self, Approach, CivCampaign, HOME_INDEX},
    civ_comm::{self, CivComm, CommEnv},
    civ_rules::{self, RelationKind},
    game::RunProgress,
    space_comm::CnvLibrary,
    spui::{
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiPointer, SpuiState},
    },
    stages::GameStage,
};

pub const COMM_LAYOUT: &str = "CommScreen-3";

/// Control ids of `CommScreen-3`.
#[allow(dead_code)]
pub mod id {
    /// Bottom panel of options and the buttons on it.
    pub const OPTIONS: u32 = 0x05E4_E5F0;
    pub const GOODBYE: u32 = 0x05E4_F778;
    pub const ACCEPT: u32 = 0x05E4_F788;
    pub const DECLINE: u32 = 0x05E4_F770;
    pub const SMALL_BUTTON: u32 = 0x05DF_F098;
    /// The option group (hidden until shown) and the left three buttons:
    /// Trade, Repair and Recharge in Space; the cost of the last two is a
    /// button of its own.
    pub const GROUP: u32 = 0x05E3_C090;
    pub const SLOTS: [u32; 3] = [0x057D_F5CA, 0x057D_F5C8, 0x057D_F5C9];
    pub const COSTS: [u32; 2] = [0x066F_C580, 0x066F_C590];
    /// Wide centre buttons (Missions, Diplomacy) and the trading panel.
    pub const MISSIONS: u32 = 0x05E5_1D28;
    pub const DIPLOMACY: u32 = 0x05E5_1D30;
    pub const TRADING: u32 = 0x05E6_2A48;
    /// Speech panel: the line, the nation's name, its kind icons (economic,
    /// religious, military), the relationship face.
    pub const SPEECH: u32 = 0x0180_72CB;
    pub const NAME: u32 = 0x0493_82E8;
    pub const KINDS: [u32; 3] = [0x02CF_2F30, 0x02CF_2F34, 0x02CF_2F38];
    pub const FACE: u32 = 0x04A4_504F;
    pub const KIND_HOLDER: u32 = 0x0493_82D8;
    /// Plate behind the nation's name and the magenta portrait placeholder.
    pub const PLATE: u32 = 0x02CF_324C;
    pub const PORTRAIT_FILL: u32 = 0x0624_3260;
    /// The window the response rows are laid into (0xDD41F0).
    pub const RESPONSES: u32 = 0x04AD_C3BF;
}

/// Price of the old vehicle gift (`VehicleGiftCost`, the `VehicleGift`
/// relationship event; kept for [`apply_gift`]).
#[allow(dead_code)]
pub const GIFT_COST: f32 = civ_rules::VEHICLE_GIFT_COST;

/// Layout element number of the (hidden) group that holds the nation's
/// specialty icons.
const KIND_GROUP_ELEMENT: usize = 437;

/// Which page of options the screen shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CommMenu {
    #[default]
    Root,
    /// The diplomacy page (the retail Diplomacy button; not built yet).
    #[allow(dead_code)]
    Diplomacy,
}

/// The open screen: the nation spoken to and the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommSession {
    pub nation: usize,
    pub menu: CommMenu,
}

/// A click on the screen, applied by [`apply_comm_clicks`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommClick {
    Goodbye,
    Slot(usize),
    Row(usize),
}

/// What an option button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommActionKind {
    /// Open a trade route (economic route to taking the city).
    Trade,
    /// Haggle for the city once trade is open.
    BuyCity,
    /// Open the gift menu.
    Gift,
}

/// One option of the screen.
#[derive(Clone, Debug, PartialEq)]
pub struct CommAction {
    pub kind: CommActionKind,
    /// Retail label (CivGame table) and the price shown, if any.
    pub label: String,
    pub cost: Option<f32>,
    pub enabled: bool,
    pub tooltip: String,
}

/// What the screen displays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommState {
    pub nation: usize,
    pub name: String,
    pub kind: Option<Approach>,
    pub relation: f32,
    pub menu: CommMenu,
    /// What the nation says.
    pub speech: String,
    pub actions: Vec<CommAction>,
    /// The conversation's response rows (label, enabled).
    pub responses: Vec<(String, bool)>,
    /// The option buttons show (no talk, or the greeting menu); the rows
    /// then start right of them.
    pub show_slots: bool,
}

/// Retail labels of the options (CivGame / ui_space tables).
pub mod text {
    pub const TRADE: &str = "loc:ui_space:0627eb7c";
    pub const START_TRADE: &str = "loc:CivGame:05f4fc30";
    pub const BUY_CITY: &str = "loc:CivGame:04d1956a";
    pub const GIFT: &str = "loc:CivGame:04d194fe";
    pub const GOODBYE: &str = "loc:ui_space:0627eb7e";
}

/// Loaded strings of the screen.
#[derive(Resource, Default)]
pub struct CommCache {
    strings: std::collections::HashMap<String, String>,
}

impl CommCache {
    fn get(&mut self, key: &str, assets: &mut SpuiAssets, index: &crate::asset_catalog::ResourceIndex) -> String {
        self.strings
            .entry(key.to_string())
            .or_insert_with(|| super::guide::resolve(assets, index, key))
            .clone()
    }
}

// ---------------------------------------------------------------------------
// Rules of the option buttons
// ---------------------------------------------------------------------------

/// Open a trade route to a rival city (what an economic vehicle arriving at
/// the city does in `civ_rts`): it costs [`civ::TRADE_COST`] and is the
/// retail `Trade` relationship event. The nation accepts from Cautious up
/// (0xCEEF90) and never at war.
pub fn apply_trade(campaign: &mut CivCampaign, spice: &mut f32, nation: usize) -> Result<String, String> {
    let Some(city) = campaign.cities.get(nation) else {
        return Err("no such nation".into());
    };
    let name = city.label();
    if city.owned() {
        return Err(format!("{name} is already yours"));
    }
    if city.trade {
        return Err(format!("A trade route to {name} is already open"));
    }
    if campaign.at_war(nation) || !civ_comm::trade_route_accepted(campaign, nation) {
        return Err(format!("{name} will not trade with you until your relationship improves"));
    }
    if *spice < civ::TRADE_COST {
        return Err(format!("{name}: trade rights cost {:.0} spice", civ::TRADE_COST));
    }
    *spice -= civ::TRADE_COST;
    campaign.cities[nation].trade = true;
    campaign.nations[nation].relation.apply(RelationKind::Trade);
    Ok(format!("Trade route to {name} opened: economic vehicles deliver cargo, the city is now for sale"))
}

/// Spice still to pay for a city once trade is open (`None` without a trade
/// route, or for a city that is already the player's).
#[allow(dead_code)]
pub fn buy_price_left(city: &civ::CityState) -> Option<f32> {
    (!city.owned() && city.trade).then(|| (city.buy_price() - city.spent).max(0.0))
}

/// Buy the city outright: pay what is left of its price (the city card's
/// buy). The campaign update then hands the city over (`conquest_of`).
#[allow(dead_code)]
pub fn apply_buy(campaign: &mut CivCampaign, spice: &mut f32, nation: usize) -> Result<String, String> {
    let Some(city) = campaign.cities.get_mut(nation) else {
        return Err("no such nation".into());
    };
    let name = city.label();
    let Some(left) = buy_price_left(city) else {
        return Err(if city.owned() {
            format!("{name} is already yours")
        } else {
            format!("{name}: open a trade route first")
        });
    };
    if *spice < left {
        return Err(format!("{name}: buying it costs {left:.0} spice (you have {:.0})", *spice));
    }
    *spice -= left;
    city.spent = city.buy_price();
    Ok(format!("{name} is bought outright"))
}

/// Give a vehicle gift: costs [`GIFT_COST`] and is the retail `VehicleGift`
/// relationship event; a war ends by itself (`civ_campaign_update`) once the
/// relationship is back at neutral.
#[allow(dead_code)]
pub fn apply_gift(campaign: &mut CivCampaign, spice: &mut f32, nation: usize) -> Result<String, String> {
    if nation == HOME_INDEX || nation >= campaign.cities.len() {
        return Err("no such nation".into());
    }
    let name = civ::city_label(nation);
    if !campaign.nation_alive(nation) {
        return Err(format!("{name} is already yours"));
    }
    if *spice < GIFT_COST {
        return Err(format!("A gift costs {GIFT_COST:.0} spice"));
    }
    let Some(state) = campaign.nations.get_mut(nation) else {
        return Err("no such nation".into());
    };
    *spice -= GIFT_COST;
    state.relation.apply(RelationKind::VehicleGift);
    let note = match (state.war, state.relation.peaceful()) {
        (true, true) => " (the war ends)",
        (true, false) => " (still at war)",
        _ => "",
    };
    Ok(format!("{name} accepts the gift: relationship {:+.1}{note}", state.relation.value()))
}

/// The smallest gift of the gift menu.
pub fn smallest_gift() -> f32 {
    civ_comm::GIFT_AMOUNTS[2]
}

/// The options of a nation's screen from the campaign.
pub fn actions_for(campaign: &CivCampaign, spice: f32, nation: usize) -> Vec<CommAction> {
    let Some(city) = campaign.cities.get(nation) else {
        return Vec::new();
    };
    let mut trade_try = (campaign.clone(), spice);
    let trade = apply_trade(&mut trade_try.0, &mut trade_try.1, nation);
    let alive = campaign.nation_alive(nation) && nation != HOME_INDEX;
    let asked = (alive && city.trade).then(|| city.buy_price());
    let buy: Result<(), String> = match asked {
        None if !alive => Err(format!("{} is already yours", city.label())),
        None => Err(format!("{}: open a trade route first", city.label())),
        Some(a) if spice < civ_comm::buy_offers(a)[2] => Err(format!("{}: even the lowest offer is out of reach", city.label())),
        Some(_) => Ok(()),
    };
    let gift = smallest_gift();
    let gift_ok: Result<(), String> = if !alive {
        Err(format!("{} is already yours", city.label()))
    } else if spice < gift {
        Err(format!("The smallest gift is {gift:.0} spice"))
    } else {
        Ok(())
    };
    vec![
        CommAction {
            kind: CommActionKind::Trade,
            label: if city.trade { text::TRADE } else { text::START_TRADE }.to_string(),
            cost: (!city.trade && civ::TRADE_COST > 0.0).then_some(civ::TRADE_COST),
            enabled: trade.is_ok(),
            tooltip: match &trade {
                Ok(_) if civ::TRADE_COST > 0.0 => format!("Propose a trade route (${:.0})", civ::TRADE_COST),
                Ok(_) => "Propose a trade route".to_string(),
                Err(why) => why.clone(),
            },
        },
        CommAction {
            kind: CommActionKind::BuyCity,
            label: text::BUY_CITY.to_string(),
            cost: asked,
            enabled: buy.is_ok(),
            tooltip: match &buy {
                Ok(_) => format!("Make an offer for {} (asking ${:.0})", city.label(), asked.unwrap_or(0.0)),
                Err(why) => why.clone(),
            },
        },
        CommAction {
            kind: CommActionKind::Gift,
            label: text::GIFT.to_string(),
            cost: Some(gift),
            enabled: gift_ok.is_ok(),
            tooltip: match &gift_ok {
                Ok(_) => "Give a gift of 1,000 to 4,000: it improves your relationship".to_string(),
                Err(why) => why.clone(),
            },
        },
    ]
}

/// What the nation says when no installed conversation is available
/// (headless runs).
pub fn speech_for(campaign: &CivCampaign, nation: usize) -> String {
    let Some(city) = campaign.cities.get(nation) else {
        return String::new();
    };
    let name = city.label();
    let standing = if campaign.at_war(nation) {
        "We are at war with you.".to_string()
    } else if city.trade {
        let bought = (city.spent / city.buy_price().max(1.0)).clamp(0.0, 1.0) * 100.0;
        format!("Our trade route stands ({bought:.0}% of the price paid).")
    } else {
        "We are at peace with you.".to_string()
    };
    // (`+ 0.0` turns the empty sum's negative zero into a plain zero.)
    let attitude = campaign.nations.get(nation).map_or(0.0, |n| n.relation.value()) + 0.0;
    format!(
        "Greetings from {name}, a {} nation. {standing}\n\nOur attitude toward you is {attitude:+.1}. Our turrets stand ready ({}/{}), our faith is {:.0}% yours.",
        city.kind.title().to_lowercase(),
        city.live_turrets(),
        city.turret_hp.len(),
        city.conversion.clamp(0.0, 1.0) * 100.0
    )
}

/// Dialogs a fallen nation may still speak (its last words).
fn last_words(dialog: u32) -> bool {
    matches!(
        dialog,
        civ_comm::dialog::CAPTURE_FROM_NPC_MILITARY | civ_comm::dialog::CAPTURE_FROM_NPC_RELIGIOUS | civ_comm::dialog::NPC_SURRENDER
    )
}

/// Open / close the conversation with the screen and fill [`CommState`].
#[allow(clippy::too_many_arguments)]
pub fn publish_comm(
    stage: Res<State<GameStage>>,
    active: Res<CivHudActive>,
    run: Res<RunProgress>,
    originals: Option<Res<crate::Originals>>,
    mut lib: Option<ResMut<CnvLibrary>>,
    mut talk: ResMut<CivComm>,
    mut local: ResMut<CivHudLocal>,
    mut hud: ResMut<CivHudState>,
    mut was_open: Local<bool>,
) {
    if *stage.get() != GameStage::Civilization || !active.0 {
        if local.comm.is_some() && *stage.get() != GameStage::Civilization {
            local.comm = None;
        }
        hud.comm = None;
        return;
    }
    let campaign = &run.civ_campaign;
    let alive = |n: usize| n != HOME_INDEX && campaign.nation_alive(n);
    if talk.open {
        // A rival's call, or a talk the player started.
        local.comm = Some(CommSession { nation: talk.nation, menu: CommMenu::Root });
    } else if *was_open {
        // A choice ended the talk: the screen goes with it.
        local.comm = None;
    } else if let Some(session) = local.comm.filter(|s| alive(s.nation)) {
        let env = CommEnv {
            index: originals.as_deref().map(|o| &o.resource_index),
            difficulty: civ_rules::Difficulty::from_index(run.cell.difficulty),
        };
        if let Some(lib) = lib.as_deref_mut() {
            talk.contact(lib, campaign, super::sim::money(&run), &env, session.nation);
        }
    }
    if talk.open && !alive(talk.nation) && !last_words(talk.dialog) {
        talk.close();
        local.comm = None;
    }
    *was_open = talk.open;
    let Some(session) = local.comm else {
        hud.comm = None;
        return;
    };
    if session.nation >= campaign.cities.len() || (!alive(session.nation) && !(talk.open && last_words(talk.dialog))) {
        local.comm = None;
        hud.comm = None;
        return;
    }
    let city = &campaign.cities[session.nation];
    let speech = if talk.open && !talk.text.is_empty() { talk.text.clone() } else { speech_for(campaign, session.nation) };
    let state = CommState {
        nation: session.nation,
        name: civ::city_label(session.nation),
        kind: Some(city.kind),
        relation: super::sim::nation_relation(campaign, session.nation),
        menu: session.menu,
        speech,
        actions: actions_for(campaign, super::sim::money(&run), session.nation),
        responses: if talk.open { talk.responses.iter().map(|r| (r.label.clone(), r.enabled)).collect() } else { Vec::new() },
        show_slots: !talk.open || talk.dialog == civ_comm::dialog::PLAYER_CONTACT_NPC,
    };
    if hud.comm.as_ref() != Some(&state) {
        hud.comm = Some(state);
    }
}

/// Spawn the screen when a session opens and remove it when it closes.
pub fn manage_comm(
    mut commands: Commands,
    active: Res<CivHudActive>,
    hud: Res<CivHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &CivHudRoot)>,
) {
    let want = active.0 && hud.comm.is_some();
    let existing = roots.iter().find(|(_, kind)| **kind == CivHudRoot::Comm).map(|(e, _)| e);
    match (want, existing) {
        (false, Some(e)) => commands.entity(e).despawn(),
        (true, None) => {
            let (Ok(camera), index) = (camera.single(), &originals.resource_index) else {
                return;
            };
            if index.records.is_empty() {
                return;
            }
            if let Some(layout) = assets.layout(index, COMM_LAYOUT) {
                let root = view::spawn(&mut commands, layout, RootPolicy::Fit, Some(camera), 104);
                commands.entity(root).insert(CivHudRoot::Comm);
            }
        }
        _ => {}
    }
}

fn hide(ui: &mut SpuiState, controls: &[u32]) {
    for c in controls {
        super::set_visible(ui, *c, false);
    }
}

/// Child windows of a window.
fn children(ui: &SpuiState, parent: usize) -> Vec<usize> {
    ui.layout.window(parent).children.clone()
}

/// Bind the screen layout.
pub fn bind_comm(
    hud: Res<CivHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut cache: ResMut<CommCache>,
    mut roots: Query<(&mut SpuiState, &CivHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    for (mut ui, kind) in &mut roots {
        if !ui.ready || *kind != CivHudRoot::Comm {
            continue;
        }
        let Some(state) = hud.comm.as_ref() else {
            continue;
        };
        bind_screen(&mut ui, state, &mut assets, &mut cache, index);
    }
}

/// Bind one screen state to the layout (public for the tests).
pub fn bind_screen(
    ui: &mut SpuiState,
    state: &CommState,
    assets: &mut SpuiAssets,
    cache: &mut CommCache,
    index: &crate::asset_catalog::ResourceIndex,
) {
    // Speech panel.
    if let Some(w) = ui.find(id::SPEECH) {
        ui.set_text(w, state.speech.clone());
    }
    if let Some(w) = ui.find(id::NAME) {
        ui.set_text(w, format!("{} {}", state.name, state.kind.map_or("", |k| k.title())));
    }
    // The nation's kind: one of three icons (economic, religious, military).
    let shown = state.kind.map(|k| match k {
        Approach::Economic => 0,
        Approach::Religious => 1,
        Approach::Military => 2,
    });
    for (i, control) in id::KINDS.iter().enumerate() {
        super::set_visible(ui, *control, shown == Some(i));
    }
    if let Some(w) = ui.find(id::FACE) {
        ui.set_frame_grid(w, 5, 1);
        ui.set_frame(w, Some(super::relation_frame(state.relation)));
    }
    // The header plate is in the nation's colour; the portrait window, which
    // the retail code fills with a 3D view of the speaker, is a flat magenta
    // placeholder in the file and is hidden (ADAPTED: no speaker portrait).
    for w in ui.find_all(id::PLATE) {
        ui.set_tint(w, super::sim::nation_argb(state.nation));
    }
    super::set_visible(ui, id::PORTRAIT_FILL, false);
    // The group that holds the specialty icons is hidden in the file (the
    // retail code shows it with the speaker).
    if let Some(group) = ui.layout.windows.iter().find(|w| w.element == KIND_GROUP_ELEMENT).map(|w| w.id) {
        ui.set_visible(group, true);
    }
    // Options: the left three buttons carry the entry points; the wide
    // centre buttons, Accept / Decline and the trading panel stay away.
    hide(ui, &[id::MISSIONS, id::DIPLOMACY, id::ACCEPT, id::DECLINE, id::SMALL_BUTTON, id::TRADING]);
    super::set_visible(ui, id::GROUP, state.show_slots);
    for (slot, control) in id::SLOTS.iter().enumerate().filter(|_| state.show_slots) {
        let Some(button) = ui.find(*control) else {
            continue;
        };
        let action = state.actions.get(slot);
        ui.set_visible(button, action.is_some());
        let Some(action) = action else {
            continue;
        };
        ui.set_enabled(button, action.enabled);
        ui.set_tooltip(button, Some(action.tooltip.clone()));
        // The label is the child button with a caption.
        let label = children(ui, button).into_iter().find(|c| {
            matches!(ui.layout.window(*c).caption, Some(crate::spui::format::Text::Localized { .. }))
        });
        if let Some(label) = label {
            let text = cache.get(&action.label, assets, index);
            ui.set_text(label, text);
        }
        // Price: the button of its own (slots two and three), shown with an
        // amount only.
        if let Some(cost_control) = slot.checked_sub(1).and_then(|i| id::COSTS.get(i)) {
            if let Some(cost) = ui.find(*cost_control) {
                ui.set_visible(cost, action.cost.is_some());
                if let Some(c) = action.cost {
                    ui.set_text(cost, super::money_text(c));
                }
            }
        }
    }
    // Goodbye text (retail string, the layout already holds it).
    let _ = text::GOODBYE;
}

/// Clicks on the screen's buttons: queued for [`apply_comm_clicks`].
pub fn comm_actions(
    hud: &CivHudState,
    local: &mut CivHudLocal,
    roots: &mut Query<(&mut SpuiState, &CivHudRoot)>,
    _run: &mut RunProgress,
) {
    for (mut ui, kind) in roots.iter_mut() {
        if *kind != CivHudRoot::Comm {
            continue;
        }
        for click in ui.take_clicks() {
            let control = ui.layout.window(click).control_id;
            if hud.comm.is_none() {
                continue;
            }
            if control == id::GOODBYE {
                local.comm_click = Some(CommClick::Goodbye);
            } else if let Some(slot) = id::SLOTS.iter().position(|c| *c == control) {
                local.comm_click = Some(CommClick::Slot(slot));
            }
        }
    }
}

/// Apply the queued click: Goodbye ends the talk, the option buttons open
/// their flows, a row runs its response (`CivComm::choose`).
pub fn apply_comm_clicks(
    hud: Res<CivHudState>,
    originals: Option<Res<crate::Originals>>,
    mut lib: Option<ResMut<CnvLibrary>>,
    mut talk: ResMut<CivComm>,
    mut local: ResMut<CivHudLocal>,
    mut run: ResMut<RunProgress>,
) {
    let Some(click) = local.comm_click.take() else {
        return;
    };
    let Some(state) = hud.comm.as_ref() else {
        return;
    };
    let nation = state.nation;
    let env = CommEnv {
        index: originals.as_deref().map(|o| &o.resource_index),
        difficulty: civ_rules::Difficulty::from_index(run.cell.difficulty),
    };
    let mut fallback = CnvLibrary::default();
    let lib = match lib.as_deref_mut() {
        Some(l) => l,
        None => &mut fallback,
    };
    let run = &mut *run;
    let mut money = super::sim::money(run);
    match click {
        CommClick::Goodbye => {
            talk.close();
            local.comm = None;
        }
        CommClick::Row(i) => {
            talk.choose(lib, &mut run.civ_campaign, &mut money, &env, i);
        }
        CommClick::Slot(slot) => {
            let Some(action) = state.actions.get(slot).filter(|a| a.enabled) else {
                return;
            };
            match action.kind {
                CommActionKind::Trade => {
                    let result = apply_trade(&mut run.civ_campaign, &mut money, nation);
                    let answer = match &result {
                        Ok(_) => civ_comm::dialog::TRADE_ROUTE_ACCEPTED,
                        Err(_) => civ_comm::dialog::TRADE_ROUTE_REJECTED,
                    };
                    run.civ_campaign.message = match result {
                        Ok(m) | Err(m) => m,
                    };
                    talk.open(lib, &run.civ_campaign, money, &env, nation, answer);
                    talk.city = Some(nation);
                }
                CommActionKind::BuyCity => {
                    if let Err(why) = talk.buy(lib, &run.civ_campaign, money, &env, nation) {
                        run.civ_campaign.message = why;
                    }
                }
                CommActionKind::Gift => {
                    talk.open(lib, &run.civ_campaign, money, &env, nation, civ_comm::dialog::NPC_GIFT);
                }
            }
        }
    }
    super::sim::set_money(run, money);
    run.request_save();
}

// ---------------------------------------------------------------------------
// The code-made response rows (0xDD3E60), as on the Space screen.
// ---------------------------------------------------------------------------

/// Retail response row height on the 1280x900 layout, colours and style.
pub const ROW_HEIGHT: f32 = 18.0;
pub const ROW_COLOUR: u32 = 0xFFFF_B400;
pub const ROW_DISABLED: u32 = 0xFF7F_7F7F;
pub const ROW_STYLE: u32 = 0x5163_044D;
const ROWS: usize = 14;

#[derive(Component)]
pub struct CivCommRows;

#[derive(Component)]
pub struct CivCommRow(pub usize);

fn argb(c: u32) -> Color {
    Color::srgba_u8((c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8)
}

/// Row rectangles inside the responses window (logical pixels).
pub fn row_rects(area: Rect, count: usize, scale: f32) -> Vec<Rect> {
    let h = ROW_HEIGHT * scale;
    (0..count)
        .map(|i| {
            let y = area.min.y + 2.0 * scale + h * i as f32;
            Rect::new(area.min.x + 4.0 * scale, y, area.max.x - 2.0 * scale, y + h)
        })
        .collect()
}

/// Spawn the row overlay while the HUD is up.
pub fn manage_rows(
    mut commands: Commands,
    active: Res<CivHudActive>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<Entity, With<CivCommRows>>,
) {
    if !active.0 {
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    if !roots.is_empty() {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let root = commands
        .spawn((
            CivCommRows,
            crate::stages::StageOwned,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            UiTargetCamera(camera),
            Pickable::IGNORE,
            GlobalZIndex(105),
        ))
        .id();
    commands.entity(root).with_children(|root| {
        for i in 0..ROWS {
            root.spawn((
                CivCommRow(i),
                Text::new(""),
                TextFont::default(),
                TextColor(argb(ROW_COLOUR)),
                TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::NoWrap),
                Node { position_type: PositionType::Absolute, ..default() },
                Visibility::Hidden,
                Pickable::IGNORE,
            ));
        }
    });
}

/// Place and fill the rows; a released click on a row is queued.
#[allow(clippy::too_many_arguments)]
pub fn update_rows(
    hud: Res<CivHudState>,
    pointer: Res<SpuiPointer>,
    mut local: ResMut<CivHudLocal>,
    mut assets: ResMut<SpuiAssets>,
    originals: Option<Res<crate::Originals>>,
    roots: Query<(&SpuiState, &CivHudRoot)>,
    mut rows: Query<(&CivCommRow, &mut Node, &mut Text, &mut TextColor, &mut TextFont, &mut Visibility)>,
    mut was_pressed: Local<bool>,
    mut font: Local<Option<(bevy::text::FontSource, f32)>>,
) {
    let ui = roots.iter().find(|(ui, k)| **k == CivHudRoot::Comm && ui.ready).map(|(ui, _)| ui);
    let responses = hud.comm.as_ref().map(|c| c.responses.clone()).unwrap_or_default();
    let slots = hud.comm.as_ref().is_some_and(|c| c.show_slots);
    let (rects, scale, screen) = match ui.and_then(|ui| ui.find(id::RESPONSES).map(|w| (ui, ui.rect(w)))) {
        Some((ui, r)) => {
            let scale = (ui.screen[1] / 900.0).min(ui.screen[0] / 1280.0).max(0.25);
            let mut area = Rect::new(r.x1, r.y1, r.x2, r.y2);
            if slots {
                // Beside the option buttons, not over them.
                let right = id::SLOTS.iter().filter_map(|c| ui.find(*c)).map(|w| ui.rect(w).x2).fold(area.min.x, f32::max);
                area.min.x = area.min.x.max(right + 12.0 * scale);
            }
            (row_rects(area, responses.len(), scale), scale, ui.screen)
        }
        None => (Vec::new(), 1.0, [1280.0, 900.0]),
    };
    if font.is_none() {
        if let Some(o) = originals.as_ref() {
            let style = assets.styles(&o.resource_index).style(ROW_STYLE);
            let family = style.family.clone();
            let size = if style.size_px > 1.0 { style.size_px } else { 10.0 };
            *font = Some((assets.font_source(&family), size));
        }
    }
    let pct = |v: f32, total: f32| Val::Percent(100.0 * v / total.max(1.0));
    let hover = pointer.position.and_then(|p| rects.iter().position(|r| r.contains(p)));
    for (row, mut node, mut text, mut colour, mut tf, mut vis) in &mut rows {
        let (Some(rect), Some((label, enabled))) = (rects.get(row.0), responses.get(row.0)) else {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        };
        node.left = pct(rect.min.x, screen[0]);
        node.top = pct(rect.min.y, screen[1]);
        if text.0 != *label {
            text.0 = label.clone();
        }
        let c = match (*enabled, hover == Some(row.0)) {
            (false, _) => ROW_DISABLED,
            (true, true) => 0xFFFF_FFFF,
            (true, false) => ROW_COLOUR,
        };
        colour.0 = argb(c);
        if let Some((source, size)) = font.as_ref() {
            if tf.font != *source {
                tf.font = source.clone();
            }
            let px = bevy::text::FontSize::Px((size * crate::spui::view::FONT_SCALE * scale).round().max(6.0));
            if tf.font_size != px {
                tf.font_size = px;
            }
        }
        *vis = Visibility::Inherited;
    }
    let pressed = pointer.pressed;
    if *was_pressed && !pressed {
        if let Some(i) = hover.filter(|i| responses.get(*i).is_some_and(|r| r.1)) {
            local.comm_click = Some(CommClick::Row(i));
        }
    }
    *was_pressed = pressed;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campaign() -> CivCampaign {
        let mut c = CivCampaign::default();
        c.ensure_cities();
        c
    }

    #[test]
    fn a_trade_route_opens_once_and_warms_the_nation() {
        let mut c = campaign();
        let mut spice = 100.0;
        assert!(apply_trade(&mut c, &mut spice, 2).is_ok());
        assert_eq!(spice, 100.0 - civ::TRADE_COST);
        assert!(c.cities[2].trade);
        assert!(c.nations[2].relation.contribution(RelationKind::Trade) > 0.0, "the retail Trade event");
        let again = apply_trade(&mut c, &mut spice, 2).unwrap_err();
        assert!(again.contains("already open"), "{again}");
        assert_eq!(spice, 100.0 - civ::TRADE_COST, "no second charge");
        // A nation below neutral will not trade.
        c.nations[3].relation.apply(RelationKind::DeclaredWar);
        let refused = apply_trade(&mut c, &mut spice, 3).unwrap_err();
        assert!(refused.contains("will not trade"), "{refused}");
        assert!(!c.cities[3].trade);
        // The capital is never a trading partner.
        assert!(apply_trade(&mut c, &mut spice, 0).unwrap_err().contains("already yours"));
        assert!(apply_trade(&mut c, &mut spice, 99).is_err());
    }

    #[test]
    fn buying_a_city_needs_a_route_and_the_rest_of_the_retail_price() {
        let mut c = campaign();
        let mut spice = 100_000.0;
        assert!(apply_buy(&mut c, &mut spice, 1).unwrap_err().contains("trade route first"));
        apply_trade(&mut c, &mut spice, 1).unwrap();
        let before = spice;
        c.cities[1].spent = 50.0;
        let left = buy_price_left(&c.cities[1]).unwrap();
        // The price is the rules' (retail buy price, at least 2000).
        assert_eq!(left, c.cities[1].buy_price() - 50.0);
        assert!(c.cities[1].buy_price() >= civ_rules::BUY_PRICE_MIN);
        assert!(apply_buy(&mut c, &mut spice, 1).is_ok());
        assert_eq!(spice, before - left);
        // The economic route now holds: the campaign update takes the city.
        assert_eq!(civ::conquest_of(&c.cities[1]), Some(civ::Conquest::Economic));
        // Too poor to buy.
        let mut c = campaign();
        let mut rich = 100_000.0;
        apply_trade(&mut c, &mut rich, 2).unwrap();
        let mut poor = 5.0;
        assert!(apply_buy(&mut c, &mut poor, 2).unwrap_err().contains("costs"));
        assert_eq!(poor, 5.0);
        assert!(buy_price_left(&c.cities[0]).is_none());
    }

    #[test]
    fn gifts_warm_a_nation_until_the_war_can_end() {
        let mut c = campaign();
        c.nations[1].war = true;
        c.nations[1].relation.apply(RelationKind::DeclaredWar);
        let start = c.nations[1].relation.value();
        let mut spice = 1000.0;
        let message = apply_gift(&mut c, &mut spice, 1).unwrap();
        assert!(message.contains("still at war"), "{message}");
        assert_eq!(spice, 1000.0 - GIFT_COST);
        assert!(c.nations[1].relation.value() > start, "the VehicleGift event");
        // Gifts keep adding (+1 each, up to +10): five bring -5 back to neutral.
        let mut last = String::new();
        for _ in 0..4 {
            last = apply_gift(&mut c, &mut spice, 1).unwrap();
        }
        assert!(c.nations[1].relation.peaceful());
        assert!(last.contains("the war ends"), "{last}");
        // A gift in peace is only a thank you.
        assert!(!apply_gift(&mut c, &mut spice, 2).unwrap().contains("war"));
        // Poor: refused, nothing taken.
        let mut poor = 99.0;
        assert!(apply_gift(&mut c, &mut poor, 3).is_err());
        assert_eq!(poor, 99.0);
        assert!(apply_gift(&mut c, &mut spice, HOME_INDEX).is_err());
    }

    #[test]
    fn the_options_follow_what_the_rules_allow() {
        let mut c = campaign();
        let rich = actions_for(&c, 1000.0, 2);
        assert_eq!(rich.iter().map(|a| a.kind).collect::<Vec<_>>(), [CommActionKind::Trade, CommActionKind::BuyCity, CommActionKind::Gift]);
        assert_eq!(rich.iter().map(|a| a.enabled).collect::<Vec<_>>(), [true, false, true], "no route: no purchase yet");
        assert_eq!(rich[0].cost, None, "the retail trade rights are free");
        assert_eq!(rich[0].label, text::START_TRADE);
        assert!(rich[1].tooltip.contains("trade route"), "{:?}", rich[1]);
        c.cities[2].trade = true;
        // The retail price is out of reach of 1000 but not of a full purse.
        let broke = actions_for(&c, 1000.0, 2);
        assert!(!broke[1].enabled, "{:?}", broke[1]);
        let after = actions_for(&c, 50_000.0, 2);
        assert_eq!(after.iter().map(|a| a.enabled).collect::<Vec<_>>(), [false, true, true]);
        assert_eq!(after[0].label, text::TRADE, "the route is open: only 'Trade' is left on the button");
        assert_eq!(after[0].cost, None);
        assert_eq!(after[1].cost, buy_price_left(&c.cities[2]));
        assert_eq!(after[2].cost, Some(smallest_gift()), "the gift menu starts at 1,000");
        let poor = actions_for(&c, 10.0, 2);
        assert_eq!(poor.iter().map(|a| a.enabled).collect::<Vec<_>>(), [false, false, false]);
        // At war: no trade, a gift still goes.
        let mut war = campaign();
        war.nations[1].war = true;
        war.nations[1].relation.apply(RelationKind::DeclaredWar);
        let at_war = actions_for(&war, 1000.0, 1);
        assert_eq!(at_war.iter().map(|a| a.enabled).collect::<Vec<_>>(), [false, false, true]);
        assert!(at_war[0].tooltip.contains("will not trade"), "{:?}", at_war[0]);
        assert!(actions_for(&c, 10.0, 99).is_empty());
    }

    #[test]
    fn the_speech_reports_the_nations_standing() {
        let mut c = campaign();
        let peace = speech_for(&c, 2);
        assert!(peace.contains("Greetings from C3"), "{peace}");
        assert!(peace.contains("at peace"));
        assert!(peace.contains("religious"));
        assert!(peace.contains("attitude toward you is +0.0"), "{peace}");
        c.nations[2].war = true;
        assert!(speech_for(&c, 2).contains("at war"));
        c.nations[2].war = false;
        c.cities[2].trade = true;
        c.cities[2].spent = c.cities[2].buy_price() / 2.0;
        assert!(speech_for(&c, 2).contains("50%"));
        assert_eq!(speech_for(&c, 99), "");
    }

    // ---- installed layout -----------------------------------------------

    fn installed() -> Option<(crate::asset_catalog::ResourceIndex, SpuiAssets)> {
        let install = crate::spore_install::discover().ok()?;
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then(|| (index, SpuiAssets::default()))
    }

    #[test]
    fn installed_comm_layout_has_every_window_the_binding_uses() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, COMM_LAYOUT).expect("CommScreen-3");
        for control in [
            id::OPTIONS, id::GOODBYE, id::ACCEPT, id::DECLINE, id::SMALL_BUTTON, id::GROUP, id::MISSIONS, id::DIPLOMACY, id::TRADING,
            id::SPEECH, id::NAME, id::FACE, id::KIND_HOLDER, id::PLATE, id::PORTRAIT_FILL,
        ]
        .iter()
        .chain(&id::SLOTS)
        .chain(&id::COSTS)
        .chain(&id::KINDS)
        {
            assert!(layout.find(*control).is_some(), "window {control:08X}");
        }
        // The cost buttons belong to slots two and three.
        for (slot, cost) in [(1, 0), (2, 1)] {
            let s = layout.find(id::SLOTS[slot]).unwrap();
            let c = layout.find(id::COSTS[cost]).unwrap();
            assert_eq!(layout.window(c).parent, Some(s), "cost {cost}");
        }
        // Every slot has a captioned label child.
        for control in id::SLOTS {
            let s = layout.find(control).unwrap();
            assert!(
                layout.window(s).children.iter().any(|c| matches!(layout.window(*c).caption, Some(crate::spui::format::Text::Localized { .. }))),
                "slot {control:08X} has a localized label"
            );
        }
    }

    #[test]
    fn retail_labels_resolve() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let mut cache = CommCache::default();
        assert_eq!(cache.get(text::TRADE, &mut assets, &index), "Trade");
        assert_eq!(cache.get(text::START_TRADE, &mut assets, &index), "Start Trade Route");
        assert_eq!(cache.get(text::BUY_CITY, &mut assets, &index), "Buy City");
        assert_eq!(cache.get(text::GIFT, &mut assets, &index), "Give Gift");
        assert_eq!(cache.get(text::GOODBYE, &mut assets, &index), "Goodbye");
    }

    #[test]
    fn the_screen_shows_the_nation_and_its_options() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, COMM_LAYOUT).unwrap();
        let mut ui = SpuiState::new(layout, RootPolicy::Fit);
        let mut cache = CommCache::default();
        let mut c = campaign();
        c.cities[2].trade = true;
        let state = CommState {
            nation: 2,
            name: "C3".into(),
            kind: Some(Approach::Religious),
            relation: 0.7,
            menu: CommMenu::Root,
            speech: speech_for(&c, 2),
            actions: actions_for(&c, 50_000.0, 2),
            responses: Vec::new(),
            show_slots: true,
        };
        bind_screen(&mut ui, &state, &mut assets, &mut cache, &index);
        let w = |ui: &SpuiState, c: u32| ui.find(c).unwrap();
        assert_eq!(ui.captions[w(&ui, id::NAME)].as_deref(), Some("C3 Religious"));
        assert!(ui.captions[w(&ui, id::SPEECH)].as_deref().unwrap().contains("Greetings from C3"));
        assert_eq!(ui.frame_overrides[w(&ui, id::FACE)], Some(4));
        // Religious: the middle kind icon only.
        assert!(!ui.visible(w(&ui, id::KINDS[0])) && ui.visible(w(&ui, id::KINDS[1])) && !ui.visible(w(&ui, id::KINDS[2])));
        // The route is open: Trade is disabled and plain, Buy City costs the rest of the price.
        let button = |slot: usize| w(&ui, id::SLOTS[slot]);
        assert!(ui.flags[button(0)] & crate::spui::ids::flag::ENABLED == 0);
        assert!(ui.flags[button(1)] & crate::spui::ids::flag::ENABLED != 0);
        let left = buy_price_left(&c.cities[2]).unwrap();
        assert_eq!(ui.captions[w(&ui, id::COSTS[0])].as_deref(), Some(super::super::money_text(left).as_str()));
        assert_eq!(ui.captions[w(&ui, id::COSTS[1])].as_deref(), Some("$1,000"));
        let label = |slot: usize| {
            ui.layout.window(button(slot)).children.iter().find_map(|c| {
                matches!(ui.layout.window(*c).caption, Some(crate::spui::format::Text::Localized { .. }))
                    .then(|| ui.captions[*c].clone())
                    .flatten()
            })
        };
        assert_eq!(label(0).as_deref(), Some("Trade"));
        assert_eq!(label(1).as_deref(), Some("Buy City"));
        assert_eq!(label(2).as_deref(), Some("Give Gift"));
        // Missions, Diplomacy, Accept and Decline are not offered.
        for control in [id::MISSIONS, id::DIPLOMACY, id::ACCEPT, id::DECLINE] {
            assert!(!ui.visible(w(&ui, control)), "{control:08X}");
        }
    }

    fn comm_app() -> App {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        app.update();
        app.update();
        app
    }

    #[test]
    fn a_session_is_published_and_closes_when_its_nation_falls() {
        let mut app = comm_app();
        app.world_mut().insert_resource(CivHudActive(true));
        app.world_mut().resource_mut::<CivHudLocal>().comm = Some(CommSession { nation: 2, menu: CommMenu::Root });
        app.update();
        let state = app.world().resource::<CivHudState>().comm.clone().expect("published");
        assert_eq!(state.nation, 2);
        assert_eq!(state.actions.len(), 3);
        app.world_mut().resource_mut::<RunProgress>().civ_campaign.cities[2].owner = civ::CityOwner::Player;
        app.update();
        assert!(app.world().resource::<CivHudState>().comm.is_none());
        assert!(app.world().resource::<CivHudLocal>().comm.is_none(), "the screen closed");
    }

    #[test]
    fn clicking_an_option_acts_and_goodbye_closes() {
        let mut app = comm_app();
        app.world_mut().insert_resource(CivHudActive(true));
        app.world_mut().resource_mut::<RunProgress>().spice = 1000.0;
        app.world_mut().resource_mut::<CivHudLocal>().comm = Some(CommSession { nation: 3, menu: CommMenu::Root });
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, COMM_LAYOUT).unwrap();
        let root = app.world_mut().spawn((CivHudRoot::Comm, SpuiState::new(layout, RootPolicy::Fit))).id();
        app.update();
        let click = |app: &mut App, control: u32| {
            let mut ui = app.world_mut().get_mut::<SpuiState>(root).unwrap();
            let w = ui.find(control).unwrap();
            ui.clicks.push(w);
            app.update();
            app.update();
        };
        click(&mut app, id::SLOTS[0]);
        let run = app.world().resource::<RunProgress>();
        assert!(run.civ_campaign.cities[3].trade, "the trade route is open");
        assert_eq!(run.spice, 1000.0 - civ::TRADE_COST);
        assert!(run.civ_campaign.message.contains("Trade route"));
        assert_eq!(app.world().resource::<CivComm>().dialog, civ_comm::dialog::TRADE_ROUTE_ACCEPTED);
        // The gift button (slot three) opens the gift menu; a row gives.
        click(&mut app, id::SLOTS[2]);
        let talk = app.world().resource::<CivComm>().clone();
        assert_eq!(talk.dialog, civ_comm::dialog::NPC_GIFT);
        if let Some(row) = talk.responses.iter().position(|r| r.action == civ_comm::action::GIFTS[2]) {
            app.world_mut().resource_mut::<CivHudLocal>().comm_click = Some(CommClick::Row(row));
            app.update();
            app.update();
            assert_eq!(app.world().resource::<RunProgress>().spice, 1000.0 - civ::TRADE_COST - 1000.0);
            assert_eq!(app.world().resource::<CivComm>().dialog, civ_comm::dialog::NPC_RECEIVE_GIFT);
        }
        // Goodbye closes the screen.
        click(&mut app, id::GOODBYE);
        assert!(app.world().resource::<CivHudLocal>().comm.is_none());
        assert!(!app.world().resource::<CivComm>().open);
    }
}
