//! Sporepedia (`AssetBrowser`, group 41464200): the retail frame with its left
//! column of creation types (`AssetBrowserFeedListCategory` rows with counts)
//! and source filters, the asset grid (`AssetBrowserAssetGrid`: "Sort By"
//! buttons and the "Create Your Own" card) and a page of creation cards,
//! paged with the frame's Previous / Next; a click on a card opens the
//! `AssetViewLarge` dialog with the `AssetBrowserVerbs` buttons (Edit, Delete,
//! Outfitter).
//!
//! Creations: the creature of every saved game and its buildings, vehicles and
//! UFO designs (`rigid_editor` records), the creations made in the menu
//! creators (`store`), and the installed Maxis creations: the named
//! `pollen_metadata` creations with their own PNGs (`maxis`: 759 creatures,
//! 310 buildings, 285 vehicles, 91 UFOs, 153 plants, 43 cells) and the few
//! BLD/VCL/UFO creation files (`creation_records`, PaintSwatch records
//! skipped). Types and filters are named by the feed strings (locale table
//! 21671745): Cells, Creatures, Buildings, Vehicles, (Spaceships), Flora (the
//! retail row order, the children of the feed's root filter; spaceships are the
//! `ufo` child of Vehicles: that row opens under Vehicles and Vehicles lists the
//! spaceships too); "Everything" / "My Creations" / "Maxis", each row an
//! `AssetBrowserFeedListItem` (the feed's icon, its name and "(count)").
//!
//! The detail dialog is retail's 800 x 600 `AssetViewLarge` layout
//! (`cSPUILargeAssetView`, 0x66EFB0 / 0x66DAC0): the name header, the author
//! button, the render window (`065F79E0`, the creation drawn by `turntable`),
//! the Description and Tags text edits, the rotate / zoom buttons, the verbs
//! in the column at the render window's right edge (`0630C829`) and the close
//! button. The online parts (buddy / rating / comments, the Switch button, the
//! expansion-pack frame) stay hidden like retail's offline dialog.
//!
//! The strip above the grid (`AssetBrowserFeedFilter`) shows the feed's icon
//! and the icons of the filters the shown type stands for, from the feed and
//! filter property files (`feeds`); a click on a filter's icon shows that
//! type (the buttons' control ids are the filter ids).
//!
//! The creature shopper: an outfitter launched from the creator wheel opens
//! Sporepedia on the creatures of the Everything feed ("Create or Choose a
//! Tribe / Civ / Space Creature", 0xDE0B70; `shoppingTemplate` selects
//! `feed_all`); a card opens the outfitter on that creature, the
//! Create Your Own card starts the Creature Creator and then the outfitter
//! on the new creature.
//!
//! The cards are the `AssetView` layout's own: its frame and bottom-bar images,
//! the name at the top, the author under it, the picture window and the time
//! stamp at the bottom, drawn as UI nodes over the grid, with the
//! `AssetBrowserCardGlow` image under the pointer. ADAPTED: the card step is
//! OpenSpore's; the sorts OpenSpore has data for are live (Time Created,
//! Title, Creator, Type), the others disabled; the author of a saved creation
//! is its planet, of a menu creation "My Creations"; the Maxis creatures,
//! buildings, vehicles and UFOs open in their editors from the creation file
//! their card PNG hides (`maxis_creation`; the result is kept as a new
//! creation of the player's), the Maxis plants and cells have no creator; the
//! Maxis creations are part of the install, so they cannot be deleted; the
//! indent of the Spaceships child row is OpenSpore's.

use std::{path::PathBuf, time::SystemTime};

use bevy::prelude::*;

use super::{creators, maxis, slots, store, thumbs, FrontEnd, Overlay};
use crate::{
    creature_editor_data::SetupKind,
    game::RunProgress,
    spui::{
        model::{Drawable, WinId},
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiPointer, SpuiState},
    },
};

mod feeds;

pub const LAYOUT: &str = "AssetBrowser";
pub const STRIP_LAYOUT: &str = "AssetBrowserFeedFilter";
pub const CATEGORY_LAYOUT: &str = "AssetBrowserFeedListCategory";
/// The feeds' rows: the feed's icon, its name and its "(count)" (`AssetBrowserFeedListItem`).
pub const FEED_ITEM_LAYOUT: &str = "AssetBrowserFeedListItem";
pub const GRID_LAYOUT: &str = "AssetBrowserAssetGrid";
pub const GLOW_LAYOUT: &str = "AssetBrowserCardGlow";
pub const VERBS_LAYOUT: &str = "AssetBrowserVerbs";
pub const LARGE_LAYOUT: &str = "AssetViewLarge";
pub const CONFIRM_LAYOUT: &str = "MessageBox-2";

/// Windows of the `AssetViewLarge` dialog (`cSPUILargeAssetView`).
pub mod large {
    /// The left panel (`065F79E0`, 0x66DC3E): the render window's frame.
    pub const PANEL: u32 = 0x065F_79E0;
    /// The render window the viewer draws into (0x66DFD2; 40,24 .. 479,583).
    pub const RENDER: u32 = 0xF3C6_D819;
    /// The column the verb icons are added to (0x66E82D).
    pub const VERB_COLUMN: u32 = 0x0630_C829;
    pub const CLOSE: u32 = 0x7417_4C59;
    /// The asset name text (0x66E018).
    pub const NAME: u32 = 0x53D6_FE29;
    /// The author button of the offline arrangement (0x66E0F2).
    pub const AUTHOR: u32 = 0x53D6_FE2A;
    pub const DESCRIPTION: u32 = 0xB456_A483;
    pub const TAGS: u32 = 0xB456_A47A;
    /// Rotate (the left / right button), zoom in / out: held buttons
    /// (0x66BF40: 04CAB571 / 570 zoom out / in, 56F / 56E turn -/+).
    pub const ROTATE_LEFT: u32 = 0x04CA_B56F;
    pub const ROTATE_RIGHT: u32 = 0x04CA_B56E;
    pub const ZOOM_IN: u32 = 0x04CA_B570;
    pub const ZOOM_OUT: u32 = 0x04CA_B571;
    /// Online groups: the buddy / author arrangement and the rating.
    pub const ONLINE_AUTHOR: u32 = 0x07E5_FD08;
    pub const ONLINE_RATING: u32 = 0x07E4_D3F0;
    /// The comments frame's offline message (`Frame` window, hidden in the file).
    pub const COMMENTS_MESSAGE: u32 = 0x0561_DD60;
    /// The baking indicator ("Please Wait...") and its spinner.
    pub const BUSY: u32 = 0x34A0_979A;
    pub const BUSY_TEXT: u32 = 0x54A6_3122;
    pub const BUSY_ICON: u32 = 0x3431_BD1E;
}

pub mod id {
    pub const CLOSE: u32 = 0xFFFF_FFF2;
    pub const BACKDROP: u32 = 0x0629_83B8;
    pub const GUIDE: u32 = 0x0447_060A;
    pub const TITLE_BAR: u32 = 0xB44E_C3DB;
    pub const WELCOME: u32 = 0x0667_87D8;
    pub const COLUMN: u32 = 0x53C5_B390;
    /// The bar above the grid (holds the filter strip).
    pub const TOP_BAR: u32 = 0x0673_7680;
    // AssetBrowserFeedFilter: the four nested slots of the breadcrumb
    // (0x65F7D0), a slot's background piece and its selected highlight (the
    // fade window 0x65F920 shows for the selected slot), and the window of the
    // feed's icon.
    pub const STRIP_SLOTS: [u32; 4] = [0xDA7F_4890, 0xDA7F_4891, 0xDA7F_4892, 0xDA7F_4893];
    pub const STRIP_PIECE: u32 = 0x0727_73B8;
    pub const STRIP_SELECTED: u32 = 0x5D0D_D57A;
    pub const STRIP_FEED_ICON: u32 = 0x12F6_6831;
    pub const GRID: u32 = 0x53C5_B391;
    pub const PAGER: u32 = 0x9417_4874;
    pub const WEB_FRAME: u32 = 0xD4A1_F5F0;
    pub const COMMENTS: u32 = 0x0561_E0A8;
    // Category row
    pub const CAT_TOGGLE: u32 = 0xB3C6_EFC2;
    pub const LABEL: u32 = 0xB3C6_EFCA;
    /// `AssetBrowserFeedListItem`: the row (156 x 30), the feed icon window, the name,
    /// the right aligned "(count)" and the 37 x 37 window of the row's highlight.
    pub const FEED_ROW: u32 = 0xF3C6_DC19;
    pub const FEED_ICON: u32 = 0x548F_3BF6;
    pub const FEED_NAME: u32 = 0xB3C6_EFCA;
    pub const FEED_COUNT: u32 = 0xB48E_225B;
    pub const CAT_CLICK: u32 = 0xB48E_2560;
    // AssetBrowserAssetGrid
    pub const CREATE_OWN: u32 = 0x1458_0F81;
    /// The empty card slot (frame image + text) the grid copies per card.
    pub const CARD_TEMPLATE: u32 = 0x0656_66C8;
    /// "No items" message group.
    pub const EMPTY: u32 = 0x0654_FCE8;
    pub const FEED_HEADER: u32 = 0x1424_8528;
    /// The status line under the grid (`cSPAssetBrowser` +0x3C, found at 0x650F5F;
    /// the other two status windows `53DA7EEC` / `07AA9F70` show the invalid /
    /// pending asset notes and stay empty here).
    pub const FOOTER: u32 = 0x13D7_04E1;
    /// The card glow of `AssetBrowserCardGlow`.
    pub const GLOW: u32 = 0x05B5_87D0;
    /// Expand / collapse of the sort list (the list stays open).
    pub const SORT_TOGGLES: [u32; 2] = [0x07CE_2701, 0x07CE_2700];
    // AssetBrowserVerbs (tooltips Delete, Ban Creation, Edit, Share, Accept,
    // Outfitter; Create Your Own; Play the Game, Paint like..., View Details).
    pub const VERBS: u32 = 0x1493_71C4;
    pub const VERB_DELETE: u32 = 0x54AC_B9F6;
    pub const VERB_BAN: u32 = 0x54AC_B9E0;
    pub const VERB_EDIT: u32 = 0x54AC_B9F1;
    pub const VERB_SHARE: u32 = 0x54AC_B9F5;
    pub const VERB_ACCEPT: u32 = 0x00AD_0E52;
    pub const VERB_OUTFITTER: u32 = 0x54AC_B9F2;
    pub const VERB_CREATE_OWN: u32 = 0x067A_32D0;
    pub const VERB_MORE: [u32; 3] = [0x54AC_B9F4, 0x54AC_B9F3, 0x05CB_EB4B];
    /// The verb icons the code puts on the buttons (the file keeps them in a
    /// row under the buttons): Delete, Ban, Edit, Share, Accept, Outfitter,
    /// and a spare Share icon.
    pub const VERB_ICONS: [(u32, u32); 6] = [
        (VERB_DELETE, 0x0667_D118),
        (VERB_BAN, 0x0668_ADB0),
        (VERB_EDIT, 0x0667_D120),
        (VERB_SHARE, 0x0668_AD90),
        (VERB_ACCEPT, 0x0667_D128),
        (VERB_OUTFITTER, 0x07A4_3648),
    ];
    pub const VERB_SPARE_ICON: u32 = 0x0668_ADA0;
}

/// Creation types of the column (Sporepedia's type filter names, English).
pub const CATEGORIES: [&str; 6] = ["Creatures", "Buildings", "Vehicles", "Spaceships", "Flora", "Cells"];
/// Their feed strings (table 21671745): Creatures, Buildings, Vehicles,
/// Spaceships, Flora, Cells.
pub const CATEGORY_IDS: [u32; 6] = [0x0002_0008, 0x0002_0002, 0x0002_0019, 0x0002_0018, 0x0002_000E, 0x0002_0003];
/// Indices into [`CATEGORIES`] of the types the column orders.
const CREATURES: usize = 0;
const VEHICLES: usize = 2;
const SPACESHIPS: usize = 3;
const FLORA: usize = 4;
const CELLS: usize = 5;
/// The rows of the column in retail's order (VERIFIED, `fe-final` finding 8): the children of the
/// feed's root filter (`ShippingContent` / `UserCreated`: cell, creatures, buildings, vehicles,
/// flora). Spaceships are the `ufo` child of Vehicles, so their row stands under Vehicles (and
/// only while the Vehicles row is open: Vehicles or Spaceships selected).
const ROW_ORDER: [usize; 6] = [CELLS, CREATURES, 1, VEHICLES, SPACESHIPS, FLORA];

/// The type rows the column shows for the selected type, top to bottom.
fn visible_rows(selected: usize) -> Vec<usize> {
    ROW_ORDER.iter().copied().filter(|c| *c != SPACESHIPS || selected == VEHICLES || selected == SPACESHIPS).collect()
}

/// Whether an entry of type `entry` is listed under the type row `category`: a filter lists the
/// assets of its child filters too, so Vehicles lists the spaceships (`ufo`).
fn in_category(entry: usize, category: usize) -> bool {
    entry == category || (category == VEHICLES && entry == SPACESHIPS)
}

/// Sporepedia's feed strings (locale table 21671745): "Everything", "My
/// Creations", "Maxis" (the filters) and "Choose a Creature" (the shopper).
pub const FEED_TABLE: u32 = 0x2167_1745;
/// The status line's text ("Displaying ~num_items~ items", `21671745:00030003`; the
/// browser's update 0x6528B0 picks it when the shown count equals the total, else
/// "Displaying ~num_filtered_items~ of ~num_items~ items", 00030005).
const STATUS_LINE: u32 = 0x0003_0003;

