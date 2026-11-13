//! The Civilization order rules of the retail UI: the vehicle command codes, the city pie menu, the
//! economic orders (trade route, city purchase, buying a rival's derrick) and the religious
//! conversion rule (helper `civ-orders`, round 2).
//!
//! Tags: **VERIFIED** (code image, march2017 / SDK addresses, disassembled with
//! `swarm-volume\target\re\dis.py`), **ADAPTED** (OpenSpore choice, marked in the code).
//!
//! Goodie huts (helper `civ-goodies`, Landing 214 base; see `GOODIE HUTS` below the pie code): the capture
//! order (type 8, behaviour 0x03F148D1: enter 0xDCCFE0, update 0xDCAAB0), the arrival rewards (0xDCAB60 ..
//! 0xDCB2A0), the hut click (0xCF4001 .. 0xCF40CB) and the level setup's hut markers (0xB9F7FE .. 0xB9F8A6).
//!
//! What the code image says (Civ UI class: message handler 0xCF56A0 = vtable 0x14786EC +0x14):
//!
//! * **The right click** (message 0x51DAB6F): when the clicked object is a spice geyser (0xE34ED0 =
//!   `Cast(cCommodityNode 0x403DF5F)`) that belongs to a civilisation (0xBFE0C0: the owner id is not
//!   -1) **and** one of the selected vehicles is economic (purpose 2), the click goes to the
//!   economic handler 0xCE8C30; every other click goes to the command handler 0xCF3D50 (civ-flow2).
//!   The command handler casts the target to a building that is *not* the city hall
//!   (`0xE9CB8BA` building, minus `0x1007AE63` hall), a turret, an animal, a vehicle, a geyser: a
//!   click on a city hall gives no order by itself - the hall opens the pie menu (the "more"
//!   cursor of `civ_cursor`).
//! * **The economic handler 0xCE8C30(object, kind)**: `kind` 0xB is the geyser popup
//!   (0xCE8940: needs a selected economic vehicle that can reach the node and a node owned by
//!   another civilisation; it shows "Bribe the Workers" with the price 0xBF0810 and colours it red
//!   when the player's wealth 0xBEF820 is lower) and `kind` 6 the city pie (0xCE84F0 / 0xCE82E0).
//!   Without a usable entry the handler plays the refusal sound 0x594303B7.
//! * **The pie menu** (layout `PieMenuIcons`, instance 0xA474ED8E: the icon atlas
//!   `0x31A44893!0x86B22644`, one window per entry; `PieMenuBase` is the empty ring) has eight entries
//!   (the names are registered in 0xCF47C0): `VehicleAttack` 0x37E95B9 (CivGame 0x04D18FF4 "Capture
//!   City"), `CityCultureAttack` 0x37E95BA ("Convert City"), `BuyCity` 0x37E95BB, `CityCreateTradeRoute`
//!   0x37E95BC ("Propose Trade Route"), `CityTradeWithCity` 0x37E95BD ("Trade With City"),
//!   `Contact` 0x46EE610, `GiveGift` 0x51B4174, `BribeNode` 0x51B40A4. Their executor is 0xCF3350:
//!   every entry orders the selected vehicles of one purpose that can reach the city (0xCA84D0) with
//!   the command code of the entry through 0xCF3210 (the first ten vehicles with phase 1, the
//!   rest with phase 2); `BuyCity` opens the purchase haggle (0xBF0520: the five offers 32000 ..
//!   4000 of `buy_city_offer_from_player`), `Contact` / `GiveGift` open the communication screen.
//! * **Entry states** (0xCE84F0, 0xCE82E0, 0xCE8940, texts of the `CivGame` table): "Capture City" needs
//!   a military vehicle (else the entry is not offered; no one that reaches: "(Cannot Reach)",
//!   0x0563CE33), "Convert City" a religious one ("(Cannot Reach)" 0x0563CE86; when the city's mood
//!   0xBD7F70 is above 50 the entry reads "Convert City (Auto-Target Entertainment to Lower
//!   Happiness)" 0x06776CA9), "Propose Trade Route" (no partner city of the player yet, 0xBDB3D0) or "Trade With
//!   City" an economic one ("Propose Trade Route (Cannot Reach)" 0x0563CEC2). "Buy City": with a
//!   trade route and the city's buy id the player's, "Buy City" once `mBuyPercent >= 100` (0xBD8110)
//!   else "Buy City (Requires More Trade)" 0x052ECBE1 in red; without a route and with an
//!   economic vehicle of the player "Buy City (Requires Trade Route)" 0x052ECBE2 in red.
//! * **The vehicle commands** (0xCAC1A0 / 0xCF3210): 1 attack, 2 capture city, 3 convert city, 5
//!   trade with city, 6 move to a point, 8 claim a geyser (any selected vehicle that can reach an
//!   unclaimed node, whatever its purpose, 0xCF3EDB), 9 create a trade route, 0xB bribe a node.
//! * **Bribe price** 0xBF0810: `(600 x cities of the buyer + 1400) x (resource points / maximum + 1)
//!   x 0.5` (0xBFDFE0 is the node's `mResourcePoints / mMaxResourcePoints`).
//!
//! ADAPTED (OpenSpore has no retail pie / popup windows, no buy percent on derricks): the entries are
//! drawn as a ring of the retail icons on the click point; the pie opens on the right click on a
//! rival city hall (retail: the click on the hall) and on a rival's derrick when an economic vehicle
//! is selected (0xCF56A0); a bribe is paid in full when the economic vehicle arrives (retail raises
//! `cCommodityNode::mBuyPercent`, rate not traced); "Buy City" is offered once the trade route is
//! open (the route's deliveries lower the price, `civ_rts`; the city is bought in one payment on the
//! communication screen, `civ_comm`), so "(Requires More Trade)" is
//! never shown; Contact / Give Gift / Buy City open the nation's communication screen.

use bevy::prelude::*;
use std::collections::HashSet;

use crate::{
    civ::{self, Approach, Building, CityState, CivCampaign, GeyserState, UnitOrder, VehicleDomain, CITY_COUNT, HOME_INDEX},
    civ_comm, civ_cursor,
    civ_planet::{Globe, Shape},
    civ_rts::{self, CivUnit, Selected},
    civ_rules,
    game::RunProgress,
    stages::{GameStage, StageOwned},
};

// ---------------------------------------------------------------------------
// Pure rules
// ---------------------------------------------------------------------------

/// A vehicle's purpose number (`cVehicle` +0xB20): 0 military, 1 religious, 2 economic.
pub const fn purpose(kind: Approach) -> usize {
    match kind {
        Approach::Military => 0,
        Approach::Religious => 1,
        Approach::Economic => 2,
    }
}

/// The retail vehicle command codes (0xCAC1A0 / 0xCF3210 / 0xCF3350).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Attack,
    Capture,
    Convert,
    TradeWithCity,
    Move,
    Claim,
    CreateTradeRoute,
    BribeNode,
}

impl Command {
    pub const fn code(self) -> u32 {
        match self {
            Command::Attack => 1,
            Command::Capture => 2,
            Command::Convert => 3,
            Command::TradeWithCity => 5,
            Command::Move => 6,
            Command::Claim => 8,
            Command::CreateTradeRoute => 9,
            Command::BribeNode => 0xB,
        }
    }
}

/// The entries of the pie menu with their retail message ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PieItem {
    CaptureCity,
    ConvertCity,
    ProposeTradeRoute,
    TradeWithCity,
    BuyCity,
    GiveGift,
    Contact,
    BribeNode,
}

/// The `PieMenuIcons` atlas (`0x31A44893!0x86B22644`, 256 x 256).
pub const ATLAS_SIZE: f32 = 256.0;

impl PieItem {
    /// The control / message id of the entry (the ids of 0xCF47C0's registration list).
    pub const fn message(self) -> u32 {
        match self {
            PieItem::CaptureCity => 0x037E_95B9,
            PieItem::ConvertCity => 0x037E_95BA,
            PieItem::ProposeTradeRoute => 0x037E_95BC,
            PieItem::TradeWithCity => 0x037E_95BD,
            PieItem::BuyCity => 0x037E_95BB,
            PieItem::GiveGift => 0x051B_4174,
            PieItem::Contact => 0x046E_E610,
            PieItem::BribeNode => 0x051B_40A4,
        }
    }

    /// The name the retail UI registers the message under.
    pub const fn registered_name(self) -> &'static str {
        match self {
            PieItem::CaptureCity => "VehicleAttack",
            PieItem::ConvertCity => "CityCultureAttack",
            PieItem::ProposeTradeRoute => "CityCreateTradeRoute",
            PieItem::TradeWithCity => "CityTradeWithCity",
            PieItem::BuyCity => "BuyCity",
            PieItem::GiveGift => "GiveGift",
            PieItem::Contact => "Contact",
            PieItem::BribeNode => "BribeNode",
        }
    }

    /// The vehicle command the entry gives (`None`: a communication entry).
    pub const fn command(self) -> Option<Command> {
        match self {
            PieItem::CaptureCity => Some(Command::Capture),
            PieItem::ConvertCity => Some(Command::Convert),
            PieItem::ProposeTradeRoute => Some(Command::CreateTradeRoute),
            PieItem::TradeWithCity => Some(Command::TradeWithCity),
            PieItem::BribeNode => Some(Command::BribeNode),
            PieItem::BuyCity | PieItem::GiveGift | PieItem::Contact => None,
        }
    }

    /// The purpose of the vehicles the entry orders.
    pub const fn purpose(self) -> Option<usize> {
        match self {
            PieItem::CaptureCity => Some(0),
            PieItem::ConvertCity => Some(1),
            PieItem::ProposeTradeRoute | PieItem::TradeWithCity | PieItem::BribeNode => Some(2),
            PieItem::BuyCity | PieItem::GiveGift | PieItem::Contact => None,
        }
    }

    /// The icon's region of the atlas in 256 px units (x0, y0, x1, y1; the layout's uv times 256, some
    /// run past the border: the part outside is clear).
    pub const fn icon(self) -> [f32; 4] {
        match self {
            PieItem::CaptureCity => [106.0, -10.0, 176.0, 54.0],
            PieItem::ConvertCity => [-10.0, -11.0, 60.0, 53.0],
            PieItem::BuyCity => [47.0, -10.0, 117.0, 54.0],
            PieItem::ProposeTradeRoute | PieItem::TradeWithCity => [221.0, -1.0, 253.0, 32.0],
            PieItem::Contact => [93.0, 52.0, 133.0, 99.0],
            PieItem::GiveGift => [171.0, 0.0, 219.0, 43.0],
            PieItem::BribeNode => [-11.0, 100.0, 53.0, 164.0],
        }
    }
}

