//! The communication screen of an alien empire (`CommScreen-3`, shared with
//! the Civilization stage) and the species/communication panel that opens it.
//!
//! The screen shows the retail conversation of the empire
//! (`space_comm::CommSession`, `space_conversation_main_menu.cnv`): the line it
//! says (chosen by the conversation flags), the responses as a column of
//! orange text buttons in window 0x04ADC3BF (retail makes them in code,
//! 0xDD3E60: 18 px rows, 0xFFFFB400, disabled 0xFF7F7F7F), the tabs Trade /
//! Repair / Recharge / Missions / Diplomacy on the main menu only, Accept /
//! Decline when the dialog asks for them, and the trading controls on the
//! Trade tab (NPC stock with the archetype prices, the hold's spice to sell).
//! Repair and Recharge are the retail purchases at the per-point price of the
//! planet's owner (S2C). The comm button glows while a transmission waits
//! (`space_universe` inbox); pressing it then answers the transmission.

use bevy::prelude::*;

use super::{SpaceHudRoot, SpaceHudState};
use crate::{
    game::{Health, Planet, Player, RunProgress},
    persistence::WorldId,
    space::SpaceCampaign,
    space_comm::{self as sc, CnvLibrary, CommEnv, CommSession, TradeSide},
    space_rules::{self as rules, PlanetOwner},
    spui::view::{SpuiAssets, SpuiPointer, SpuiState},
    stages::GameStage,
};

pub const COMM_LAYOUT: &str = "CommScreen-3";

/// Control ids of `CommScreen-3` (S3E, ModAPI `CommWindowIDs`, the layout).
#[allow(dead_code)]
pub mod cid {
    /// The whole screen (slides in) and its two halves.
    pub const SCREEN: u32 = 0x01C3_BB0C;
    pub const BOTTOM: u32 = 0x05E4_E5F0;
    pub const SPEAKER: u32 = 0x05E4_E5F8;
    pub const GOODBYE: u32 = 0x05E4_F778;
    pub const ACCEPT: u32 = 0x05E4_F788;
    pub const DECLINE: u32 = 0x05E4_F770;
    /// Tab group and its buttons.
    pub const TABS: u32 = 0x05E3_C090;
    pub const TAB_TRADE: u32 = 0x057D_F5CA;
    pub const TAB_REPAIR: u32 = 0x057D_F5C8;
    pub const TAB_RECHARGE: u32 = 0x057D_F5C9;
    /// Price buttons of the Repair and Recharge tabs.
    pub const PRICE_REPAIR: u32 = 0x066F_C580;
    pub const PRICE_RECHARGE: u32 = 0x066F_C590;
    pub const TAB_MISSIONS: u32 = 0x05E5_1D28;
    pub const TAB_DIPLOMACY: u32 = 0x05E5_1D30;
    /// Trading controls (cargo, price, add / remove, count, buy / sell).
    pub const TRADING: u32 = 0x05E6_2A48;
    pub const TRADE_PRICE: u32 = 0x04C1_B588;
    pub const ADD_ONE: u32 = 0x04C1_510F;
    pub const ADD_MANY: u32 = 0x05DB_90D0;
    pub const REMOVE_ONE: u32 = 0x04C1_510D;
    pub const REMOVE_MANY: u32 = 0x05DB_90D1;
    pub const COUNT: u32 = 0x04C1_510E;
    pub const BUY: u32 = 0x04C1_510A;
    pub const SELL: u32 = 0x04C1_510C;
    pub const TRADE_MESSAGE: u32 = 0x04D1_49A8;
    pub const PLAYER_INVENTORY: u32 = 0x05DA_83B9;
    pub const NPC_INVENTORY: u32 = 0x05DA_83B8;
    /// The satellite button: back to the main menu (ModAPI `kBtnMenu`).
    pub const MENU: u32 = 0x05DF_F098;
    /// The response list (0xDD41F0 keeps it at +0x24).
    pub const RESPONSES: u32 = 0x04AD_C3BF;
    pub const NATION_NAME: u32 = 0x0493_82E8;
    pub const GREETING: u32 = 0x0180_72CB;
    /// The portrait frame: the design-time fill (purple) is cleared.
    pub const PORTRAIT_FILL: u32 = 0x0624_3260;
    /// The speaker's viewport panel (ModAPI `kWindowPanelViewport`).
    pub const VIEWPORT: u32 = 0x0642_9C80;
    /// The windows the retail loader (0xDD41F0) puts a `ScrollFrameVerticalGeneric`
    /// into: the greeting text's frame and the response list's frame. The text
    /// window 0x018072CB and the list window 0x04ADC3BF move into those frames.
    pub const TEXT_FRAME: u32 = 0x04B3_0B3B;
    pub const RESPONSE_FRAME: u32 = 0x04AD_E899;
}

/// The comm screen is at most 768 px high and 4:3 (0xDD41F0: `SetLayoutArea`
/// (0, 0, h x 1.3333334, h) with h = min(the window's height, 768)), centred in
/// its parent and standing on the parent's bottom edge (`SetLocation`
/// (floor((parentW - w) / 2 + parent.left), floor(parent.bottom - h))).
pub const SCREEN_MAX_HEIGHT: f32 = 768.0;

/// Stored area of the screen window (a `SimpleLayout` 15 child of the root) that
/// solves to the retail box of a `screen`-sized root.
pub fn screen_area(screen: [f32; 2]) -> [f32; 4] {
    let h = screen[1].min(SCREEN_MAX_HEIGHT);
    let w = h * 1.333_333_4;
    let x = ((screen[0] - w) * 0.5).floor();
    let y = (screen[1] - h).floor();
    // Anchors on all four edges add the parent's size to x2 / y2.
    [x, y, x + w - screen[0], y + h - screen[1]]
}

/// `ScrollFrameVerticalGeneric` (the layout 0x40464100!0x0510A95B, class
/// `UI::ScrollFrameVertical` 0x8076F0 / update 0x82A5E0): the scroll bar window
/// 0x04AB5E70 is 20 wide, starts 3 px inside the clip window and ends 4 px
/// beyond the frame, so the content is `frame width + 4` wide while everything
/// fits and the scroll bar (20 - 3) takes 17 of it when it does not.
pub const SCROLL_BAR_OVERHANG: f32 = 4.0;
pub const SCROLL_BAR_TAKES: f32 = 17.0;
/// Gap between two response buttons (0xDD3230 adds 2.0 to the 18.0 high row).
pub const ROW_GAP: f32 = 2.0;

/// What a scroll frame of `frame_w` x `frame_h` shows for content `content_h` high.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollFrame {
    /// Width of the content window (the text window's width).
    pub content_w: f32,
    /// The scroll bar is shown (and the clip window clips).
    pub scrolls: bool,
}

pub fn scroll_frame(frame_w: f32, frame_h: f32, content_h: f32) -> ScrollFrame {
    let scrolls = content_h > frame_h;
    let full = frame_w + SCROLL_BAR_OVERHANG;
    ScrollFrame { content_w: if scrolls { full - SCROLL_BAR_TAKES } else { full }, scrolls }
}

/// The greeting text window as the retail UI keeps it (0xDD2A60): the content
/// window of the text scroll frame, the text window is as wide as the content,
/// sits at its top-left corner (`SetLocation(0, 0)`) and is as high as its text
/// (`IText` 0x1C(true)); the frame then shows the scroll bar when the text is
/// higher than the frame.
fn fit_text(ui: &mut SpuiState, text: crate::spui::model::WinId) {
    let (Some(frame), Some(parent)) = (ui.find(cid::TEXT_FRAME), ui.layout.window(text).parent) else {
        return;
    };
    let (f, p) = (ui.rect(frame), ui.rect(parent));
    let measured = ui.text_extent(text).map_or(0.0, |e| e.y);
    let shown = scroll_frame(f.width(), f.height(), measured);
    // ADAPTED: the scroll bar itself (the layout's 0x04AB5E70 window) and the
    // clip of a text taller than the frame are not drawn: only the narrower
    // content width of that case is taken. No text of the captures comes near
    // the frame's height (326 px at 1280x800); not measured over every .cnv text.
    // The first frame of a text has no measure yet: the frame's height stands in.
    let height = if measured > 0.0 { measured } else { f.height() };
    let (x, y) = (f.x1 - p.x1, f.y1 - p.y1);
    ui.set_area(text, [x, y, x + shown.content_w, y + height]);
}

/// Who the UFO can talk to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommTarget {
    pub empire: crate::space_galaxy::EmpireId,
    pub name: String,
    /// Relation label ("Neutral", "Friendly", ...).
    pub relation: String,
    /// The empire's archetype greeting (legacy text; the screen shows the
    /// retail conversation).
    pub greeting: String,
    /// A transmission waits (the comm button glows).
    pub incoming: bool,
    /// Short text of the empire's contract, if the player has one.
    pub contract: Option<String>,
    /// Cost index of the planet's owner for the repair and recharge prices
    /// (`PlanetOwner::cost_index`: homeworld 0, colony 1 .. hostile 5).
    pub owner_cost_index: usize,
    /// The world talked to.
    pub world: u32,
    /// The player's own world (the colonists answer: `MySpecies`).
    pub own: bool,
}