/// The status line under the grid for `shown` listed creations.
pub fn status_line(template: &str, shown: usize) -> String {
    template.replace("~num_items~", &shown.to_string())
}
pub const FILTER_STRINGS: [(u32, &str); 3] = [(0x0001_0001, "Everything"), (0x0001_0002, "My Creations"), (0x0001_0003, "Maxis")];
pub const CHOOSE_CREATURE: (u32, &str) = (0x0004_0011, "Choose a Creature");
/// The outfitters' shopper titles: "Create or Choose a Tribe / Civ / Space
/// Creature" (00040018 / 16 / 17).
pub fn shopper_title(kind: SetupKind) -> (u32, &'static str) {
    match kind {
        SetupKind::Civ => (0x0004_0016, "Create or Choose a Civ Creature"),
        SetupKind::Space => (0x0004_0017, "Create or Choose a Space Creature"),
        SetupKind::Tribal => (0x0004_0018, "Create or Choose a Tribe Creature"),
        _ => CHOOSE_CREATURE,
    }
}

/// Sporepedia is a shopper: the outfitters' creature shopper (`FrontEnd::shopper`) or
/// the shopper of a new-game card (`FrontEnd::flow_pick`).
fn is_shopper(fe: &FrontEnd) -> bool {
    fe.shopper.is_some() || fe.flow_pick.is_some()
}

/// The one type a shopper lists (an index into [`CATEGORIES`]): creatures for the
/// outfitters' shopper, the card's type for a new-game card.
fn shopper_category(fe: &FrontEnd) -> usize {
    fe.flow_pick.map_or(0, |shop| shop.category())
}

/// Which creations a page lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    Everything,
    Mine,
    Maxis,
}

pub const FILTERS: [Filter; 3] = [Filter::Everything, Filter::Mine, Filter::Maxis];

/// Sorts of the grid's "Sort By" buttons that OpenSpore has data for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    TimeCreated,
    Title,
    Creator,
    Type,
}

/// The grid's sort buttons (tooltips Time Created, Time Downloaded, Creepy &
/// Cute, Title, Creator, Type, Shared, Relationship, Health, Military /
/// Religious / Economic Power, Speed, Omnivore, Carnivore, Herbivore, Attack,
/// Social) with the live ones.
pub const SORT_BUTTONS: [(u32, Option<Sort>); 18] = [
    (0xC3FD_21B1, Some(Sort::TimeCreated)),
    (0x1DC1_C781, None),
    (0xE6AA_9254, None),
    (0x4543_C5EC, Some(Sort::Title)),
    (0x339D_2BF7, Some(Sort::Creator)),
    (0xA924_06A6, Some(Sort::Type)),
    (0xE6B0_FDD0, None),
    (0x6DCF_5398, None),
    (0x33D2_FCF4, None),
    (0x67DE_1872, None),
    (0x0103_B547, None),
    (0xB1A9_59B4, None),
    (0x9780_173B, None),
    (0xAB6C_82DC, None),
    (0xA7AF_2DF4, None),
    (0x2DC4_9BDB, None),
    (0xD95F_A1B1, None),
    (0xE63E_3E5C, None),
];

/// Card size and grid steps (the card template is 123 x 174).
const CARD: Vec2 = Vec2::new(123.0, 174.0);
const STEP: Vec2 = Vec2::new(135.0, 192.0);
/// The pop-in of a card (VERIFIED, the animation built by 0x658E40 for a card
/// window, called from the card layout 0x64F9E0): the card starts transparent
/// and at the scale 1 / 65536 (0x13FFD1C) about its centre and returns to its
/// colour and scale 1 over 0.4 s (0x13EC414, overridden by the property
/// `0x24CCFB15` of the Sporepedia config `851D4139!B55619C2`, installed 0.4);
/// its callback 0x657330 plays `ui_sporepedia_card_appear` for that card. The
/// grid creates its cards one per `AssetLoadInterval` (the property
/// `B6C69EB8`, 30 ms, compared with a clock at 0x6560B0).
/// ADAPTED: the curves of the animation steps (curve codes 2 / 4 / 5 of
/// 0x805170) are not decoded: the scale and the colour run linearly, and OpenSpore
/// leaves the card's colour alone (UI nodes have no group alpha), so only the
/// scale and the staggered start are shown.
pub const POP_IN_SECONDS: f32 = 0.4;
pub const LOAD_INTERVAL: f32 = 0.030;
const POP_IN_FROM: f32 = 1.0 / 65536.0;

/// Seconds after a listing appears at which its `index`th card starts to pop in.
pub fn pop_in_start(index: usize) -> f32 {
    index as f32 * LOAD_INTERVAL
}

/// Scale of a card `age` seconds after its pop-in started (`None` before it
/// started: the card is not there yet).
pub fn pop_in_scale(age: f32) -> Option<f32> {
    (age >= 0.0).then(|| POP_IN_FROM + (1.0 - POP_IN_FROM) * (age / POP_IN_SECONDS).clamp(0.0, 1.0))
}

/// What a page shows: changes when the grid builds new cards.
type ListingKey = (usize, usize, u8, u8, bool);

/// The start of the card pop-ins of the shown listing.
#[derive(Default)]
pub struct PopIn {
    key: Option<ListingKey>,
    started: f32,
}
/// The `AssetView` layout's atlas (a PNG) and the regions of the card parts:
/// the frame (window E133, 141 x 182 on screen) and the bottom bar (E137
/// 065BD7C8, 141 x 21). The frame sits 9 px left of and 4 px above the slot
/// (the slot is the 123 x 174 card of `AssetBrowserAssetGrid`).
const CARD_ATLAS: crate::dbpf::ResourceKey = crate::dbpf::ResourceKey { group_id: 0x31A4_4893, instance_id: 0x5334_E5B1, type_id: 0x2F7D_0004 };
const CARD_FRAME_UV: [f32; 4] = [0.0, 0.0, 0.554_688, 0.5625];
const CARD_BAR_UV: [f32; 4] = [0.0, 0.570_313, 0.558_594, 0.664_063];
const CARD_FRAME_AT: (f32, f32) = (-9.0, -4.0);
/// Card nodes kept (the most a page can show).
const MAX_CARDS: usize = 40;
/// Step of the verb buttons down the dialog's verb column.
const VERB_STEP: f32 = 44.0;

/// Where a creation comes from (what Edit / Delete / Outfitter can do).
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// A saved game's creature (`setup` None) or rigid design.
    Game { path: PathBuf, setup: Option<String> },
    /// A creation of the Sporepedia store.
    Stored(u64),
    /// Installed with the game.
    Maxis,
}

/// One Sporepedia creation.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Index into [`CATEGORIES`].
    pub category: usize,
    pub name: String,
    pub author: String,
    /// What it is ("Creature", "House", "Military Land Vehicle", ...).
    pub kind: String,
    pub description: String,
    /// The creation's tags (the installed metadata's; the dialog's Tags box).
    pub tags: Vec<String>,
    pub created: Option<SystemTime>,
    /// Installed order (the time sort of the Maxis creations).
    pub order: usize,
    pub parts: usize,
    pub source: thumbs::Source,
    /// Thumbnail cache name (content hash).
    pub thumb: String,
    pub origin: Origin,
}

impl Entry {
    pub fn is_maxis(&self) -> bool {
        self.origin == Origin::Maxis
    }
    /// Edit / Outfitter can open it: the player's creations, and the Maxis
    /// creatures, buildings, vehicles and UFOs whose creation file is
    /// installed (their PNG's hidden data, `maxis_creation`).
    pub fn editable(&self) -> bool {
        !self.is_maxis() || crate::maxis_creation::editable(self)
    }
    pub fn passes(&self, filter: Filter) -> bool {
        match filter {
            Filter::Everything => true,
            Filter::Mine => !self.is_maxis(),
            Filter::Maxis => self.is_maxis(),
        }
    }
}

/// A saved game as Sporepedia reads it.
pub struct SavedGame {
    pub path: PathBuf,
    pub planet: String,
    pub modified: Option<SystemTime>,
    pub run: RunProgress,
}

impl PediaState {
    /// Cards the shown page lists (without the "Create Your Own" card).
    pub fn page_cards(&self) -> usize {
        let per_page = self.per_page.max(1);
        let list = listed(&self.entries, self.category, self.filter, self.sort);
        list.len().saturating_sub(self.page * per_page).min(per_page)
    }

    /// The listing the grid shows (page, category, filter, sort, any card).
    pub fn listing_key(&self) -> (usize, usize, u8, u8, bool) {
        (self.page, self.category, self.filter as u8, self.sort as u8, self.page_cards() > 0)
    }
}

#[derive(Resource, Default)]
pub struct PediaState {
    pub category: usize,
    pub filter: Filter,
    pub page: usize,
    pub sort: Sort,
    pub entries: Vec<Entry>,
    /// The card whose detail view is open (index into `entries`).
    pub detail: Option<usize>,
    /// The creation waiting for the delete confirmation (index into
    /// `entries`).
    pub confirm: Option<usize>,
    /// Cards per page (from the grid's size) and the card under the pointer.
    pub per_page: usize,
    pub hovered: Option<usize>,
    /// The simulation was paused for Sporepedia (resume on close).
    pub paused_game: bool,
    /// The installed creations (worker thread) and whether `entries` has
    /// them.
    pub maxis: maxis::Catalog,
    pub maxis_listed: bool,
    pub status: String,
    /// The feeds' property files (Everything, My Creations, Maxis).
    pub feeds: Vec<feeds::Feed>,
    /// Where the open dialog's parts are on screen (filled by `bind`).
    pub large: Option<LargeRects>,
}

/// Screen rectangles of the `AssetViewLarge` dialog's parts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LargeRects {
    pub dialog: crate::spui::solve::Rect,
    pub render: crate::spui::solve::Rect,
    pub verbs: crate::spui::solve::Rect,
    pub rotate_left: crate::spui::solve::Rect,
    pub rotate_right: crate::spui::solve::Rect,
    pub zoom_in: crate::spui::solve::Rect,
    pub zoom_out: crate::spui::solve::Rect,
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PediaRoot {
    Frame,
    Category(usize),
    Filter(usize),
    /// The filter strip (`AssetBrowserFeedFilter`).
    Strip,
    Grid,
    Verbs,
    /// The `AssetViewLarge` dialog.
    Large,
    Confirm,
}

/// The UI-node layer of the cards and the detail view.
#[derive(Component)]
pub struct PediaCards;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardPart {
    Card(usize),
    Glow(usize),
    Thumb(usize),
    Name(usize),
    Author(usize),
    /// The time stamp at the bottom of a card.
    Stamp(usize),
    /// The detail view's layer (over the grid, under the dialog) with its
    /// backdrop and the creation's image in the render window.
    Detail,
    /// The detail view's backdrop around the dialog (top, bottom, left,
    /// right of it).
    Shade(u8),
    DetailThumb,
}

/// English names of the rigid editor setups: the fallback of the locale names
/// ([`KIND_TITLES`]); ADAPTED for the three outfitters and the generic
/// building / vehicle, which have no shopper title.
pub fn design_kind(kind: SetupKind) -> &'static str {
    use SetupKind::*;
    match kind {
        BuildingCityHall => "City Hall",
        BuildingHouse => "House",
        BuildingFactory => "Factory",
        BuildingEntertainment => "Entertainment",
        Building => "Building",
        VehicleMilitaryLand => "Military Land Vehicle",
        VehicleEconomicLand => "Economic Land Vehicle",
        VehicleCulturalLand => "Religious Land Vehicle",
        VehicleMilitaryWater => "Military Sea Vehicle",
        VehicleEconomicWater => "Economic Sea Vehicle",
        VehicleCulturalWater => "Religious Sea Vehicle",
        VehicleMilitaryAir => "Military Air Vehicle",
        VehicleEconomicAir => "Economic Air Vehicle",
        VehicleCulturalAir => "Religious Air Vehicle",
        Vehicle => "Vehicle",
        Ufo => "Spaceship",
        Tribal => "Tribal Outfit",
        Civ => "Civilization Outfit",
        Space => "Space Outfit",
        _ => "Creature",
    }
}

/// Locale ids (table 21671745) of the rigid editor setups' names: the
/// shopper titles "Choose a Military Land Vehicle", "Choose a Factory", ...
/// (retail's own wording for each setup; the lead-in is dropped). Cultural
/// vehicles are "Religious" there. VERIFIED data.
const KIND_TITLES: [(SetupKind, u32); 14] = [
    (SetupKind::BuildingCityHall, 0x0004_0003),
    (SetupKind::BuildingEntertainment, 0x0004_000A),
    (SetupKind::BuildingFactory, 0x0004_000B),
    (SetupKind::BuildingHouse, 0x0004_000C),
    (SetupKind::VehicleCulturalAir, 0x0004_0004),
    (SetupKind::VehicleCulturalLand, 0x0004_0005),
    (SetupKind::VehicleCulturalWater, 0x0004_0006),
    (SetupKind::VehicleEconomicAir, 0x0004_0007),
    (SetupKind::VehicleEconomicLand, 0x0004_0008),
    (SetupKind::VehicleEconomicWater, 0x0004_0009),
    (SetupKind::VehicleMilitaryAir, 0x0004_000D),
    (SetupKind::VehicleMilitaryLand, 0x0004_000E),
    (SetupKind::VehicleMilitaryWater, 0x0004_000F),
    (SetupKind::Ufo, 0x0004_0010),
];

/// A shopper title as a setup name: "Choose a Military Land Vehicle" ->
/// "Military Land Vehicle".
fn setup_name_of(title: &str) -> &str {
    title.strip_prefix("Choose an ").or_else(|| title.strip_prefix("Choose a ")).unwrap_or(title)
}

/// The entries' type names from the locale strings of [`KIND_TITLES`] (the
/// English names of [`design_kind`] stay for the setups without one: the
/// outfitters, the generic building / vehicle).
fn localize_kinds(entries: &mut [Entry], assets: &mut SpuiAssets, index: &crate::asset_catalog::ResourceIndex) {
    let names: Vec<(&str, String)> = KIND_TITLES
        .iter()
        .map(|(kind, id)| {
            let english = design_kind(*kind);
            (english, setup_name_of(&super::loc(assets, index, FEED_TABLE, *id, english)).to_string())
        })
        .collect();
    for e in entries.iter_mut() {
        if let Some((_, text)) = names.iter().find(|(english, _)| *english == e.kind) {
            e.kind = text.clone();
        }
    }
}