/// `CivGame` strings of the entries (id, text; read from the install, `spinspect.py locale CivGame`).
pub mod text {
    pub const CAPTURE_CITY: (u32, &str) = (0x04D1_8FF4, "Capture City");
    pub const CAPTURE_CANNOT_REACH: (u32, &str) = (0x0563_CE33, "Capture City (Cannot Reach)");
    pub const CONVERT_CITY: (u32, &str) = (0x04D1_9085, "Convert City");
    pub const CONVERT_CANNOT_REACH: (u32, &str) = (0x0563_CE86, "Convert City (Cannot Reach)");
    pub const CONVERT_AUTO_TARGET: (u32, &str) =
        (0x0677_6CA9, "Convert City (Auto-Target Entertainment to Lower Happiness)");
    pub const PROPOSE_TRADE_ROUTE: (u32, &str) = (0x04D1_912C, "Propose Trade Route");
    pub const PROPOSE_CANNOT_REACH: (u32, &str) = (0x0563_CEC2, "Propose Trade Route (Cannot Reach)");
    pub const TRADE_WITH_CITY: (u32, &str) = (0x04D1_9177, "Trade With City");
    pub const BUY_CITY: (u32, &str) = (0x04D1_956A, "Buy City");
    pub const BUY_MORE_TRADE: (u32, &str) = (0x052E_CBE1, "Buy City (Requires More Trade)");
    pub const BUY_NEEDS_ROUTE: (u32, &str) = (0x052E_CBE2, "Buy City (Requires Trade Route)");
    pub const GIVE_GIFT: (u32, &str) = (0x04D1_94FE, "Give Gift");
    pub const CONTACT: (u32, &str) = (0x04D2_9B0C, "Contact");
    pub const BRIBE: (u32, &str) = (0x06A5_6B8F, "Bribe the Workers");
}

/// One entry of the pie as the retail state code shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct PieEntry {
    pub item: PieItem,
    /// The `CivGame` string id and its text.
    pub label: u32,
    pub text: String,
    /// The entry can be used (retail: a "(Cannot Reach)" / "(Requires ..)" text is drawn but its
    /// message gives no order).
    pub enabled: bool,
    /// Drawn in red (retail colour 1, 0, 0 of the unusable buy and bribe texts).
    pub warn: bool,
}

impl PieEntry {
    fn new(item: PieItem, text: (u32, &str), enabled: bool, warn: bool) -> Self {
        Self { item, label: text.0, text: text.1.to_string(), enabled, warn }
    }
}

/// What the selection holds, by purpose (military, religious, economic): how many vehicles and how
/// many of them can reach the target (0xCA84D0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PieSelection {
    pub selected: [usize; 3],
    pub reach: [usize; 3],
}

/// The city facts the pie reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CityFacts {
    /// The player's money-making partner relation: a trade route to the city is open
    /// (0xBDB350 / 0xBDB3D0).
    pub route: bool,
    /// 0xBD7F70: the mood is low enough to convert (<= 50).
    pub convertible: bool,
    /// The nation is known (always, OpenSpore).
    pub known: bool,
}

/// The mood at or under which the religious vehicles convert a city (0xBD7F70: `mood <= 50`).
pub const CONVERT_MOOD_LIMIT: i32 = 50;

/// The entries the pie of a rival city shows for the selection (0xCE84F0 then 0xCE82E0).
pub fn city_pie(sel: &PieSelection, city: &CityFacts) -> Vec<PieEntry> {
    let mut out = Vec::new();
    if sel.selected[0] > 0 {
        out.push(if sel.reach[0] > 0 {
            PieEntry::new(PieItem::CaptureCity, text::CAPTURE_CITY, true, false)
        } else {
            PieEntry::new(PieItem::CaptureCity, text::CAPTURE_CANNOT_REACH, false, true)
        });
    }
    if sel.selected[1] > 0 {
        out.push(if sel.reach[1] == 0 {
            PieEntry::new(PieItem::ConvertCity, text::CONVERT_CANNOT_REACH, false, true)
        } else if city.convertible {
            PieEntry::new(PieItem::ConvertCity, text::CONVERT_CITY, true, false)
        } else {
            PieEntry::new(PieItem::ConvertCity, text::CONVERT_AUTO_TARGET, true, false)
        });
    }
    if sel.selected[2] > 0 {
        out.push(if sel.reach[2] == 0 {
            PieEntry::new(PieItem::ProposeTradeRoute, text::PROPOSE_CANNOT_REACH, false, true)
        } else if city.route {
            PieEntry::new(PieItem::TradeWithCity, text::TRADE_WITH_CITY, true, false)
        } else {
            PieEntry::new(PieItem::ProposeTradeRoute, text::PROPOSE_TRADE_ROUTE, true, false)
        });
    }
    // 0xCE82E0: Buy City with a route, or the red "requires a trade route" while the player owns an
    // economic vehicle (`sel.selected[2]` stands in for 0xBF0330: the player's economic vehicles).
    if city.route {
        out.push(PieEntry::new(PieItem::BuyCity, text::BUY_CITY, true, false));
    } else if sel.selected[2] > 0 {
        out.push(PieEntry::new(PieItem::BuyCity, text::BUY_NEEDS_ROUTE, false, true));
    }
    if city.known {
        out.push(PieEntry::new(PieItem::GiveGift, text::GIVE_GIFT, true, false));
    }
    out.push(PieEntry::new(PieItem::Contact, text::CONTACT, true, false));
    out
}

/// The popup of a rival's derrick (0xCE8940): `None` unless an economic vehicle can reach it.
pub fn node_popup(sel: &PieSelection, price: f32, wealth: f32) -> Option<PieEntry> {
    if sel.reach[2] == 0 {
        return None;
    }
    let mut entry = PieEntry::new(PieItem::BribeNode, text::BRIBE, true, price > wealth);
    entry.text = format!("{} (${})", text::BRIBE.1, with_thousands(price.round() as i64));
    Some(entry)
}

fn with_thousands(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

/// Where a right click goes (0xCF56A0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickRoute {
    /// The economic handler 0xCE8C30 (popups).
    Economic,
    /// The command handler 0xCF3D50.
    Command,
}

/// VERIFIED 0xCF56A0 (message 0x51DAB6F): a geyser held by a civilisation and an economic vehicle
/// in the selection take the economic handler.
pub fn route_right_click(owned_geyser: bool, selection_has_economic: bool) -> ClickRoute {
    if owned_geyser && selection_has_economic {
        ClickRoute::Economic
    } else {
        ClickRoute::Command
    }
}

/// Who holds a geyser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    Nobody,
    Player,
    Rival,
}

/// VERIFIED 0xCF3D50 geyser branch (+ the popup of 0xCE8C30): the command a vehicle of `kind` gets
/// from a click on a geyser it can reach. Unclaimed: every vehicle claims (command 8, 0xCF3EDB);
/// a rival's: military and religious attack (command 1), economic vehicles never (they bribe through
/// the popup: command 0xB); the player's own: nothing.
pub fn geyser_command(holder: Holder, kind: Approach, via_popup: bool) -> Option<Command> {
    match holder {
        Holder::Nobody => Some(Command::Claim),
        Holder::Player => None,
        Holder::Rival => match kind {
            Approach::Economic => via_popup.then_some(Command::BribeNode),
            _ => Some(Command::Attack),
        },
    }
}

/// VERIFIED 0xBF0810 (the node popup's price): `(600 x buyer cities + 1400) x (points / maximum + 1) x 0.5`.
pub fn bribe_price(buyer_cities: usize, points: f32) -> f32 {
    let remaining = (points / civ_rules::DERRICK_START_POINTS).clamp(0.0, 1.0);
    (buyer_cities as f32 * 600.0 + 1400.0) * (remaining + 1.0) * 0.5
}

/// What bribing a derrick came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Bribe {
    /// Bought: price, the former owner.
    Done { price: f32, seller: usize },
    /// Not enough money.
    Poor { price: f32 },
    /// Nothing to buy (unclaimed, the buyer's own, or no such geyser).
    Refused,
}

/// Cities a nation holds (the buyer count of 0xBF0810).
pub fn cities_of(campaign: &CivCampaign, nation: usize) -> usize {
    campaign.cities.iter().filter(|c| c.nation() == nation && campaign.present(c.index)).count()
}

/// Buy the derrick on geyser `g` for `buyer` (command 0xB). ADAPTED: paid in full at once (retail
/// raises `mBuyPercent` while the economic vehicle stays; the rate was not traced). `money` is the
/// player's purse; a rival pays from its own.
pub fn bribe(campaign: &mut CivCampaign, money: &mut f32, buyer: usize, g: usize) -> Bribe {
    let Some(state) = campaign.geysers.get(g) else { return Bribe::Refused };
    let Some(seller) = state.owner else { return Bribe::Refused };
    if seller == buyer {
        return Bribe::Refused;
    }
    let points = state.points;
    let price = bribe_price(cities_of(campaign, buyer), points);
    let purse = if buyer == HOME_INDEX {
        &mut *money
    } else {
        match campaign.nations.get_mut(buyer) {
            Some(n) => &mut n.spice,
            None => return Bribe::Refused,
        }
    };
    if *purse < price {
        return Bribe::Poor { price };
    }
    *purse -= price;
    if seller == HOME_INDEX {
        *money = (*money + price).min(civ_rules::WEALTH_CAP);
    } else if let Some(n) = campaign.nations.get_mut(seller) {
        n.spice = (n.spice + price).min(civ_rules::WEALTH_CAP);
    }
    campaign.geysers[g] = GeyserState::derrick(buyer, points);
    Bribe::Done { price, seller }
}