/// The screen's open state and the clicks for the session.
#[derive(Resource, Default, Debug)]
pub struct CommState {
    pub open: bool,
    /// Control ids clicked on the screen (from `space_hud::actions`).
    pub clicks: Vec<u32>,
    /// A response / inventory row clicked on the overlay.
    pub row_click: Option<RowClick>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowClick {
    Response(usize),
    Npc(usize),
    Player(usize),
}

/// Distance beyond a world's radius within which the UFO talks to it (the
/// Space module's action range).
fn reach() -> f32 {
    crate::space::action_range()
}

/// Who owns the world, for the repair prices: an ally, or another empire by
/// its stance (RE S2C: stance 3-4 friendly, 2 neutral, 0-1 hostile; the stance
/// is the count of attitude thresholds the relationship has passed).
pub fn owner_for(campaign: &SpaceCampaign, empire: crate::space_galaxy::EmpireId) -> PlanetOwner {
    owner_of(campaign.relationship(empire), campaign.allies.contains(&empire))
}

/// [`owner_for`] from the relationship value and the alliance.
pub fn owner_of(relationship: f32, ally: bool) -> PlanetOwner {
    if ally {
        return PlanetOwner::Ally;
    }
    match rules::ATTITUDE_THRESHOLDS.iter().filter(|t| relationship >= **t).count() {
        0 | 1 => PlanetOwner::Hostile,
        2 => PlanetOwner::Neutral,
        _ => PlanetOwner::Friendly,
    }
}

/// Price of the missing health at the owner's per-point price.
pub fn repair_price(missing: f32, owner_cost_index: usize) -> f32 {
    rules::repair_purchase(missing, rules::REPAIR_COST[owner_cost_index.min(5)], f32::MAX).1
}

/// Price of the missing energy at the owner's per-point price.
pub fn recharge_price(missing: f32, owner_cost_index: usize) -> f32 {
    rules::repair_purchase(missing, rules::RECHARGE_COST[owner_cost_index.min(5)], f32::MAX).1
}

/// Buy back the missing health (only the affordable part with too little
/// money); returns the Sporebucks spent.
pub fn buy_repair(owner_cost_index: usize, health: &mut Health, wallet: &mut f32) -> f32 {
    let (points, cost) =
        rules::repair_purchase(health.max - health.hp, rules::REPAIR_COST[owner_cost_index.min(5)], *wallet);
    health.heal(points);
    *wallet -= cost;
    cost
}

/// Buy back the missing energy; returns the Sporebucks spent.
pub fn buy_recharge(owner_cost_index: usize, campaign: &mut SpaceCampaign, wallet: &mut f32) -> f32 {
    let missing = campaign.max_reactor_energy() - campaign.energy;
    let (points, cost) = rules::repair_purchase(missing, rules::RECHARGE_COST[owner_cost_index.min(5)], *wallet);
    campaign.energy = (campaign.energy + points).min(campaign.max_reactor_energy());
    *wallet -= cost;
    cost
}

fn player_species(run: &RunProgress) -> String {
    let n = run.creature_creation.name.trim();
    if n.is_empty() { "Colonists".into() } else { n.to_string() }
}

/// The world the UFO is over: the nearest planet of an empire or of the player.
fn target_for(run: &RunProgress, position: Vec3, planets: &[(u32, Vec3, f32)]) -> Option<CommTarget> {
    let campaign = &run.space_campaign;
    let (id, _, _) = planets
        .iter()
        .copied()
        .filter(|(_, at, radius)| position.distance(*at) < radius + reach())
        .min_by(|a, b| position.distance(a.1).total_cmp(&position.distance(b.1)))?;
    if campaign.colonies.contains(&id) || id == 0 {
        let owner = if id == 0 { PlanetOwner::Homeworld } else { PlanetOwner::Colony };
        return Some(CommTarget {
            name: player_species(run),
            relation: "Your colony".into(),
            owner_cost_index: owner.cost_index(),
            world: id,
            own: true,
            ..Default::default()
        });
    }
    let empire = campaign.world_empire(id)?;
    Some(empire_target(campaign, empire, id))
}

fn empire_target(campaign: &SpaceCampaign, empire: crate::space_galaxy::EmpireId, world: u32) -> CommTarget {
    let name = SpaceCampaign::empire_name(empire).to_string();
    let archetype = crate::space::empire_archetype(empire);
    let contract = campaign.active_alien_mission.as_ref().filter(|m| m.empire == empire).map(|m| {
        format!("{} ({}/{}): {}", m.kind.title(), m.current_progress(campaign), m.target_count, m.description)
    });
    let incoming = campaign.active_alien_mission.as_ref().is_some_and(|m| m.empire == empire && m.is_complete(campaign))
        || campaign.universe.inbox.iter().any(|m| m.empire == Some(empire));
    CommTarget {
        empire,
        owner_cost_index: owner_for(campaign, empire).cost_index(),
        greeting: crate::space::archetype_philosophical_greeting(archetype, &name).to_string(),
        relation: campaign.relation_label(empire).to_string(),
        name,
        incoming,
        contract,
        world,
        own: false,
    }
}

/// Fill [`SpaceHudState::comm`]: the world in reach, else the sender of a
/// waiting transmission; close the screen when there is nobody to talk to.
#[allow(clippy::too_many_arguments)]
pub fn publish_comm(
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    player: Query<&Transform, With<Player>>,
    planets: Query<(&WorldId, &Transform, &Planet)>,
    session: Res<CommSession>,
    mut state: ResMut<CommState>,
    mut hud: ResMut<SpaceHudState>,
) {
    if *stage.get() != GameStage::Space {
        state.open = false;
        return;
    }
    let worlds: Vec<(u32, Vec3, f32)> = planets.iter().map(|(id, t, p)| (id.0, t.translation, p.radius)).collect();
    hud.comm = player.single().ok().and_then(|t| target_for(&run, t.translation, &worlds));
    let inbox = &run.space_campaign.universe.inbox;
    if let Some(m) = inbox.first() {
        match hud.comm.as_mut() {
            Some(t) => t.incoming = true,
            None => {
                hud.comm = Some(match m.empire {
                    Some(e) => CommTarget { incoming: true, ..empire_target(&run.space_campaign, e, m.world) },
                    None => CommTarget {
                        name: player_species(&run),
                        relation: "Your colony".into(),
                        incoming: true,
                        world: m.world,
                        own: true,
                        owner_cost_index: 5,
                        ..Default::default()
                    },
                })
            }
        }
    }
    if hud.comm.is_none() && !session.open {
        state.open = false;
    }
    hud.comm_open = state.open || session.open;
}

/// Capture / test flags: `--space-comm-tab menu|trade|diplomacy|missions|offer` picks a
/// tab once the screen is open, `--space-comm-incoming raid|tribute|gift`
/// queues a transmission and opens the screen on it.
#[derive(Default)]
pub struct CommDemo {
    read: bool,
    tab: Option<String>,
    incoming: Option<String>,
    /// `--space-comm-mission artifact|commodity|eradicate|balance|multidelivery|scanfetch`:
    /// with `--space-comm-tab offer` the offered mission is of that kind.
    mission: Option<String>,
    empire: Option<crate::space_galaxy::EmpireId>,
    open: bool,
    done: bool,
}

impl CommDemo {
    fn read_args(&mut self) {
        if self.read {
            return;
        }
        self.read = true;
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
        self.tab = value("--space-comm-tab");
        self.incoming = value("--space-comm-incoming");
        self.mission = value("--space-comm-mission");
        self.empire = value("--space-comm-empire").and_then(|v| v.parse().ok());
        self.open = args.iter().any(|a| a == "--space-hud-comm") || self.empire.is_some();
    }
}

/// Open / close the session with the screen, run its clicks, play its sounds.
#[allow(clippy::too_many_arguments)]
pub fn drive_session(
    stage: Res<State<GameStage>>,
    mut state: ResMut<CommState>,
    mut hud: ResMut<SpaceHudState>,
    mut session: ResMut<CommSession>,
    mut lib: ResMut<CnvLibrary>,
    originals: Option<Res<crate::Originals>>,
    mut run: ResMut<RunProgress>,
    mut ship: Query<&mut Health, With<Player>>,
    mut cues: MessageWriter<crate::game_audio::PlayCue>,
    mut demo: Local<CommDemo>,
    (mut tools, mut assets): (ResMut<super::palette::ToolInfoCache>, ResMut<SpuiAssets>),
) {
    if *stage.get() != GameStage::Space {
        if session.open {
            session.close();
        }
        return;
    }
    demo.read_args();
    let index = originals.as_ref().map(|o| &o.resource_index);
    let damaged = ship.single().is_ok_and(|h| h.hp < h.max);
    let env = CommEnv { index, ufo_damaged: damaged };
    // Demo: queue a transmission once and open on it.
    if !demo.done && run.space_campaign.economy_version != 0 {
        if let Some(kind) = demo.incoming.clone() {
            let e = run.space_campaign.current_system_empire.or(hud.comm.as_ref().filter(|t| !t.own).map(|t| t.empire)).unwrap_or(3);
            let colony = run.space_campaign.colonies.iter().copied().find(|w| *w != 0).unwrap_or(0);
            let msg = match kind.as_str() {
                "tribute" => {
                    let amount = sc::tribute_amount(usize::from(run.space_campaign.captain_rank()), run.space_campaign.difficulty());
                    run.space_campaign.meet_empire(e);
                    run.space_campaign.tribute_demand = Some(crate::space::TributeDemand { empire: e, amount, time_left: rules::NPC_TRIBUTE_TIME_LIMIT });
                    crate::space_universe::Incoming { empire: Some(e), file: sc::INCOMING_FILE, dialog: sc::dialog::DEMAND_TRIBUTE, world: 0, amount, time_left: 300.0 }
                }
                "gift" => crate::space_universe::Incoming { empire: Some(e), file: sc::INCOMING_FILE, dialog: sc::dialog::OFFER_GIFT, world: 0, amount: rules::NPC_GIFT_AMOUNT, time_left: 120.0 },
                _ => crate::space_universe::Incoming { empire: None, file: sc::EVENT_RAID_FILE, dialog: sc::dialog::MISSION_ASSIGN, world: colony, amount: 0.0, time_left: 300.0 },
            };
            run.space_campaign.universe.inbox.insert(0, msg);
            state.open = true;
        } else if let Some(e) = demo.empire {
            // Talk to empire `e` as if the UFO were over its world.
            let world = crate::space_galaxy::world_id(run.space_campaign.current_system, 0);
            let seed = 0xC0FFEE ^ e as u32;
            session.open_with(&mut lib, &mut run, &env, Some(e), world, seed);
            state.open = session.open;
        } else if demo.open {
            state.open = true;
        }
        demo.done = true;
    }
    let wanted = state.open || hud.comm_open;
    if wanted && !session.open {
        if let Some(target) = hud.comm.clone() {
            let seed = run.space_campaign.universe_clock.to_bits() ^ target.world;
            let run = &mut *run;
            let msg = run.space_campaign.universe.inbox.first().copied().filter(|m| target.own || m.empire == Some(target.empire) || target.incoming);
            match msg {
                Some(m) => {
                    run.space_campaign.universe.inbox.remove(0);
                    session.open_incoming(&mut lib, run, &env, m, seed);
                }
                None if target.own => session.open_with(&mut lib, run, &env, None, target.world, seed),
                None => session.open_with(&mut lib, run, &env, Some(target.empire), target.world, seed),
            }
            state.open = session.open;
        }
    } else if !wanted && session.open {
        session.close();
    }
    if session.open && demo.done {
        if let Some(tab) = demo.tab.take() {
            let run = &mut *run;
            match tab.as_str() {
                "trade" => session.tab_trade(run),
                // The main menu (the response "Thank you for receiving me." of the
                // greeting): the Trade / Repair / Recharge / Missions / Diplomacy tabs.
                "menu" => session.show(&mut lib, run, &env, sc::MAIN_MENU_FILE, sc::dialog::DEFAULT),
                "missions" => session.tab_missions(&mut lib, run, &env),
                "offer" => {
                    session.tab_missions(&mut lib, run, &env);
                    let forced = demo.mission.as_deref().and_then(|k| {
                        use rules::MissionKind as K;
                        Some(match k {
                            "artifact" => K::FetchArtifact,
                            "commodity" => K::FetchCommodity,
                            "eradicate" => K::EradicateAnimals,
                            "balance" => K::Balance,
                            "multidelivery" => K::MultiDelivery,
                            "scanfetch" => K::ScanFetch,
                            _ => return None,
                        })
                    });
                    match (forced, session.empire) {
                        (Some(kind), Some(e)) => {
                            let m = run.space_campaign.generate_alien_mission_with(e, Some(kind));
                            session.offer(&mut lib, run, &env, m);
                        }
                        _ => session.apply(&mut lib, run, &env, sc::action::TRY_MISSION, (0, 0, 0)),
                    }
                }
                _ => session.tab_diplomacy(&mut lib, run, &env),
            }
        }
    }
    let owner = hud.comm.as_ref().map_or(5, |t| t.owner_cost_index);
    for control in std::mem::take(&mut state.clicks) {
        let run = &mut *run;
        match control {
            cid::GOODBYE => {
                session.close();
                // 0xAEA041: the screen powers down.
                session.sound = Some(crate::game_audio::Cue::UfoComPowerdown);
            }
            cid::MENU if session.empire.is_some() => {
                session.sound = Some(crate::game_audio::Cue::UfoComSelect);
                session.show(&mut lib, run, &env, sc::MAIN_MENU_FILE, sc::dialog::DEFAULT);
            }
            cid::ACCEPT => session.accept(&mut lib, run, &env),
            cid::DECLINE => session.decline(&mut lib, run, &env),
            cid::TAB_TRADE => session.tab_trade(run),
            cid::TAB_MISSIONS => session.tab_missions(&mut lib, run, &env),
            cid::TAB_DIPLOMACY => session.tab_diplomacy(&mut lib, run, &env),
            cid::TAB_REPAIR | cid::PRICE_REPAIR | cid::TAB_RECHARGE | cid::PRICE_RECHARGE => {
                let spent = match (control, ship.single_mut()) {
                    (cid::TAB_REPAIR | cid::PRICE_REPAIR, Ok(mut health)) => buy_repair(owner, &mut health, &mut run.spice),
                    (cid::TAB_RECHARGE | cid::PRICE_RECHARGE, _) => buy_recharge(owner, &mut run.space_campaign, &mut run.spice),
                    _ => 0.0,
                };
                if spent > 0.0 {
                    run.space_campaign.message = format!("Spent {spent:.0} Sporebucks");
                    run.request_save();
                    // Repair and recharge have their own sounds (retail 0xDD5446 / 0xDD549D).
                    session.sound = Some(if matches!(control, cid::TAB_REPAIR | cid::PRICE_REPAIR) {
                        crate::game_audio::Cue::UfoComBuyHealth
                    } else {
                        crate::game_audio::Cue::UfoComBuyEnergy
                    });
                }
            }
            cid::ADD_ONE => session.trade_count(1, run),
            cid::ADD_MANY => session.trade_count(10, run),
            cid::REMOVE_ONE => session.trade_count(-1, run),
            cid::REMOVE_MANY => session.trade_count(-10, run),
            cid::BUY => session.trade_buy(run),
            cid::SELL => session.trade_sell(run),
            _ => {}
        }
    }
    if let Some(click) = state.row_click.take() {
        let run = &mut *run;
        match click {
            RowClick::Response(i) => session.choose(&mut lib, run, &env, i),
            RowClick::Npc(i) => session.trade_select(TradeSide::Buy, i),
            RowClick::Player(i) => session.trade_select(TradeSide::Sell, i),
        }
    }
    if !session.open {
        state.open = false;
        hud.comm_open = false;
    }
    if let Some(cue) = session.sound.take() {
        cues.write(crate::game_audio::PlayCue(cue));
    }
    // Store rows carry the tool's retail name (`spaceToolDescription` of the
    // tool property file, as the palette shows it).
    if let (Some(t), Some(index)) = (session.trade.as_mut(), index) {
        if !t.named {
            for item in &mut t.stock {
                if let Some(info) = tools.get_any(&item.tool, index, &mut assets) {
                    if !info.name.trim().is_empty() {
                        item.label = info.name.trim().to_string();
                    }
                }
            }
            t.named = true;
        }
    }
}

/// Bind the communication screen to the session.
pub fn bind_comm(
    hud: Res<SpaceHudState>,
    session: Res<CommSession>,
    run: Res<RunProgress>,
    mut roots: Query<(&mut SpuiState, &SpaceHudRoot)>,
) {
    use cid::*;
    for (mut ui, kind) in &mut roots {
        if *kind != SpaceHudRoot::Comm || !ui.ready {
            continue;
        }
        let show = session.open;
        // 0xDD41F0: the screen is a 4:3 box of at most 768 px, bottom centre.
        if let Some(w) = ui.find(SCREEN) {
            let area = screen_area(ui.screen);
            ui.set_area(w, area);
        }
        super::set_visible(&mut ui, SCREEN, show);
        // The portrait is the alien's picture in retail; the layout's design
        // fill stays hidden until there is one.
        super::set_visible(&mut ui, PORTRAIT_FILL, false);
        super::set_visible(&mut ui, TRADING, show && session.trade.is_some());
        if !show {
            continue;
        }
        let name = match session.empire {
            Some(e) => SpaceCampaign::empire_name(e).to_string(),
            None => player_species(&run),
        };
        if let Some(w) = ui.find(NATION_NAME) {
            ui.set_text(w, name);
        }
        let trading = session.trade.is_some();
        if let Some(w) = ui.find(GREETING) {
            ui.set_text(w, if trading { String::new() } else { session.text.clone() });
            fit_text(&mut ui, w);
        }
        // The trading controls take the tab group's place on the Trade tab.
        let tabs = session.show_tabs && !trading;
        super::set_visible(&mut ui, TABS, tabs);
        super::set_visible(&mut ui, ACCEPT, session.show_accept);
        super::set_visible(&mut ui, DECLINE, session.show_decline);
        if tabs {
            let owner = hud.comm.as_ref().map_or(5, |t| t.owner_cost_index);
            let repair = repair_price(hud.max_health - hud.health, owner);
            let recharge = recharge_price(hud.max_energy - hud.energy, owner);
            for (tab, price, cost) in [(TAB_REPAIR, PRICE_REPAIR, repair), (TAB_RECHARGE, PRICE_RECHARGE, recharge)] {
                if let Some(w) = ui.find(tab) {
                    ui.set_visible(w, true);
                    ui.set_enabled(w, cost > 0.0 && run.spice > 0.0);
                }
                if let Some(w) = ui.find(price) {
                    ui.set_visible(w, true);
                    ui.set_text(w, super::format_money(cost.round() as i64));
                }
            }
            let empire = session.empire.is_some();
            for (control, on, selected) in [
                (TAB_TRADE, empire, trading),
                (TAB_MISSIONS, empire, false),
                (TAB_DIPLOMACY, empire, false),
            ] {
                if let Some(w) = ui.find(control) {
                    ui.set_visible(w, true);
                    ui.set_enabled(w, on);
                    ui.set_selected(w, selected);
                }
            }
        }
        if let Some(t) = session.trade.as_ref() {
            let sells = sc::sell_list(&run.space_campaign);
            let (unit, max) = match t.side {
                TradeSide::Buy => (t.stock.get(t.selected).map_or(0.0, |i| i.price), t.stock.len()),
                TradeSide::Sell => (sells.get(t.selected).map_or(0.0, |s| s.2), sells.len()),
            };
            if let Some(w) = ui.find(TRADE_PRICE) {
                ui.set_text(w, super::format_money((unit * t.count as f32).round() as i64));
            }
            if let Some(w) = ui.find(COUNT) {
                ui.set_text(w, format!("{}", t.count));
            }
            if let Some(w) = ui.find(TRADE_MESSAGE) {
                ui.set_text(w, t.message.clone());
            }
            if let Some(w) = ui.find(BUY) {
                ui.set_enabled(w, t.side == TradeSide::Buy && max > 0);
            }
            if let Some(w) = ui.find(SELL) {
                ui.set_enabled(w, t.side == TradeSide::Sell && max > 0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The code-made rows: responses and the two inventories.
// ---------------------------------------------------------------------------

/// Retail response row height (0xDD3E60: 18.0) on the 1280x900 layout.
pub const ROW_HEIGHT: f32 = 18.0;
/// Response text colours (0xDD3E60).
pub const ROW_COLOUR: u32 = 0xFFFF_B400;
pub const ROW_DISABLED: u32 = 0xFF7F_7F7F;
/// Text style id the retail rows use (0xDD3E60).
pub const ROW_STYLE: u32 = 0x5163_044D;
const ROWS: usize = 28;

#[derive(Component)]
pub struct CommRowsRoot;

#[derive(Component)]
pub struct CommRow(pub usize);

/// What one overlay row shows.
#[derive(Clone, Debug, PartialEq)]
pub struct RowSpec {
    pub rect: Rect,
    pub text: String,
    pub colour: u32,
    pub click: RowClick,
}

fn argb(c: u32) -> Color {
    Color::srgba_u8((c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8)
}

/// Scale of the code-made rows. The retail rows are 18 px high whatever the
/// screen is (0xDD3E60): the comm screen is a fixed-size 4:3 box (at most 768 px
/// high, [`screen_area`]), so the rows are not scaled any more.
pub fn row_scale(_screen: [f32; 2]) -> f32 {
    1.0
}

/// Lay the rows out in the solved rects of the comm screen.
pub fn row_specs(session: &CommSession, run: &RunProgress, responses: Rect, npc: Rect, player: Rect, scale: f32) -> Vec<RowSpec> {
    let h = ROW_HEIGHT * scale;
    let mut out = Vec::new();
    let mut column = |area: Rect, items: Vec<(String, u32, RowClick)>| {
        for (i, (text, colour, click)) in items.into_iter().enumerate() {
            let y = area.min.y + 2.0 * scale + h * i as f32;
            if y + h > area.max.y + h * 0.5 && i > 0 {
                break;
            }
            out.push(RowSpec { rect: Rect::new(area.min.x + 4.0 * scale, y, area.max.x - 2.0 * scale, y + h), text, colour, click });
        }
    };
    if let Some(t) = session.trade.as_ref() {
        let mark = |side: TradeSide, i: usize| t.side == side && t.selected == i;
        column(
            npc,
            t.stock
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let colour = if s.locked.is_some() {
                        ROW_DISABLED
                    } else if mark(TradeSide::Buy, i) {
                        0xFFFF_FFFF
                    } else {
                        ROW_COLOUR
                    };
                    (format!("{}  {}", s.label, sc::money_text(s.price)), colour, RowClick::Npc(i))
                })
                .collect(),
        );
        column(
            player,
            sc::sell_list(&run.space_campaign)
                .iter()
                .enumerate()
                .map(|(i, (c, n, p))| {
                    let colour = if mark(TradeSide::Sell, i) { 0xFFFF_FFFF } else { ROW_COLOUR };
                    (format!("{} spice x{n}  {}", c.label(), sc::money_text(*p)), colour, RowClick::Player(i))
                })
                .collect(),
        );
    } else {
        // The response list: its window is the content window of the response
        // scroll frame (0xDD3230): 18 px buttons from the content's top-left
        // corner, each below the previous one with a 2 px gap, as wide as the
        // content.
        let pitch = (ROW_HEIGHT + ROW_GAP) * scale;
        let mut responses_column = |frame: Rect, items: Vec<(String, u32, RowClick)>| {
            for (i, (text, colour, click)) in items.into_iter().enumerate() {
                let y = frame.min.y + pitch * i as f32;
                if y + h > frame.max.y + h * 0.5 && i > 0 {
                    break;
                }
                out.push(RowSpec { rect: Rect::new(frame.min.x, y, frame.max.x + SCROLL_BAR_OVERHANG * scale, y + h), text, colour, click });
            }
        };
        responses_column(
            responses,
            session
                .responses
                .iter()
                .enumerate()
                .map(|(i, r)| (r.label.clone(), if r.enabled { ROW_COLOUR } else { ROW_DISABLED }, RowClick::Response(i)))
                .collect(),
        );
    }
    out
}

/// Spawn the row overlay while the HUD is up.
pub fn manage_rows(
    mut commands: Commands,
    active: Res<super::SpaceHudActive>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<Entity, With<CommRowsRoot>>,
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
            CommRowsRoot,
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
                CommRow(i),
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

/// Place and fill the rows; a click on a row is queued for the session.
#[allow(clippy::too_many_arguments)]
pub fn update_rows(
    session: Res<CommSession>,
    run: Res<RunProgress>,
    pointer: Res<SpuiPointer>,
    mut state: ResMut<CommState>,
    mut assets: ResMut<SpuiAssets>,
    originals: Option<Res<crate::Originals>>,
    roots: Query<(&SpuiState, &SpaceHudRoot)>,
    mut rows: Query<(&CommRow, &mut Node, &mut Text, &mut TextColor, &mut TextFont, &mut Visibility)>,
    mut was_pressed: Local<bool>,
    mut font: Local<Option<(bevy::text::FontSource, f32)>>,
) {
    let Some(ui) = roots.iter().find(|(ui, k)| **k == SpaceHudRoot::Comm && ui.ready).map(|(ui, _)| ui) else {
        return;
    };
    let rect = |control: u32| {
        ui.find(control).map(|w| {
            let r = ui.rect(w);
            Rect::new(r.x1, r.y1, r.x2, r.y2)
        })
    };
    // The response list lives in the response scroll frame (0xDD41F0).
    let resp_rect = rect(cid::RESPONSE_FRAME).or_else(|| rect(cid::RESPONSES));
    let specs = match (session.open, resp_rect, rect(cid::GREETING), rect(cid::PLAYER_INVENTORY)) {
        (true, Some(resp), Some(greet), player) => {
            let scale = row_scale(ui.screen);
            let player = player.filter(|r| r.height() > 30.0 * scale).unwrap_or_else(|| {
                // The hold list sits under the trading controls when the
                // layout's inventory window has no height.
                rect(cid::TRADING).map_or(resp, |t| Rect::new(t.min.x, t.max.y + 4.0 * scale, t.max.x, t.max.y + 120.0 * scale))
            });
            row_specs(&session, &run, resp, greet, player, scale)
        }
        _ => Vec::new(),
    };
    if font.is_none() {
        if let Some(o) = originals.as_ref() {
            let style = assets.styles(&o.resource_index).style(ROW_STYLE);
            let family = style.family.clone();
            let size = if style.size_px > 1.0 { style.size_px } else { 10.0 };
            *font = Some((assets.font_source(&family), size));
        }
    }
    let scale = row_scale(ui.screen);
    let pct = |v: f32, total: f32| Val::Percent(100.0 * v / total.max(1.0));
    let hover = pointer.position.and_then(|p| specs.iter().position(|s| s.rect.contains(p)));
    for (row, mut node, mut text, mut colour, mut tf, mut vis) in &mut rows {
        let Some(spec) = specs.get(row.0) else {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        };
        node.left = pct(spec.rect.min.x, ui.screen[0]);
        node.top = pct(spec.rect.min.y, ui.screen[1]);
        if text.0 != spec.text {
            text.0 = spec.text.clone();
        }
        let c = if hover == Some(row.0) && spec.colour != ROW_DISABLED { 0xFFFF_FFFF } else { spec.colour };
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
    // A release over the row that was pressed.
    let pressed = pointer.pressed;
    if *was_pressed && !pressed {
        if let Some(i) = hover {
            state.row_click = Some(specs[i].click);
        }
    }
    *was_pressed = pressed;
}

// ---------------------------------------------------------------------------
// The speaker's portrait.
// ---------------------------------------------------------------------------

/// Render layer of the comm portrait (creature 26, tribe 25/26, editors 28/29,
/// mechanical preview 30, part thumbnails 31).
pub const PORTRAIT_LAYER: usize = 23;
/// Long side of the portrait picture; its other side follows the aspect of the
/// frame's picture window ([`portrait_aspect`]).
const PORTRAIT_SIZE: u32 = 384;
/// Where the portrait subject stands (far below every Space scene).
const PORTRAIT_AT: Vec3 = Vec3::new(0.0, -30_000.0, 0.0);
/// ADAPTED: the retail screen shows the speaker in its own environment with
/// emotion animations; here the species' installed model plays its idle clips
/// before a flat backdrop, lit by the portrait's own light.
const PORTRAIT_BACKDROP: (u8, u8, u8) = (18, 44, 58);
/// The creature portraits' field of view (`creature_hud::portrait`).
const PORTRAIT_FOV: f32 = 28.0;

#[derive(Resource, Default)]
pub struct CommPortrait {
    image: Option<Handle<Image>>,
    rig: Vec<Entity>,
    node: Option<Entity>,
    /// Who is shown: Some(None) the player's species, Some(Some(e)) empire e.
    shown: Option<Option<crate::space_galaxy::EmpireId>>,
    seen: u32,
    /// Aspect (width over height) of the picture window the rig was built for.
    aspect: f32,
}

#[derive(Component)]
pub struct CommPortraitNode;

/// The installed creature model of a speaker: the player's own species model,
/// or the NPC visual of the empire's species (`baked_model::species_key`, the
/// same pick the creature-stage NPC visuals use; ADAPTED: retail shows the
/// empire's Sporepedia species).
fn portrait_key(originals: &crate::Originals, who: Option<crate::space_galaxy::EmpireId>) -> Option<crate::dbpf::ResourceKey> {
    match who {
        None => crate::baked_model::player_source_key(originals),
        Some(e) => crate::baked_model::species_key(originals, e as u32),
    }
}

/// Keep the portrait camera, light and model in step with the session.
#[allow(clippy::too_many_arguments)]
pub fn update_portrait(
    mut commands: Commands,
    mut portrait: ResMut<CommPortrait>,
    session: Res<CommSession>,
    originals: Option<ResMut<crate::Originals>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    run: Option<Res<crate::game::RunProgress>>,
    roots: Query<(&SpuiState, &SpaceHudRoot)>,
) {
    use bevy::camera::{visibility::RenderLayers, RenderTarget};
    let want = session.open.then_some(session.empire);
    let solved = portrait_aspect(&roots);
    // The picture window moved after the rig was built (the box is laid out a
    // frame after the screen opens): build the picture again at its aspect.
    let stale = want.is_some() && want == portrait.shown && solved.is_some_and(|a| (a - portrait.aspect).abs() > 0.02);
    if want == portrait.shown && !stale {
        if want.is_some() {
            portrait.seen = portrait.seen.saturating_add(1);
        }
        return;
    }
    // The picture has the aspect of the frame's picture window, so that the
    // speaker fills it undistorted (the box is 4:3 now, the window is not
    // square): wait for the solved layout before building the rig.
    let aspect = match (solved, want) {
        (Some(aspect), _) => aspect,
        (None, Some(_)) => return,
        (None, None) => 1.0,
    };
    portrait.aspect = aspect;
    if stale {
        portrait.image = None;
    }
    for e in std::mem::take(&mut portrait.rig) {
        if let Ok(mut entity) = commands.get_entity(e) {
            entity.despawn();
        }
    }
    portrait.shown = want;
    portrait.seen = 0;
    let (Some(who), Some(mut originals)) = (want, originals) else {
        return;
    };
    let layer = RenderLayers::layer(PORTRAIT_LAYER);
    // The player's own species (its colonies' calls): the outfitted creature
    // in its Space outfit, posed by the crowd's clips (`outfit_crowd`).
    let captain = match (who, run.as_deref()) {
        (None, Some(run)) => crate::outfit_crowd::spawn_captain(
            &mut commands,
            &mut originals,
            run,
            (&mut *meshes, &mut *images, &mut materials),
            PORTRAIT_AT,
            layer.clone(),
        ),
        _ => None,
    };
    let (subject, (min, max)) = match captain {
        Some((subject, min, max)) => (subject, (min, max)),
        None => {
            let Some(key) = portrait_key(&originals, who) else {
                return;
            };
            let Ok(model) = crate::baked_model::ensure(&mut originals, key, &mut meshes, &mut images, &mut materials) else {
                return;
            };
            let Ok(fit) = crate::world_objects::fitted_transform(&model, &meshes, Vec3::new(0.0, -1.0, 0.0), 2.5) else {
                return;
            };
            // The species' installed (painted) creature as a native NPC
            // visual, so the clip player rigs it and plays its idle clips
            // (`locomotion_preview`; posed every frame although it stands
            // outside the game camera's view).
            let subject = commands
                .spawn((
                    Transform::from_translation(PORTRAIT_AT),
                    Visibility::Visible,
                    crate::stages::StageOwned,
                    crate::baked_model::NativeCreature,
                    crate::locomotion_preview::PoseAlways,
                ))
                .with_children(|p| {
                    for surface in &model.surfaces {
                        p.spawn((
                            crate::material_binding::OriginalSurfaceChild {
                                source_section: surface.source_section,
                                state_sections: surface.state_sections.clone(),
                            },
                            crate::baked_model::source(key),
                            Mesh3d(surface.handle.clone()),
                            MeshMaterial3d(surface.material.clone()),
                            fit,
                            layer.clone(),
                            bevy::light::NotShadowCaster,
                        ));
                    }
                })
                .id();
            // Frame the whole model from the front (installed creatures face -Z).
            let bounds = meshes
                .get(&model.handle)
                .and_then(|m| match m.attribute(Mesh::ATTRIBUTE_POSITION) {
                    Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) => Some(v.iter().fold(
                        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                        |(lo, hi), p| {
                            let w = PORTRAIT_AT + fit.transform_point(Vec3::from_array(*p));
                            (lo.min(w), hi.max(w))
                        },
                    )),
                    _ => None,
                })
                .filter(|(lo, hi)| lo.is_finite() && hi.is_finite())
                .unwrap_or((PORTRAIT_AT - Vec3::splat(1.25), PORTRAIT_AT + Vec3::splat(1.25)));
            (subject, bounds)
        }
    };
    let (width, height) = if aspect >= 1.0 {
        (PORTRAIT_SIZE, ((PORTRAIT_SIZE as f32 / aspect) as u32).max(64))
    } else {
        (((PORTRAIT_SIZE as f32 * aspect) as u32).max(64), PORTRAIT_SIZE)
    };
    let image = portrait
        .image
        .get_or_insert_with(|| {
            images.add(Image::new_target_texture(
                width,
                height,
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                None,
            ))
        })
        .clone();
    let (r, g, b) = PORTRAIT_BACKDROP;
    let (eye, target, sun) = crate::creature_hud::portrait::portrait_pose(min, max, Vec3::NEG_Z);
    // A narrow picture sees less of the width (the field of view is vertical).
    let eye = target + (eye - target) * (1.0 / aspect).max(1.0);
    let camera = commands
        .spawn((
            Camera3d::default(),
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            Camera { order: -6, clear_color: ClearColorConfig::Custom(Color::srgb_u8(r, g, b)), ..default() },
            RenderTarget::from(image.clone()),
            Projection::Perspective(PerspectiveProjection { fov: PORTRAIT_FOV.to_radians(), near: 0.05, far: 200.0, ..default() }),
            Transform::from_translation(eye).looking_at(target, Vec3::Y),
            layer.clone(),
            AmbientLight { color: Color::WHITE, brightness: crate::cell_editor_scene::AMBIENT_BRIGHTNESS, ..default() },
            crate::stages::StageOwned,
        ))
        .id();
    let light = commands
        .spawn((
            DirectionalLight {
                color: Color::WHITE,
                illuminance: crate::cell_editor_scene::SUN_ILLUMINANCE,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(sun).looking_at(target, Vec3::Y),
            layer,
            crate::stages::StageOwned,
        ))
        .id();
    portrait.rig = vec![subject, camera, light];
    if portrait.node.is_none() {
        portrait.node = Some(
            commands
                .spawn((
                    CommPortraitNode,
                    Node { position_type: PositionType::Absolute, ..default() },
                    ImageNode::new(image),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    GlobalZIndex(104),
                    crate::stages::StageOwned,
                ))
                .id(),
        );
    }
}

/// 0xAEF060 (VERIFIED switch on the speaker's archetype index): the
/// `animations` variant of a speaker. NPC archetypes Warrior 0 .. Grob 8 by
/// name; the player's own species by the player's archetype: PlayerWarrior 9
/// -> `playerm`, PlayerTrader 10 -> `playere`, PlayerShaman 12 -> `playerc`;
/// every other index (PlayerScientist, PlayerBard, PlayerZealot,
/// PlayerDiplomat, PlayerEcologist, Wanderer, Knight) -> `diplomat`.
pub fn emotion_variant(empire: Option<crate::space_galaxy::EmpireId>, player: crate::space::SpaceArchetype) -> &'static str {
    use crate::space::SpaceArchetype as A;
    match empire {
        Some(0) => "grob",
        Some(e) => match crate::space::empire_archetype(e) {
            A::Warrior => "warrior",
            A::Trader => "trader",
            A::Scientist => "scientist",
            A::Shaman => "shaman",
            A::Bard => "bard",
            A::Zealot => "zealot",
            A::Ecologist => "ecologist",
            _ => "diplomat",
        },
        None => match player {
            A::Warrior => "playerm",
            A::Trader => "playere",
            A::Shaman => "playerc",
            _ => "diplomat",
        },
    }
}

/// The speaker's emotion animation (the shown text's emotion, `animations`
/// table): its action clip once, then its idle, restarted with every new
/// line. The variant is `emotion_variant` (the player's own species too),
/// else the first. INFERRED: the ids are action-list ids
/// (`creature_clips::clip` resolves them; one that does not resolve leaves
/// the idle clip playing).
#[allow(clippy::too_many_arguments)]
pub fn portrait_emotion(
    mut commands: Commands,
    session: Option<Res<CommSession>>,
    portrait: Option<Res<CommPortrait>>,
    lib: Option<ResMut<crate::space_comm::CnvLibrary>>,
    originals: Option<Res<crate::Originals>>,
    run: Option<Res<crate::game::RunProgress>>,
    mut last: Local<(Option<Entity>, u32, u64)>,
) {
    let (Some(session), Some(portrait), Some(mut lib)) = (session, portrait, lib) else { return };
    let Some(&subject) = portrait.rig.first().filter(|_| session.open && portrait.shown.is_some()) else {
        *last = (None, 0, 0);
        return;
    };
    let emotion = if session.emotion == 0 { crate::space_comm::EMOTION_NEUTRAL } else { session.emotion };
    let text = session.text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    if *last == (Some(subject), emotion, text) {
        return;
    }
    let index = originals.as_deref().map(|o| &o.resource_index);
    let Some(table) = lib.conversation(index, crate::space_comm::ANIMATIONS_FILE) else { return };
    let Some(variants) = table.anims.get(&emotion).or_else(|| table.anims.get(&crate::space_comm::EMOTION_NEUTRAL)) else { return };
    let player = run.as_deref().map(|r| r.space_campaign.archetype).unwrap_or_default();
    let name = emotion_variant(session.empire, player);
    let Some(v) = variants.iter().find(|v| v.name.eq_ignore_ascii_case(name)).or_else(|| variants.first()) else { return };
    let pick = |ids: &[u32]| (!ids.is_empty()).then(|| ids[(text % ids.len() as u64) as usize]);
    let Some(idle) = pick(&v.idle) else { return };
    let serial = last.1.wrapping_add(1).max(1) ^ (text as u32);
    if let Some(o) = originals.as_deref() {
        let ok = |id: Option<u32>| id.is_some_and(|id| crate::creature_clips::clip(o, id).is_some());
        info!("comm portrait emotion {emotion:08X} ({}): idle {idle:08X} resolves {}, action resolves {}", v.name, ok(Some(idle)), ok(pick(&v.action)));
    }
    if let Ok(mut e) = commands.get_entity(subject) {
        e.insert(crate::locomotion_preview::PoseClip { action: pick(&v.action), idle, serial });
    }
    *last = (Some(subject), emotion, text);
}

/// The picture window of the speaker's frame (the design-time fill window
/// 0x06243260; 0x03619A7E and 0x05EDFF88 have the same rectangle): the portrait
/// is drawn over all of it.
fn portrait_window(roots: &Query<(&SpuiState, &SpaceHudRoot)>) -> Option<(crate::spui::solve::Rect, [f32; 2])> {
    roots
        .iter()
        .find(|(ui, kind)| **kind == SpaceHudRoot::Comm && ui.ready)
        .and_then(|(ui, _)| ui.find(cid::PORTRAIT_FILL).map(|w| (ui.rect(w), ui.screen)))
}

/// Width over height of the picture window (`None` until the layout is solved).
fn portrait_aspect(roots: &Query<(&SpuiState, &SpaceHudRoot)>) -> Option<f32> {
    portrait_window(roots)
        .filter(|(r, _)| r.width() > 8.0 && r.height() > 8.0)
        .map(|(r, _)| (r.width() / r.height()).clamp(0.4, 2.5))
}

/// Show the portrait image over the speaker's picture window.
pub fn place_portrait(
    portrait: Res<CommPortrait>,
    roots: Query<(&SpuiState, &SpaceHudRoot)>,
    mut nodes: Query<(&mut Node, &mut Visibility, &mut ImageNode), With<CommPortraitNode>>,
) {
    let Some(node) = portrait.node else {
        return;
    };
    let Ok((mut n, mut visibility, mut picture)) = nodes.get_mut(node) else {
        return;
    };
    if let Some(image) = portrait.image.as_ref().filter(|i| picture.image != **i) {
        picture.image = image.clone();
    }
    let want = match portrait_window(&roots) {
        Some((rect, screen)) if portrait.shown.is_some() && portrait.seen >= 3 => {
            let pct = |v: f32, total: f32| Val::Percent(100.0 * v / total.max(1.0));
            n.left = pct(rect.x1, screen[0]);
            n.top = pct(rect.y1, screen[1]);
            n.width = pct(rect.width(), screen[0]);
            n.height = pct(rect.height(), screen[1]);
            Visibility::Inherited
        }
        _ => Visibility::Hidden,
    };
    if *visibility != want {
        *visibility = want;
    }
}

// ---------------------------------------------------------------------------
// Mission-log cards of raids and transmissions.
// ---------------------------------------------------------------------------

/// Event-log ids of the `event_*` conversations (UIShortDesc_InProgress,
/// UIMediumDesc_InProgress).
pub const LOG_SHORT_IN_PROGRESS: u32 = 0xAAD9_C0B3;
pub const LOG_MEDIUM_IN_PROGRESS: u32 = 0xD79A_86A8;

/// The first text of an event log of `file`, tokens substituted.
fn event_log(lib: &mut CnvLibrary, index: Option<&crate::asset_catalog::ResourceIndex>, file: u32, log: u32, tokens: &sc::Tokens) -> Option<String> {
    let conv = lib.conversation(index, file)?;
    let text = conv.logs.get(&log)?.first()?.clone();
    let s = lib.string(index, text.table, text.id);
    (!s.is_empty()).then(|| sc::substitute(&s, tokens))
}

/// Where the player is against a tutorial mission's target planet, as the
/// retail mission text getters ask it (0xC4EBA0 for TutorialColonize, 0xC6122D
/// state 0 for TutorialTerraform: `GetCurrentContext` (0 planet, 1 solar
/// system, 2 galaxy), the active planet record against the target planet and
/// the active star record against the target's star).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Near {
    /// Neither: the mission's plain `UIShortDesc_InProgress`.
    Elsewhere,
    /// Solar system view in the target's star system.
    RightStar,
    /// Solar system view with the target as the active planet.
    RightPlanet,
    /// Planet view of the target planet.
    OnPlanet,
}

/// The target planet of the colonize / terraform missions against the view.
pub fn near_target(
    zoom: super::HudZoom,
    active_planet: Option<u32>,
    current_system: u32,
    target: Option<u32>,
) -> Near {
    let Some(target) = target else { return Near::Elsewhere };
    match zoom {
        super::HudZoom::System if active_planet == Some(target) => Near::RightPlanet,
        super::HudZoom::System if crate::space_galaxy::system_of_world(target) == current_system => Near::RightStar,
        super::HudZoom::Planet if active_planet == Some(target) => Near::OnPlanet,
        _ => Near::Elsewhere,
    }
}

/// The conversation file and the (short, medium) log names a tutorial step's
/// mission card shows (VERIFIED data: `mission_tutorial_*.cnv`, the log ids are
/// the lower-case FNV-1 of these names; the card's headline is the short text,
/// its description the medium one, as for the raids below). `None` keeps the
/// campaign's own text: the Flight 101 city orbs and diseased animals have no
/// counterpart in the rewrite (`UIShortDesc_InProgress_Step1` / `_Step4`),
/// Story 201 names the ruin and wreck planets (`mission_ruins_target_planet`,
/// not modelled) once the player left the home system, and the fulfilled
/// texts ("Open your communications.") belong to the retail report-back flow,
/// which completes a step here the moment it is done.
pub fn tutorial_logs(
    step: rules::TutorialStep,
    near: Near,
    leaving_home: bool,
) -> Option<(&'static str, &'static str, &'static str)> {
    use rules::TutorialStep as T;
    let flight = "mission_tutorial_flight_101";
    Some(match step {
        T::Flight101Scan => (flight, "UIShortDesc_InProgress_Step2", "UIMediumDesc_InProgress_Step2"),
        // The fetch child's state 0 (0xC55F40): the abduct texts; the drop-off
        // texts follow once the creature is aboard, which completes the step.
        T::Flight101FetchAnimal => (flight, "UIShortDesc_InProgress_Step3_Abduct", "UIMediumDesc_InProgress_Step3_Abduct"),
        // 0xC5F860: in the home system outside the galaxy view the headline is
        // `UIShortDesc_LeaveHomeSystem`.
        T::Story201 if leaving_home => (
            "mission_tutorial_story_201",
            "UIShortDesc_LeaveHomeSystem",
            "UIMediumDesc_LeaveHomeSystem",
        ),
        T::TeachColonize => (
            "mission_tutorial_colonize",
            match near {
                Near::Elsewhere => "UIShortDesc_InProgress",
                Near::RightStar => "UIShortDesc_InProgress_RightStar",
                Near::RightPlanet => "UIShortDesc_InProgress_RightPlanet",
                Near::OnPlanet => "UIShortDesc_InProgress_Colonize",
            },
            "UIMediumDesc_InProgress",
        ),
        T::TeachTerraform => (
            "mission_tutorial_terraform",
            match near {
                Near::Elsewhere => "UIShortDesc_InProgress",
                Near::RightStar => "UIShortDesc_InProgress_RightStar",
                Near::RightPlanet => "UIShortDesc_InProgress_RightPlanet",
                Near::OnPlanet => "UIShortDesc_InProgress_Terraform",
            },
            "UIMediumDesc_InProgress",
        ),
        T::TeachTradeSpice => ("mission_tutorial_trade_spice", "UIShortDesc_InProgress", "UIMediumDesc_InProgress"),
        _ => return None,
    })
}

/// Replace the headline and description of the tutorial mission card with the
/// retail texts of `tutorial_logs`.
fn retail_mission_card(
    card: &mut crate::cell_contract::CellGoalCard,
    run: &RunProgress,
    zoom: super::HudZoom,
    active_planet: Option<u32>,
    lib: &mut CnvLibrary,
    index: Option<&crate::asset_catalog::ResourceIndex>,
) {
    let campaign = &run.space_campaign;
    let tutorial = &campaign.tutorial;
    let near = near_target(zoom, active_planet, campaign.current_system, tutorial.colonize_target);
    let leaving_home = zoom != super::HudZoom::Galaxy
        && campaign.current_system == crate::space_galaxy::HOME_STAR
        && tutorial.story_star.is_none();
    let Some((file, short, medium)) = tutorial_logs(tutorial.step, near, leaving_home) else { return };
    let target = tutorial.colonize_target.unwrap_or(0);
    let mut t = sc::tokens_for(run, None, target);
    if let Some(g) = crate::space_galaxy::registered_galaxy() {
        t.star_name = g.star_name(crate::space_galaxy::system_of_world(target));
    }
    // ADAPTED: the Flight 101 fetch names the animal's species (`mission_item`);
    // the rewrite takes any wild animal; TutorialTerraform asks T1.
    t.extra = vec![("mission_item", "wild".into()), ("mission_terraform_target_score", "1".into())];
    let (file, short, medium) = (sc::fnv(file), sc::fnv(short), sc::fnv(medium));
    if let Some(title) = event_log(lib, index, file, short, &t) {
        card.title = title;
        card.body = event_log(lib, index, file, medium, &t).unwrap_or_default();
    }
}

/// Put the raids in progress and the waiting transmissions on the mission
/// cards: the attack's `event_*` log texts (VERIFIED data: "Hostile UFOs are
/// attacking planet ~mission_target_planet~!!") with ships destroyed and the
/// time left. The tutorial mission's card takes its retail text
/// (`retail_mission_card`).
pub fn publish_event_cards(
    run: Res<RunProgress>,
    mut hud: ResMut<SpaceHudState>,
    mut lib: ResMut<CnvLibrary>,
    originals: Option<Res<crate::Originals>>,
    view: Option<Res<crate::space_view::SpaceView>>,
) {
    let index = originals.as_ref().map(|o| &o.resource_index);
    let campaign = &run.space_campaign;
    let zoom = hud.zoom;
    let active_planet = view.as_deref().and_then(|v| v.active_planet);
    if let Some(card) = hud.goal_cards.iter_mut().find(|c| c.kind == "Mission") {
        retail_mission_card(card, &run, zoom, active_planet, &mut lib, index);
    }
    let galaxy = crate::space_galaxy::registered_galaxy();
    let tokens_at = |world: u32| {
        let mut t = sc::tokens_for(&run, None, world);
        if let Some(g) = galaxy.as_ref() {
            t.star_name = g.star_name(crate::space_galaxy::system_of_world(world));
        }
        t
    };
    let mut cards = Vec::new();
    for a in &campaign.universe.attacks {
        let file = match a.kind {
            crate::space_universe::AttackKind::War => sc::EVENT_RAID_WAR_FILE,
            crate::space_universe::AttackKind::Grox => sc::EVENT_GROB_WAR_RAID_FILE,
            crate::space_universe::AttackKind::GroxTease => sc::EVENT_GROB_TEASER_RAID_FILE,
            crate::space_universe::AttackKind::AllyRaid => sc::EVENT_RAID_ALLY_FILE,
        };
        let mut t = tokens_at(a.world);
        t.attacker = sc::attacker_name(campaign, a.world);
        let title = event_log(&mut lib, index, file, LOG_SHORT_IN_PROGRESS, &t).unwrap_or_else(|| "Raid".into());
        let body = event_log(&mut lib, index, file, LOG_MEDIUM_IN_PROGRESS, &t).unwrap_or_default();
        let destroyed = if a.spawned { campaign.kills.saturating_sub(a.kills_at_start).min(a.fighters) } else { 0 };
        cards.push(crate::cell_contract::CellGoalCard {
            kind: "Distress call".into(),
            progress: Some((destroyed as i32, a.fighters as i32)),
            title,
            body: format!("{body} {:.0} seconds left.", a.time_left.max(0.0)).trim().to_string(),
        });
    }
    if let Some(d) = campaign.active_distress.filter(|d| d.kind == crate::space::DistressKind::PirateRaid) {
        let t = tokens_at(d.world_id);
        if let Some(title) = event_log(&mut lib, index, sc::EVENT_RAID_FILE, LOG_SHORT_IN_PROGRESS, &t) {
            if let Some(card) = hud.goal_cards.iter_mut().find(|c| c.kind == "Distress call") {
                let body = event_log(&mut lib, index, sc::EVENT_RAID_FILE, LOG_MEDIUM_IN_PROGRESS, &t).unwrap_or_default();
                card.title = title;
                card.body = format!("{body} {:.0} seconds left.", d.time_remaining.max(0.0)).trim().to_string();
            }
        }
    }
    // ADAPTED: a waiting transmission gets a card naming its sender.
    if let Some(m) = campaign.universe.inbox.iter().find(|m| m.dialog != sc::dialog::MISSION_ASSIGN) {
        let who = m.empire.map_or_else(|| player_species(&run), |e| SpaceCampaign::empire_name(e).to_string());
        cards.push(crate::cell_contract::CellGoalCard {
            kind: "Transmission".into(),
            progress: None,
            title: format!("Message from the {who}"),
            body: "Open communication to answer.".into(),
        });
    }
    if cards.is_empty() {
        return;
    }
    cards.append(&mut hud.goal_cards);
    cards.truncate(super::guide::MAX_CARDS);
    hud.goal_cards = cards;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tutorial_cards_pick_the_retail_logs_by_where_the_player_is() {
        use super::super::HudZoom as Z;
        use rules::TutorialStep as T;
        let far = 9_999; // a foreign world (system != the current one)
        // 0xC4EBA0: the solar view with the target as the active planet, or in its star system.
        let target = Some(crate::space_galaxy::FOREIGN_WORLD_BASE + crate::space_galaxy::WORLDS_PER_SYSTEM * 3);
        let t_system = crate::space_galaxy::system_of_world(target.unwrap());
        assert_eq!(near_target(Z::System, target, 0, target), Near::RightPlanet);
        assert_eq!(near_target(Z::System, Some(far), t_system, target), Near::RightStar);
        assert_eq!(near_target(Z::System, None, 0, target), Near::Elsewhere);
        assert_eq!(near_target(Z::Planet, target, 0, target), Near::OnPlanet);
        assert_eq!(near_target(Z::Planet, Some(far), t_system, target), Near::Elsewhere);
        assert_eq!(near_target(Z::Galaxy, target, t_system, target), Near::Elsewhere);
        assert_eq!(near_target(Z::System, target, 0, None), Near::Elsewhere);
        let colonize = |n| tutorial_logs(T::TeachColonize, n, false).unwrap().1;
        assert_eq!(colonize(Near::Elsewhere), "UIShortDesc_InProgress");
        assert_eq!(colonize(Near::RightStar), "UIShortDesc_InProgress_RightStar");
        assert_eq!(colonize(Near::RightPlanet), "UIShortDesc_InProgress_RightPlanet");
        assert_eq!(colonize(Near::OnPlanet), "UIShortDesc_InProgress_Colonize");
        assert_eq!(tutorial_logs(T::TeachTerraform, Near::OnPlanet, false).unwrap().1, "UIShortDesc_InProgress_Terraform");
        assert_eq!(tutorial_logs(T::Flight101Scan, Near::Elsewhere, false).unwrap().2, "UIMediumDesc_InProgress_Step2");
        assert_eq!(tutorial_logs(T::Story201, Near::Elsewhere, true).unwrap().1, "UIShortDesc_LeaveHomeSystem");
        // The orbs, the diseased animals, the ruin planets and the free play keep the campaign's text.
        for (step, leaving) in [(T::Flight101VisitCities, false), (T::Flight101EradicateAnimals, false), (T::Story201, false), (T::Done, false)] {
            assert!(tutorial_logs(step, Near::Elsewhere, leaving).is_none(), "{step:?}");
        }
    }

    #[test]
    fn the_tutorial_card_replaces_the_campaigns_text() {
        let conv = |logs: &[(&str, u32)]| {
            let mut c = sc::Conversation::default();
            for (name, id) in logs {
                c.logs.insert(sc::fnv(name), vec![sc::CnvText { id: *id, table: 7, ..Default::default() }]);
            }
            c
        };
        let mut lib = CnvLibrary::with(
            vec![(
                sc::fnv("mission_tutorial_flight_101"),
                conv(&[("UIShortDesc_InProgress_Step2", 1), ("UIMediumDesc_InProgress_Step2", 2)]),
            )],
            vec![((7, 1), "Scan the missing species.".into()), ((7, 2), "Use the Scan Tool.".into())],
        );
        let mut run = RunProgress::default();
        run.space_campaign.tutorial.step = rules::TutorialStep::Flight101Scan;
        let mut card = crate::cell_contract::CellGoalCard {
            kind: "Mission".into(),
            progress: Some((0, 1)),
            title: "Flight 101: Scan".into(),
            body: "Scan with V near your homeworld".into(),
        };
        retail_mission_card(&mut card, &run, super::super::HudZoom::System, None, &mut lib, None);
        assert_eq!((card.title.as_str(), card.body.as_str()), ("Scan the missing species.", "Use the Scan Tool."));
        assert_eq!(card.progress, Some((0, 1)), "the count stays");
        // A step whose logs are not in the library keeps the campaign's text.
        run.space_campaign.tutorial.step = rules::TutorialStep::TeachTradeSpice;
        card.title = "Trade spice".into();
        retail_mission_card(&mut card, &run, super::super::HudZoom::System, None, &mut lib, None);
        assert_eq!(card.title, "Trade spice");
    }

    /// Every log the tutorial cards ask for exists in the installed files and
    /// resolves to a non-empty string.
    #[test]
    #[ignore = "reads the installed game"]
    fn the_installed_tutorial_conversations_have_the_card_logs() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut lib = CnvLibrary::default();
        let mut t = sc::Tokens::default();
        t.extra = vec![("mission_item", "wild".into())];
        for step in rules::TutorialStep::ORDER {
            for near in [Near::Elsewhere, Near::RightStar, Near::RightPlanet, Near::OnPlanet] {
                for leaving in [false, true] {
                    let Some((file, short, medium)) = tutorial_logs(step, near, leaving) else { continue };
                    for log in [short, medium] {
                        let text = event_log(&mut lib, Some(&index), sc::fnv(file), sc::fnv(log), &t);
                        assert!(text.is_some_and(|s| !s.is_empty()), "{file} / {log}");
                    }
                }
            }
        }
        let short = event_log(
            &mut lib,
            Some(&index),
            sc::fnv("mission_tutorial_flight_101"),
            sc::fnv("UIShortDesc_InProgress_Step2"),
            &t,
        );
        assert_eq!(short.as_deref(), Some("Scan the missing species."));
    }

    #[test]
    fn emotion_variants_follow_the_archetype_switch() {
        use crate::space::SpaceArchetype as A;
        // The player's own species: playerm / playere / playerc, else diplomat.
        assert_eq!(emotion_variant(None, A::Warrior), "playerm");
        assert_eq!(emotion_variant(None, A::Trader), "playere");
        assert_eq!(emotion_variant(None, A::Shaman), "playerc");
        assert_eq!(emotion_variant(None, A::Scientist), "diplomat");
        assert_eq!(emotion_variant(None, A::Knight), "diplomat");
        // NPC empires by archetype; the Grox read `grob`.
        assert_eq!(emotion_variant(Some(0), A::Warrior), "grob");
        assert_eq!(emotion_variant(Some(1), A::Warrior), "zealot");
        assert_eq!(emotion_variant(Some(5), A::Warrior), "trader");
    }

    #[test]
    fn repairs_cost_the_retail_price_per_missing_point() {
        // A neutral empire (cost index 4): 15 per health point, 150 per energy point.
        assert_eq!(repair_price(100.0, 4), 1_500.0);
        assert_eq!(recharge_price(10.0, 4), 1_500.0);
        assert_eq!(repair_price(0.0, 4), 0.0, "nothing is missing");
        // The homeworld is free, the price grows with the owner's hostility.
        assert_eq!(repair_price(500.0, PlanetOwner::Homeworld.cost_index()), 0.0);
        assert!(
            repair_price(100.0, PlanetOwner::Hostile.cost_index())
                > repair_price(100.0, PlanetOwner::Ally.cost_index())
        );
        // Buying: the whole price when affordable, else only what the money covers.
        let mut health = Health { hp: 400.0, max: 1000.0 };
        let mut wallet = 100_000.0;
        let spent = buy_repair(4, &mut health, &mut wallet);
        assert_eq!((spent, health.hp, wallet), (9_000.0, 1000.0, 91_000.0));
        let mut health = Health { hp: 400.0, max: 1000.0 };
        let mut wallet = 300.0;
        let spent = buy_repair(4, &mut health, &mut wallet);
        assert_eq!((spent, health.hp, wallet), (300.0, 420.0, 0.0), "20 points for 300");
        let mut campaign = SpaceCampaign::default();
        campaign.energy = campaign.max_reactor_energy() - 2.0;
        let mut wallet = 10_000.0;
        let spent = buy_recharge(4, &mut campaign, &mut wallet);
        assert_eq!((spent, wallet), (300.0, 9_700.0));
        assert_eq!(campaign.energy, campaign.max_reactor_energy());
    }

    #[test]
    fn the_owner_of_a_world_follows_alliance_and_stance() {
        // Thresholds -7 / -3 / 3 / 7: stance 0-1 hostile, 2 neutral, 3-4 friendly.
        assert_eq!(owner_of(-9.0, false), PlanetOwner::Hostile);
        assert_eq!(owner_of(-3.5, false), PlanetOwner::Hostile);
        assert_eq!(owner_of(-3.0, false), PlanetOwner::Neutral);
        assert_eq!(owner_of(2.9, false), PlanetOwner::Neutral);
        assert_eq!(owner_of(3.0, false), PlanetOwner::Friendly);
        assert_eq!(owner_of(9.0, false), PlanetOwner::Friendly);
        assert_eq!(owner_of(-9.0, true), PlanetOwner::Ally, "an ally is an ally");
        let mut campaign = SpaceCampaign::default();
        campaign.allies.push(3);
        assert_eq!(owner_for(&campaign, 3), PlanetOwner::Ally);
        assert_eq!(owner_for(&campaign, 2), owner_of(campaign.relationship(2), false));
    }

    #[test]
    fn empire_and_own_worlds_in_reach_can_be_hailed() {
        let mut run = RunProgress::default();
        let world = crate::space_galaxy::world_id(5, 0);
        run.space_campaign.current_system = 5;
        run.space_campaign.current_system_empire = Some(2);
        let at = Vec3::new(10.0, 0.0, 0.0);
        let planets = [(world, at, 3.0)];
        // Over the planet: its empire.
        let target = target_for(&run, at + Vec3::X, &planets).expect("an empire world");
        assert_eq!(target.empire, 2);
        assert_eq!(target.name, crate::space_galaxy::EMPIRES[2].0);
        assert!(!target.own && !target.incoming && target.world == world);
        // Far away: nobody.
        assert!(target_for(&run, at + Vec3::X * 100.0, &planets).is_none());
        // A planet of nobody: nobody.
        assert!(target_for(&run, at, &[(7, at, 3.0)]).is_none());
        // The player's own colony answers as the player's species.
        run.space_campaign.colonies.push(world);
        let own = target_for(&run, at, &planets).expect("own colony");
        assert!(own.own);
        assert_eq!(own.owner_cost_index, PlanetOwner::Colony.cost_index());
    }

    #[test]
    fn the_screen_is_a_43_box_of_at_most_768_px_standing_on_the_bottom_edge() {
        // Solve the stored area the way the SPUI solver does for an all-anchored child.
        let solved = |screen: [f32; 2]| {
            let a = screen_area(screen);
            [a[0], a[1], a[2] + screen[0], a[3] + screen[1]]
        };
        // 1280 x 800: 1024 x 768 at (128, 32); 1280 x 900: at (128, 132).
        assert_eq!(solved([1280.0, 800.0]), [128.0, 32.0, 1152.0, 800.0]);
        let box900 = solved([1280.0, 900.0]);
        assert_eq!((box900[0], box900[1], box900[3]), (128.0, 132.0, 900.0));
        assert!((box900[2] - box900[0] - 1024.0).abs() < 0.01);
        // Below 768 the box follows the height (h x 4/3).
        let small = solved([1024.0, 600.0]);
        assert_eq!((small[1], small[3]), (0.0, 600.0));
        assert!((small[2] - small[0] - 800.0).abs() < 0.01);
    }

    #[test]
    fn a_scroll_frame_is_four_wider_than_its_window_and_gives_17_to_the_bar() {
        let fits = scroll_frame(707.0, 326.0, 120.0);
        assert_eq!(fits, ScrollFrame { content_w: 711.0, scrolls: false });
        let overflows = scroll_frame(707.0, 326.0, 400.0);
        assert_eq!(overflows, ScrollFrame { content_w: 694.0, scrolls: true });
    }

    #[test]
    fn the_greeting_text_sits_at_the_frames_corner_as_wide_as_the_content() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        let Some(layout) = assets.layout(&index, COMM_LAYOUT) else {
            return;
        };
        let mut ui = SpuiState::new(layout, crate::spui::solve::RootPolicy::Fit);
        ui.screen = [1280.0, 800.0];
        let solve = |ui: &mut SpuiState| {
            ui.rects = crate::spui::solve::solve(&ui.layout, &ui.areas, ui.screen, crate::spui::solve::RootPolicy::Fit);
        };
        let screen = ui.find(cid::SCREEN).unwrap();
        ui.set_area(screen, screen_area(ui.screen));
        solve(&mut ui);
        let box_rect = ui.rect(screen);
        assert_eq!((box_rect.x1, box_rect.y1, box_rect.x2, box_rect.y2), (128.0, 32.0, 1152.0, 800.0));
        let text = ui.find(cid::GREETING).unwrap();
        let frame = ui.rect(ui.find(cid::TEXT_FRAME).unwrap());
        fit_text(&mut ui, text);
        solve(&mut ui);
        let r = ui.rect(text);
        assert_eq!((r.x1, r.y1), (frame.x1, frame.y1), "at the frame's top-left corner");
        assert_eq!(r.width(), frame.width() + SCROLL_BAR_OVERHANG, "as wide as the content");
    }

    #[test]
    fn rows_list_responses_or_the_two_inventories() {
        let mut run = RunProgress::default();
        let mut s = CommSession { open: true, ..Default::default() };
        s.responses = vec![
            sc::ShownResponse { label: "Gift".into(), action: 1, key: (0, 0, 0), enabled: true },
            sc::ShownResponse { label: "Alliance".into(), action: 2, key: (0, 0, 0), enabled: false },
        ];
        let area = Rect::new(100.0, 700.0, 500.0, 850.0);
        let rows = row_specs(&s, &run, area, area, area, 1.0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].rect.height(), ROW_HEIGHT);
        // 18 px buttons with a 2 px gap from the frame's top-left corner (0xDD3230).
        assert_eq!(rows[1].rect.min.y - rows[0].rect.min.y, ROW_HEIGHT + ROW_GAP);
        assert_eq!((rows[0].rect.min.x, rows[0].rect.min.y), (area.min.x, area.min.y));
        assert_eq!((rows[0].colour, rows[1].colour), (ROW_COLOUR, ROW_DISABLED));
        assert_eq!(rows[1].click, RowClick::Response(1));
        // Trade: NPC stock and the hold.
        run.space_campaign.cargo_spice.insert(crate::space::SpiceColor::Red, 4);
        s.trade = Some(sc::TradeView {
            stock: vec![sc::StockItem { id: "a".into(), tool: "a".into(), label: "A".into(), price: 10.0, consumable: true, locked: None }],
            ..Default::default()
        });
        let rows = row_specs(&s, &run, area, area, Rect::new(600.0, 700.0, 900.0, 850.0), 1.0);
        assert_eq!(rows.iter().map(|r| r.click).collect::<Vec<_>>(), vec![RowClick::Npc(0), RowClick::Player(0)]);
        assert!(rows[1].text.contains("x4"));
    }
}