fn setup_kind(setup: &str) -> Option<SetupKind> {
    SetupKind::ALL.into_iter().find(|k| k.setup_name() == setup)
}

fn design_category(kind: SetupKind) -> usize {
    if kind.is_building() {
        1
    } else if kind == SetupKind::Ufo {
        3
    } else {
        2
    }
}

/// Where Sporepedia's entries come from.
pub struct Sources<'a> {
    pub saves: &'a [SavedGame],
    pub catalog: &'a crate::creation_records::CreationCatalog,
    pub store: &'a store::Store,
    pub maxis: &'a [maxis::InstalledCreation],
    /// The author shown on the store's creations.
    pub mine: &'a str,
}

/// The thumbnail cache name of a saved game's creature (by its parts, body and
/// paint): the Sporepedia card and the galaxy screen's game picture share it.
pub fn creature_thumb_name(run: &RunProgress) -> String {
    let hash = thumbs::content_hash(&serde_json::to_vec(&(&run.parts, &run.body, &run.creature_creation.paint)).unwrap_or_default());
    format!("c-{hash:016x}")
}

/// Every creation Sporepedia lists.
pub fn gather(src: &Sources) -> Vec<Entry> {
    use crate::creation_records::{BLD, UFO, VCL};
    let mut out = Vec::new();
    for save in src.saves {
        let run = &save.run;
        let name = slots::creature_name(run);
        out.push(Entry {
            category: 0,
            name: if name.trim().is_empty() { "Unnamed Creature".into() } else { name },
            author: save.planet.clone(),
            kind: "Creature".into(),
            description: String::new(),
            tags: Vec::new(),
            created: save.modified,
            order: 0,
            parts: run.parts.len(),
            source: thumbs::Source::Creature(save.path.clone()),
            thumb: creature_thumb_name(run),
            origin: Origin::Game { path: save.path.clone(), setup: None },
        });
        for (setup, design) in &run.rigid.designs {
            if !design.parts.iter().any(|p| p.block.is_some()) {
                continue;
            }
            let Some(kind) = setup_kind(setup) else { continue };
            let hash = thumbs::content_hash(&serde_json::to_vec(design).unwrap_or_default());
            out.push(Entry {
                category: design_category(kind),
                name: if design.name.trim().is_empty() { design_kind(kind).to_string() } else { design.name.clone() },
                author: save.planet.clone(),
                kind: design_kind(kind).into(),
                description: String::new(),
                tags: Vec::new(),
                created: save.modified,
                order: 0,
                parts: design.parts.len(),
                source: thumbs::Source::Design(save.path.clone(), setup.clone()),
                thumb: format!("d-{hash:016x}"),
                origin: Origin::Game { path: save.path.clone(), setup: Some(setup.clone()) },
            });
        }
    }
    for c in &src.store.creations {
        let hash = thumbs::content_hash(&serde_json::to_vec(&c.kind).unwrap_or_default());
        let (category, kind, parts) = match &c.kind {
            store::StoredKind::Creature(creature) => {
                let kind = setup_kind(&creature.outfitter).map_or("Creature", design_kind);
                (0, kind.to_string(), creature.parts.len() + creature.outfit.len())
            }
            store::StoredKind::Design { setup, design } => {
                let kind = setup_kind(setup).unwrap_or(SetupKind::Vehicle);
                (design_category(kind), design_kind(kind).to_string(), design.parts.len())
            }
        };
        let fallback = if category == 0 { "Unnamed Creature".to_string() } else { kind.clone() };
        out.push(Entry {
            category,
            name: if c.name.trim().is_empty() { fallback } else { c.name.clone() },
            author: src.mine.to_string(),
            kind,
            description: String::new(),
            tags: Vec::new(),
            created: Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(c.created)),
            order: 0,
            parts,
            source: thumbs::Source::Stored(Box::new(c.clone())),
            thumb: format!("s-{hash:016x}"),
            origin: Origin::Stored(c.id),
        });
    }
    for (order, c) in src.catalog.creations.iter().enumerate() {
        let (category, kind) = match c.key.type_id {
            BLD => (1, "Building"),
            VCL => (2, "Vehicle"),
            UFO => (3, "Spaceship"),
            _ => continue,
        };
        if c.name.trim().is_empty() || c.name.contains("PaintSwatch") {
            continue;
        }
        out.push(Entry {
            category,
            name: c.name.clone(),
            author: "Maxis".into(),
            kind: kind.into(),
            description: String::new(),
            tags: Vec::new(),
            created: None,
            order: order + 1,
            parts: c.blocks.len(),
            source: thumbs::Source::Installed(c.key),
            thumb: format!("m-{:08x}-{:08x}", c.key.group_id, c.key.instance_id),
            origin: Origin::Maxis,
        });
    }
    out.extend(maxis_entries(src.maxis, src.catalog.creations.len()));
    out
}

const NO_KEY: crate::dbpf::ResourceKey = crate::dbpf::ResourceKey { group_id: 0, instance_id: 0, type_id: 0 };

/// The installed metadata creations as entries (after the creation files).
pub fn maxis_entries(list: &[maxis::InstalledCreation], first_order: usize) -> Vec<Entry> {
    list.iter()
        .map(|c| Entry {
            category: c.category,
            name: c.name.clone(),
            author: if c.author.is_empty() { "Maxis".into() } else { c.author.clone() },
            kind: CATEGORIES[c.category].trim_end_matches('s').to_string(),
            description: c.description.clone(),
            tags: c.tags.clone(),
            created: None,
            order: first_order + c.order,
            parts: 0,
            source: thumbs::Source::Png(c.png.unwrap_or(NO_KEY)),
            thumb: format!("p-{:08x}-{:08x}", c.key.1, c.key.2),
            origin: Origin::Maxis,
        })
        .collect()
}

/// The entries of a category and filter in the chosen order (indices into
/// `entries`).
pub fn listed(entries: &[Entry], category: usize, filter: Filter, sort: Sort) -> Vec<usize> {
    let mut list: Vec<usize> = (0..entries.len()).filter(|i| in_category(entries[*i].category, category) && entries[*i].passes(filter)).collect();
    let lower = |s: &str| s.to_lowercase();
    list.sort_by(|a, b| {
        let (a, b) = (&entries[*a], &entries[*b]);
        match sort {
            // Newest first; the installed creations after the player's.
            Sort::TimeCreated => b.created.cmp(&a.created).then(a.order.cmp(&b.order)),
            Sort::Title => lower(&a.name).cmp(&lower(&b.name)),
            Sort::Creator => lower(&a.author).cmp(&lower(&b.author)).then(lower(&a.name).cmp(&lower(&b.name))),
            Sort::Type => lower(&a.kind).cmp(&lower(&b.kind)).then(lower(&a.name).cmp(&lower(&b.name))),
        }
    });
    list
}

/// A `--front-end pedia:...` capture request.
#[derive(Clone, Copy, Debug, Default)]
pub struct Demo {
    pub category: usize,
    pub filter: Filter,
    /// Open the first card's detail view (`-detail`); the first store
    /// creation's (`-stored`); its delete confirmation (`-delete`).
    pub detail: bool,
    pub stored: bool,
    pub delete: bool,
}

/// `--front-end pedia:<category>[-maxis|-mine][-detail|-stored|-delete]`.
static DEMO: std::sync::Mutex<Option<Demo>> = std::sync::Mutex::new(None);
/// A `-detail` demo whose list was empty until the installed creations load.
static LATE_DEMO: std::sync::Mutex<Option<Demo>> = std::sync::Mutex::new(None);

/// `-click=<control id>` / `-hold=<control id>` of a `--front-end pedia:`
/// capture: the pointer presses (6 frames; held 30 frames) the centre of that
/// window once the page is up (`demo_pointer`). (control, hold).
static POINTER: std::sync::Mutex<Option<(u32, bool)>> = std::sync::Mutex::new(None);

/// Presses the window of a `-click=` / `-hold=` capture (the capture harness
/// has no mouse): registered before the SPUI input pass.
pub fn demo_pointer(
    fe: Res<FrontEnd>,
    mut pointer: ResMut<SpuiPointer>,
    roots: Query<(&SpuiState, &PediaRoot)>,
    mut frame: Local<u32>,
) {
    let Some((control, hold)) = POINTER.lock().ok().and_then(|p| *p) else { return };
    if fe.overlay != Some(Overlay::Sporepedia) {
        return;
    }
    *frame += 1;
    const START: u32 = 400;
    let len = if hold { 30 } else { 6 };
    if *frame < START {
        return;
    }
    if *frame < START + len {
        let at = roots.iter().filter(|(ui, _)| ui.ready).find_map(|(ui, _)| ui.find(control).filter(|w| ui.visible(*w)).map(|w| ui.rect(w)));
        if let Some(r) = at {
            let (x, y) = r.center();
            pointer.position_override = Some(Vec2::new(x, y));
            pointer.pressed_override = Some(true);
        }
    } else {
        pointer.pressed_override = Some(false);
    }
}

pub fn demo(arg: &str) {
    let mut name = arg;
    let mut d = Demo::default();
    // `-click=<hex id>` / `-hold=<hex id>`: a pointer press at that window.
    for (suffix, hold) in [("-click=", false), ("-hold=", true)] {
        if let Some((rest, id)) = name.rsplit_once(suffix) {
            if let (Ok(control), Ok(mut slot)) = (u32::from_str_radix(id, 16), POINTER.lock()) {
                *slot = Some((control, hold));
                name = rest;
            }
        }
    }
    'strip: loop {
        for suffix in ["-maxis", "-mine", "-detail", "-stored", "-delete"] {
            if let Some(n) = name.strip_suffix(suffix) {
                name = n;
                match suffix {
                    "-maxis" => d.filter = Filter::Maxis,
                    "-mine" => d.filter = Filter::Mine,
                    "-detail" => d.detail = true,
                    "-stored" => (d.detail, d.stored) = (true, true),
                    _ => (d.detail, d.stored, d.delete) = (true, true, true),
                }
                continue 'strip;
            }
        }
        break;
    }
    d.category = CATEGORIES.iter().position(|c| c.eq_ignore_ascii_case(name)).unwrap_or(match name {
        "plants" => 4,
        _ => 0,
    });
    if let Ok(mut slot) = DEMO.lock() {
        *slot = Some(d);
    }
}

/// The saved games (their creatures and designs).
fn saved_games(fe: &FrontEnd) -> Vec<SavedGame> {
    let infos = match (&fe.base, fe.slots.is_empty()) {
        (Some(base), true) => slots::scan(base),
        _ => fe.slots.clone(),
    };
    infos
        .iter()
        .filter_map(|info| {
            let summary = info.saved()?;
            let run = crate::persistence::load_from(&info.path).ok()?;
            Some(SavedGame { path: info.path.clone(), planet: summary.planet.clone(), modified: summary.modified, run })
        })
        .collect()
}

/// A creature of a run on a fresh record (what a creator works on).
pub fn creature_scratch(src: &RunProgress) -> RunProgress {
    let mut run = RunProgress::default();
    run.body = src.body.clone();
    run.parts = src.parts.clone();
    run.creature_creation = src.creature_creation.clone();
    run.sync_legacy_accessory();
    run
}

/// The record an entry's creature or design opens with, if it has one.
pub fn entry_record(entry: &Entry, pedia_store: &store::Store) -> Option<RunProgress> {
    match &entry.origin {
        Origin::Game { path, setup } => {
            let run = crate::persistence::load_from(path).ok()?;
            match setup {
                None => Some(creature_scratch(&run)),
                Some(setup) => {
                    let mut scratch = RunProgress::default();
                    scratch.rigid.designs.insert(setup.clone(), run.rigid.designs.get(setup)?.clone());
                    Some(scratch)
                }
            }
        }
        Origin::Stored(id) => pedia_store.get(*id).map(|c| c.to_run()),
        Origin::Maxis => None,
    }
}

/// Edit: the matching creator on a copy of the creation (a store creation is
/// replaced by the result, the others are kept as new creations).
pub fn edit_launch(entry: &Entry, pedia_store: &store::Store) -> Option<creators::MenuLaunch> {
    let scratch = entry_record(entry, pedia_store)?;
    let (kind, replace) = match &entry.origin {
        Origin::Game { setup: None, .. } => (SetupKind::Creature, None),
        Origin::Game { setup: Some(setup), .. } => (setup_kind(setup)?, None),
        Origin::Stored(id) => {
            let kind = match &pedia_store.get(*id)?.kind {
                store::StoredKind::Creature(c) => setup_kind(&c.outfitter).filter(|k| k.is_outfitter()).unwrap_or(SetupKind::Creature),
                store::StoredKind::Design { setup, .. } => setup_kind(setup)?,
            };
            (kind, Some(*id))
        }
        Origin::Maxis => return None,
    };
    Some(creators::MenuLaunch { kind, scratch: Box::new(scratch), replace })
}