/// The nearest standing Entertainment building of the city to `at`.
pub fn nearest_entertainment(city: &CityState, at: Vec3, shape: Shape) -> Option<(usize, Vec3)> {
    (0..city.plots.len())
        .filter(|p| city.plots[*p] == Some(Building::Entertainment) && city.standing(*p))
        .map(|p| (p, civ_rts::plot_world(city, p, shape)))
        .min_by(|a, b| shape.distance(at, a.1).total_cmp(&shape.distance(at, b.1)))
}

/// VERIFIED 0xBD7F70 + the entry text 0x06776CA9: religious vehicles ordered against a city whose
/// mood is above 50 "auto-target entertainment to lower happiness" - they shoot its Entertainment
/// buildings and convert only once the mood is 50 or less. ADAPTED: a city without a standing
/// Entertainment building converts at any mood (the retail fallback is not known, and without
/// the exception such a city could never be converted).
pub fn convert_locked(city: &CityState) -> bool {
    city.happiness() > CONVERT_MOOD_LIMIT && city.plots.iter().enumerate().any(|(p, b)| *b == Some(Building::Entertainment) && city.standing(p))
}

// ---------------------------------------------------------------------------
// The pie in the world
// ---------------------------------------------------------------------------

/// What a pie is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieTarget {
    City(usize),
    Geyser(usize),
}

/// A right click that wants a pie (set by `civ_rts::rts_input`, taken by [`pie_open`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieRequest {
    pub target: PieTarget,
    pub at: Vec2,
}

/// The open pie: its entries, where each is drawn (screen rectangles) and which of the player's
/// vehicles can reach the target, by purpose.
#[derive(Clone, Debug)]
pub struct OpenPie {
    pub target: PieTarget,
    pub at: Vec2,
    pub entries: Vec<PieEntry>,
    pub rects: Vec<Rect>,
    pub reach: [Vec<u32>; 3],
}

/// The right click's request, kept apart from [`CivPie`] so `rts_input` can write it while its
/// pointer reads the open pie.
#[derive(Resource, Default)]
pub struct PieRequests(pub Option<PieRequest>);

#[derive(Resource, Default)]
pub struct CivPie {
    pub open: Option<OpenPie>,
}

impl CivPie {
    /// The cursor is over the pie (clicks belong to it).
    pub fn covers(&self, p: Vec2) -> bool {
        self.open.as_ref().is_some_and(|o| o.rects.iter().any(|r| r.contains(p)))
    }
}

/// Entry box size (the retail icons are 70 x 64).
const BOX: Vec2 = Vec2::new(76.0, 70.0);
const RING: f32 = 84.0;

/// Screen rectangles of `n` entries on a ring around `at`, kept inside the screen.
pub fn ring(n: usize, at: Vec2, screen: Vec2) -> Vec<Rect> {
    let radius = if n <= 1 { 0.0 } else { RING + 6.0 * (n.saturating_sub(4)) as f32 };
    let mut centre = at;
    let margin = radius + BOX.max_element() * 0.5 + 4.0;
    centre.x = centre.x.clamp(margin.min(screen.x * 0.5), (screen.x - margin).max(screen.x * 0.5));
    centre.y = centre.y.clamp(margin.min(screen.y * 0.5), (screen.y - margin).max(screen.y * 0.5));
    (0..n)
        .map(|i| {
            let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n.max(1) as f32;
            let c = centre + Vec2::new(angle.cos(), angle.sin()) * radius;
            Rect::from_center_size(c, BOX)
        })
        .collect()
}

/// Build the pie of a request: the selection's reach by purpose, the entries, their places.
#[allow(clippy::too_many_arguments)]
pub fn pie_open(
    mut requests: ResMut<PieRequests>,
    mut pie: ResMut<CivPie>,
    run: Res<RunProgress>,
    windows: Query<&Window>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    units: Query<(&CivUnit, &Transform, Has<Selected>), Without<civ::CivCommander>>,
    mut regions: Local<Option<civ_cursor::Regions>>,
) {
    let Some(request) = requests.0.take() else { return };
    let campaign = &run.civ_campaign;
    let surface = surface.as_deref().filter(|s| s.stage == GameStage::Civilization);
    let globe = Globe::from_surface(surface);
    if globe.on_planet() && regions.as_ref().map(|r| r.key()) != Some(campaign.layout_key) {
        *regions = civ_cursor::Regions::build(&globe, campaign.layout_key);
    }
    let regions_ref = regions.as_ref().filter(|_| globe.on_planet());
    let to = match request.target {
        PieTarget::City(c) => campaign.cities.get(c).map(|c| c.center()),
        PieTarget::Geyser(g) => (g < civ::GEYSER_COUNT).then(|| campaign.geyser_at(g)),
    };
    let Some(to) = to else { return };
    let mut sel = PieSelection::default();
    let mut reach: [Vec<u32>; 3] = Default::default();
    for (unit, t, selected) in &units {
        if !selected || unit.nation != HOME_INDEX {
            continue;
        }
        let k = purpose(unit.kind);
        sel.selected[k] += 1;
        // 0xCA84D0: aircraft reach everything, land and sea vehicles need their medium and landmass.
        if unit.domain == VehicleDomain::Air || civ_cursor::medium_reach(regions_ref, &globe, unit.domain, t.translation, to) {
            sel.reach[k] += 1;
            reach[k].push(unit.id);
        }
    }
    let entries: Vec<PieEntry> = match request.target {
        PieTarget::City(c) => {
            let city = &campaign.cities[c];
            let facts = CityFacts { route: city.trade, convertible: !(city.happiness() > CONVERT_MOOD_LIMIT), known: true };
            city_pie(&sel, &facts)
        }
        PieTarget::Geyser(g) => {
            let points = campaign.geysers[g].points;
            let price = bribe_price(cities_of(campaign, HOME_INDEX), points);
            node_popup(&sel, price, run.spice).into_iter().collect()
        }
    };
    if entries.is_empty() {
        // Retail 0xCE8C30: no usable entry plays the refusal sound; the message stands in for it.
        pie.open = None;
        return;
    }
    let screen = windows.iter().next().map_or(Vec2::new(1280.0, 800.0), |w| Vec2::new(w.width(), w.height()));
    let rects = ring(entries.len(), request.at, screen);
    pie.open = Some(OpenPie { target: request.target, at: request.at, entries, rects, reach });
}

/// The pie's windows (despawned with it).
#[derive(Component)]
pub struct PieUi;

#[derive(Component)]
pub struct PieHint;

/// Draw the open pie as a ring of the retail icons with the hovered entry's text under the ring.
#[allow(clippy::too_many_arguments)]
pub fn pie_ui(
    mut commands: Commands,
    pie: Res<CivPie>,
    shown: Query<Entity, With<PieUi>>,
    state: Option<Res<civ_rts::RtsState>>,
    originals: Option<Res<crate::Originals>>,
    mut assets: Option<ResMut<crate::spui::view::SpuiAssets>>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut hints: Query<(&mut Text, &mut TextColor, &mut Node), With<PieHint>>,
    windows: Query<&Window>,
) {
    if pie.is_changed() {
        for e in &shown {
            commands.entity(e).despawn();
        }
        if let Some(open) = &pie.open {
            let camera = state.as_deref().and_then(|s| s.camera);
            let atlas = match (originals.as_deref(), assets.as_mut(), images.as_mut()) {
                (Some(o), Some(a), Some(i)) => a.texture(
                    &o.resource_index,
                    i,
                    crate::dbpf::ResourceKey { type_id: 0x2F7D_0004, group_id: 0x31A4_4893, instance_id: 0x86B2_2644 },
                ),
                _ => None,
            };
            for (entry, rect) in open.entries.iter().zip(&open.rects) {
                let dim: f32 = if entry.enabled { 1.0 } else { 0.4 };
                let mut ui = commands.spawn((
                    StageOwned,
                    PieUi,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(rect.min.x),
                        top: Val::Px(rect.min.y),
                        width: Val::Px(rect.width()),
                        height: Val::Px(rect.height()),
                        border: UiRect::all(Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(12.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.02, 0.05, 0.1, 0.78 * dim.max(0.6))),
                    BorderColor::all(if entry.warn { Color::srgba(1.0, 0.2, 0.2, 0.9) } else { Color::srgba(0.6, 0.9, 1.0, 0.9) }),
                    GlobalZIndex(120),
                ));
                if let Some(camera) = camera {
                    ui.insert(UiTargetCamera(camera));
                }
                let icon = entry.item.icon();
                let tex = atlas.as_ref();
                ui.with_children(|b| match tex {
                    Some(tex) => {
                        // The clear part of the region outside the atlas is not drawn.
                        let (x0, y0) = (icon[0].max(0.0), icon[1].max(0.0));
                        let (x1, y1) = (icon[2].min(ATLAS_SIZE), icon[3].min(ATLAS_SIZE));
                        b.spawn((
                            Node { width: Val::Px(x1 - x0), height: Val::Px(y1 - y0), ..default() },
                            ImageNode {
                                image: tex.handle.clone(),
                                rect: Some(Rect::new(x0, y0, x1, y1)),
                                color: Color::srgba(1.0, 1.0, 1.0, dim),
                                ..default()
                            },
                        ));
                    }
                    None => {
                        b.spawn((
                            Text::new(entry.text.clone()),
                            TextFont { font_size: bevy::text::FontSize::Px(10.0), ..default() },
                            TextColor(Color::WHITE),
                        ));
                    }
                });
            }
            // The text line under the ring.
            let (lo, hi) = open.rects.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), r| (lo.min(r.min), hi.max(r.max)));
            let mut hint = commands.spawn((
                StageOwned,
                PieUi,
                PieHint,
                Text::new(String::new()),
                TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() },
                TextColor(Color::WHITE),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(lo.x),
                    top: Val::Px(hi.y + 6.0),
                    width: Val::Px((hi.x - lo.x).max(220.0)),
                    ..default()
                },
                GlobalZIndex(121),
            ));
            if let Some(camera) = camera {
                hint.insert(UiTargetCamera(camera));
            }
        }
    }
    // The hovered entry's text (retail: the tooltip of the entry's window).
    if let Some(open) = &pie.open {
        let cursor = windows.iter().find_map(|w| w.cursor_position());
        let hovered = cursor.and_then(|c| open.rects.iter().position(|r| r.contains(c)));
        for (mut text, mut color, _) in &mut hints {
            let (line, warn) = match hovered {
                Some(i) => (open.entries[i].text.clone(), open.entries[i].warn),
                None => (String::new(), false),
            };
            if text.0 != line {
                text.0 = line;
            }
            color.0 = if warn { Color::srgb(1.0, 0.35, 0.35) } else { Color::WHITE };
        }
    }
}