/// The outfitter the Outfitter verb opens for a creature: the outfit it
/// already wears (a store creation dressed in an outfitter), else the
/// outfitter of the stage its game is in (Civilization / Space; Tribal for a
/// game before or in the Tribal stage).
///
/// Traced of retail's Outfitter verb (0x64C040 with the outfit flag +0x234, then
/// 0x64AC40, `cSPAssetBrowser`'s editor launch): it builds an `EditorRequest`
/// (0x5A9140) carrying the asset's key (+0x24..0x30), the browser's editor id
/// (+0x1B8; `sporepediaConfigEditorTypeOverride` 0x05653D86 is the override,
/// 0x051B22B0 takes a request path of its own at 0x64B09E), the selected
/// asset's editor flags and the config's editor type, posts the editor message
/// 0xB03BC30C (plus 0x051CC0B8 in game mode 0xDBDBA1) and submits it (0x5A92C0;
/// a request field is compared with 0x1654C10 first). The editor id itself is
/// stored by the config setup (0x64A370 / 0x644DD0), not by this routine, and
/// the verb tray files (`verbTrays`, group AF028F41) only hold the stat trays'
/// names and icons. Decoded since: the request's editor name is the shopper
/// config's `sporepediaConfigEditorForOutfitter` (0xE3475119, read at 0x6457F9
/// into +0x238; the accept routine 0x64B051 takes it when the outfit flag +0x23C
/// is set), and the verb itself only shows when the config sets
/// `sporepediaConfigEnableOutfitButton` (0x86C7FF4F, +0x23D; tray builder
/// 0x656C30 also needs an editable creature whose editor differs from the
/// outfitter). A scan of every installed property file finds neither property
/// (no shopper config enables the verb; the Galactic Adventures configs are not
/// installed), so retail's Sporepedia detail view has no Outfitter verb here
/// and the verb is hidden ([`bind`]). This rule (the outfit it wears, else the
/// outfitter of its stage) only serves the launch code that remains for a
/// config that enables the verb. ADAPTED: the outfit / stage rule above.
pub fn outfitter_for(entry: &Entry, pedia_store: &store::Store) -> SetupKind {
    match &entry.origin {
        Origin::Stored(id) => match pedia_store.get(*id).map(|c| &c.kind) {
            Some(store::StoredKind::Creature(c)) => setup_kind(&c.outfitter).filter(|k| k.is_outfitter()).unwrap_or(SetupKind::Tribal),
            _ => SetupKind::Tribal,
        },
        Origin::Game { path, setup: None } => match crate::persistence::load_from(path).map(|r| r.current_stage()) {
            Ok(crate::stages::GameStage::Civilization) => SetupKind::Civ,
            Ok(crate::stages::GameStage::Space) => SetupKind::Space,
            _ => SetupKind::Tribal,
        },
        _ => SetupKind::Tribal,
    }
}

/// The outfitter on a store creature (the shopper's Create Your Own: the
/// creature just made in the Creature Creator).
pub fn outfit_stored(id: u64, kind: SetupKind, pedia_store: &store::Store, catalog: &crate::asset_catalog::PartCatalog) -> Option<creators::MenuLaunch> {
    let c = pedia_store.get(id)?;
    if !c.is_creature() {
        return None;
    }
    Some(creators::MenuLaunch { kind, scratch: Box::new(creators::outfit_scratch(c.to_run(), catalog)), replace: None })
}

/// An outfitter on an entry's creature.
pub fn outfit_launch(
    entry: &Entry,
    kind: SetupKind,
    pedia_store: &store::Store,
    catalog: &crate::asset_catalog::PartCatalog,
) -> Option<creators::MenuLaunch> {
    if entry.category != 0 {
        return None;
    }
    let mut scratch = entry_record(entry, pedia_store)?;
    // Another outfitter's accessories stay off.
    for other in [SetupKind::Tribal, SetupKind::Civ, SetupKind::Space] {
        if other != kind {
            if let Some(o) = store::outfit_record(&mut scratch, other.setup_name()) {
                o.parts.clear();
            }
        }
    }
    Some(creators::MenuLaunch { kind, scratch: Box::new(creators::outfit_scratch(scratch, catalog)), replace: None })
}

#[allow(clippy::too_many_arguments)]
pub fn manage(
    mut commands: Commands,
    mut fe: ResMut<FrontEnd>,
    mut pedia: ResMut<PediaState>,
    pedia_store: Res<store::Store>,
    originals: Res<crate::Originals>,
    stage: Res<State<crate::stages::GameStage>>,
    mut assets: ResMut<SpuiAssets>,
    mut pause: ResMut<crate::pause_menu::PauseMenu>,
    mut thumbs: ResMut<thumbs::Thumbnails>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &PediaRoot)>,
    cards: Query<Entity, With<PediaCards>>,
) {
    let open = fe.overlay == Some(Overlay::Sporepedia);
    // The installed creations load in the background once the galaxy screen
    // or Sporepedia is up.
    if (open || *stage.get() == crate::stages::GameStage::MainMenu) && !originals.resource_index.records.is_empty() {
        let list = pedia.maxis.poll(&originals.resource_index);
        if open && !pedia.maxis_listed && !roots.is_empty() {
            if let Some(list) = list {
                let first = originals.creation_catalog.creations.len();
                let mut installed = maxis_entries(&list, first);
                // A new-game card's shopper lists its own type only.
                if let Some(shop) = fe.flow_pick {
                    installed.retain(|e| shop.accepts(e, &pedia_store));
                }
                pedia.entries.extend(installed);
                pedia.maxis_listed = true;
                // A capture's detail view waiting for the installed creations.
                if let Some(d) = LATE_DEMO.lock().ok().and_then(|mut slot| slot.take()) {
                    pedia.detail = listed(&pedia.entries, d.category, d.filter, pedia.sort).first().copied();
                }
            }
        }
    }
    if !open {
        fe.shopper = None;
        fe.flow_pick = None;
        if !roots.is_empty() || !cards.is_empty() {
            for (e, _) in &roots {
                commands.entity(e).despawn();
            }
            for e in &cards {
                commands.entity(e).despawn();
            }
            pedia.entries.clear();
            pedia.detail = None;
            pedia.confirm = None;
            if pedia.paused_game {
                pause.open = false;
                pedia.paused_game = false;
            }
        }
        return;
    }
    if !roots.is_empty() || originals.resource_index.records.is_empty() {
        return;
    }
    let Ok(camera) = camera.single() else { return };
    let index = &originals.resource_index;
    thumbs.cache = fe.base.as_deref().map(thumbs::cache_dir);
    let installed = pedia.maxis.poll(index);
    pedia.maxis_listed = installed.is_some();
    let mine = super::loc(&mut assets, index, FEED_TABLE, FILTER_STRINGS[1].0, FILTER_STRINGS[1].1);
    let saves = saved_games(&fe);
    pedia.entries = gather(&Sources {
        saves: &saves,
        catalog: &originals.creation_catalog,
        store: &pedia_store,
        maxis: installed.as_deref().unwrap_or(&[]),
        mine: &mine,
    });
    localize_kinds(&mut pedia.entries, &mut assets, index);
    // A new-game card's shopper lists its own type only.
    if let Some(shop) = fe.flow_pick {
        pedia.entries.retain(|e| shop.accepts(e, &pedia_store));
    }
    pedia.feeds = feeds::read_feeds(index);
    pedia.page = 0;
    pedia.detail = None;
    pedia.confirm = None;
    pedia.status.clear();
    pedia.filter = Filter::Everything;
    // Open on the first type that has creations; the shopper on creatures.
    pedia.category = (0..CATEGORIES.len()).find(|c| pedia.entries.iter().any(|e| e.category == *c)).unwrap_or(0);
    // A shopper opens on the creatures of the Everything feed: the shopping
    // configs' parent `shoppingTemplate` (9A74416B!D4083B87) selects
    // `feed_all` (`sporepediaConfigSelectedFeed` 0x3449565E) with the root
    // filter of the type (`CreatureShopper`: Creatures); the grid always has
    // the Create Your Own card, so a game without creatures of its own needs
    // no stand-in (Maxis creatures are listed too).
    if is_shopper(&fe) {
        pedia.category = shopper_category(&fe);
        pedia.filter = Filter::Everything;
    }
    pedia.paused_game = pause.open;
    if let Some(d) = DEMO.lock().ok().and_then(|mut slot| slot.take()) {
        pedia.category = d.category;
        pedia.filter = d.filter;
        let list = listed(&pedia.entries, d.category, d.filter, pedia.sort);
        let pick = if d.stored {
            list.iter().copied().find(|i| matches!(pedia.entries[*i].origin, Origin::Stored(_)))
        } else {
            list.first().copied()
        };
        if d.detail {
            pedia.detail = pick;
            if pick.is_none() && !pedia.maxis_listed {
                if let Ok(mut slot) = LATE_DEMO.lock() {
                    *slot = Some(d);
                }
            }
        }
        if d.delete {
            pedia.confirm = pick;
        }
    }
    let Some(frame) = assets.layout(index, LAYOUT) else { return };
    let root = view::spawn(&mut commands, frame, RootPolicy::Fit, Some(camera), 190);
    commands.entity(root).insert(PediaRoot::Frame);
    if let Some(layout) = assets.layout(index, CATEGORY_LAYOUT) {
        for i in 0..CATEGORIES.len() {
            let root = view::spawn(&mut commands, layout.clone(), RootPolicy::Authored, Some(camera), 191);
            commands.entity(root).insert(PediaRoot::Category(i));
        }
    }
    // The feeds under the types: the feed list item rows (icon, name, "(count)").
    if let Some(layout) = assets.layout(index, FEED_ITEM_LAYOUT).or_else(|| assets.layout(index, CATEGORY_LAYOUT)) {
        for i in 0..FILTERS.len() {
            let root = view::spawn(&mut commands, layout.clone(), RootPolicy::Authored, Some(camera), 191);
            commands.entity(root).insert(PediaRoot::Filter(i));
        }
    }
    if let Some(layout) = assets.layout(index, STRIP_LAYOUT) {
        let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 192);
        commands.entity(root).insert(PediaRoot::Strip);
    }
    if let Some(layout) = assets.layout(index, GRID_LAYOUT) {
        let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 191);
        commands.entity(root).insert(PediaRoot::Grid);
    }
    if let Some(layout) = assets.layout(index, VERBS_LAYOUT) {
        let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 195);
        commands.entity(root).insert(PediaRoot::Verbs);
    }
    // The dialog: its frame over the creation's image (the cards layer, 193),
    // under the verbs (195) it holds in its column.
    if let Some(layout) = assets.layout(index, LARGE_LAYOUT) {
        let root = view::spawn(&mut commands, layout, RootPolicy::Centered, Some(camera), 194);
        commands.entity(root).insert(PediaRoot::Large);
    }
    if let Some(layout) = assets.layout(index, CONFIRM_LAYOUT) {
        let root = view::spawn(&mut commands, layout, RootPolicy::Centered, Some(camera), 197);
        commands.entity(root).insert(PediaRoot::Confirm);
    }
}

fn find(ui: &SpuiState, control: u32) -> Option<WinId> {
    ui.find(control)
}

/// Move a row layout's root to `(x, y)` keeping its authored size.
fn place(ui: &mut SpuiState, x: f32, y: f32) {
    let Some(&root) = ui.layout.roots.first() else { return };
    let a = ui.layout.window(root).area;
    let area = [x, y, x + a[2] - a[0], y + a[3] - a[1]];
    if ui.area(root) != area {
        ui.set_area(root, area);
    }
}

/// Move a window (keeping its size, or taking `size`) so its top-left corner
/// is at the screen position `(x, y)` (areas are relative to the parent).
fn place_in_parent(ui: &mut SpuiState, w: WinId, x: f32, y: f32, size: Option<[f32; 2]>) {
    let Some(parent) = ui.layout.window(w).parent else { return };
    let origin = ui.rect(parent);
    let a = ui.area(w);
    let (width, height) = size.map_or((a[2] - a[0], a[3] - a[1]), |s| (s[0], s[1]));
    let (x, y) = (x - origin.x1, y - origin.y1);
    set_area(ui, w, [x, y, x + width, y + height]);
}

fn set_area(ui: &mut SpuiState, w: WinId, area: [f32; 4]) {
    if ui.area(w) != area {
        ui.set_area(w, area);
    }
}

/// Where the cards of a page go: slot 0 is the "Create Your Own" card.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CardGrid {
    pub origin: Vec2,
    pub columns: usize,
    pub rows: usize,
    /// The grid host (detail view area).
    pub host: [f32; 4],
}

impl CardGrid {
    pub fn slots(&self) -> usize {
        self.columns * self.rows
    }

    pub fn slot_rect(&self, slot: usize) -> [f32; 4] {
        let (c, r) = ((slot % self.columns.max(1)) as f32, (slot / self.columns.max(1)) as f32);
        let x = self.origin.x + c * STEP.x;
        let y = self.origin.y + r * STEP.y;
        [x, y, x + CARD.x, y + CARD.y]
    }
}

/// The grid geometry for a grid host and the "Create Your Own" card.
pub fn card_grid(host: [f32; 4], first: [f32; 2]) -> CardGrid {
    let columns = (((host[2] - first[0]) + (STEP.x - CARD.x)) / STEP.x).floor().max(1.0) as usize;
    let rows = (((host[3] - first[1]) + (STEP.y - CARD.y)) / STEP.y).floor().max(1.0) as usize;
    let columns = columns.min(MAX_CARDS);
    let rows = rows.min((MAX_CARDS + 1) / columns.max(1)).max(1);
    CardGrid { origin: Vec2::new(first[0], first[1]), columns, rows, host }
}

/// The verbs a detail view offers: (Edit, Delete, Outfitter) enabled.
pub fn verbs(entry: &Entry, in_menu: bool) -> [bool; 3] {
    let editable = entry.editable() && in_menu;
    [editable, matches!(entry.origin, Origin::Stored(_)), editable && entry.category == 0]
}