/// A click on the open pie: an entry gives its order, anything else (or Escape) closes it.
#[allow(clippy::too_many_arguments)]
pub fn pie_click(
    mut commands: Commands,
    mut pie: ResMut<CivPie>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    mut run: ResMut<RunProgress>,
    mut units: Query<(Entity, &mut CivUnit), Without<civ::CivCommander>>,
    local: Option<ResMut<crate::civ_hud::CivHudLocal>>,
) {
    let Some(open) = pie.open.clone() else { return };
    let cursor = windows.iter().find_map(|w| w.cursor_position());
    let clicked = cursor.filter(|_| mouse.just_pressed(MouseButton::Left));
    if keys.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
        pie.open = None;
        return;
    }
    let Some(click) = clicked else { return };
    let Some(i) = open.rects.iter().position(|r| r.contains(click)) else {
        pie.open = None;
        return;
    };
    let entry = &open.entries[i];
    pie.open = None;
    if !entry.enabled {
        run.civ_campaign.message = entry.text.clone();
        return;
    }
    let run = &mut *run;
    match (open.target, entry.item) {
        (PieTarget::City(c), item) if item.command().is_some() => {
            let k = item.purpose().unwrap_or(0);
            let allowed: HashSet<u32> = open.reach[k].iter().copied().collect();
            let mut ordered = 0;
            for (e, mut unit) in &mut units {
                if unit.nation == HOME_INDEX && purpose(unit.kind) == k && allowed.contains(&unit.id) {
                    unit.order = UnitOrder::City(c);
                    unit.idle = 0.0;
                    commands.entity(e).insert(crate::civ_voices::VoiceOrder);
                    ordered += 1;
                }
            }
            run.civ_campaign.message = format!("{ordered} vehicles: {} - {}", entry.text, civ::city_label(c));
        }
        (PieTarget::Geyser(g), PieItem::BribeNode) => {
            let allowed: HashSet<u32> = open.reach[2].iter().copied().collect();
            let mut ordered = 0;
            for (e, mut unit) in &mut units {
                if unit.nation == HOME_INDEX && unit.kind == Approach::Economic && allowed.contains(&unit.id) {
                    unit.order = UnitOrder::Geyser(g);
                    unit.idle = 0.0;
                    commands.entity(e).insert(crate::civ_voices::VoiceOrder);
                    ordered += 1;
                }
            }
            run.civ_campaign.message = format!("{ordered} vehicles go to bribe the derrick's workers");
        }
        (PieTarget::City(c), _) => {
            // Buy City / Contact / Give Gift: the nation's communication screen (ADAPTED).
            if let Some(mut local) = local {
                local.comm = Some(crate::civ_hud::comm::CommSession { nation: c, menu: crate::civ_hud::comm::CommMenu::Root });
            }
            run.civ_campaign.message = format!("{}: {}", civ::city_label(c), entry.text);
        }
        _ => {}
    }
}

fn reset(mut pie: ResMut<CivPie>, mut requests: ResMut<PieRequests>, mut huts: ResMut<HutRequests>) {
    *pie = CivPie::default();
    *requests = PieRequests::default();
    *huts = HutRequests::default();
}

// ---------------------------------------------------------------------------
// GOODIE HUTS
// ---------------------------------------------------------------------------
//
// VERIFIED (march2017, `swarm-volume\target\re\dis.py`):
// * Where they stand: the Civ level setup 0xB9D970 ends (0xB9F7FE .. 0xB9F833) by registering the
//   `Civ_GoodyHut` marker (`noun_placement_markers~` 0x05F4AF23, group 0x0302A1C9; noun 0x57F09E5E = the NPC
//   tribe marker; ring 121 .. 573 from the origin, exclusive radius 90) once per city marker through the
//   marker placer 0xB9B1E0, and 0xB9F833 .. 0xB9F89F walks every `0x57F09E5E` marker and builds a tribe at it
//   (0xB942A0 -> 0xC932F0, `RandomInt(3)` as its type): the tribe's hut is the goodie hut. `civ_planet::hut_sites`.
// * The click: 0xCF4001 .. 0xCF40CB, the hut's tribe flag `+0x557` must be clear; every selected vehicle that
//   can reach the hut (0xC9EAF0) gets the capture order (0xCAC1A0(vehicle, hut, type 8, 1)); at least one:
//   `sfx_civ_target_hut` (0x5580E0F9) at the hut (0xCF40BF). The cursor says goodie-hut (`civ_cursor`).
// * The order (0xDCAAB0, three states): 0 drive to the hut (0xC9FCD0, radii 8 and 4); 1 while driving, when
//   the vehicle is within 30 of the hut the tribe's members are alarmed once (hut `+0x220`; not modelled: the
//   Civ planet has no tribe members); arrival (0xC42F80) -> 2; 2 the capture (0xDCAB60): a hut whose tribe is
//   not a Civ slot (0xB26910 list, 0xAE3CC0) gives the goodie, then the tribe is destroyed (0xDCB2A0), the hut
//   goes. The voice `Capture_GoodieHut` is the order voice (0xDCC5A0: type 8 on a node that is not the geyser
//   noun 0x403DF5C).
// * The goodie (0xDCACF1): `RandomInt(2)`: 0 -> money `400 + R(400) + R(400) + R(200)` (0xDCAD68 .. 0xDCADB4,
//   `cCivilization::AddMoney` 0xBEFED0), the event-log line 0x4B7AE141 for the player; else a vehicle
//   (0xDCB031): the capturer's purpose, rerolled `RandomInt(3)` for the player, one of three premade vehicle
//   ids by that number (0x7D433FAD / 0x9AD7D4AA / 0xF670AA43), created by 0xBF58F0 at the hut; the player hears
//   `New_Goodie` of the new vehicle's class (table 0x147C5DC, 0xDCB1FA) and sees the event-log line 0x9964AE79.
// ADAPTED / INFERRED: the numbers of huts (one per city, the marker count was not tallied), the picking radius,
// the capture radius roles (8 = capture, 4 = stop, both read from the move call), the purposes of the three
// premade ids (0 military, 1 religious, 2 economic, the order of the table `New_Goodie`), the event log lines
// (OpenSpore's message line), the flag and the alarm of the tribe members (no tribes).

/// 0xDCB4B9 `fld 4.0` of the move call: the vehicle stops this close to the hut (retail units).
pub const HUT_STOP: f32 = 4.0;
/// 0xDCB490 `fld 8.0` of the move call: a vehicle this close has arrived and captures (retail units).
pub const HUT_CAPTURE: f32 = 8.0;

/// What a goodie hut gives (0xDCACF1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Goodie {
    /// Spice (the retail money) for the capturer's civilization.
    Money(f32),
    /// A free vehicle of the class.
    Vehicle(Approach),
}

/// The goodie roll in the retail order of draws (`roll(n)` = `RandomInt(n)`): `RandomInt(2)` picks money
/// (0: `400 + R(400) + R(400) + R(200)`, 400 .. 1398) or a vehicle (the capturer's purpose, rerolled
/// `RandomInt(3)` when the capturer is the player: 0 military, 1 religious, 2 economic).
pub fn goodie_roll(player: bool, own: Approach, mut roll: impl FnMut(u32) -> u32) -> Goodie {
    if roll(2) == 0 {
        let a = roll(400);
        let b = roll(400);
        let c = roll(200);
        return Goodie::Money(400.0 + (a + b + c) as f32);
    }
    let kind = if player {
        match roll(3) {
            0 => Approach::Military,
            1 => Approach::Religious,
            _ => Approach::Economic,
        }
    } else {
        own
    };
    Goodie::Vehicle(kind)
}

/// A right-click on a hut: the fleet that was ordered (`civ_rts::rts_input` fills it).
#[derive(Clone, Debug)]
pub struct HutRequest {
    pub hut: usize,
    pub fleet: Vec<Entity>,
}

#[derive(Resource, Default)]
pub struct HutRequests(pub Option<HutRequest>);

/// A hut standing on the planet (the model entity).
#[derive(Component, Clone, Copy, Debug)]
pub struct CivHut(pub usize);

/// Give the fleet the capture order: the vehicles that can reach the hut (0xC9EAF0: aircraft always, land and
/// sea vehicles by medium and landmass) take it; the order voice and `sfx_civ_target_hut` come from
/// `civ_voices` (the `VoiceOrder` marker).
pub fn hut_command(
    mut requests: ResMut<HutRequests>,
    mut run: ResMut<RunProgress>,
    mut commands: Commands,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut units: Query<(&mut CivUnit, &Transform)>,
    mut regions: Local<Option<civ_cursor::Regions>>,
) {
    let Some(request) = requests.0.take() else { return };
    let campaign = &run.civ_campaign;
    let Some(to) = campaign.hut_at(request.hut).filter(|_| campaign.hut_standing(request.hut)) else { return };
    let surface = surface.as_deref().filter(|s| s.stage == GameStage::Civilization);
    let globe = Globe::from_surface(surface);
    if globe.on_planet() && regions.as_ref().map(|r| r.key()) != Some(campaign.layout_key) {
        *regions = civ_cursor::Regions::build(&globe, campaign.layout_key);
    }
    let regions_ref = regions.as_ref().filter(|_| globe.on_planet());
    let mut ordered = 0;
    for e in request.fleet {
        let Ok((mut unit, t)) = units.get_mut(e) else { continue };
        if unit.nation != HOME_INDEX {
            continue;
        }
        if unit.domain == VehicleDomain::Air
            || civ_cursor::medium_reach(regions_ref, &globe, unit.domain, t.translation, to)
        {
            unit.order = UnitOrder::Hut(request.hut);
            unit.idle = 0.0;
            commands.entity(e).insert(crate::civ_voices::VoiceOrder);
            ordered += 1;
        }
    }
    run.civ_campaign.message = if ordered > 0 {
        format!("{ordered} vehicles head for the goodie hut")
    } else {
        "None of the selected vehicles can reach that goodie hut.".into()
    };
}

/// The capture (0xDCAB60): the first vehicle of a capture order within `HUT_CAPTURE` of its hut takes the
/// goodie, the hut is gone (its tribe destroyed, 0xDCB2A0) and the order is done. A rival's vehicle never
/// holds a hut order here (the AI does not hunt huts: not decoded), the nation is still honoured.
pub fn hut_arrival(
    mut commands: Commands,
    mut run: ResMut<RunProgress>,
    mut spawns: ResMut<civ_rts::UnitSpawns>,
    mut units: Query<(&mut CivUnit, &Transform)>,
) {
    let shape = run.civ_campaign.shape();
    let mut arrived: Vec<(u32, usize)> = units
        .iter()
        .filter_map(|(u, t)| match u.order {
            UnitOrder::Hut(h) => {
                let at = run.civ_campaign.hut_at(h)?;
                (shape.distance(t.translation, at) <= HUT_CAPTURE).then_some((u.id, h))
            }
            _ => None,
        })
        .collect();
    arrived.sort_unstable();
    for (id, h) in arrived {
        if !run.civ_campaign.hut_standing(h) {
            continue;
        }
        let Some(at) = run.civ_campaign.hut_at(h) else { continue };
        let Some((nation, own)) = units.iter().find(|(u, _)| u.id == id).map(|(u, _)| (u.nation, u.kind)) else {
            continue;
        };
        let goodie = goodie_roll(nation == HOME_INDEX, own, |n| rand::random::<u32>() % n);
        run.civ_campaign.huts_taken.push(h);
        match goodie {
            Goodie::Money(amount) => {
                if nation == HOME_INDEX {
                    run.spice += amount;
                    run.civ_campaign.message = format!("Goodie hut: {amount:.0} spice for your treasury");
                } else if let Some(n) = run.civ_campaign.nations.get_mut(nation) {
                    n.spice += amount;
                }
            }
            Goodie::Vehicle(kind) => {
                let unit = run.civ_campaign.add_unit_domain(nation, kind, VehicleDomain::Land, at);
                spawns.0.push(unit);
                if nation == HOME_INDEX {
                    run.civ_campaign.message = format!("Goodie hut: a free {} vehicle joins you", kind.title().to_lowercase());
                    commands.spawn((StageOwned, crate::civ_voices::GoodieCue { kind, at }));
                }
            }
        }
        // Everyone who was driving to this hut is done.
        for (mut u, _) in &mut units {
            if u.order == UnitOrder::Hut(h) {
                u.order = UnitOrder::Idle;
            }
        }
    }
}

/// One rival vehicle as the hut hunt reads it.
#[derive(Clone, Copy, Debug)]
pub struct HuntUnit {
    pub id: u32,
    pub nation: usize,
    /// No order of its own (a rival's patrol counts) and not stunned or bribed.
    pub free: bool,
    /// The hut it already drives to (a type 8 order on a hut).
    pub hut: Option<usize>,
    pub domain: VehicleDomain,
    pub at: Vec3,
}

/// One pass of 0xBF8850's hut part over every rival (`roll(n)` = `RandomInt(n)`): the `(vehicle id, hut)`
/// capture orders (see [`civ_comm::hut_hunt`]). `regions` / `globe` give the continent and reach tests
/// (no planet: everywhere).
pub fn hunt_pass(
    campaign: &CivCampaign,
    fleet: &[HuntUnit],
    regions: Option<&civ_cursor::Regions>,
    globe: &Globe,
    roll: &mut dyn FnMut(u32) -> u32,
) -> Vec<(u32, usize)> {
    let standing: Vec<usize> = (0..campaign.hut_sites.len()).filter(|h| campaign.hut_standing(*h)).collect();
    let mut ordered: Vec<(u32, usize)> = Vec::new();
    if standing.is_empty() {
        return ordered;
    }
    for nation in 1..CITY_COUNT {
        if !campaign.nation_alive(nation) || !campaign.present(nation) {
            continue;
        }
        if roll(civ_comm::HUT_HUNT_ODDS) != 0 {
            continue;
        }
        // 0xBF01E0(continent, -1): a city of the nation stands on the hut's landmass (no planet: everywhere).
        let own: Vec<Vec3> = campaign.cities.iter().filter(|c| c.nation() == nation).map(|c| c.center()).collect();
        let huts: Vec<(usize, bool)> = standing
            .iter()
            .filter_map(|h| {
                let at = campaign.hut_at(*h)?;
                let near = match regions {
                    Some(r) => {
                        let label = r.land_region(at);
                        label != 0 && own.iter().any(|p| r.land_region(*p) == label)
                    }
                    None => true,
                };
                Some((*h, near))
            })
            .collect();
        let mine: Vec<&HuntUnit> = fleet.iter().filter(|u| u.nation == nation).collect();
        let vehicles: Vec<(u32, bool, Option<usize>)> = mine.iter().map(|u| (u.id, u.free, u.hut)).collect();
        let can_reach = |id: u32, hut: usize| match (mine.iter().find(|u| u.id == id), campaign.hut_at(hut)) {
            (Some(u), Some(to)) => {
                u.domain == VehicleDomain::Air || civ_cursor::medium_reach(regions, globe, u.domain, u.at, to)
            }
            _ => false,
        };
        ordered.extend(civ_comm::hut_hunt(&huts, &vehicles, &can_reach));
    }
    ordered
}

/// The rivals' hut hunting (0xBF8850, from `cCivilization::Update` 0xBFBD30 once a second for every non-player
/// civilization): one second in ten (`RandomInt(10) == 0`) a rival walks the standing huts; a hut on a
/// continent where it has a city and that none of its vehicles is driving to gets the first free vehicle that
/// reaches it (0xC9EAF0) with the capture order (type 8). A rival's patrol (`UnitOrder::Goto`, the OpenSpore
/// stand-in for a standing vehicle) counts as free (ADAPTED).
pub fn rival_hut_hunt(
    time: Res<Time>,
    mut clock: Local<f32>,
    run: Res<RunProgress>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut units: Query<(&mut CivUnit, &Transform)>,
    mut regions: Local<Option<civ_cursor::Regions>>,
) {
    *clock += time.delta_secs();
    if *clock < 1.0 {
        return;
    }
    *clock -= 1.0;
    let campaign = &run.civ_campaign;
    if campaign.hut_sites.is_empty() {
        return;
    }
    let surface = surface.as_deref().filter(|s| s.stage == GameStage::Civilization);
    let globe = Globe::from_surface(surface);
    if globe.on_planet() && regions.as_ref().map(|r| r.key()) != Some(campaign.layout_key) {
        *regions = civ_cursor::Regions::build(&globe, campaign.layout_key);
    }
    let regions_ref = regions.as_ref().filter(|_| globe.on_planet());
    let fleet: Vec<HuntUnit> = units
        .iter()
        .filter(|(u, _)| u.nation != HOME_INDEX)
        .map(|(u, t)| HuntUnit {
            id: u.id,
            nation: u.nation,
            free: matches!(u.order, UnitOrder::Idle | UnitOrder::Goto(_)) && u.stunned <= 0.0 && u.bribed <= 0.0,
            hut: match u.order {
                UnitOrder::Hut(h) => Some(h),
                _ => None,
            },
            domain: u.domain,
            at: t.translation,
        })
        .collect();
    let orders = hunt_pass(campaign, &fleet, regions_ref, &globe, &mut |n| rand::random::<u32>() % n.max(1));
    for (id, hut) in orders {
        if let Some((mut u, _)) = units.iter_mut().find(|(u, _)| u.id == id) {
            u.order = UnitOrder::Hut(hut);
            u.idle = 0.0;
        }
    }
}