/// The dialog's texts for the shown creation and its state. The render window
/// (`F3C6D819`) shows the creation once the model has rendered; before that the
/// baking cover E358 (`34A0979A`, "Please Wait..." with its spinner) is up, as
/// in retail (0x66ED48: the cover shows while the viewer is not active and the
/// render window hides). The buddy / rating groups stay hidden (retail shows
/// them when logged in), the comments frame shows its "Online content only
/// available when logged in" message (ADAPTED: retail's offline comments frame
/// was not observed) and a message of Sporepedia ("Cannot Edit") takes the
/// cover's text.
fn bind_large(ui: &mut SpuiState, e: &Entry, status: &str, wait: &str, model: Option<crate::dbpf::ResourceKey>, loading: bool) {
    if let Some(w) = find(ui, large::RENDER) {
        ui.set_image(w, model.map(|file| crate::spui::model::ImageRef { file, uv: [0.0, 0.0, 1.0, 1.0], dims: None }));
    }
    for hidden in [large::ONLINE_AUTHOR, large::ONLINE_RATING] {
        if let Some(w) = find(ui, hidden) {
            ui.set_visible(w, false);
        }
    }
    let mut text = |control: u32, text: String| {
        if let Some(w) = find(ui, control) {
            ui.set_text(w, text);
        }
    };
    text(large::NAME, e.name.clone());
    text(large::AUTHOR, e.author.clone());
    text(large::DESCRIPTION, e.description.clone());
    text(large::TAGS, e.tags.join(", "));
    text(large::BUSY_TEXT, if status.is_empty() { wait.to_string() } else { status.to_string() });
    if let Some(w) = find(ui, large::COMMENTS_MESSAGE) {
        ui.set_visible(w, true);
    }
    let cover = loading || !status.is_empty();
    if let Some(w) = find(ui, large::BUSY) {
        ui.set_visible(w, cover);
    }
    if let Some(w) = find(ui, large::BUSY_ICON) {
        ui.set_visible(w, loading);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn bind(
    mut commands: Commands,
    fe: Res<FrontEnd>,
    mut pedia: ResMut<PediaState>,
    mut thumbs: ResMut<thumbs::Thumbnails>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    turntable: Res<super::turntable::Turntable>,
    mut roots: Query<(&mut SpuiState, &PediaRoot)>,
) {
    if fe.overlay != Some(Overlay::Sporepedia) {
        return;
    }
    let index = &originals.resource_index;
    let in_menu = fe.base.is_some() && fe.launch.is_none() && fe.session.is_none();
    let shopper = is_shopper(&fe);
    let shop_category = shopper_category(&fe);
    let labels: Vec<String> = CATEGORY_IDS.iter().zip(CATEGORIES).map(|(id, name)| super::loc(&mut assets, index, FEED_TABLE, *id, name)).collect();
    let title = match (fe.flow_pick, fe.shopper) {
        (Some(shop), _) => super::loc(&mut assets, index, FEED_TABLE, shop.title.0, shop.title.1),
        (None, Some(kind)) => {
            let (id, text) = shopper_title(kind);
            super::loc(&mut assets, index, FEED_TABLE, id, text)
        }
        (None, None) => labels[pedia.category.min(CATEGORIES.len() - 1)].clone(),
    };
    let filter_labels: Vec<String> = FILTER_STRINGS.iter().map(|(id, f)| super::loc(&mut assets, index, FEED_TABLE, *id, f)).collect();
    let wait = super::loc(&mut assets, index, 0x7C65_11A1, 0xD4AF_63E8, "Please Wait...");
    let status = super::loc(&mut assets, index, FEED_TABLE, STATUS_LINE, "Displaying ~num_items~ items");
    let mut column = None;
    let mut grid = None;
    let mut bar = None;
    for (mut ui, kind) in &mut roots {
        if *kind != PediaRoot::Frame || !ui.ready {
            continue;
        }
        // Online parts of the frame stay hidden; the pager shows with pages.
        for hidden in [id::WEB_FRAME, id::COMMENTS, id::WELCOME] {
            if let Some(w) = find(&ui, hidden) {
                ui.set_visible(w, false);
            }
        }
        if let Some(w) = find(&ui, id::BACKDROP) {
            ui.set_visible(w, true);
        }
        if let Some(w) = find(&ui, id::GUIDE) {
            ui.set_enabled(w, false);
        }
        // The title bar names the shown type (the file holds a placeholder).
        if let Some(w) = find(&ui, id::TITLE_BAR) {
            ui.set_text(w, title.clone());
        }
        column = find(&ui, id::COLUMN).map(|w| ui.rect(w));
        grid = find(&ui, id::GRID).map(|w| ui.rect(w));
        bar = find(&ui, id::TOP_BAR).map(|w| ui.rect(w));
    }
    let (Some(column), Some(grid)) = (column, grid) else { return };
    let list = listed(&pedia.entries, pedia.category, pedia.filter, pedia.sort);
    let detail = pedia.detail.and_then(|i| pedia.entries.get(i)).filter(|_| !shopper).cloned();
    let verbs_on = detail.as_ref().map(|e| verbs(e, in_menu));
    let confirm = pedia.confirm.and_then(|i| pedia.entries.get(i)).map(|e| e.name.clone());
    // The dialog first: the verbs go into its column.
    let mut large = None;
    for (mut ui, kind) in &mut roots {
        if *kind != PediaRoot::Large || !ui.ready {
            continue;
        }
        for root in ui.layout.roots.clone() {
            ui.set_visible(root, detail.is_some() && confirm.is_none());
        }
        let rect = |ui: &SpuiState, control: u32| find(ui, control).map(|w| ui.rect(w)).unwrap_or_default();
        large = Some(LargeRects {
            // The frame's extent (the corner pieces E18 .. E26: 34,17 .. 762,586
            // of the 800 x 600 root, which is transparent around it).
            dialog: ui.layout.roots.first().map(|r| ui.rect(*r)).map(|r| crate::spui::solve::Rect::new(r.x1 + 34.0, r.y1 + 17.0, r.x1 + 762.0, r.y1 + 586.0)).unwrap_or_default(),
            render: rect(&ui, large::PANEL),
            verbs: rect(&ui, large::VERB_COLUMN),
            rotate_left: rect(&ui, large::ROTATE_LEFT),
            rotate_right: rect(&ui, large::ROTATE_RIGHT),
            zoom_in: rect(&ui, large::ZOOM_IN),
            zoom_out: rect(&ui, large::ZOOM_OUT),
        });
        if let Some(e) = &detail {
            // The creation turning in the render window: the turntable's
            // render target once it has rendered (the card image shows
            // before, as a UI node over the window).
            let model = turntable.image(&e.thumb).map(|_| super::turntable::image_key(&e.thumb));
            let loading = model.is_none() && !turntable.failed;
            bind_large(&mut ui, e, &pedia.status, &wait, model, loading);
        }
    }
    pedia.large = large.filter(|_| detail.is_some());
    let mut layout = None;
    for (mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match *kind {
            PediaRoot::Frame | PediaRoot::Large => {}
            PediaRoot::Category(i) => {
                let rows = visible_rows(pedia.category);
                let slot = rows.iter().position(|r| *r == i);
                for root in ui.layout.roots.clone() {
                    if ui.visible(root) != slot.is_some() {
                        ui.set_visible(root, slot.is_some());
                    }
                }
                // ADAPTED: the child row (Spaceships) is indented by half a button.
                let indent = if i == SPACESHIPS { 14.0 } else { 0.0 };
                place(&mut ui, column.x1 + 8.0 + indent, column.y1 + 12.0 + slot.unwrap_or(0) as f32 * 34.0);
                let n = pedia.entries.iter().filter(|e| in_category(e.category, i) && e.passes(pedia.filter)).count();
                if let Some(w) = find(&ui, id::LABEL) {
                    ui.set_text(w, format!("{} ({n})", labels[i]));
                }
                if let Some(w) = find(&ui, id::CAT_TOGGLE) {
                    ui.set_selected(w, pedia.category == i);
                    ui.set_enabled(w, !shopper || i == shop_category);
                }
            }
            // The feeds under the types (after a gap of half a row), each with the
            // number of creations it lists (`AssetBrowserFeedListItem`: name and "(count)").
            PediaRoot::Filter(i) => {
                let rows = visible_rows(pedia.category).len();
                place(&mut ui, column.x1 + 8.0, column.y1 + 12.0 + (rows as f32 + 0.5 + i as f32) * 34.0);
                let n = pedia.entries.iter().filter(|e| e.passes(FILTERS[i])).count();
                if let Some(w) = find(&ui, id::FEED_NAME) {
                    // The row's own name text (the old category row layout keeps one text).
                    ui.set_text(w, filter_labels[i].clone());
                    if let Some(count) = find(&ui, id::FEED_COUNT) {
                        ui.set_text(count, format!("({n})"));
                    }
                    // The feed's own 25 x 25 icon (property `F4906970`).
                    if let Some(icon) = find(&ui, id::FEED_ICON) {
                        let key = pedia.feeds.get(i).and_then(|f| f.icon).filter(|key| assets.texture(index, &mut images, *key).is_some());
                        ui.set_image(icon, key.map(|key| crate::spui::model::ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None }));
                    }
                } else {
                    if let Some(w) = find(&ui, id::LABEL) {
                        ui.set_text(w, format!("{} ({n})", filter_labels[i]));
                    }
                    if let Some(w) = find(&ui, id::CAT_TOGGLE) {
                        ui.set_selected(w, pedia.filter == FILTERS[i]);
                    }
                }
            }
            // The filter strip in the bar above the grid: the feed's icon and
            // the icons of the filters the shown type stands for. The slots
            // show up to the last filter and the last one is selected
            // (0x65F7D0 / 0x65F920); the filters' own icon buttons of the
            // layout move into their slots.
            PediaRoot::Strip => {
                let Some(bar) = bar else { continue };
                for root in ui.layout.roots.clone() {
                    ui.set_visible(root, detail.is_none() && confirm.is_none());
                }
                // The strip's icons start at (14, 10) inside its root: their
                // top edge on the bar's top (the sort buttons take the
                // bar's lower half).
                place(&mut ui, bar.x1 + 2.0, bar.y1 - 10.0);
                let chain = feeds::chain(pedia.category);
                let shown = (1 + chain.len()).min(id::STRIP_SLOTS.len());
                for (i, slot) in id::STRIP_SLOTS.iter().enumerate() {
                    if let Some(w) = find(&ui, *slot) {
                        ui.set_visible(w, i < shown);
                    }
                }
                let pieces = ui.find_all(id::STRIP_PIECE);
                for (i, w) in ui.find_all(id::STRIP_SELECTED).into_iter().enumerate() {
                    ui.set_visible(w, i + 1 == shown);
                }
                // Slot 0: the feed's 25 x 25 icon PNG.
                let feed = FILTERS.iter().position(|f| *f == pedia.filter).and_then(|i| pedia.feeds.get(i));
                if let (Some(w), Some(piece)) = (find(&ui, id::STRIP_FEED_ICON), pieces.first()) {
                    let icon = feed.and_then(|f| f.icon).filter(|key| assets.texture(index, &mut images, *key).is_some());
                    ui.set_image(w, icon.map(|key| crate::spui::model::ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None }));
                    let r = ui.rect(*piece);
                    place_in_parent(&mut ui, w, r.x1 + 7.0 + 5.5, r.y1 + 6.0, Some([25.0, 25.0]));
                }
                // The following slots: the filters' icon buttons.
                for filter in [feeds::CREATURES, feeds::BUILDINGS, feeds::VEHICLES, feeds::UFO, feeds::FLORA, feeds::CELL] {
                    let Some(w) = find(&ui, filter) else { continue };
                    let slot = chain.iter().position(|c| *c == filter).map(|k| k + 1);
                    ui.set_visible(w, slot.is_some_and(|s| s < shown));
                    ui.set_enabled(w, true);
                    if let Some(piece) = slot.and_then(|s| pieces.get(s)) {
                        let r = ui.rect(*piece);
                        place_in_parent(&mut ui, w, r.x1 + 7.0, r.y1, None);
                    }
                }
            }
            PediaRoot::Grid => {
                // The grid layout fills the host (like the settings pages).
                if let Some(&root) = ui.layout.roots.first() {
                    set_area(&mut ui, root, [grid.x1, grid.y1, grid.x2, grid.y2]);
                }
                for (control, sort) in SORT_BUTTONS {
                    if let Some(w) = find(&ui, control) {
                        ui.set_enabled(w, sort.is_some());
                        ui.set_selected(w, sort == Some(pedia.sort));
                        // The second and third rows of the sort bar (Time
                        // Downloaded, Creepy & Cute, Religious / Economic Power)
                        // drop down under their row; none is live here.
                        if matches!(control, 0x1DC1_C781 | 0xE6AA_9254 | 0x0103_B547 | 0xB1A9_59B4) {
                            ui.set_visible(w, false);
                        }
                    }
                }
                for hidden in [id::CARD_TEMPLATE, id::EMPTY, id::FEED_HEADER, id::SORT_TOGGLES[0], id::SORT_TOGGLES[1]] {
                    if let Some(w) = find(&ui, hidden) {
                        ui.set_visible(w, false);
                    }
                }
                // The status line (0x6528B0): the number of creations the feed shows for the type.
                if let Some(w) = find(&ui, id::FOOTER) {
                    ui.set_text(w, status_line(&status, list.len()));
                }
                if let Some(w) = find(&ui, id::CREATE_OWN) {
                    // The shopper's card makes a new creature ("Create or
                    // Choose"); no creator makes flora or cells. The detail
                    // view and the confirmation replace the grid.
                    ui.set_visible(w, detail.is_none() && confirm.is_none() && pedia.category < 4 && fe.flow_pick.is_none());
                    ui.set_enabled(w, in_menu);
                    let r = ui.rect(w);
                    layout = Some(card_grid([grid.x1, grid.y1, grid.x2, grid.y2], [r.x1, r.y1]));
                }
            }
            // The detail view's verbs under its thumbnail: Edit, Delete (Ban /
            // Share / Accept and the play / paint / details row are online or
            // in-game verbs; the Outfitter verb is off in every installed config).
            PediaRoot::Verbs => {
                for root in ui.layout.roots.clone() {
                    ui.set_visible(root, false);
                }
                let Some((on, at)) = verbs_on.zip(large) else { continue };
                let Some(row) = find(&ui, id::VERBS) else { continue };
                ui.set_visible(row, confirm.is_none());
                // The verbs stack down the dialog's verb column (the 33 px
                // column holds the 36 px buttons centred).
                let (x, y) = (at.verbs.x1 + (at.verbs.width() - 36.0) * 0.5, at.verbs.y1 + 8.0);
                set_area(&mut ui, row, [x, y, x + 36.0, y + 3.0 * VERB_STEP]);
                // Retail's tray builder (0x656C30) shows the Outfitter verb only for a
                // config with `sporepediaConfigEnableOutfitButton` (0x86C7FF4F, read at
                // 0x64581E into +0x23D); none of the 138 installed shopper configs
                // (group 9A74416B) sets it, so the verb never shows (VERIFIED by a scan
                // of every installed property file); the outfitters open from the
                // Create wheel's shoppers.
                let shown = [(id::VERB_EDIT, on[0]), (id::VERB_DELETE, on[1])];
                for (control, icon) in id::VERB_ICONS {
                    let slot = shown.iter().position(|(c, _)| *c == control);
                    for w in [find(&ui, control), find(&ui, icon)].into_iter().flatten() {
                        ui.set_visible(w, slot.is_some());
                    }
                    let Some(slot) = slot else { continue };
                    let area = [0.0, slot as f32 * VERB_STEP, 36.0, slot as f32 * VERB_STEP + 37.0];
                    if let Some(w) = find(&ui, control) {
                        set_area(&mut ui, w, area);
                        ui.set_enabled(w, shown[slot].1);
                    }
                    if let Some(w) = find(&ui, icon) {
                        set_area(&mut ui, w, [area[0], area[1], area[0] + 36.0, area[1] + 36.0]);
                    }
                }
                for hidden in id::VERB_MORE.into_iter().chain([id::VERB_SPARE_ICON, id::VERB_CREATE_OWN]) {
                    if let Some(w) = find(&ui, hidden) {
                        ui.set_visible(w, false);
                    }
                }
            }
            // "Delete" / "This is local content. Do you want to delete
            // "~generic0~"?" (A1C68F50:04A5B7EB / 04519652, the text before
            // its in-game note); buttons Delete / Cancel.
            PediaRoot::Confirm => {
                for root in ui.layout.roots.clone() {
                    ui.set_visible(root, confirm.is_some());
                }
                let Some(name) = &confirm else { continue };
                let title = super::loc(&mut assets, index, 0xA1C6_8F50, 0x04A5_B7EB, "Delete");
                let body = super::loc(&mut assets, index, 0xA1C6_8F50, 0x0451_9652, "Do you want to delete \"~generic0~\"?");
                let body = body.split("~br~").next().unwrap_or_default().replace("~generic0~", name);
                if let Some(w) = find(&ui, super::gge::id::MB_TITLE) {
                    ui.set_text(w, title);
                }
                if let Some(w) = find(&ui, super::gge::id::MB_TEXT) {
                    ui.set_text(w, body.trim().to_string());
                }
                let yes = super::loc(&mut assets, index, super::GGE_TABLE, 0x0677_35D4, "Delete");
                let no = super::loc(&mut assets, index, 0xC015_2A6D, 0x05B0_8744, "Cancel");
                for (i, control) in super::gge::id::MB_BUTTONS.iter().enumerate() {
                    if let Some(w) = find(&ui, *control) {
                        let label = match i {
                            2 => Some(&yes),
                            3 => Some(&no),
                            _ => None,
                        };
                        ui.set_visible(w, label.is_some());
                        if let Some(label) = label {
                            ui.set_enabled(w, true);
                            ui.set_text(w, label.clone());
                        }
                    }
                }
            }
        }
    }
    let Some(layout) = layout else { return };
    pedia.per_page = layout.slots().saturating_sub(1).max(1);
    let pages = list.len().div_ceil(pedia.per_page).max(1);
    pedia.page = pedia.page.min(pages - 1);
    for (mut ui, kind) in &mut roots {
        if *kind == PediaRoot::Frame && ui.ready {
            if let Some(w) = find(&ui, id::PAGER) {
                ui.set_visible(w, pages > 1);
            }
        }
    }
    // The page's thumbnails (rendered one at a time).
    let start = pedia.page * pedia.per_page;
    for &i in list.iter().skip(start).take(pedia.per_page) {
        let e = &pedia.entries[i];
        thumbs.request(&e.thumb, &e.source);
    }
    if let Some(e) = detail {
        thumbs.request(&e.thumb, &e.source);
    }
    commands.insert_resource(layout);
}