/// The hut models: one per standing hut when the stage opens (the canvas hut `he_canvas_2` of the
/// NPC tribes' `npcHutList`, the same PROP the Tribal villages show), a cone without an install.
#[allow(clippy::too_many_arguments)]
pub fn spawn_huts(
    mut commands: Commands,
    run: Res<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut focus: Query<&mut Transform, With<civ::CivCommander>>,
) {
    let campaign = &run.civ_campaign;
    let shape = campaign.shape();
    // Capture aid: `OPENSPORE_CIV_VIEW_HUT=<n>` puts the camera focus on hut n (default 0).
    if let (Ok(want), Ok(mut f)) = (std::env::var("OPENSPORE_CIV_VIEW_HUT"), focus.single_mut()) {
        if let Some(at) = campaign.hut_at(want.parse().unwrap_or(0)) {
            f.translation = at;
        }
    }
    for h in 0..campaign.hut_sites.len() {
        let (true, Some(at)) = (campaign.hut_standing(h), campaign.hut_at(h)) else { continue };
        let up = shape.up(at);
        let rotation = Quat::from_rotation_arc(Vec3::Y, up);
        info!("civ goodie hut {h} at {at:?}");
        let model = crate::world_objects::spawn(
            &mut commands,
            &mut originals,
            crate::world_objects::HUT_GROUP,
            crate::tribal_village::npc_hut_name(1),
            &mut meshes,
            &mut images,
            &mut materials,
            Vec3::ZERO,
            5.0,
        );
        match model {
            Some(e) => {
                commands.entity(e).insert(CivHut(h)).queue(move |mut entity: EntityWorldMut| {
                    let mut lift = 0.0;
                    if let Some(mut t) = entity.get_mut::<Transform>() {
                        // The model's own lift (its base on the ground) rides along the planet's up.
                        lift = t.translation.y;
                        t.translation = at + rotation * Vec3::new(0.0, lift, 0.0);
                        t.rotation = rotation;
                    }
                    // `terrain::ground` keeps it on the rendered surface along its radial line.
                    entity.insert((crate::terrain::Radial, crate::terrain::Grounded::new(lift)));
                });
            }
            None => {
                commands.spawn((
                    StageOwned,
                    CivHut(h),
                    Name::new(format!("civ goodie hut {h}")),
                    Mesh3d(meshes.add(Cone::new(2.0, 2.6))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgb(0.62, 0.5, 0.32),
                        perceptual_roughness: 1.0,
                        ..default()
                    })),
                    Transform::from_translation(at + up * 1.3).with_rotation(rotation),
                    crate::terrain::Radial,
                    crate::terrain::Grounded::new(1.3),
                ));
            }
        }
    }
}

/// A captured hut disappears.
pub fn hut_sync(mut commands: Commands, run: Res<RunProgress>, huts: Query<(Entity, &CivHut)>) {
    for (e, hut) in &huts {
        if !run.civ_campaign.hut_standing(hut.0) {
            commands.entity(e).despawn();
        }
    }
}

pub struct CivOrdersPlugin;

impl Plugin for CivOrdersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CivPie>()
            .init_resource::<PieRequests>()
            .init_resource::<HutRequests>()
            .add_systems(OnEnter(GameStage::Civilization), spawn_huts.after(civ::setup_civ))
            .add_systems(
                Update,
                (hut_command.after(civ_rts::rts_input), hut_arrival.after(civ_rts::rts_move), hut_sync.after(hut_arrival))
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Civilization))
                    .run_if(resource_exists::<RunProgress>),
            )
            .add_systems(
                Update,
                rival_hut_hunt
                    .after(civ_rts::rts_ai)
                    .before(hut_arrival)
                    .run_if(crate::civ_hud::guide::world_runs)
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Civilization))
                    .run_if(resource_exists::<RunProgress>),
            )
            .add_systems(OnExit(GameStage::Civilization), reset)
            .add_systems(
                Update,
                (pie_open, pie_click)
                    .chain()
                    .after(civ_rts::rts_input)
                    .in_set(crate::GameSystems::Simulation)
                    .run_if(in_state(GameStage::Civilization))
                    .run_if(resource_exists::<RunProgress>),
            )
            .add_systems(
                Update,
                pie_ui
                    .in_set(crate::GameSystems::Presentation)
                    .run_if(in_state(GameStage::Civilization))
                    .run_if(resource_exists::<RunProgress>),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(selected: [usize; 3], reach: [usize; 3]) -> PieSelection {
        PieSelection { selected, reach }
    }

    fn facts(route: bool, convertible: bool) -> CityFacts {
        CityFacts { route, convertible, known: true }
    }

    fn texts(entries: &[PieEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.text.as_str()).collect()
    }

    #[test]
    fn the_pie_entries_are_the_registered_messages() {
        // The ids and names of the registration list at 0xCF47C0 (PieMenuIcons windows).
        let all = [
            (PieItem::CaptureCity, 0x037E_95B9, "VehicleAttack", Some(2)),
            (PieItem::ConvertCity, 0x037E_95BA, "CityCultureAttack", Some(3)),
            (PieItem::BuyCity, 0x037E_95BB, "BuyCity", None),
            (PieItem::ProposeTradeRoute, 0x037E_95BC, "CityCreateTradeRoute", Some(9)),
            (PieItem::TradeWithCity, 0x037E_95BD, "CityTradeWithCity", Some(5)),
            (PieItem::GiveGift, 0x051B_4174, "GiveGift", None),
            (PieItem::Contact, 0x046E_E610, "Contact", None),
            (PieItem::BribeNode, 0x051B_40A4, "BribeNode", Some(0xB)),
        ];
        for (item, id, name, code) in all {
            assert_eq!(item.message(), id);
            assert_eq!(item.registered_name(), name);
            assert_eq!(item.command().map(Command::code), code, "{name}");
        }
        assert_eq!(Command::Claim.code(), 8);
        assert_eq!(Command::Attack.code(), 1);
        assert_eq!(Command::Move.code(), 6);
        // Each entry orders one purpose of vehicle.
        assert_eq!(PieItem::CaptureCity.purpose(), Some(purpose(Approach::Military)));
        assert_eq!(PieItem::ConvertCity.purpose(), Some(purpose(Approach::Religious)));
        assert_eq!(PieItem::TradeWithCity.purpose(), Some(purpose(Approach::Economic)));
        assert_eq!(PieItem::BuyCity.purpose(), None);
    }

    #[test]
    fn the_pie_offers_what_the_selection_can_do() {
        // A military vehicle that reaches the city, no trade route: Capture, Give Gift, Contact.
        let entries = city_pie(&sel([2, 0, 0], [1, 0, 0]), &facts(false, true));
        assert_eq!(texts(&entries), ["Capture City", "Give Gift", "Contact"]);
        assert!(entries.iter().all(|e| e.enabled && !e.warn));
        // Out of reach: the retail "(Cannot Reach)" texts, red and unusable.
        let entries = city_pie(&sel([1, 1, 1], [0, 0, 0]), &facts(false, true));
        assert_eq!(
            texts(&entries)[..4],
            [
                "Capture City (Cannot Reach)",
                "Convert City (Cannot Reach)",
                "Propose Trade Route (Cannot Reach)",
                "Buy City (Requires Trade Route)"
            ]
        );
        assert!(entries[..4].iter().all(|e| !e.enabled && e.warn));
        // An unhappy-enough city converts; a happy one makes the preachers target entertainment.
        let convert = |convertible| city_pie(&sel([0, 1, 0], [0, 1, 0]), &facts(false, convertible))[0].clone();
        assert_eq!(convert(true).text, "Convert City");
        assert_eq!(convert(false).text, "Convert City (Auto-Target Entertainment to Lower Happiness)");
        assert!(convert(false).enabled, "the auto-target entry still gives the order");
        assert_eq!(convert(false).label, 0x0677_6CA9);
    }

    #[test]
    fn trade_and_purchase_follow_the_route() {
        // No partner city yet: Propose Trade Route (command 9); with one: Trade With City (command 5).
        let propose = city_pie(&sel([0, 0, 2], [0, 0, 2]), &facts(false, true));
        assert_eq!(propose[0].item, PieItem::ProposeTradeRoute);
        assert_eq!(propose[0].item.command(), Some(Command::CreateTradeRoute));
        assert_eq!(propose[1].text, "Buy City (Requires Trade Route)", "red until a route is open");
        assert!(!propose[1].enabled && propose[1].warn);
        let route = city_pie(&sel([0, 0, 2], [0, 0, 2]), &facts(true, true));
        assert_eq!(route[0].item, PieItem::TradeWithCity);
        assert_eq!(route[0].item.command(), Some(Command::TradeWithCity));
        assert_eq!(route[1].item, PieItem::BuyCity);
        assert!(route[1].enabled && route[1].text == "Buy City");
        // Without economic vehicles and without a route there is no Buy City entry at all.
        let none = city_pie(&sel([1, 0, 0], [1, 0, 0]), &facts(false, true));
        assert!(none.iter().all(|e| e.item != PieItem::BuyCity));
    }

    #[test]
    fn right_clicks_route_like_0xcf56a0() {
        assert_eq!(route_right_click(true, true), ClickRoute::Economic);
        assert_eq!(route_right_click(true, false), ClickRoute::Command);
        assert_eq!(route_right_click(false, true), ClickRoute::Command);
        assert_eq!(route_right_click(false, false), ClickRoute::Command);
    }

    #[test]
    fn geyser_commands_follow_the_command_handler() {
        // 0xCF3EDB: every vehicle claims an unclaimed node (command 8), economic included.
        for kind in [Approach::Military, Approach::Religious, Approach::Economic] {
            assert_eq!(geyser_command(Holder::Nobody, kind, false), Some(Command::Claim));
            assert_eq!(geyser_command(Holder::Player, kind, true), None);
        }
        // A rival's node: military and religious attack, economic vehicles only bribe (popup).
        assert_eq!(geyser_command(Holder::Rival, Approach::Military, false), Some(Command::Attack));
        assert_eq!(geyser_command(Holder::Rival, Approach::Religious, false), Some(Command::Attack));
        assert_eq!(geyser_command(Holder::Rival, Approach::Economic, false), None);
        assert_eq!(geyser_command(Holder::Rival, Approach::Economic, true), Some(Command::BribeNode));
    }

    #[test]
    fn the_bribe_price_is_0xbf0810() {
        // (600 x cities + 1400) x (points / max + 1) x 0.5
        assert_eq!(bribe_price(1, 100.0), 2000.0);
        assert_eq!(bribe_price(4, 100.0), 3800.0);
        assert_eq!(bribe_price(1, 0.0), 1000.0);
        assert!((bribe_price(2, 50.0) - 1950.0).abs() < 1e-3);
        let popup = node_popup(&sel([0, 0, 1], [0, 0, 1]), 2000.0, 1500.0).expect("popup");
        assert_eq!(popup.text, "Bribe the Workers ($2,000)");
        assert!(popup.warn && popup.enabled, "red when the wealth is lower, still usable");
        assert!(node_popup(&sel([0, 0, 1], [0, 0, 1]), 2000.0, 2500.0).is_some_and(|e| !e.warn));
        assert!(node_popup(&sel([1, 1, 1], [1, 1, 0]), 2000.0, 2500.0).is_none(), "no economic vehicle reaches it");
        assert_eq!(with_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn bribing_buys_a_rival_derrick() {
        let mut campaign = CivCampaign::default();
        campaign.ensure_cities();
        let g = 3;
        campaign.geysers[g] = GeyserState::derrick(2, 100.0);
        let seller_before = campaign.nations[2].spice;
        let price = bribe_price(cities_of(&campaign, HOME_INDEX), 100.0);
        let mut money = price - 1.0;
        // Short of money: nothing changes.
        assert_eq!(bribe(&mut campaign, &mut money, HOME_INDEX, g), Bribe::Poor { price });
        assert_eq!(campaign.geysers[g].owner, Some(2));
        money = price + 500.0;
        assert_eq!(bribe(&mut campaign, &mut money, HOME_INDEX, g), Bribe::Done { price, seller: 2 });
        assert_eq!(campaign.geysers[g].owner, Some(HOME_INDEX));
        assert_eq!(money, 500.0);
        assert_eq!(campaign.nations[2].spice, seller_before + price, "the seller is paid");
        // Nothing to buy: your own, an unclaimed one, an unknown geyser.
        assert_eq!(bribe(&mut campaign, &mut money, HOME_INDEX, g), Bribe::Refused);
        let free = (0..civ::GEYSER_COUNT).find(|i| campaign.geysers[*i].owner.is_none()).unwrap();
        assert_eq!(bribe(&mut campaign, &mut money, HOME_INDEX, free), Bribe::Refused);
        assert_eq!(bribe(&mut campaign, &mut money, HOME_INDEX, 999), Bribe::Refused);
    }

    #[test]
    fn preachers_lock_onto_entertainment_above_mood_50() {
        let mut city = CityState::new(3);
        // Strip the ring: no entertainment, nothing to target whatever the mood.
        for plot in city.plots.iter_mut() {
            *plot = None;
        }
        assert!(!convert_locked(&city));
        // One Entertainment building raises the mood above 50: the preachers shoot it first.
        city.plots[0] = Some(Building::Entertainment); // an even plot links to the hall
        assert!(city.happiness() > CONVERT_MOOD_LIMIT, "mood {}", city.happiness());
        assert!(convert_locked(&city));
        let shape = Shape::default();
        let (plot, _) = nearest_entertainment(&city, city.center(), shape).expect("target");
        assert_eq!(city.plots[plot], Some(Building::Entertainment));
        // The building falls: the preachers convert again.
        city.damage_building(plot, 1.0e6, HOME_INDEX);
        assert!(!convert_locked(&city));
        assert!(nearest_entertainment(&city, city.center(), shape).is_none());
    }

    #[test]
    fn the_ring_keeps_the_pie_on_screen() {
        let screen = Vec2::new(1280.0, 800.0);
        for at in [Vec2::new(5.0, 5.0), Vec2::new(1275.0, 795.0), Vec2::new(640.0, 400.0)] {
            let rects = ring(6, at, screen);
            assert_eq!(rects.len(), 6);
            for r in &rects {
                assert!(r.min.x >= 0.0 && r.min.y >= 0.0 && r.max.x <= screen.x && r.max.y <= screen.y, "{r:?}");
            }
        }
        let one = ring(1, Vec2::new(640.0, 400.0), screen);
        assert!(one[0].contains(Vec2::new(640.0, 400.0)));
        let mut pie = CivPie::default();
        assert!(!pie.covers(Vec2::ZERO));
        pie.open = Some(OpenPie {
            target: PieTarget::City(1),
            at: Vec2::ZERO,
            entries: Vec::new(),
            rects: one.clone(),
            reach: Default::default(),
        });
        assert!(pie.covers(Vec2::new(640.0, 400.0)) && !pie.covers(Vec2::ZERO));
    }

    // ---- the Civ scene (headless): the pie menu, the bribe and the locked preachers ----

    fn civ_app() -> App {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<RunProgress>().enter_stage(GameStage::Civilization);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        for _ in 0..6 {
            app.update();
        }
        app
    }

    fn spawn_unit(app: &mut App, id: u32, kind: Approach, at: Vec3, order: UnitOrder, selected: bool) -> Entity {
        let state = civ::UnitState { id, nation: HOME_INDEX, kind, ..Default::default() };
        let mut unit = CivUnit::from_state(&state);
        unit.order = order;
        let mut entity = app.world_mut().spawn((unit, Transform::from_translation(at)));
        if selected {
            entity.insert(Selected);
        }
        entity.id()
    }

    fn order_of(app: &App, e: Entity) -> UnitOrder {
        app.world().get::<CivUnit>(e).expect("unit").order
    }

    fn click_at(app: &mut App, p: Vec2) {
        let mut window = Window::default();
        window.set_cursor_position(Some(p));
        app.world_mut().spawn(window);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        let mut input = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        input.release(MouseButton::Left);
        input.clear();
        let world = app.world_mut();
        let windows: Vec<Entity> = world.query_filtered::<Entity, With<Window>>().iter(world).collect();
        for w in windows {
            world.despawn(w);
        }
    }

    #[test]
    fn the_pie_menu_orders_the_vehicles_of_its_purpose() {
        let mut app = civ_app();
        let city = 2;
        let at = app.world().resource::<RunProgress>().civ_campaign.cities[city].center();
        let eco = spawn_unit(&mut app, 901, Approach::Economic, at + Vec3::X * 60.0, UnitOrder::Idle, true);
        let mil = spawn_unit(&mut app, 902, Approach::Military, at + Vec3::X * 70.0, UnitOrder::Idle, true);
        let rel = spawn_unit(&mut app, 903, Approach::Religious, at + Vec3::X * 80.0, UnitOrder::Idle, true);
        app.world_mut().resource_mut::<PieRequests>().0 =
            Some(PieRequest { target: PieTarget::City(city), at: Vec2::new(640.0, 400.0) });
        app.update();
        let open = app.world().resource::<CivPie>().open.clone().expect("the pie opens on a request");
        let items: Vec<PieItem> = open.entries.iter().map(|e| e.item).collect();
        assert_eq!(
            items,
            [
                PieItem::CaptureCity,
                PieItem::ConvertCity,
                PieItem::ProposeTradeRoute,
                PieItem::BuyCity,
                PieItem::GiveGift,
                PieItem::Contact
            ]
        );
        assert!(open.entries.iter().all(|e| e.item == PieItem::BuyCity || e.enabled), "a flat planet reaches everything");
        // Clicking Propose Trade Route sends the economic vehicle only.
        let trade = items.iter().position(|i| *i == PieItem::ProposeTradeRoute).unwrap();
        click_at(&mut app, open.rects[trade].center());
        assert_eq!(order_of(&app, eco), UnitOrder::City(city));
        assert_eq!(order_of(&app, mil), UnitOrder::Idle);
        assert_eq!(order_of(&app, rel), UnitOrder::Idle);
        assert!(app.world().resource::<CivPie>().open.is_none(), "the pie closes");
        // Capture City: the military one; a click beside the pie only closes it.
        app.world_mut().resource_mut::<PieRequests>().0 =
            Some(PieRequest { target: PieTarget::City(city), at: Vec2::new(640.0, 400.0) });
        app.update();
        let open = app.world().resource::<CivPie>().open.clone().expect("opens again");
        click_at(&mut app, open.rects[0].center());
        assert_eq!(order_of(&app, mil), UnitOrder::City(city));
        assert_eq!(order_of(&app, rel), UnitOrder::Idle);
        app.world_mut().resource_mut::<PieRequests>().0 =
            Some(PieRequest { target: PieTarget::City(city), at: Vec2::new(640.0, 400.0) });
        app.update();
        click_at(&mut app, Vec2::new(2.0, 2.0));
        assert!(app.world().resource::<CivPie>().open.is_none());
        assert_eq!(order_of(&app, rel), UnitOrder::Idle);
    }

    #[test]
    fn the_buy_entry_opens_the_communication_screen() {
        let mut app = civ_app();
        let city = 3;
        app.world_mut().resource_mut::<RunProgress>().civ_campaign.cities[city].trade = true;
        let at = app.world().resource::<RunProgress>().civ_campaign.cities[city].center();
        spawn_unit(&mut app, 911, Approach::Economic, at + Vec3::X * 60.0, UnitOrder::Idle, true);
        app.world_mut().resource_mut::<PieRequests>().0 =
            Some(PieRequest { target: PieTarget::City(city), at: Vec2::new(500.0, 300.0) });
        app.update();
        let open = app.world().resource::<CivPie>().open.clone().expect("opens");
        // With a route the economic entry reads Trade With City and Buy City is usable.
        assert_eq!(open.entries[0].item, PieItem::TradeWithCity);
        let buy = open.entries.iter().position(|e| e.item == PieItem::BuyCity).expect("Buy City");
        assert!(open.entries[buy].enabled);
        click_at(&mut app, open.rects[buy].center());
        let local = app.world().resource::<crate::civ_hud::CivHudLocal>();
        assert_eq!(local.comm.map(|c| c.nation), Some(city));
    }

    #[test]
    fn an_economic_vehicle_bribes_a_rival_derrick_when_it_arrives() {
        let mut app = civ_app();
        let g = 3;
        let (p, price) = {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.spice = 50_000.0;
            run.civ_campaign.geysers[g] = GeyserState::derrick(2, 100.0);
            (run.civ_campaign.geyser_at(g), bribe_price(cities_of(&run.civ_campaign, HOME_INDEX), 100.0))
        };
        // A military vehicle there only shoots; the economic one buys.
        let eco = spawn_unit(&mut app, 921, Approach::Economic, p, UnitOrder::Geyser(g), false);
        for _ in 0..4 {
            app.update();
        }
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.civ_campaign.geysers[g].owner, Some(HOME_INDEX), "the derrick is bought");
        assert!(run.civ_campaign.message.contains("bribed"), "{}", run.civ_campaign.message);
        assert!(run.spice < 50_000.0 - price + 1000.0, "the price was paid ({})", run.spice);
        assert_eq!(order_of(&app, eco), UnitOrder::Idle);
    }

    #[test]
    fn locked_preachers_shoot_the_entertainment_before_converting() {
        let mut app = civ_app();
        let city = 3;
        let at = {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            let c = &mut run.civ_campaign.cities[city];
            for plot in c.plots.iter_mut() {
                *plot = None;
            }
            c.plots[0] = Some(Building::Entertainment);
            c.turret_hp.iter_mut().for_each(|hp| *hp = 0.0);
            // Nearly down already: one religious hit finishes it (a building takes 800).
            c.building_damage[0] = civ::BUILDING_HP - 10.0;
            assert!(convert_locked(c));
            c.center()
        };
        let preacher = spawn_unit(&mut app, 931, Approach::Religious, at + Vec3::X * 30.0, UnitOrder::City(city), false);
        let mut fell = false;
        for _ in 0..1800 {
            app.update();
            let run = app.world().resource::<RunProgress>();
            let c = &run.civ_campaign.cities[city];
            if !c.standing(0) {
                fell = true;
                break;
            }
            assert_eq!(c.conversion, 0.0, "no conversion while the mood is above 50");
        }
        assert!(fell, "the preachers brought the entertainment down");
        assert!(app.world().get::<CivUnit>(preacher).is_some());
        // With the mood at 50 the same preachers convert (one citizen a second
        // with the odds of 0xCEEF10, about one in five for a single vehicle).
        for _ in 0..3600 {
            app.update();
        }
        let run = app.world().resource::<RunProgress>();
        assert!(run.civ_campaign.cities[city].conversion > 0.0, "converting once the entertainment is gone");
    }

    // ---- goodie huts ----

    #[test]
    fn the_goodie_roll_follows_the_retail_draw_order() {
        // RandomInt(2) = 0: money 400 + R(400) + R(400) + R(200) (0xDCAD68 .. 0xDCADB4).
        let mut draws = vec![0u32, 399, 399, 199].into_iter();
        let money = goodie_roll(true, Approach::Economic, |n| {
            let v = draws.next().expect("a draw");
            assert!(v < n);
            v
        });
        assert_eq!(money, Goodie::Money(400.0 + 399.0 + 399.0 + 199.0));
        assert_eq!(goodie_roll(false, Approach::Military, |_| 0), Goodie::Money(400.0));
        // RandomInt(2) = 1: the player's vehicle class is rerolled from RandomInt(3) (0xDCB085) ...
        for (roll, kind) in [(0, Approach::Military), (1, Approach::Religious), (2, Approach::Economic)] {
            let mut d = vec![1u32, roll].into_iter();
            assert_eq!(goodie_roll(true, Approach::Military, |_| d.next().expect("a draw")), Goodie::Vehicle(kind));
        }
        // ... a rival keeps the class of the vehicle that took the hut.
        let mut d = vec![1u32].into_iter();
        assert_eq!(goodie_roll(false, Approach::Economic, |_| d.next().expect("a draw")), Goodie::Vehicle(Approach::Economic));
    }

    fn hut_app() -> (App, Vec3) {
        let mut app = civ_app();
        let at = {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            let at = run.civ_campaign.cities[HOME_INDEX].center() + Vec3::X * 60.0;
            run.civ_campaign.hut_sites = vec![at.to_array()];
            run.civ_campaign.huts_taken.clear();
            at
        };
        (app, at)
    }

    #[test]
    fn a_right_click_on_a_hut_gives_the_capture_order_and_a_close_vehicle_takes_it() {
        let (mut app, at) = hut_app();
        let far = spawn_unit(&mut app, 9101, Approach::Military, at + Vec3::X * 200.0, UnitOrder::Idle, true);
        app.world_mut().resource_mut::<HutRequests>().0 = Some(HutRequest { hut: 0, fleet: vec![far] });
        app.update();
        assert_eq!(order_of(&app, far), UnitOrder::Hut(0), "the command (0xCF4001) orders the vehicle");
        // A vehicle already at the hut captures it (0xDCAB60) and the goodie lands.
        let before = {
            let run = app.world().resource::<RunProgress>();
            (run.spice, run.civ_campaign.units.len())
        };
        let near = spawn_unit(&mut app, 9102, Approach::Economic, at + Vec3::X * 2.0, UnitOrder::Hut(0), false);
        app.update();
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.civ_campaign.huts_taken, vec![0], "the hut is gone");
        assert!(!run.civ_campaign.hut_standing(0));
        assert!(
            run.spice > before.0 || run.civ_campaign.units.len() == before.1 + 1,
            "money or a free vehicle: {} -> {}, {} units",
            before.0,
            run.spice,
            run.civ_campaign.units.len()
        );
        assert_eq!(order_of(&app, near), UnitOrder::Idle);
        app.update();
        assert_eq!(order_of(&app, far), UnitOrder::Idle, "the order is done once the hut is gone");
    }

    #[test]
    fn a_rival_hunts_a_standing_hut_with_a_free_vehicle_one_second_in_ten() {
        let (app, _) = hut_app();
        let mut campaign = app.world().resource::<RunProgress>().civ_campaign.clone();
        let nation = (1..CITY_COUNT).find(|n| campaign.nation_alive(*n) && campaign.present(*n)).expect("a rival");
        let globe = Globe::flat();
        let at = campaign.cities[nation].center();
        let unit = |id, free, hut| HuntUnit { id, nation, free, hut, domain: VehicleDomain::Land, at };
        // The ten-sided die must come up 0 for the nation to hunt (0xBF8864 `RandomInt(10)`).
        assert!(hunt_pass(&campaign, &[unit(1, true, None)], None, &globe, &mut |_| 3).is_empty());
        assert_eq!(hunt_pass(&campaign, &[unit(1, true, None)], None, &globe, &mut |_| 0), vec![(1, 0)]);
        // A vehicle already driving to the hut (0xBF893C), or none without an order: nothing new.
        assert!(hunt_pass(&campaign, &[unit(1, true, Some(0)), unit(2, true, None)], None, &globe, &mut |_| 0).is_empty());
        assert!(hunt_pass(&campaign, &[unit(1, false, None)], None, &globe, &mut |_| 0).is_empty());
        // A vehicle of another nation is not this nation's: the huts wait for their own rival.
        let other = HuntUnit { nation: nation + 1, ..unit(3, true, None) };
        let orders = hunt_pass(&campaign, &[other], None, &globe, &mut |_| 0);
        assert_eq!(orders, vec![(3, 0)].into_iter().filter(|_| campaign.nation_alive(nation + 1)).collect::<Vec<_>>());
        // A taken hut is not hunted.
        campaign.huts_taken.push(0);
        assert!(hunt_pass(&campaign, &[unit(1, true, None)], None, &globe, &mut |_| 0).is_empty());
    }

    #[test]
    fn a_hut_taken_twice_pays_once() {
        let (mut app, at) = hut_app();
        spawn_unit(&mut app, 9201, Approach::Military, at + Vec3::X * 1.0, UnitOrder::Hut(0), false);
        spawn_unit(&mut app, 9202, Approach::Military, at - Vec3::X * 1.0, UnitOrder::Hut(0), false);
        app.update();
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.civ_campaign.huts_taken, vec![0]);
    }

    #[test]
    fn a_military_vehicle_ordered_against_an_epic_closes_in_and_fires_until_it_falls() {
        let mut app = civ_app();
        let (id, at) = {
            let world = app.world_mut();
            let mut q = world.query::<(&crate::civ_wildlife::CivEpic, &Transform)>();
            let (e, t) = q.iter(world).next().expect("an epic predator");
            (e.id, t.translation)
        };
        // A hardy military vehicle a little way off (the epic's maul is 20 every 3 s).
        let tank = spawn_unit(&mut app, 9301, Approach::Military, at + Vec3::X * 12.0, UnitOrder::Epic(id), false);
        app.world_mut().get_mut::<CivUnit>(tank).expect("unit").hp = 1.0e6;
        app.world_mut().get_mut::<CivUnit>(tank).expect("unit").max_hp = 1.0e6;
        let hp_of = |app: &mut App| {
            let world = app.world_mut();
            let mut q = world.query::<&crate::civ_wildlife::CivEpic>();
            q.iter(world).find(|e| e.id == id).map(|e| e.hp)
        };
        let before = hp_of(&mut app).expect("the epic");
        for _ in 0..1200 {
            app.update();
            if hp_of(&mut app).is_none_or(|hp| hp < before) {
                break;
            }
        }
        let after = hp_of(&mut app);
        assert!(after.is_none_or(|hp| hp < before), "the ordered vehicle shot the epic: {before} -> {after:?}");
        assert!(matches!(order_of(&app, tank), UnitOrder::Epic(_) | UnitOrder::Idle));
    }
}