/// Clicks: categories, filters, sorts, pager, Create Your Own, cards, the
/// shopper's pick, the detail view and its verbs, the delete confirmation.
#[allow(clippy::too_many_arguments)]
pub fn actions(
    mut fe: ResMut<FrontEnd>,
    mut pedia: ResMut<PediaState>,
    mut pedia_store: ResMut<store::Store>,
    originals: Res<crate::Originals>,
    grid: Option<Res<CardGrid>>,
    pointer: Res<SpuiPointer>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut prev_pressed: Local<bool>,
    mut drag: Local<Option<Vec2>>,
    time: Res<Time>,
    mut view: ResMut<super::turntable::View>,
    mut roots: Query<(&mut SpuiState, &PediaRoot)>,
    mut sounds: MessageWriter<crate::game_audio::PlayCue>,
) {
    let edge = pointer.pressed && !*prev_pressed;
    *prev_pressed = pointer.pressed;
    if fe.overlay != Some(Overlay::Sporepedia) {
        return;
    }
    let in_menu = fe.base.is_some() && fe.launch.is_none() && fe.session.is_none();
    let mut clicks = Vec::new();
    let mut verbs_rect = None;
    for (mut ui, kind) in &mut roots {
        if *kind == PediaRoot::Verbs {
            verbs_rect = find(&ui, id::VERBS).filter(|w| ui.visible(*w)).map(|w| ui.rect(w));
        }
        for w in ui.take_clicks() {
            // The pager buttons have no control id: tell them by caption order.
            let element = ui.layout.window(w).element;
            clicks.push((*kind, ui.layout.window(w).control_id, element));
        }
        // A feed list item row is not a button: a press on the row picks the feed.
        if let (PediaRoot::Filter(_), true, Some(p), Some(row)) = (*kind, edge, pointer.position, find(&ui, id::FEED_ROW)) {
            let r = ui.rect(row);
            if ui.visible(row) && p.x >= r.x1 && p.x <= r.x2 && p.y >= r.y1 && p.y <= r.y2 {
                clicks.push((*kind, id::CAT_CLICK, 0));
            }
        }
    }
    let click = |sounds: &mut MessageWriter<crate::game_audio::PlayCue>| sounds.write(crate::game_audio::PlayCue(crate::game_audio::Cue::UiClick));
    // The delete confirmation takes every click.
    if let Some(i) = pedia.confirm {
        let mut done = keys.just_pressed(KeyCode::Escape);
        for (kind, control, _) in &clicks {
            if *kind != PediaRoot::Confirm {
                continue;
            }
            click(&mut sounds);
            if *control == super::gge::id::MB_BUTTONS[2] {
                if let Some(Origin::Stored(id)) = pedia.entries.get(i).map(|e| e.origin.clone()) {
                    if pedia_store.delete(id) {
                        if let Err(error) = pedia_store.store() {
                            pedia.status = format!("Cannot save the Sporepedia store: {error}");
                        }
                        pedia.entries.remove(i);
                        pedia.detail = None;
                    }
                }
                done = true;
            } else if *control == super::gge::id::MB_BUTTONS[3] || *control == super::gge::id::MB_CLOSE {
                done = true;
            }
        }
        if done {
            keys.clear_just_pressed(KeyCode::Escape);
            pedia.confirm = None;
        }
        return;
    }
    // The detail view: its verbs, else Escape or a click elsewhere closes it.
    if let Some(i) = pedia.detail {
        let entry = pedia.entries.get(i).cloned();
        for (kind, control, _) in &clicks {
            if *kind != PediaRoot::Verbs {
                continue;
            }
            let Some(entry) = &entry else { continue };
            let on = verbs(entry, in_menu);
            // The verbs play their retail sounds (`ui_sounds`: `spd_verb_*_click`).
            if !crate::ui_sounds::has_retail_click(*control) {
                click(&mut sounds);
            }
            match *control {
                id::VERB_EDIT if on[0] => {
                    if let Some(launch) = edit_launch(entry, &pedia_store).or_else(|| crate::maxis_creation::edit_launch(entry, &originals)) {
                        fe.menu_launch = Some(launch);
                        fe.overlay = None;
                    }
                }
                id::VERB_DELETE if on[1] => pedia.confirm = Some(i),
                // The outfitter of the creature's outfit / stage. The button is hidden
                // (no installed config enables it, see the verb tray above); kept for
                // a config that would.
                id::VERB_OUTFITTER if on[2] => {
                    let kind = outfitter_for(entry, &pedia_store);
                    if let Some(launch) = outfit_launch(entry, kind, &pedia_store, &originals.part_catalog)
                        .or_else(|| crate::maxis_creation::outfit_launch(entry, kind, &originals))
                    {
                        fe.menu_launch = Some(launch);
                        fe.overlay = None;
                    }
                }
                _ => {}
            }
            return;
        }
        // The held rotate / zoom buttons and a drag in the render window turn
        // and magnify the view (`turntable`: the retail steps).
        let mut outside = false;
        if let (Some(at), Some(p)) = (pedia.large, pointer.position) {
            let over = |r: crate::spui::solve::Rect| r.contains(p.x, p.y);
            let steps = time.delta_secs().min(0.1) * super::turntable::UPDATE_RATE;
            if pointer.pressed {
                if over(at.rotate_left) {
                    view.rotate(-super::turntable::ROTATE_STEP * steps);
                }
                if over(at.rotate_right) {
                    view.rotate(super::turntable::ROTATE_STEP * steps);
                }
                if over(at.zoom_in) {
                    view.zoom_by(super::turntable::ZOOM_STEP * steps);
                }
                if over(at.zoom_out) {
                    view.zoom_by(-super::turntable::ZOOM_STEP * steps);
                }
            }
            let on_control = [at.rotate_left, at.rotate_right, at.zoom_in, at.zoom_out].into_iter().any(over)
                || verbs_rect.is_some_and(over);
            if edge && over(at.render) && !on_control {
                *drag = Some(p);
            }
            if !pointer.pressed {
                *drag = None;
            }
            if let Some(last) = *drag {
                view.rotate(super::turntable::DRAG_GAIN * (p.x - last.x));
                *drag = Some(p);
            }
            outside = edge && !over(at.dialog);
        } else {
            *drag = None;
        }
        // Close button, Escape, or a click outside the dialog (the modal
        // dialog keeps clicks inside it).
        let closed = clicks.iter().any(|(k, c, _)| *k == PediaRoot::Large && *c == large::CLOSE);
        if keys.just_pressed(KeyCode::Escape) || closed || outside {
            keys.clear_just_pressed(KeyCode::Escape);
            pedia.detail = None;
            *drag = None;
            // `spd_largecard_hide` plays from `ui_sounds`.
        }
        return;
    }
    let list = listed(&pedia.entries, pedia.category, pedia.filter, pedia.sort);
    let per_page = pedia.per_page.max(1);
    // Cards under the pointer (UI nodes, not SPUI windows).
    pedia.hovered = None;
    if let (Some(grid), Some(p)) = (grid.as_deref(), pointer.position) {
        for slot in 1..grid.slots() {
            let r = grid.slot_rect(slot);
            if p.x >= r[0] && p.x <= r[2] && p.y >= r[1] && p.y <= r[3] {
                let at = pedia.page * per_page + slot - 1;
                if at < list.len() {
                    pedia.hovered = Some(slot);
                    if edge {
                        // `spd_assetcard_click` plays from `ui_sounds`.
                        if fe.flow_pick.is_some() {
                            // A new-game card's shopper hands the creation back to the flow.
                            fe.picked = Some(pedia.entries[list[at]].clone());
                            fe.overlay = None;
                            continue;
                        }
                        match fe.shopper {
                            // The shopper's pick opens the outfitter on it.
                            Some(kind) => {
                                let entry = pedia.entries[list[at]].clone();
                                match outfit_launch(&entry, kind, &pedia_store, &originals.part_catalog)
                                    .or_else(|| crate::maxis_creation::outfit_launch(&entry, kind, &originals))
                                {
                                    Some(launch) => {
                                        fe.menu_launch = Some(launch);
                                        fe.overlay = None;
                                    }
                                    None => pedia.status = "Cannot Edit".into(),
                                }
                            }
                            None => pedia.detail = Some(list[at]),
                        }
                    }
                }
            }
        }
    }
    for (kind, control, element) in clicks {
        // Sorts, Create Your Own and the category rows play their retail sounds, closing the
        // window plays `spd_exit` (`ui_sounds`).
        if !crate::ui_sounds::has_retail_click(control) && control != id::CLOSE {
            click(&mut sounds);
        }
        match (kind, control) {
            (PediaRoot::Frame, id::CLOSE) => fe.overlay = None,
            // Pager: element 9 "Next", 13 "Previous" (AssetBrowser layout).
            (PediaRoot::Frame, 0) if element == 9 => {
                if (pedia.page + 1) * per_page < list.len() {
                    pedia.page += 1;
                }
            }
            (PediaRoot::Frame, 0) if element == 13 => pedia.page = pedia.page.saturating_sub(1),
            (PediaRoot::Category(i), id::CAT_CLICK | id::CAT_TOGGLE) => {
                if !is_shopper(&fe) || i == shopper_category(&fe) {
                    pedia.category = i;
                    pedia.page = 0;
                }
            }
            (PediaRoot::Filter(i), id::CAT_CLICK | id::CAT_TOGGLE) => {
                pedia.filter = FILTERS[i];
                pedia.page = 0;
            }
            // The strip's filter icons: a click shows that filter's type (the
            // buttons' control ids are the filter ids).
            (PediaRoot::Strip, filter) => {
                if let Some(category) = feeds::category_of(filter).filter(|c| !is_shopper(&fe) || *c == shopper_category(&fe)) {
                    pedia.category = category;
                    pedia.page = 0;
                }
            }
            // The shopper's Create New: the Creature Creator, then the
            // outfitter on the new creature (`creators::sessions`).
            (PediaRoot::Grid, id::CREATE_OWN) if fe.shopper.is_some() => {
                if in_menu {
                    fe.outfit_after = fe.shopper;
                    fe.menu_launch = Some(creators::MenuLaunch::new(SetupKind::Creature));
                    fe.overlay = None;
                }
            }
            (PediaRoot::Grid, id::CREATE_OWN) if pedia.category >= 4 => {}
            (PediaRoot::Grid, id::CREATE_OWN) => {
                // Create Your Own: the creator wheel at this type's creator.
                fe.overlay = None;
                fe.tab = super::Tab::Create;
                let (hub, creator) = match pedia.category {
                    0 => (Some(0), super::creators::by_name("creature")),
                    1 => (Some(1), None),
                    2 => (Some(2), None),
                    _ => (None, super::creators::by_name("ufo")),
                };
                fe.hub = hub;
                fe.creator = creator;
            }
            (PediaRoot::Grid, c) => {
                if let Some((_, Some(sort))) = SORT_BUTTONS.iter().find(|(b, _)| *b == c) {
                    pedia.sort = *sort;
                    pedia.page = 0;
                }
            }
            _ => {}
        }
    }
}

/// Image region of a window's image drawable (atlas file, pixel rect, size).
fn drawable_image(layout: &crate::spui::model::Layout, control: u32, child_image: bool) -> Option<crate::spui::model::ImageRef> {
    let w = layout.windows.iter().find(|w| w.control_id == control)?;
    let image_of = |w: &crate::spui::model::Window| match &w.drawable {
        Some(Drawable::Image(d)) => d.image.clone(),
        _ => None,
    };
    if !child_image {
        return image_of(w);
    }
    w.children.iter().find_map(|c| image_of(layout.window(*c)))
}

fn atlas_node(assets: &mut SpuiAssets, index: &crate::asset_catalog::ResourceIndex, images: &mut Assets<Image>, image: &crate::spui::model::ImageRef) -> Option<ImageNode> {
    let tex = assets.texture(index, images, image.file)?;
    let [w, h] = tex.size.map(|v| v as f32);
    Some(ImageNode {
        image: tex.handle.clone(),
        rect: Some(Rect::new(image.uv[0] * w, image.uv[1] * h, image.uv[2] * w, image.uv[3] * h)),
        // The card frame is stretched over its node (the layout scales its
        // 142 x 144 region to 141 x 182).
        image_mode: bevy::ui::widget::NodeImageMode::Stretch,
        ..default()
    })
}

fn abs(x: f32, y: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(x),
        top: Val::Px(y),
        width: Val::Px(w),
        height: Val::Px(h),
        ..default()
    }
}

/// The cards of the page and the detail view (UI nodes over the grid).
#[allow(clippy::too_many_arguments)]
pub fn cards(
    mut commands: Commands,
    fe: Res<FrontEnd>,
    pedia: Res<PediaState>,
    grid: Option<Res<CardGrid>>,
    thumbs: Res<thumbs::Thumbnails>,
    turntable: Res<super::turntable::Turntable>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    camera: Query<Entity, With<crate::GameCamera>>,
    layers: Query<Entity, With<PediaCards>>,
    mut parts: Query<(&CardPart, &mut Node, &mut Visibility, Option<&mut ImageNode>, Option<&mut Text>, Option<&mut UiTransform>)>,
    time: Res<Time<Real>>,
    mut pop: Local<PopIn>,
) {
    if fe.overlay != Some(Overlay::Sporepedia) {
        // The next opening builds the cards again (0x64F9E0).
        *pop = PopIn::default();
        return;
    }
    let now = time.elapsed_secs();
    let key = pedia.listing_key();
    if pop.key != Some(key) {
        pop.key = Some(key);
        pop.started = now;
    }
    let Some(grid) = grid else { return };
    if layers.is_empty() {
        let Ok(camera) = camera.single() else { return };
        let index = &originals.resource_index;
        let glow_layout = assets.layout(index, GLOW_LAYOUT);
        let glow = glow_layout.as_deref().and_then(|l| drawable_image(l, id::GLOW, false));
        let glow = glow.and_then(|g| atlas_node(&mut assets, index, &mut images, &g));
        let part = |uv: [f32; 4]| crate::spui::model::ImageRef { file: CARD_ATLAS, uv, dims: None };
        let frame = atlas_node(&mut assets, index, &mut images, &part(CARD_FRAME_UV));
        let bar = atlas_node(&mut assets, index, &mut images, &part(CARD_BAR_UV));
        let font = assets.font_source("Trebuchet EA");
        let text_font = |px: f32| TextFont { font: font.clone(), font_size: bevy::text::FontSize::Px(px), ..default() };
        let layer = commands
            .spawn((
                Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                GlobalZIndex(193),
                UiTargetCamera(camera),
                Pickable::IGNORE,
                PediaCards,
                Name::new("sporepedia cards"),
            ))
            .id();
        commands.entity(layer).with_children(|p| {
            for slot in 1..MAX_CARDS {
                // The `AssetView` layout's card: frame and bottom-bar images
                // around the slot, name / author / picture / time stamp at
                // the layout's positions (relative to the 123 x 174 slot).
                let face = abs(0.0, 0.0, CARD.x, CARD.y);
                p.spawn((face, Visibility::Hidden, CardPart::Card(slot), UiTransform::IDENTITY, Pickable::IGNORE)).with_children(|c| {
                    if let Some(glow) = glow.clone() {
                        c.spawn((abs(-16.0, -13.0, 156.0, 211.0), glow, Visibility::Hidden, CardPart::Glow(slot), Pickable::IGNORE));
                    }
                    if let Some(frame) = frame.clone() {
                        c.spawn((abs(CARD_FRAME_AT.0, CARD_FRAME_AT.1, 141.0, 182.0), frame, Pickable::IGNORE));
                    }
                    if let Some(bar) = bar.clone() {
                        c.spawn((abs(CARD_FRAME_AT.0, CARD_FRAME_AT.1 + 174.0, 141.0, 21.0), bar, Pickable::IGNORE));
                    }
                    c.spawn((abs(7.0, 54.0, 110.0, 110.0), ImageNode::default(), Visibility::Hidden, CardPart::Thumb(slot), Pickable::IGNORE));
                    let mut name = abs(-1.0, 6.0, 126.0, 21.0);
                    name.overflow = Overflow::clip();
                    c.spawn((
                        name,
                        Text::new(""),
                        text_font(10.0),
                        TextColor(Color::WHITE),
                        TextLayout::justify(Justify::Center),
                        CardPart::Name(slot),
                        Pickable::IGNORE,
                    ));
                    c.spawn((
                        abs(3.0, 27.0, 118.0, 12.0),
                        Text::new(""),
                        text_font(8.0),
                        TextColor(Color::WHITE),
                        TextLayout::justify(Justify::Center),
                        CardPart::Author(slot),
                        Pickable::IGNORE,
                    ));
                    c.spawn((
                        abs(0.0, 175.0, 126.0, 12.0),
                        Text::new(""),
                        text_font(8.0),
                        TextColor(Color::WHITE),
                        TextLayout::justify(Justify::Center),
                        CardPart::Stamp(slot),
                        Pickable::IGNORE,
                    ));
                });
            }
            // The detail view replaces the cards: a backdrop over the grid and
            // the creation's image in the dialog's render window (the dialog's
            // frame, SPUI, is drawn over both).
            // (The UI nodes draw over SPUI, so the backdrop leaves the dialog open.)
            p.spawn((abs(0.0, 0.0, 10.0, 10.0), Visibility::Hidden, CardPart::Detail, Pickable::IGNORE)).with_children(|d| {
                for i in 0..4 {
                    d.spawn((abs(0.0, 0.0, 1.0, 1.0), BackgroundColor(Color::srgba(0.02, 0.04, 0.08, 0.88)), CardPart::Shade(i), Pickable::IGNORE));
                }
            });
            p.spawn((abs(0.0, 0.0, 1.0, 1.0), ImageNode::default(), Visibility::Hidden, CardPart::DetailThumb, Pickable::IGNORE));
        });
        return;
    }
    let list = listed(&pedia.entries, pedia.category, pedia.filter, pedia.sort);
    let per_page = pedia.per_page.max(1);
    let entry_at = |slot: usize| list.get(pedia.page * per_page + slot - 1).map(|i| &pedia.entries[*i]);
    let detail = pedia.detail.and_then(|i| pedia.entries.get(i)).filter(|_| !is_shopper(&fe));
    // The delete confirmation (SPUI, under the UI nodes) hides the cards and
    // the detail view while it is up.
    let modal = pedia.confirm.is_some();
    for (part, mut node, mut vis, image, text, transform) in &mut parts {
        let set_vis = |vis: &mut Visibility, on: bool| {
            let want = if on { Visibility::Inherited } else { Visibility::Hidden };
            if *vis != want {
                *vis = want;
            }
        };
        match *part {
            CardPart::Card(slot) => {
                // The pop-in (see `POP_IN_SECONDS`): the cards start one after another.
                let scale = pop_in_scale(now - pop.started - pop_in_start(slot.saturating_sub(1)));
                let shown = slot < grid.slots() && entry_at(slot).is_some() && detail.is_none() && !modal && scale.is_some();
                set_vis(&mut vis, shown);
                if shown {
                    let r = grid.slot_rect(slot);
                    node.left = Val::Px(r[0]);
                    node.top = Val::Px(r[1]);
                }
                if let (Some(mut transform), Some(scale)) = (transform, scale) {
                    let want = UiTransform::from_scale(Vec2::splat(scale));
                    if *transform != want {
                        *transform = want;
                    }
                }
            }
            CardPart::Glow(slot) => set_vis(&mut vis, pedia.hovered == Some(slot) && detail.is_none()),
            CardPart::Thumb(slot) => {
                let handle = entry_at(slot).and_then(|e| thumbs.image(&e.thumb));
                set_vis(&mut vis, handle.is_some());
                if let (Some(handle), Some(mut image)) = (handle, image) {
                    if image.image != handle {
                        image.image = handle;
                    }
                }
            }
            CardPart::Name(slot) => {
                if let (Some(e), Some(mut text)) = (entry_at(slot), text) {
                    if text.0 != e.name {
                        text.0 = e.name.clone();
                    }
                }
            }
            CardPart::Author(slot) => {
                if let (Some(e), Some(mut text)) = (entry_at(slot), text) {
                    if text.0 != e.author {
                        text.0 = e.author.clone();
                    }
                }
            }
            // The creation's date (the installed creations have none).
            CardPart::Stamp(slot) => {
                if let (Some(e), Some(mut text)) = (entry_at(slot), text) {
                    let stamp = e.created.map(slots::format_date).unwrap_or_default();
                    if text.0 != stamp {
                        text.0 = stamp;
                    }
                }
            }
            CardPart::Detail => {
                set_vis(&mut vis, detail.is_some() && !modal);
                let h = grid.host;
                node.left = Val::Px(h[0]);
                node.top = Val::Px(h[1]);
                node.width = Val::Px(h[2] - h[0]);
                node.height = Val::Px(h[3] - h[1]);
            }
            CardPart::Shade(i) => {
                let h = grid.host;
                let (w, ht) = (h[2] - h[0], h[3] - h[1]);
                // The dialog's rectangle relative to the host.
                let d = pedia.large.map_or([0.0; 4], |l| [l.dialog.x1 - h[0], l.dialog.y1 - h[1], l.dialog.x2 - h[0], l.dialog.y2 - h[1]]);
                let r = match i {
                    0 => [0.0, 0.0, w, d[1]],
                    1 => [0.0, d[3], w, ht],
                    2 => [0.0, d[1], d[0], d[3]],
                    _ => [d[2], d[1], w, d[3]],
                };
                node.left = Val::Px(r[0]);
                node.top = Val::Px(r[1]);
                node.width = Val::Px((r[2] - r[0]).max(0.0));
                node.height = Val::Px((r[3] - r[1]).max(0.0));
            }
            // The card image in the render window until the model renders
            // (and for a creation without a model): square, between the
            // dialog's header and its rotate bar, so over no control.
            CardPart::DetailThumb => {
                let at = pedia.large.filter(|_| detail.is_some() && !modal);
                let model = detail.is_some_and(|e| turntable.image(&e.thumb).is_some());
                let handle = detail.and_then(|e| thumbs.image(&e.thumb));
                set_vis(&mut vis, at.is_some() && handle.is_some() && !model);
                if let Some(at) = at {
                    let r = at.render;
                    node.left = Val::Px(r.x1);
                    node.top = Val::Px(r.y1 + (r.height() - r.width()) * 0.5);
                    node.width = Val::Px(r.width());
                    node.height = Val::Px(r.width());
                }
                if let (Some(handle), Some(mut image)) = (handle, image) {
                    if image.image != handle {
                        image.image = handle;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_names_come_from_the_shopper_titles() {
        assert_eq!(setup_name_of("Choose a Military Land Vehicle"), "Military Land Vehicle");
        assert_eq!(setup_name_of("Choose an Economic Sea Vehicle"), "Economic Sea Vehicle");
        assert_eq!(setup_name_of("Factory"), "Factory");
        // Every title setup has an English fallback of the same shape.
        for (kind, _) in KIND_TITLES {
            assert!(!design_kind(kind).is_empty());
        }
        let mut seen: Vec<u32> = KIND_TITLES.iter().map(|(_, id)| *id).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), KIND_TITLES.len(), "one string per setup");
    }

    fn record(type_id: u32, name: &str) -> crate::creation_records::CreationRecord {
        crate::creation_records::CreationRecord {
            key: crate::dbpf::ResourceKey { group_id: 0, instance_id: crate::world_objects::name_id(name), type_id },
            format_version: 1,
            name: name.into(),
            model_type: 0,
            blocks: Vec::new(),
        }
    }

    fn house_run() -> RunProgress {
        let mut run = RunProgress::default();
        run.creature_creation.name = "Blib".into();
        let mut block = crate::rigid_editor::RigidBlock::default();
        block.position = [0.0, 1.0, 0.0];
        run.rigid.designs.insert(
            "BuildingHouse".into(),
            crate::rigid_editor::RigidDesign {
                name: String::new(),
                parts: vec![crate::game::VehiclePart {
                    instance: 1,
                    definition_id: Some(2),
                    slot: 0,
                    yaw: 0.0,
                    scale: 1.0,
                    offset: [0.0; 3],
                    block: Some(block),
                }],
            },
        );
        run
    }

    #[test]
    fn saved_stored_and_maxis_creations_are_listed_filtered_and_sorted() {
        use crate::creation_records::{CreationCatalog, BLD, UFO};
        let catalog = CreationCatalog {
            creations: vec![record(BLD, "Hut"), record(UFO, "Saucer"), record(BLD, "Arena"), record(BLD, "PaintSwatch01")],
            errors: Vec::new(),
        };
        let saves = vec![SavedGame { path: "x.json".into(), planet: "Gorgonia".into(), modified: None, run: house_run() }];
        let mut pedia_store = store::Store::default();
        let mut made = RunProgress::default();
        made.creature_creation.name = "Zork".into();
        pedia_store.put("Zork".into(), store::StoredKind::Creature(store::creature_of(&made)), None);
        let installed = vec![maxis::InstalledCreation {
            category: 0,
            name: "Narl".into(),
            author: "Maxis".into(),
            description: String::new(),
            tags: Vec::new(),
            png: None,
            key: (maxis::CRT, 0x4062_6200, 7),
            order: 1,
        }];
        let entries = gather(&Sources { saves: &saves, catalog: &catalog, store: &pedia_store, maxis: &installed, mine: "My Creations" });
        let names = |c: usize, f: Filter, s: Sort| listed(&entries, c, f, s).iter().map(|i| entries[*i].name.clone()).collect::<Vec<_>>();
        assert_eq!(names(0, Filter::Everything, Sort::Title), vec!["Blib", "Narl", "Zork"]);
        assert_eq!(names(0, Filter::Mine, Sort::Title), vec!["Blib", "Zork"]);
        assert_eq!(names(0, Filter::Maxis, Sort::Title), vec!["Narl"]);
        // Newest first: the stored creation has a date, the save none here.
        assert_eq!(names(0, Filter::Everything, Sort::TimeCreated)[0], "Zork");
        assert_eq!(entries[0].author, "Gorgonia");
        assert!(entries[0].thumb.starts_with("c-"));
        assert_eq!(names(1, Filter::Everything, Sort::Title), vec!["Arena", "House", "Hut"]);
        assert_eq!(names(1, Filter::Everything, Sort::Creator), vec!["House", "Arena", "Hut"]);
        assert_eq!(names(1, Filter::Everything, Sort::TimeCreated), vec!["House", "Hut", "Arena"]);
        assert_eq!(names(3, Filter::Everything, Sort::Type), vec!["Saucer"]);
        // Vehicles lists its `ufo` child's creations too (the filter tree).
        assert_eq!(names(2, Filter::Everything, Sort::Title), vec!["Saucer"]);
        assert_eq!(entries.iter().find(|e| e.name == "Hut").map(|e| e.author.as_str()), Some("Maxis"));
        let zork = entries.iter().find(|e| e.name == "Zork").unwrap();
        assert_eq!((zork.author.as_str(), zork.thumb.starts_with("s-")), ("My Creations", true));
        let narl = entries.iter().find(|e| e.name == "Narl").unwrap();
        assert!(narl.thumb.starts_with("p-") && narl.is_maxis());
    }

    #[test]
    fn detail_verbs_follow_the_origin() {
        let entry = |origin: Origin, category: usize| Entry {
            category,
            name: "x".into(),
            author: String::new(),
            kind: String::new(),
            description: String::new(),
            tags: Vec::new(),
            created: None,
            order: 0,
            parts: 0,
            source: thumbs::Source::Png(NO_KEY),
            thumb: String::new(),
            origin,
        };
        // Edit, Delete, Outfitter.
        assert_eq!(verbs(&entry(Origin::Stored(1), 0), true), [true, true, true]);
        assert_eq!(verbs(&entry(Origin::Stored(1), 1), true), [true, true, false]);
        assert_eq!(verbs(&entry(Origin::Game { path: "x".into(), setup: None }, 0), true), [true, false, true]);
        assert_eq!(verbs(&entry(Origin::Maxis, 0), true), [false, false, false]);
        assert_eq!(verbs(&entry(Origin::Stored(1), 0), false), [false, true, false], "in a stage only Delete");
    }

    #[test]
    fn edit_opens_the_matching_creator_on_a_copy() {
        let mut pedia_store = store::Store::default();
        let run = house_run();
        let design = run.rigid.designs["BuildingHouse"].clone();
        let id = pedia_store.put("Hut".into(), store::StoredKind::Design { setup: "BuildingHouse".into(), design }, None);
        let mut dressed = store::creature_of(&run);
        dressed.outfitter = SetupKind::Space.setup_name().into();
        let dressed_id = pedia_store.put("Spacer".into(), store::StoredKind::Creature(dressed), None);
        let entries = gather(&Sources {
            saves: &[],
            catalog: &Default::default(),
            store: &pedia_store,
            maxis: &[],
            mine: "Me",
        });
        let hut = entries.iter().find(|e| e.origin == Origin::Stored(id)).unwrap();
        let launch = edit_launch(hut, &pedia_store).expect("editable");
        assert_eq!((launch.kind, launch.replace), (SetupKind::BuildingHouse, Some(id)));
        assert!(launch.scratch.rigid.designs.contains_key("BuildingHouse"));
        let spacer = entries.iter().find(|e| e.origin == Origin::Stored(dressed_id)).unwrap();
        assert_eq!(edit_launch(spacer, &pedia_store).map(|l| l.kind), Some(SetupKind::Space), "Edit Outfit");
        let catalog = crate::asset_catalog::PartCatalog::default();
        let outfit = outfit_launch(spacer, SetupKind::Tribal, &pedia_store, &catalog).expect("a creature");
        assert_eq!(outfit.kind, SetupKind::Tribal);
        assert!(outfit.scratch.spice > 0.0, "the outfitter's allowance");
        assert!(outfit_launch(hut, SetupKind::Tribal, &pedia_store, &catalog).is_none());
    }

    #[test]
    fn outfitter_verb_follows_the_outfit_and_shopper_titles_ask_to_create() {
        let mut pedia_store = store::Store::default();
        let run = house_run();
        let mut dressed = store::creature_of(&run);
        dressed.outfitter = SetupKind::Civ.setup_name().into();
        let civ = pedia_store.put("Civvy".into(), store::StoredKind::Creature(dressed), None);
        let plain = pedia_store.put("Plain".into(), store::StoredKind::Creature(store::creature_of(&run)), None);
        let entries = gather(&Sources { saves: &[], catalog: &Default::default(), store: &pedia_store, maxis: &[], mine: "Me" });
        let of = |id| entries.iter().find(|e| e.origin == Origin::Stored(id)).unwrap();
        assert_eq!(outfitter_for(of(civ), &pedia_store), SetupKind::Civ);
        assert_eq!(outfitter_for(of(plain), &pedia_store), SetupKind::Tribal);
        let catalog = crate::asset_catalog::PartCatalog::default();
        assert_eq!(outfit_stored(plain, SetupKind::Space, &pedia_store, &catalog).map(|l| l.kind), Some(SetupKind::Space));
        assert_eq!(shopper_title(SetupKind::Space).1, "Create or Choose a Space Creature");
        assert_eq!(CATEGORIES.len(), CATEGORY_IDS.len());
    }

    #[test]
    fn the_type_rows_follow_the_retail_order_with_spaceships_under_vehicles() {
        // Cells, Creatures, Buildings, Vehicles, Flora; Spaceships open under Vehicles.
        assert_eq!(visible_rows(CREATURES), vec![CELLS, CREATURES, 1, VEHICLES, FLORA]);
        for selected in [VEHICLES, SPACESHIPS] {
            assert_eq!(visible_rows(selected), vec![CELLS, CREATURES, 1, VEHICLES, SPACESHIPS, FLORA]);
        }
        // The Vehicles filter lists its `ufo` child's assets, the child row only its own.
        assert!(in_category(SPACESHIPS, VEHICLES) && in_category(VEHICLES, VEHICLES));
        assert!(!in_category(VEHICLES, SPACESHIPS) && !in_category(CREATURES, VEHICLES));
        let entry = |category: usize| Entry {
            category,
            name: "x".into(),
            author: String::new(),
            kind: String::new(),
            description: String::new(),
            tags: Vec::new(),
            created: None,
            order: 0,
            parts: 0,
            source: thumbs::Source::Png(NO_KEY),
            thumb: String::new(),
            origin: Origin::Maxis,
        };
        let entries = [entry(VEHICLES), entry(SPACESHIPS), entry(CREATURES)];
        assert_eq!(listed(&entries, VEHICLES, Filter::Everything, Sort::Title).len(), 2);
        assert_eq!(listed(&entries, SPACESHIPS, Filter::Everything, Sort::Title).len(), 1);
    }

    #[test]
    fn the_status_line_counts_the_listed_creations() {
        assert_eq!(status_line("Displaying ~num_items~ items", 759), "Displaying 759 items");
        assert_eq!(status_line("Displaying ~num_items~ items", 0), "Displaying 0 items");
    }

    /// The retail strings behind the status line and the theme drop-down, and the
    /// grid's status window (needs the game install).
    #[test]
    #[ignore = "reads the installed packages"]
    fn the_installed_status_line_and_theme_strings_exist() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut assets = SpuiAssets::default();
        let line = assets.localized(&index, FEED_TABLE, STATUS_LINE).expect("21671745:00030003");
        assert!(line.contains("~num_items~"), "{line}");
        assert_eq!(assets.localized(&index, crate::front_end::GGE_TABLE, 0x05B0_8761).as_deref(), Some("No Theme"));
        let grid = assets.layout(&index, "AssetBrowserAssetGrid").expect("AssetBrowserAssetGrid");
        assert!(grid.find(id::FOOTER).is_some(), "the status window");
    }

    #[test]
    fn a_creature_thumbnail_is_named_by_its_content() {
        let a = RunProgress::default();
        let mut b = RunProgress::default();
        assert_eq!(creature_thumb_name(&a), creature_thumb_name(&b), "the galaxy picture and the card share the name");
        assert!(creature_thumb_name(&a).starts_with("c-"));
        b.body.spine[0].radius += 0.1;
        assert_ne!(creature_thumb_name(&a), creature_thumb_name(&b));
    }

    /// The installed `AssetViewLarge` layout has the windows the dialog binds
    /// (needs the game install).
    #[test]
    #[ignore = "reads the installed packages"]
    fn the_installed_dialog_layout_has_its_windows() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut assets = SpuiAssets::default();
        let layout = assets.layout(&index, LARGE_LAYOUT).expect("AssetViewLarge");
        for control in [
            large::PANEL,
            large::RENDER,
            large::VERB_COLUMN,
            large::CLOSE,
            large::NAME,
            large::AUTHOR,
            large::DESCRIPTION,
            large::TAGS,
            large::ROTATE_LEFT,
            large::ROTATE_RIGHT,
            large::ZOOM_IN,
            large::ZOOM_OUT,
            large::ONLINE_AUTHOR,
            large::ONLINE_RATING,
            large::COMMENTS_MESSAGE,
            large::BUSY,
            large::BUSY_TEXT,
            large::BUSY_ICON,
        ] {
            assert!(layout.find(control).is_some(), "window {control:08X}");
        }
        // The solved rectangles in the 800 x 600 dialog: the render window
        // and its panel share one (40,24 .. 479,583), and the frame's corner
        // pieces give the dialog's extent used for the backdrop and the
        // outside click.
        let rects = crate::spui::solve::solve(&layout, &crate::spui::solve::authored_areas(&layout), [800.0, 600.0], RootPolicy::Authored);
        let rect = |c: u32| rects[layout.find(c).unwrap()].to_array().map(f32::round);
        assert_eq!(rect(large::RENDER), [40.0, 24.0, 479.0, 583.0]);
        assert_eq!(rect(large::PANEL), rect(large::RENDER));
        let element = |e: usize| layout.windows.iter().find(|w| w.element == e).map(|w| rects[w.id].to_array().map(f32::round));
        assert_eq!(element(18).map(|r| (r[0], r[1])), Some((34.0, 17.0)));
        assert_eq!(element(26).map(|r| (r[2], r[3])), Some((762.0, 586.0)));
        assert_eq!(element(381).map(|r| r[2] - r[0]), Some(439.0));
    }

    #[test]
    fn plants_and_cells_have_their_own_types() {
        let installed = vec![
            maxis::InstalledCreation { category: 4, name: "Fern".into(), author: "Maxis".into(), description: String::new(), tags: Vec::new(), png: None, key: (maxis::FLR, 0x4066_6200, 1), order: 1 },
            maxis::InstalledCreation { category: 5, name: "Blob".into(), author: "Maxis".into(), description: String::new(), tags: Vec::new(), png: None, key: (maxis::CLL, 0x4061_6200, 2), order: 2 },
        ];
        let entries = maxis_entries(&installed, 0);
        assert_eq!(listed(&entries, 4, Filter::Maxis, Sort::Title).len(), 1);
        assert_eq!(entries[1].kind, "Cell");
        assert_eq!(entries[0].kind, "Flora");
    }

    #[test]
    fn cards_fill_the_grid_after_create_your_own() {
        let g = card_grid([180.0, 106.0, 1278.0, 874.0], [181.0, 135.0]);
        assert_eq!((g.columns, g.rows), (8, 3));
        assert_eq!(g.slot_rect(0), [181.0, 135.0, 304.0, 309.0]);
        assert_eq!(g.slot_rect(9)[0], 181.0 + 135.0);
        assert!(g.slot_rect(g.slots() - 1)[3] <= 874.0);
    }
}
