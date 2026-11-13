//! The new-game flow (`GGENewGameFlowUIv1-flip`, the `NewGameFlowLayout` of the
//! `gge` tuning). Phase 1 rings the picked star with the stage wheel ("Choose
//! A Stage": the five stage buttons, the hovered stage's card); phase 2 walks
//! the setup panels of the chosen stage one at a time (Forward / Back) in the
//! order of the tuning's step list (diet, creature, theme, difficulty,
//! specialty, assets, name) and starts the game with the existing
//! `RunProgress` creation.
//!
//! VERIFIED (march2017 image, `notesront-end-newgame-notes.md`): the flow
//! object keeps one step list (0xDF4780 builds it from the tuning's string
//! array through the name -> mode table level 1, diet 2, creature 3, name 4,
//! difficulty 5, theme 6, specialty 7, assets 8, assets and creature 9,
//! captain name 0xA, start 0xB); the panel switch 0xDF0640 -> 0xDF0250 hides
//! every panel and shows the one of the mode, so retail shows one step at a
//! time; Forward (0xDF2520) goes to the next listed mode the stage's own mode
//! set accepts (0xDEEE40: the theme step stays hidden until the Creature stage
//! is unlocked), Back (0xDF2630) to the previous one or the stage wheel; the
//! last step shows no Forward, its panel carries Go. A new flow starts on Easy
//! (0xDF4F4C -> 0xDF28F0) with Military selected (0xDF2740).
//!
//! The per-stage mode sets (VERIFIED, 0xDF10A0 / 0xDF3760): the six stage
//! records of the flow object (+ 0x84, 0xC0 bytes each, indexed by the game
//! mode 0 Cell, 1 Creature, 2 Tribal, 4 Civilization, 5 Space) are loaded from the
//! property files `GGENewCellGame` .. `GGENewSpaceGame` (the `-ep1` copies when
//! Galactic Adventures is installed, 0x6854E0(2); not installed here): one bool per
//! mode (`ChooseName`, `ChooseDifficulty`, `ChooseTheme`, `ChooseSpecialty`,
//! `ChooseAssets`, `ChooseCreature`, `ChooseDiet`, ...), the creature / vehicle /
//! building asset types (`CreatureAssetType`, `VehicleAssetType`,
//! `BuildingAssetType`) and the card windows of the stage (see [`STAGE_MODES`]).
//! The theme step belongs to every record but stays hidden until the Creature stage
//! is unlocked (0xDEEE40). The creature and assets steps open Sporepedia as the
//! shopper of their card (a click on a card, 0xDF3CD2; `pedia`'s `flow_pick`), and
//! the chosen creature / city hall / vehicle / UFO start the stage.
//!
//! The theme step (VERIFIED, decoded in `fe-final`): it is NOT a list of terrain
//! scripts and has no effect on the home planet. 0xDED560 fills the drop-down
//! `0684BBE8` with "No Theme" (GGE 05B08761) followed by the theme records of the
//! Sporepedia-side manager at 0x15FCC70 (0x70-byte records added by message
//! 0x3CDD5F9); 0xDEFDA0 stores the chosen record's id at params + 0xB0 and shows,
//! in the ten icon windows `057F74F6 .. FF` (`NumThemeIcons` 10), the first ten
//! assets that the object template DB returns for "asset type 0x366A930D
//! (adventure) and property 0x3CC89B1 = theme id" ("Sample Assets"); with no
//! theme the icons stay hidden and the tile placeholders `05BEB40D` show. The
//! asset type is the adventure scenario (INFERRED from the name `adv` of 0x366A930D
//! in `reg_type.txt`: Galactic Adventures content): no installed package holds a
//! resource of that type, so the retail panel with this install is the drop-down
//! reading "No Theme" over the ten empty tiles, which is what OpenSpore shows.
//! The home planet is the star's (`slots`), not a theme.
//!
//! The step dots (VERIFIED, fe-rows2, 0xDF3170 / 0xDF10A0 / 0xDF0500 / 0xDECF30): five
//! dots always show (every stage record enables at least five modes); the dot a panel
//! lights is the one the record loader gave its mode, in the loader's order name,
//! difficulty, theme, specialty, assets, creature, diet (`dot_of`), not the panel's
//! place in the walk. The city hall and vehicle shoppers (VERIFIED: the
//! shopper configs of group 9A74416B) list only the class of their root filter, whose
//! `browserFilterData` is the creation's `modeltype` (`Shop::model_type`; the installed
//! creations' class is read from their creation file, `maxis::model_type_of`).
//!
//! ADAPTED: the default creature and assets are random installed ones (stored creatures
//! of the player's first). Retail's are the object template DB's first answer for the
//! record's asset type (its order is not derivable) or, for the Cell record, a random
//! `startingcellModels` entry (0xDF2B80); `NewGameArchetypeFilter` (required graspers /
//! feet / model types) is not applied to the creature shopper.

use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};

use super::{gge::GgeRoot, maxis, pedia, slots, store, FrontEnd, FrontEndActive, GameSetup, Overlay};
use crate::{
    creature_editor_data::SetupKind,
    dbpf::ResourceKey,
    game::RunProgress,
    rigid_editor::RigidDesign,
    spui::{
        model::{ImageRef, WinId},
        view::{SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

pub const LAYOUT: &str = "GGENewGameFlowUIv1-flip";

pub mod id {
    pub const ROOT: u32 = 0x0538_4881;
    pub const PICK_TEXT: u32 = 0x05BD_7603;
    pub const STAGE_WHEEL: u32 = 0x0538_5D73;
    /// The stage buttons' group (hidden in the file; cascades in).
    pub const STAGE_BUTTONS: u32 = 0x065F_F7E8;
    pub const DEV_OVERRIDE: u32 = 0x4EC1_B8D8;
    /// Stage buttons Cell .. Space.
    pub const STAGES: [u32; 5] = [0x053F_06E1, 0x053F_06EE, 0x053F_06F5, 0x053F_06FC, 0x053F_0707];
    /// Stage cards (title + description) Cell .. Space.
    pub const CARDS: [u32; 5] = [0x05B2_18D0, 0x05B2_18C8, 0x05AD_F8E1, 0x05AD_F8E0, 0x05B2_18D8];
    /// Stage pictures of the wheel, Cell .. Space (INFERRED order).
    pub const PICTURES: [u32; 5] = [0x0594_1BB0, 0x0594_1BB1, 0x0594_1BB2, 0x0594_1BB3, 0x0594_1BB4];
    pub const FORWARD: u32 = 0x0538_4D05;
    /// The Forward button's group with its glow (hidden on the last step, 0xDF25E3).
    pub const FORWARD_GROUP: u32 = 0x0594_4089;
    pub const BACK: u32 = 0x0593_3FE0;
    pub const CANCEL: u32 = 0x0595_E0A8;
    pub const GO: u32 = 0x0595_1103;
    pub const MENU_PANEL: u32 = 0x0594_2B08;
    pub const NUMBERS_PANEL: u32 = 0x0594_4300;
    pub const STEP_TOGGLES: u32 = 0x05B1_BA99;
    // Panels
    pub const DIET: u32 = 0x0677_5B6A;
    pub const DIET2: u32 = 0x0677_5EBB;
    pub const DIET2_BUTTONS: [u32; 2] = [0x0677_5E1F, 0x0677_5E20];
    pub const DIET3: u32 = 0x0677_5EBC;
    /// Herbivore, carnivore, omnivore.
    pub const DIET3_BUTTONS: [u32; 3] = [0x0677_5E22, 0x0677_5E23, 0x0677_5E21];
    pub const DIET_TEXTS: u32 = 0x0669_F710;
    pub const CREATURE: u32 = 0x0661_08E1;
    /// The creature card (a button), the picture window inside it, the verbs
    /// shown over it (Edit / Create New / Load) and Randomize.
    pub const CREATURE_CARD: u32 = 0x0540_45CE;
    pub const CREATURE_PICTURE: u32 = 0x05B7_44BC;
    pub const CREATURE_VERBS: u32 = 0x05B6_9A71;
    pub const CREATURE_RANDOM: u32 = 0x0645_1451;
    pub const THEME: u32 = 0x0538_5D5B;
    /// The theme drop-down (its entries are set by code, 0xDED560).
    pub const THEME_COMBO: u32 = 0x0684_BBE8;
    pub const DIFFICULTY: u32 = 0x0614_8F13;
    pub const DIFFICULTY_BUTTONS: [u32; 3] = [0x0614_9007, 0x0614_9008, 0x0614_9009];
    pub const SPECIALTY: u32 = 0x0538_5DC7;
    pub const SPECIALTY_BUTTONS: [u32; 3] = [0x0614_C680, 0x0614_C681, 0x0614_C682];
    pub const COLOR_PICKER: u32 = 0x05F0_FDBD;
    pub const ASSETS: u32 = 0x0594_5038;
    /// The assets panel's title text (the stage's own string, `GGENew<Stage>Game`).
    pub const ASSETS_TITLE: u32 = 0x05AF_2B91;
    /// The vehicle card (left) and the building card (right) of the assets panel,
    /// the picture windows inside them and their Randomize buttons.
    pub const ASSET_VEHICLE_CARD: u32 = 0x0540_45E3;
    pub const ASSET_BUILDING_CARD: u32 = 0x0540_4617;
    pub const ASSET_VEHICLE_PICTURE: u32 = 0x05B7_45CC;
    pub const ASSET_BUILDING_PICTURE: u32 = 0x05B7_3F5B;
    pub const ASSET_VEHICLE_RANDOM: u32 = 0x0645_1450;
    pub const ASSET_BUILDING_RANDOM: u32 = 0x0594_34C1;
    /// Lock toggles, verb groups (Edit / Load / Create New) and the per-stage card
    /// backs of the assets panel: not used here.
    pub const ASSET_HIDDEN: [u32; 14] = [
        0x0594_61B0, 0x0594_61B1, 0x0594_61B2, 0x05B6_9A70, 0x05B6_9A72, 0x062E_BD99, 0x062D_89EA, 0x162D_838F,
        0x062D_838F, 0x062D_838E, 0x062D_838D, 0x062D_838C, 0x062D_838B, 0x062D_838A,
    ];
    /// The five step toggles ("dots") of the group `STEP_TOGGLES`.
    pub const DOTS: [u32; 5] = [0x0538_5002, 0x0538_4EEC, 0x0538_4FBB, 0x053F_00C7, 0x0614_92B9];
    /// The icon windows over the cards' pictures (the stage's silhouette / glyph windows
    /// of the creature card, and the vehicle and building cards' own): hidden while a
    /// creation's picture shows.
    pub const CREATURE_ICONS: [u32; 6] = [0x05A5_CF59, 0x0645_1155, 0x0645_1156, 0x0645_1157, 0x0645_1150, 0x0627_49B8];
    pub const VEHICLE_ICONS: [u32; 7] = [0x0645_1151, 0x0645_1152, 0x0645_1153, 0x0645_1154, 0x0540_45E4, 0x0669_F7B0, 0x05A5_CF6E];
    pub const BUILDING_ICONS: [u32; 3] = [0x0540_4618, 0x0669_F798, 0x05A5_CF80];
    pub const NAME: u32 = 0x0538_5E09;
    pub const NAME_EDIT: u32 = 0x0540_0FA5;
    pub const NAME_RANDOM: u32 = 0x05A7_25B4;
    pub const NAME_TAKEN: u32 = 0x05F0_FCA5;
    pub const NAME_TITLE: u32 = 0x0677_7671;
}

/// One setup panel of the flow, in the order of the `gge` step list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Diet,
    Creature,
    Theme,
    Difficulty,
    Specialty,
    Assets,
    Name,
}

impl Step {
    /// The flow mode (0xDF4780's name table; mode 1 is the stage wheel).
    pub const fn mode(self) -> u32 {
        match self {
            Step::Diet => 2,
            Step::Creature => 3,
            Step::Name => 4,
            Step::Difficulty => 5,
            Step::Theme => 6,
            Step::Specialty => 7,
            Step::Assets => 8,
        }
    }

    /// The panel the mode shows (the jump table of 0xDF0640).
    pub const fn panel(self) -> u32 {
        match self {
            Step::Diet => id::DIET,
            Step::Creature => id::CREATURE,
            Step::Name => id::NAME,
            Step::Difficulty => id::DIFFICULTY,
            Step::Theme => id::THEME,
            Step::Specialty => id::SPECIALTY,
            Step::Assets => id::ASSETS,
        }
    }
}

/// `GGENewGameFlowUIv1-flip` of the `gge` tuning (VERIFIED order).
pub const ORDER: [Step; 7] =
    [Step::Diet, Step::Creature, Step::Theme, Step::Difficulty, Step::Specialty, Step::Assets, Step::Name];

/// `MaxPlanetNameLength`.
pub const MAX_PLANET_NAME: usize = 32;

/// The modes a start stage's record accepts: the bools of its property file
/// `GGENew<Stage>Game` (VERIFIED, installed data read with the property ids of
/// the record loader 0xDF10A0; the `-ep1` copies are the Galactic Adventures
/// variants, not installed). `ChooseCreature` is absent from the Cell file (the
/// loader keeps the previous bool, `ChooseAssets`: off).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modes {
    pub diet: bool,
    pub creature: bool,
    pub theme: bool,
    pub difficulty: bool,
    pub specialty: bool,
    pub assets: bool,
    pub name: bool,
}

/// Cell, Creature, Tribal, Civilization, Space (`GGENewCellGame`,
/// `GGENewCreatureGame`, `GGENewTribeGame`, `GGENewCivGame`, `GGENewSpaceGame`).
pub const STAGE_MODES: [Modes; 5] = [
    Modes { diet: true, creature: false, theme: true, difficulty: true, specialty: false, assets: false, name: true },
    Modes { diet: true, creature: false, theme: true, difficulty: true, specialty: false, assets: false, name: true },
    Modes { diet: true, creature: false, theme: true, difficulty: true, specialty: false, assets: false, name: true },
    Modes { diet: false, creature: true, theme: true, difficulty: true, specialty: true, assets: true, name: true },
    Modes { diet: false, creature: true, theme: true, difficulty: true, specialty: false, assets: true, name: true },
];

/// The panels a game started at `stage` (0 Cell .. 4 Space) walks through, in
/// the order of the tuning's list: the modes of the stage's record
/// ([`STAGE_MODES`]) in the order of `GGENewGameFlowUIv1-flip` (0xDF2520 skips the
/// listed modes the record lacks, 0xDEEE40). The theme step is hidden until the
/// Creature stage is unlocked (`HideThemeUntilCreatureGameUnlocked`,
/// `ALG-UnlockCreature` or the `-unlock` cheat): `theme_open`.
pub fn steps(stage: usize, theme_open: bool) -> Vec<Step> {
    let modes = STAGE_MODES[stage.min(4)];
    ORDER
        .into_iter()
        .filter(|s| match s {
            Step::Diet => modes.diet,
            Step::Creature => modes.creature,
            Step::Theme => modes.theme && theme_open,
            Step::Difficulty => modes.difficulty,
            Step::Specialty => modes.specialty,
            Step::Assets => modes.assets,
            Step::Name => modes.name,
        })
        .collect()
}

/// The dot (index into [`id::DOTS`]) the panel of `step` lights (VERIFIED, the
/// record loader 0xDF10A0 and the panel switch 0xDF0500 / 0xDECF30): the loader walks
/// the modes in the order name, difficulty, theme, specialty, assets, creature,
/// diet, creature-and-assets-together (every record enables the last one) and gives each enabled
/// mode the next entry n of the dot table 0x147E428 (`05385002 05384EEC 05384FBB
/// 053F00C7 061492B9 061492B9`, then other windows); the panel switch selects the dot of the
/// shown mode. So the dots follow the loader's order, not the order of the
/// panels (a Cell game's Diet panel lights the fourth dot, its Name panel the first), the
/// sixth entry repeats the fifth dot, and a mode past the table (a Civilization record's
/// seventh) lights no dot.
pub fn dot_of(stage: usize, step: Step) -> Option<usize> {
    let m = STAGE_MODES[stage.min(4)];
    let loader = [
        (Step::Name, m.name),
        (Step::Difficulty, m.difficulty),
        (Step::Theme, m.theme),
        (Step::Specialty, m.specialty),
        (Step::Assets, m.assets),
        (Step::Creature, m.creature),
        (Step::Diet, m.diet),
    ];
    let mut n = 0;
    for (s, on) in loader {
        if on {
            if s == step {
                return (n <= 5).then_some(n.min(id::DOTS.len() - 1));
            }
            n += 1;
        }
    }
    None
}

/// The specialty value retail stores (`GameLoadParameters + 0x90`, 0xDF2740: the
/// radio buttons 0x0614C680 / 81 / 82 = Military / Economic / Religious map to
/// 0 / 2 / 1; `VehiclePurpose`: kVehicleMilitary 0, kVehicleCultural 1,
/// kVehicleEconomic 2).
pub const fn retail_specialty(button: u8) -> u8 {
    match button {
        0 => 0,
        1 => 2,
        _ => 1,
    }
}

/// The radio button of a stored specialty (the inverse of [`retail_specialty`]).
pub const fn specialty_button(retail: u8) -> u8 {
    match retail {
        0 => 0,
        2 => 1,
        _ => 2,
    }
}

/// A card of the flow whose click opens a Sporepedia shopper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    Creature,
    Building,
    Vehicle,
}

/// Sporepedia opened as the shopper of one of the flow's cards
/// (`FrontEnd::flow_pick`): it lists the card's type and a click on a card hands the
/// creation back (`FrontEnd::picked`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shop {
    pub card: Card,
    /// The start stage (0 Cell .. 4 Space).
    pub stage: usize,
    /// The specialty chosen on the specialty panel (retail's value: 0 military, 1 cultural /
    /// religious, 2 economic) for a Civilization vehicle.
    pub specialty: u8,
    /// The title (string id of the shoppers' locale table, English text).
    pub title: (u32, &'static str),
}

impl Shop {
    /// Index into `pedia::CATEGORIES`: creatures, buildings, vehicles (a Space start
    /// picks a UFO: spaceships).
    pub fn category(&self) -> usize {
        match self.card {
            Card::Creature => 0,
            Card::Building => 1,
            Card::Vehicle if self.stage >= 4 => 3,
            Card::Vehicle => 2,
        }
    }

    /// The class the shopper's filter lists (VERIFIED: the shopper configs of group
    /// 9A74416B name the root filter `CityHall` / `MilitaryLand` / `EconomicLand` /
    /// `CulturalLand` / `ufo`, whose `browserFilterData` is the creation's `modeltype`:
    /// `BuildingCityHall`, `VehicleMilitaryLand`, `VehicleEconomicLand`,
    /// `VehicleCulturalLand`, `VehicleUFO`). `None`: every creation of the shopper's type
    /// (creatures; the UFO filter lists every UFO).
    pub fn model_type(&self) -> Option<&'static str> {
        match self.card {
            Card::Creature => None,
            Card::Building => Some("BuildingCityHall"),
            Card::Vehicle if self.stage >= 4 => None,
            Card::Vehicle => Some(match self.specialty {
                1 => "VehicleCulturalLand",
                2 => "VehicleEconomicLand",
                _ => "VehicleMilitaryLand",
            }),
        }
    }

    /// The creations the shopper lists: its type, and of that the class of its filter. The
    /// installed creations carry their class (`modeltype`) in their creation file
    /// (`maxis::model_type_of`); the player's designs in the setup they were made in (a
    /// generic template design passes: OpenSpore's creators open the classes directly).
    pub fn accepts(&self, entry: &pedia::Entry, store: &store::Store) -> bool {
        if entry.category != self.category() {
            return false;
        }
        let Some(wanted) = self.model_type() else { return true };
        let by_setup = |setup: &str| setup == wanted || matches!(setup, "BuildingTemplate" | "VehicleTemplate");
        match &entry.origin {
            pedia::Origin::Maxis => match entry.source {
                super::thumbs::Source::Png(png) => {
                    super::maxis::model_type_of(png).is_none_or(|t| t == crate::world_objects::name_id(wanted))
                }
                _ => true,
            },
            pedia::Origin::Stored(id) => match store.get(*id).map(|c| &c.kind) {
                Some(store::StoredKind::Design { setup, .. }) => by_setup(setup),
                _ => true,
            },
            pedia::Origin::Game { setup: Some(setup), .. } => by_setup(setup),
            pedia::Origin::Game { .. } => true,
        }
    }

    /// An asset already picked still belongs to this shopper (its class is the filter's).
    pub fn wants_kind(&self, kind: SetupKind) -> bool {
        self.model_type().is_none_or(|wanted| kind.setup_name() == wanted || matches!(kind, SetupKind::Building | SetupKind::Vehicle))
    }

    /// The shopper of `card` for a start at `stage` with the specialty button chosen
    /// on the specialty panel: the creature shopper titles of the stage
    /// (`CRG` / `TRG` / `CVG` / `SPG_CreatureShopper`), "Choose a City Hall", the
    /// military / economic / religious land vehicle or "Choose a Spaceship".
    pub fn of(card: Card, stage: usize, specialty: Option<u8>) -> Self {
        let title = match card {
            Card::Creature => pedia::shopper_title(match stage {
                2 => SetupKind::Tribal,
                3 => SetupKind::Civ,
                4 => SetupKind::Space,
                _ => SetupKind::Creature,
            }),
            Card::Building => (0x0004_0003, "Choose a City Hall"),
            Card::Vehicle if stage >= 4 => (0x0004_0010, "Choose a Spaceship"),
            Card::Vehicle => match specialty.unwrap_or(0) {
                1 => (0x0004_0005, "Choose a Religious Land Vehicle"),
                2 => (0x0004_0008, "Choose an Economic Land Vehicle"),
                _ => (0x0004_000E, "Choose a Military Land Vehicle"),
            },
        };
        Self { card, stage, specialty: specialty.unwrap_or(0), title }
    }
}

/// A building, vehicle or UFO picked on the assets panel, already converted to the
/// rigid editors' design that starts the stage.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    /// The card picture (an installed creation's PNG).
    pub png: Option<ResourceKey>,
    pub kind: SetupKind,
    pub design: RigidDesign,
}

impl Asset {
    /// The design of a Sporepedia entry (the Edit verb's conversion: the player's
    /// creations from their record, the installed ones from their creation file).
    pub fn of(entry: &pedia::Entry, store: &store::Store, originals: &crate::Originals) -> Option<Self> {
        let launch = pedia::edit_launch(entry, store).or_else(|| crate::maxis_creation::edit_launch(entry, originals))?;
        let design = launch.scratch.rigid.designs.get(launch.kind.setup_name())?.clone();
        let png = match entry.source {
            super::thumbs::Source::Png(k) if k.instance_id != 0 => Some(k),
            _ => None,
        };
        Some(Self { name: entry.name.clone(), png, kind: launch.kind, design })
    }
}

/// A creature offered by the creature panel ("Choose or Create Your Creature").
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    /// An installed Maxis creature: its card PNG and name.
    Maxis { png: ResourceKey, name: String },
    /// A creature of the player's Sporepedia store.
    Stored { name: String, creature: Box<store::StoredCreature> },
    /// The creature of a saved game (the shopper's creations of the player's games).
    Game { path: std::path::PathBuf, name: String },
}

impl Pick {
    pub fn name(&self) -> &str {
        match self {
            Pick::Maxis { name, .. } | Pick::Stored { name, .. } | Pick::Game { name, .. } => name,
        }
    }

    /// The creature a shopper's entry stands for (`None`: not a creature).
    pub fn of(entry: &pedia::Entry, store: &store::Store) -> Option<Self> {
        if entry.category != 0 {
            return None;
        }
        match &entry.origin {
            pedia::Origin::Stored(id) => match &store.get(*id)?.kind {
                store::StoredKind::Creature(c) => Some(Pick::Stored { name: entry.name.clone(), creature: Box::new(c.clone()) }),
                _ => None,
            },
            pedia::Origin::Game { path, setup: None } => Some(Pick::Game { path: path.clone(), name: entry.name.clone() }),
            pedia::Origin::Game { .. } => None,
            pedia::Origin::Maxis => match entry.source {
                super::thumbs::Source::Png(png) if png.instance_id != 0 => Some(Pick::Maxis { png, name: entry.name.clone() }),
                _ => None,
            },
        }
    }
}

/// The creatures the creature panel can offer: the player's store first, then
/// the installed Maxis creatures (the Sporepedia feeds "My Creations" and
/// "Maxis", `AssetBrowserFeedFilter` strings 00010002 / 00010003).
#[derive(Resource, Default)]
pub struct Shoppers {
    catalog: maxis::Catalog,
    maxis: Vec<Pick>,
    /// The installed buildings, vehicles and UFOs (the assets panel's defaults and
    /// Randomize).
    assets: Vec<pedia::Entry>,
    listed: bool,
}

impl Shoppers {
    /// Poll the installed catalog (built on a worker thread) once.
    fn poll(&mut self, index: &crate::asset_catalog::ResourceIndex) {
        if self.listed || index.records.is_empty() {
            return;
        }
        if let Some(list) = self.catalog.poll(index) {
            self.assets = pedia::maxis_entries(&list, 0)
                .into_iter()
                .filter(|e| (1..=3).contains(&e.category) && matches!(e.source, super::thumbs::Source::Png(k) if k.instance_id != 0))
                .collect();
            self.maxis = list
                .into_iter()
                .filter(|c| c.category == 0)
                .filter_map(|c| c.png.map(|png| Pick::Maxis { png, name: c.name }))
                .collect();
            self.listed = true;
        }
    }

    /// Candidate `i` of the store's creatures followed by the Maxis ones.
    pub fn nth(&self, store: &store::Store, i: usize) -> Option<Pick> {
        let mine: Vec<&store::StoredCreation> = store.creations.iter().filter(|c| c.is_creature()).collect();
        if i < mine.len() {
            return match &mine[i].kind {
                store::StoredKind::Creature(c) => Some(Pick::Stored { name: mine[i].name.clone(), creature: Box::new(c.clone()) }),
                _ => None,
            };
        }
        self.maxis.get(i - mine.len()).cloned()
    }

    pub fn count(&self, store: &store::Store) -> usize {
        store.creations.iter().filter(|c| c.is_creature()).count() + self.maxis.len()
    }

    /// A building, vehicle or UFO of the installed creations that the shopper lists, chosen by
    /// `seed`; tries the following ones when a creation file does not convert.
    fn random_asset(&self, shop: Shop, seed: &mut u64, store: &store::Store, originals: &crate::Originals) -> Option<Asset> {
        let list: Vec<&pedia::Entry> = self.assets.iter().filter(|e| shop.accepts(e, store)).collect();
        if list.is_empty() {
            return None;
        }
        let first = random_index(list.len(), seed);
        (0..list.len().min(24)).find_map(|k| Asset::of(list[(first + k) % list.len()], store, originals))
    }
}

/// The new game being set up.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub slot: usize,
    /// Chosen starting stage (phase 2 once set).
    pub stage: Option<usize>,
    pub hovered_stage: Option<usize>,
    /// 0 herbivore, 1 carnivore, 2 omnivore.
    pub diet: Option<u8>,
    pub difficulty: u8,
    pub specialty: Option<u8>,
    pub planet: String,
    /// Seed of the next "Randomize".
    pub name_seed: u64,
    /// The panel shown (index into `steps(stage)`).
    pub step: usize,
    /// The creature that starts the stage (the creature panel).
    pub pick: Option<Pick>,
    /// Seed of the creature panel's "Randomize" and the shopper position of `pick`.
    pub pick_seed: u64,
    pub pick_index: usize,
    /// The city hall (building card) and the vehicle or UFO (vehicle card) that
    /// start a Civilization / Space game (the assets panel).
    pub building: Option<Asset>,
    pub vehicle: Option<Asset>,
    /// The card whose shopper (Sporepedia) is open.
    pub shopping: Option<Card>,
    /// The Creature stage is unlocked, so the theme step is listed
    /// (`HideThemeUntilCreatureGameUnlocked`).
    pub theme_open: bool,
}

impl Draft {
    pub fn new(slot: usize) -> Self {
        Self {
            slot,
            stage: None,
            hovered_stage: None,
            diet: None,
            // A new flow starts on Easy with Military selected (VERIFIED, 0xDF4F4C / 0xDF28F0, 0xDF2740).
            difficulty: 0,
            specialty: Some(0),
            planet: String::new(),
            name_seed: 0x4F4D_E5EE + slot as u64,
            step: 0,
            pick: None,
            pick_seed: 0xC4EA_7012 + slot as u64,
            pick_index: 0,
            building: None,
            vehicle: None,
            shopping: None,
            theme_open: false,
        }
    }

    /// The step list of the chosen stage.
    pub fn steps(&self) -> Vec<Step> {
        self.stage.map(|s| steps(s, self.theme_open)).unwrap_or_default()
    }

    /// The panel shown (none while the stage wheel is up).
    pub fn current(&self) -> Option<Step> {
        self.steps().get(self.step).copied()
    }

    /// The current panel has its answer, so Forward goes on (the diet panel
    /// has no default; the others start answered).
    pub fn answered(&self) -> bool {
        match self.current() {
            Some(Step::Diet) => self.diet.is_some(),
            Some(Step::Creature) => self.pick.is_some(),
            Some(Step::Specialty) => self.specialty.is_some(),
            Some(Step::Assets) => self.building.is_some() && self.vehicle.is_some(),
            Some(Step::Name) => !self.planet.trim().is_empty(),
            _ => true,
        }
    }

    /// Every panel of the chosen stage has its answer.
    pub fn ready(&self) -> bool {
        let Some(stage) = self.stage else { return false };
        let steps = steps(stage, self.theme_open);
        // The record's flags decide which selections must exist (0xDEC460: the
        // creature, the building and the vehicle asset types).
        (!steps.contains(&Step::Diet) || self.diet.is_some())
            && (!steps.contains(&Step::Creature) || self.pick.is_some())
            && (!steps.contains(&Step::Specialty) || self.specialty.is_some())
            && (!steps.contains(&Step::Assets) || (self.building.is_some() && self.vehicle.is_some()))
            && !self.planet.trim().is_empty()
    }

    pub fn setup(&self) -> GameSetup {
        GameSetup {
            planet_name: self.planet.trim().to_string(),
            difficulty: self.difficulty.min(2),
            specialty: self.specialty,
            specialty_retail: true,
            start_stage: self.stage.unwrap_or(0),
        }
    }
}

/// The planet names of the saved games (lower case), for the "Planet Name
/// Taken" check.
pub fn taken_names(slots: &[slots::SlotInfo]) -> Vec<String> {
    slots.iter().filter_map(|s| s.saved()).map(|s| s.planet.to_lowercase()).filter(|n| !n.is_empty()).collect()
}

/// "Planet Name Taken" (`05F0FCA5`, GGE string 3, shown under the name field,
/// the field's text turns red). VERIFIED rule (0xDEF3C0, called from the name
/// panel's update 0xDEF9xx while the flow mode is 4): the typed text is
/// lower-cased character by character and compared for equality (0x87DA90)
/// with the lower-cased name of the first planet record of every star the star
/// manager lists (`cStarManager` + 0xF0); no trimming. ADAPTED: the stars are
/// the saved games' planets (OpenSpore's galaxy has no other named planets).
pub fn name_taken(planet: &str, taken: &[String]) -> bool {
    !planet.is_empty() && taken.contains(&planet.to_lowercase())
}

/// The next "Randomize" seed.
fn next_seed(seed: u64) -> u64 {
    seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407)
}

/// A generated planet name that no saved game uses: the name of `seed`, or of
/// the next seeds when it is taken (`seed` is left at the one used).
fn free_name(names: &super::gge::PlanetNames, seed: &mut u64, taken: &[String]) -> String {
    let mut name = names.0.name_for(crate::space_worldgen::NameKind::Planet, *seed);
    for _ in 0..64 {
        if !name_taken(&name, taken) {
            break;
        }
        *seed = next_seed(*seed);
        name = names.0.name_for(crate::space_worldgen::NameKind::Planet, *seed);
    }
    name
}

/// The draft is complete and its planet name is free.
pub fn can_start(draft: &Draft, taken: &[String]) -> bool {
    draft.ready() && !name_taken(&draft.planet, taken)
}

/// Type into the planet name (printable characters, Backspace).
pub fn edit_name(name: &mut String, input: &KeyboardInput) {
    if !input.state.is_pressed() {
        return;
    }
    match &input.logical_key {
        Key::Backspace => {
            name.pop();
        }
        Key::Character(s) => {
            for c in s.chars().filter(|c| !c.is_control()) {
                if name.chars().count() < MAX_PLANET_NAME {
                    name.push(c);
                }
            }
        }
        Key::Space if name.chars().count() < MAX_PLANET_NAME => name.push(' '),
        _ => {}
    }
}

/// The two halves of the wheel's backing disc (no control ids; elements of
/// the layout, the first mirrored).
const WHEEL_HALVES: [usize; 2] = [1679, 1681];
/// Pixels the name panel moves down in the setup phase.
const NAME_PANEL_DROP: f32 = 70.0;

fn by_element(ui: &SpuiState, element: usize) -> Option<WinId> {
    ui.layout.windows.iter().find(|w| w.element == element).map(|w| w.id)
}

fn move_name_panel(ui: &mut SpuiState, dy: f32) {
    let Some(w) = ui.find(id::NAME) else { return };
    let mut a = ui.layout.window(w).area;
    a[1] += dy;
    a[3] += dy;
    if ui.area(w) != a {
        ui.set_area(w, a);
    }
}

fn show(ui: &mut SpuiState, control: u32, on: bool) {
    if let Some(w) = ui.find(control) {
        ui.set_visible(w, on);
    }
}

fn select(ui: &mut SpuiState, controls: &[u32], chosen: Option<usize>) {
    for (i, c) in controls.iter().enumerate() {
        if let Some(w) = ui.find(*c) {
            ui.set_selected(w, chosen == Some(i));
        }
    }
}

/// Place the flow's root so its centre is the picked star (phase 1) or at its
/// authored place (phase 2: the panels fill the screen under the star).
fn place_root(ui: &mut SpuiState, star: Option<Vec2>) {
    let Some(root) = ui.find(id::ROOT) else { return };
    let authored = ui.layout.window(root).area;
    let area = match star {
        Some(p) => {
            let parent = ui.layout.window(root).parent;
            let origin = parent.map(|p| ui.rect(p)).map_or(Vec2::ZERO, |r| Vec2::new(r.x1, r.y1));
            let (hw, hh) = ((authored[2] - authored[0]) * 0.5, (authored[3] - authored[1]) * 0.5);
            [p.x - origin.x - hw, p.y - origin.y - hh, p.x - origin.x + hw, p.y - origin.y + hh]
        }
        None => authored,
    };
    if ui.area(root) != area {
        ui.set_area(root, area);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn bind(
    active: Res<FrontEndActive>,
    mut fe: ResMut<FrontEnd>,
    stars: Option<Res<super::gge::StarScreen>>,
    names: Option<Res<super::gge::PlanetNames>>,
    profile: Res<super::profile::Profile>,
    mut typing: MessageReader<KeyboardInput>,
    mut roots: Query<(&mut SpuiState, &GgeRoot)>,
    mut shoppers: ResMut<Shoppers>,
    pedia_store: Res<store::Store>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
) {
    let typed: Vec<KeyboardInput> = typing.read().cloned().collect();
    if !active.0 {
        return;
    }
    // The stages the profile has unlocked (0xDF4780).
    let unlocked = profile.offered();
    let starters_shown = fe.tab == super::Tab::Play && fe.overlay.is_none() && fe.selected.is_none();
    let Some((mut ui, _)) = roots.iter_mut().find(|(_, k)| **k == GgeRoot::NewGame) else { return };
    if !ui.ready {
        return;
    }
    // "Pick A Star For Your New Game" and "Override Planet (dev)" are literal
    // (unlocalized) debug captions of the layout: hidden.
    let _ = starters_shown;
    show(&mut ui, id::PICK_TEXT, false);
    show(&mut ui, id::DEV_OVERRIDE, false);
    // A card's shopper (Sporepedia) covers the flow.
    if fe.overlay == Some(Overlay::Sporepedia) && fe.flow_pick.is_some() {
        show(&mut ui, id::ROOT, false);
        return;
    }
    let taken = taken_names(&fe.slots);
    // A card's shopper closed: the creation it handed back, if any.
    let shopper_closed = fe.overlay != Some(Overlay::Sporepedia);
    let returned = if shopper_closed { fe.picked.take() } else { None };
    let Some(draft) = fe.new_game.as_mut() else {
        show(&mut ui, id::ROOT, false);
        return;
    };
    if draft.planet.is_empty() {
        if let Some(names) = &names {
            draft.planet = free_name(names, &mut draft.name_seed, &taken);
        }
    }
    show(&mut ui, id::ROOT, true);
    for hidden in [id::MENU_PANEL, id::NUMBERS_PANEL, id::COLOR_PICKER, id::DIET_TEXTS, id::NAME_TAKEN] {
        show(&mut ui, hidden, false);
    }
    let star = stars.and_then(|s| s.0.get(draft.slot).copied().flatten());
    match draft.stage {
        None => {
            place_root(&mut ui, star);
            show(&mut ui, id::STAGE_WHEEL, true);
            show(&mut ui, id::STAGE_BUTTONS, true);
            for element in WHEEL_HALVES {
                if let Some(w) = by_element(&ui, element) {
                    ui.set_visible(w, true);
                }
            }
            move_name_panel(&mut ui, 0.0);
            // The hovered stage button shows its card and picture.
            draft.hovered_stage = None;
            for (i, control) in id::STAGES.iter().enumerate() {
                if let Some(w) = ui.find(*control) {
                    ui.set_enabled(w, unlocked[i]);
                    if ui.states[w] & crate::spui::ids::state::HOVER != 0 {
                        draft.hovered_stage = Some(i);
                    }
                }
            }
            let shown = draft.hovered_stage;
            for i in 0..5 {
                show(&mut ui, id::CARDS[i], shown == Some(i));
                show(&mut ui, id::PICTURES[i], shown == Some(i));
            }
            for step in ORDER {
                show(&mut ui, step.panel(), false);
            }
            show(&mut ui, id::STEP_TOGGLES, false);
            show(&mut ui, id::BACK, false);
            show(&mut ui, id::FORWARD, false);
            show(&mut ui, id::FORWARD_GROUP, false);
        }
        Some(stage) => {
            place_root(&mut ui, None);
            show(&mut ui, id::STAGE_WHEEL, false);
            for element in WHEEL_HALVES {
                if let Some(w) = by_element(&ui, element) {
                    ui.set_visible(w, false);
                }
            }
            // ADAPTED: with the flow root at its authored place the name
            // panel's header lies above the screen; it moves down by this.
            move_name_panel(&mut ui, NAME_PANEL_DROP);
            for i in 0..5 {
                show(&mut ui, id::CARDS[i], false);
                show(&mut ui, id::PICTURES[i], false);
            }
            // The theme step belongs to the list once the Creature stage is unlocked (0xDEEE40).
            draft.theme_open = unlocked[1];
            let list = steps(stage, draft.theme_open);
            draft.step = draft.step.min(list.len().saturating_sub(1));
            // The shopper of a card closed: the creation it handed back becomes the pick.
            if shopper_closed {
                if let Some(card) = draft.shopping.take() {
                    if let Some(entry) = &returned {
                        take_pick(draft, card, entry, &pedia_store, &originals);
                    }
                }
            }
            let current = list.get(draft.step).copied();
            // One panel at a time (0xDF0250 hides every panel, shows the mode's).
            for step in ORDER {
                show(&mut ui, step.panel(), current == Some(step));
            }
            // The diet panel has two diets for a Cell start, three for the others (the
            // Creature and Tribal starts: 0xDECD00).
            show(&mut ui, id::DIET2, stage == 0);
            show(&mut ui, id::DIET3, stage != 0);
            select(&mut ui, &id::DIET2_BUTTONS, draft.diet.map(usize::from).filter(|_| stage == 0));
            select(&mut ui, &id::DIET3_BUTTONS, draft.diet.map(usize::from).filter(|_| stage != 0));
            select(&mut ui, &id::DIFFICULTY_BUTTONS, Some(draft.difficulty as usize));
            select(&mut ui, &id::SPECIALTY_BUTTONS, draft.specialty.map(|v| usize::from(specialty_button(v))));
            // The step dots (0xDF3170): the first n of the five toggles show, n = the record's
            // enabled modes (every record has at least five), and the shown panel's own dot is
            // selected (`dot_of`).
            show(&mut ui, id::STEP_TOGGLES, true);
            let lit = current.and_then(|step| dot_of(stage, step));
            for (i, dot) in id::DOTS.iter().enumerate() {
                if let Some(w) = ui.find(*dot) {
                    ui.set_visible(w, true);
                    ui.set_selected(w, Some(i) == lit);
                }
            }
            // The creature panel: the installed and stored creatures, the
            // card of the chosen one.
            if current == Some(Step::Creature) {
                shoppers.poll(&originals.resource_index);
                if draft.pick.is_none() && shoppers.count(&pedia_store) > 0 {
                    // ADAPTED: retail's default creature is not traced; a stored creature of the player's, else a random Maxis one.
                    draft.pick_index = if pedia_store.creations.iter().any(|c| c.is_creature()) { 0 } else { random_index(shoppers.count(&pedia_store), &mut draft.pick_seed) };
                    draft.pick = shoppers.nth(&pedia_store, draft.pick_index);
                }
                show(&mut ui, id::CREATURE_VERBS, false);
                for icon in id::CREATURE_ICONS {
                    show(&mut ui, icon, false);
                }
                if let Some(w) = ui.find(id::CREATURE_PICTURE) {
                    let image = match &draft.pick {
                        Some(Pick::Maxis { png, .. }) if assets.texture(&originals.resource_index, &mut images, *png).is_some() => {
                            Some(ImageRef { file: *png, uv: [0.0, 0.0, 1.0, 1.0], dims: None })
                        }
                        _ => None,
                    };
                    ui.set_image(w, image);
                }
            }
            // The assets panel (Civilization and Space starts): the city hall and the
            // vehicle / UFO cards with the installed creations as defaults.
            if current == Some(Step::Assets) {
                shoppers.poll(&originals.resource_index);
                let vehicle_shop = Shop::of(Card::Vehicle, stage, draft.specialty);
                // The specialty can change after a default was picked: the vehicle follows it.
                if draft.vehicle.as_ref().is_some_and(|v| !vehicle_shop.wants_kind(v.kind)) {
                    draft.vehicle = None;
                }
                if draft.building.is_none() {
                    draft.building = shoppers.random_asset(Shop::of(Card::Building, stage, None), &mut draft.pick_seed, &pedia_store, &originals);
                }
                if draft.vehicle.is_none() {
                    draft.vehicle = shoppers.random_asset(vehicle_shop, &mut draft.pick_seed, &pedia_store, &originals);
                }
                for hidden in id::ASSET_HIDDEN.into_iter().chain(id::VEHICLE_ICONS).chain(id::BUILDING_ICONS) {
                    show(&mut ui, hidden, false);
                }
                let title = super::loc(&mut assets, &originals.resource_index, super::GGE_TABLE, 0x05AF_2A12 + stage as u32, "Choose Your City Stuff");
                if let Some(w) = ui.find(id::ASSETS_TITLE) {
                    ui.set_text(w, title);
                }
                for (picture, asset) in [(id::ASSET_VEHICLE_PICTURE, &draft.vehicle), (id::ASSET_BUILDING_PICTURE, &draft.building)] {
                    if let Some(w) = ui.find(picture) {
                        let image = match asset.as_ref().and_then(|a| a.png) {
                            Some(png) if assets.texture(&originals.resource_index, &mut images, png).is_some() => {
                                Some(ImageRef { file: png, uv: [0.0, 0.0, 1.0, 1.0], dims: None })
                            }
                            _ => None,
                        };
                        ui.set_image(w, image);
                    }
                }
            }
            // The theme panel: the drop-down reads "No Theme" (0xDED560 adds it first,
            // 0xDEFDA0(0) selects it; GGE 05B08761); see the module notes.
            if current == Some(Step::Theme) {
                let none = super::loc(&mut assets, &originals.resource_index, super::GGE_TABLE, 0x05B0_8761, "No Theme");
                if let Some(w) = ui.find(id::THEME_COMBO) {
                    ui.set_text(w, none);
                }
            }
            // INFERRED: the layout puts the "Name Your Planet" text (hidden,
            // fading in) over the name field itself: it is the field's prompt
            // while the field is empty (drawn over a name it showed as ghost
            // letters after the name).
            show(&mut ui, id::NAME_TITLE, draft.planet.is_empty());
            // The name field takes the keyboard on the name panel.
            if current == Some(Step::Name) {
                for input in &typed {
                    edit_name(&mut draft.planet, input);
                }
            }
            let planet = draft.planet.clone();
            if let Some(w) = ui.find(id::NAME_EDIT) {
                ui.set_text(w, planet);
            }
            let taken_now = name_taken(&draft.planet, &taken);
            show(&mut ui, id::NAME_TAKEN, taken_now && current == Some(Step::Name));
            // Forward goes to the next panel (not shown on the last one,
            // 0xDF25E3); Go on the name panel starts the game.
            let last = draft.step + 1 >= list.len();
            show(&mut ui, id::FORWARD_GROUP, !last);
            if let Some(w) = ui.find(id::FORWARD) {
                ui.set_visible(w, !last);
                ui.set_enabled(w, draft.answered());
            }
            if let Some(w) = ui.find(id::GO) {
                ui.set_visible(w, true);
                ui.set_enabled(w, can_start(draft, &taken));
            }
            show(&mut ui, id::BACK, true);
        }
    }
}

/// A card's shopper handed back `entry`: it becomes the creature / city hall /
/// vehicle of the draft (a creation that does not convert changes nothing).
fn take_pick(draft: &mut Draft, card: Card, entry: &pedia::Entry, store: &store::Store, originals: &crate::Originals) {
    match card {
        Card::Creature => {
            if let Some(pick) = Pick::of(entry, store) {
                draft.pick = Some(pick);
            }
        }
        Card::Building => draft.building = Asset::of(entry, store, originals).or(draft.building.take()),
        Card::Vehicle => draft.vehicle = Asset::of(entry, store, originals).or(draft.vehicle.take()),
    }
}

/// A seeded index below `n`.
fn random_index(n: usize, seed: &mut u64) -> usize {
    *seed = next_seed(*seed);
    ((*seed >> 33) as usize) % n.max(1)
}

/// Clicks of the flow.
#[allow(clippy::too_many_arguments)]
pub fn actions(
    active: Res<FrontEndActive>,
    mut fe: ResMut<FrontEnd>,
    mut roots: Query<(&mut SpuiState, &GgeRoot)>,
    names: Option<Res<super::gge::PlanetNames>>,
    profile: Res<super::profile::Profile>,
    mut sounds: MessageWriter<crate::game_audio::PlayCue>,
    shoppers: Res<Shoppers>,
    pedia_store: Res<store::Store>,
    originals: Res<crate::Originals>,
) {
    if !active.0 {
        return;
    }
    let Some((mut ui, _)) = roots.iter_mut().find(|(_, k)| **k == GgeRoot::NewGame) else { return };
    let clicks: Vec<u32> = ui.take_clicks().into_iter().map(|w: WinId| ui.layout.window(w).control_id).collect();
    let unlocked = profile.offered();
    let taken = taken_names(&fe.slots);
    for control in clicks {
        // The controls with a retail sound play it (`ui_sounds`); the others the generic click.
        if !crate::ui_sounds::has_retail_click(control) {
            sounds.write(crate::game_audio::PlayCue(crate::game_audio::Cue::UiClick));
        }
        let Some(draft) = fe.new_game.as_mut() else { return };
        match control {
            id::CANCEL => {
                fe.new_game = None;
                return;
            }
            // Back (0xDF2630): the previous panel, from the first one the stage wheel.
            id::BACK => {
                if draft.step == 0 {
                    draft.stage = None;
                } else {
                    draft.step -= 1;
                }
            }
            // Forward (0xDF2520): the next listed panel.
            id::FORWARD if draft.answered() && draft.step + 1 < draft.steps().len() => draft.step += 1,
            id::NAME_RANDOM => {
                draft.name_seed = next_seed(draft.name_seed);
                if let Some(names) = &names {
                    draft.planet = free_name(names, &mut draft.name_seed, &taken);
                }
            }
            // Randomize on the creature panel: another creature of the shopper's feed.
            id::CREATURE_RANDOM => {
                let n = shoppers.count(&pedia_store);
                if n > 0 {
                    draft.pick_index = random_index(n, &mut draft.pick_seed);
                    draft.pick = shoppers.nth(&pedia_store, draft.pick_index);
                }
            }
            // A click on a card opens Sporepedia as that card's shopper (0xDF3CD2): the
            // creatures, or the buildings / vehicles / UFOs; its pick comes back through
            // `FrontEnd::picked`.
            id::CREATURE_CARD | id::ASSET_BUILDING_CARD | id::ASSET_VEHICLE_CARD => {
                let card = match control {
                    id::CREATURE_CARD => Card::Creature,
                    id::ASSET_BUILDING_CARD => Card::Building,
                    _ => Card::Vehicle,
                };
                draft.shopping = Some(card);
                let shop = Shop::of(card, draft.stage.unwrap_or(0), draft.specialty);
                fe.flow_pick = Some(shop);
                fe.overlay = Some(Overlay::Sporepedia);
                return;
            }
            // Randomize on the assets panel: another installed building / vehicle.
            id::ASSET_BUILDING_RANDOM | id::ASSET_VEHICLE_RANDOM => {
                let vehicle = control == id::ASSET_VEHICLE_RANDOM;
                let stage = draft.stage.unwrap_or(0);
                let shop = if vehicle { Shop::of(Card::Vehicle, stage, draft.specialty) } else { Shop::of(Card::Building, stage, None) };
                let mut seed = draft.pick_seed;
                if let Some(asset) = shoppers.random_asset(shop, &mut seed, &pedia_store, &originals) {
                    if vehicle {
                        draft.vehicle = Some(asset);
                    } else {
                        draft.building = Some(asset);
                    }
                }
                draft.pick_seed = seed;
            }
            // The zoom into the star, then the game (`intro::launch`).
            id::GO if can_start(draft, &taken) => {
                let draft = draft.clone();
                super::intro::begin(&mut fe, super::intro::LaunchKind::New(draft));
                return;
            }
            c => {
                if let Some(i) = id::STAGES.iter().position(|s| *s == c) {
                    if unlocked[i] {
                        // The stage's first panel comes up (0xDF2E70 -> Forward).
                        draft.stage = Some(i);
                        draft.step = 0;
                        draft.diet = None;
                        draft.building = None;
                        draft.vehicle = None;
                    }
                } else if let Some(i) = id::DIET2_BUTTONS.iter().position(|s| *s == c) {
                    draft.diet = Some(i as u8);
                } else if let Some(i) = id::DIET3_BUTTONS.iter().position(|s| *s == c) {
                    draft.diet = Some(i as u8);
                } else if let Some(i) = id::DIFFICULTY_BUTTONS.iter().position(|s| *s == c) {
                    draft.difficulty = i as u8;
                } else if let Some(i) = id::SPECIALTY_BUTTONS.iter().position(|s| *s == c) {
                    // Stored as retail does (0xDF2740: Military 0, Religious 1, Economic 2).
                    draft.specialty = Some(retail_specialty(i as u8));
                }
            }
        }
    }
}

/// The creature a pick stands for (a creature record in a scratch run).
fn creature_of_pick(pick: &Pick, originals: &crate::Originals) -> Option<RunProgress> {
    match pick {
        Pick::Stored { name, creature } => {
            let creation = store::StoredCreation {
                id: 0,
                name: name.clone(),
                created: 0,
                modified: 0,
                kind: store::StoredKind::Creature((**creature).clone()),
            };
            Some(creation.to_run())
        }
        Pick::Game { path, .. } => {
            let run = crate::persistence::load_from(path).ok()?;
            Some(pedia::creature_scratch(&run))
        }
        Pick::Maxis { png, name } => {
            let source = super::thumbs::Source::Png(*png);
            let (creation_type, name, xml) = crate::maxis_creation::creation_file(&originals.resource_index, &source, name)?;
            let (kind, scratch, _) = crate::maxis_creation::scratch_of(originals, creation_type, &name, &xml)?;
            (kind == crate::creature_editor_data::SetupKind::Creature).then_some(scratch)
        }
    }
}

/// Start the stage with the chosen creature (the key at +0x4C of the game
/// parameters, 0xDEB8E0 -> 0xDEB720): the body, parts and paint of the
/// creation become the player's creature.
fn apply_pick(run: &mut RunProgress, pick: &Pick, originals: &crate::Originals, editor: &mut crate::editor::EditorState) {
    let Some(scratch) = creature_of_pick(pick, originals) else {
        warn!("new game: the creature \"{}\" could not be loaded; the stage starts with the menu's creature", pick.name());
        return;
    };
    run.body = scratch.body;
    run.parts = scratch.parts;
    run.creature_creation = scratch.creature_creation;
    run.outfits = scratch.outfits;
    run.sync_legacy_accessory();
    editor.visuals_dirty = true;
}

/// Create the game in the draft's slot and enter its first stage. Uses the
/// same lineage creation as OpenSpore's Enter key (`RunProgress::new_lineage`)
/// and the Cell stage's diet step (`cell::choose_diet`).
pub fn start(
    fe: &mut FrontEnd,
    draft: &Draft,
    run: &mut RunProgress,
    save: &mut crate::persistence::SaveState,
    originals: &crate::Originals,
    editor: &mut crate::editor::EditorState,
    next: &mut NextState<GameStage>,
) {
    let stage_index = draft.stage.unwrap_or(0).min(4);
    let stage = RunProgress::stage_order()[stage_index].clone();
    if let Some(base) = fe.base.clone() {
        save.path = slots::slot_path(&base, draft.slot);
    }
    if fe.save_suspended {
        save.enabled = true;
        fe.save_suspended = false;
    }
    *run = run.new_lineage();
    run.game_setup = draft.setup();
    run.cell.difficulty = i32::from(run.game_setup.difficulty);
    // The chosen creature starts the stage (the Civilization and Space records choose one).
    if let Some(pick) = draft.pick.as_ref().filter(|_| STAGE_MODES[stage_index].creature) {
        apply_pick(run, pick, originals, editor);
    }
    match stage.clone() {
        GameStage::Cell => {
            crate::cell::choose_diet(run, originals, editor, draft.diet == Some(1));
        }
        other => {
            run.reset_to(other.clone());
            // The chosen city hall and vehicle / UFO are the player's designs of the stage
            // (the keys at +0x70 / +0x64 of the game parameters, 0xDEB8E0).
            if STAGE_MODES[stage_index].assets {
                for asset in [&draft.building, &draft.vehicle].into_iter().flatten() {
                    run.rigid.designs.insert(asset.kind.setup_name().to_string(), asset.design.clone());
                }
            }
            // ADAPTED: the Creature start's diet seeds the diet counters the
            // later stages read (consequence traits). Retail keeps no such
            // counters: the new-game parameters hold the choice (+0x90 / +0xA0
            // are the specialty and difficulty; the diet panel's pick is
            // applied by the Creature game's own set-up, not traced).
            match draft.diet {
                Some(0) => run.diet_herbivore = 1.0,
                Some(1) => run.diet_carnivore = 1.0,
                Some(2) => {
                    run.diet_herbivore = 1.0;
                    run.diet_carnivore = 1.0;
                }
                _ => {}
            }
        }
    }
    run.request_save();
    fe.new_game = None;
    fe.selected = None;
    next.set(stage);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ResourceKey {
        ResourceKey { group_id: 0, instance_id: 0, type_id: 0 }
    }

    fn asset(kind: SetupKind) -> Asset {
        Asset { name: "x".into(), png: None, kind, design: RigidDesign::default() }
    }

    #[test]
    fn a_saved_games_planet_name_is_taken() {
        // VERIFIED rule: equality of the lower-cased texts, no trimming.
        let taken = vec!["ixtapa".to_string()];
        assert!(name_taken("IXTAPA", &taken));
        assert!(!name_taken("  Ixtapa ", &taken), "retail does not trim (0xDEF3C0)");
        assert!(!name_taken("Eleis", &taken));
        assert!(!name_taken("", &taken));
        let mut d = Draft::new(1);
        d.stage = Some(3);
        d.pick = Some(Pick::Maxis { png: key(), name: "x".into() });
        d.building = Some(asset(SetupKind::BuildingCityHall));
        d.vehicle = Some(asset(SetupKind::VehicleMilitaryLand));
        d.planet = "Ixtapa".into();
        assert!(d.ready() && !can_start(&d, &taken));
        d.planet = "Eleis".into();
        assert!(can_start(&d, &taken));
    }

    #[test]
    fn generated_names_skip_the_taken_ones() {
        let names = super::super::gge::PlanetNames(std::sync::Arc::new(crate::space_worldgen::SpaceNames::builtin()));
        let first = names.0.name_for(crate::space_worldgen::NameKind::Planet, 5);
        let taken = vec![first.to_lowercase()];
        let mut seed = 5;
        let name = free_name(&names, &mut seed, &taken);
        assert!(!name_taken(&name, &taken) && seed != 5, "{name} after seed {seed}");
        let mut again = 5;
        assert_eq!(free_name(&names, &mut again, &[]), first, "a free name keeps its seed");
    }

    /// The per-stage mode sets are the bools of `GGENew<Stage>Game` (read from the
    /// installed property files; the record loader is 0xDF10A0).
    #[test]
    fn stage_panels_follow_the_stage_records() {
        use Step::*;
        assert_eq!(steps(0, false), vec![Diet, Difficulty, Name]);
        assert_eq!(steps(1, false), vec![Diet, Difficulty, Name]);
        assert_eq!(steps(2, false), vec![Diet, Difficulty, Name]);
        assert_eq!(steps(3, false), vec![Creature, Difficulty, Specialty, Assets, Name]);
        assert_eq!(steps(4, false), vec![Creature, Difficulty, Assets, Name]);
        // The theme step (listed between the creature and difficulty steps) comes with the
        // Creature stage unlocked (0xDEEE40).
        assert_eq!(steps(0, true), vec![Diet, Theme, Difficulty, Name]);
        assert_eq!(steps(3, true), vec![Creature, Theme, Difficulty, Specialty, Assets, Name]);
        // The five toggles of the layout bound the dots; modes follow the name table (0xDF4780).
        for stage in 0..5 {
            assert!(steps(stage, true).len() <= 6);
        }
        let modes: Vec<u32> = ORDER.iter().map(|s| s.mode()).collect();
        assert_eq!(modes, vec![2, 3, 6, 5, 7, 8, 4]);
    }

    #[test]
    fn the_dots_follow_the_record_loaders_order() {
        use Step::*;
        // Cell (name, difficulty, theme, diet): the loader's order, not the panels'.
        assert_eq!(
            [Name, Difficulty, Theme, Diet].map(|s| dot_of(0, s)),
            [Some(0), Some(1), Some(2), Some(3)]
        );
        assert_eq!(dot_of(0, Creature), None, "a Cell record has no creature mode");
        // Civilization: specialty 3, assets 4, creature is the sixth mode, the table repeats dot 4.
        assert_eq!(
            [Name, Difficulty, Theme, Specialty, Assets, Creature].map(|s| dot_of(3, s)),
            [Some(0), Some(1), Some(2), Some(3), Some(4), Some(4)]
        );
        // Space has no specialty: assets 3, creature 4.
        assert_eq!([Assets, Creature].map(|s| dot_of(4, s)), [Some(3), Some(4)]);
        // Every record has at least five modes (the four chosen ones + creature-and-assets together): five dots.
        for stage in 0..5 {
            let m = STAGE_MODES[stage];
            let n = [m.name, m.difficulty, m.theme, m.specialty, m.assets, m.creature, m.diet].into_iter().filter(|b| *b).count() + 1;
            assert!(n >= id::DOTS.len(), "stage {stage}: {n} modes");
        }
    }

    #[test]
    fn a_vehicle_shopper_lists_the_class_of_the_specialty() {
        assert_eq!(Shop::of(Card::Building, 3, None).model_type(), Some("BuildingCityHall"));
        assert_eq!(Shop::of(Card::Vehicle, 3, None).model_type(), Some("VehicleMilitaryLand"));
        assert_eq!(Shop::of(Card::Vehicle, 3, Some(2)).model_type(), Some("VehicleEconomicLand"));
        assert_eq!(Shop::of(Card::Vehicle, 3, Some(1)).model_type(), Some("VehicleCulturalLand"));
        assert_eq!(Shop::of(Card::Vehicle, 4, Some(1)).model_type(), None, "the UFO filter lists every UFO");
        assert_eq!(Shop::of(Card::Creature, 3, None).model_type(), None);
        let economic = Shop::of(Card::Vehicle, 3, Some(2));
        assert!(economic.wants_kind(SetupKind::VehicleEconomicLand));
        assert!(!economic.wants_kind(SetupKind::VehicleMilitaryLand));
        assert!(!economic.wants_kind(SetupKind::VehicleEconomicAir));
        assert!(Shop::of(Card::Building, 3, None).wants_kind(SetupKind::BuildingCityHall));
        assert!(!Shop::of(Card::Building, 3, None).wants_kind(SetupKind::BuildingHouse));
    }

    #[test]
    fn specialty_is_stored_as_retail_does() {
        // Military / Economic / Religious buttons = 0 / 2 / 1 (0xDF2740), the inverse maps back.
        assert_eq!([0, 1, 2].map(retail_specialty), [0, 2, 1]);
        assert_eq!([0, 1, 2].map(specialty_button), [0, 2, 1]);
        assert!(Draft::new(0).setup().specialty_retail);
        let mut d = Draft::new(0);
        d.stage = Some(3);
        d.specialty = Some(retail_specialty(1));
        assert_eq!(d.setup().specialty, Some(2), "Economic is 2");
    }

    #[test]
    fn a_card_shopper_lists_its_type_and_names_itself() {
        let creature = Shop::of(Card::Creature, 4, None);
        assert_eq!((creature.category(), creature.title.1), (0, "Create or Choose a Space Creature"));
        assert_eq!(Shop::of(Card::Creature, 3, None).title.1, "Create or Choose a Civ Creature");
        assert_eq!(Shop::of(Card::Building, 3, None).category(), 1);
        assert_eq!(Shop::of(Card::Vehicle, 3, None).category(), 2);
        assert_eq!(Shop::of(Card::Vehicle, 4, None).category(), 3, "a Space start picks a UFO");
        assert_eq!(Shop::of(Card::Vehicle, 3, Some(2)).title.1, "Choose an Economic Land Vehicle");
        assert_eq!(Shop::of(Card::Vehicle, 3, Some(1)).title.1, "Choose a Religious Land Vehicle");
    }

    #[test]
    fn a_new_flow_starts_on_easy_with_military_and_walks_its_panels() {
        let mut d = Draft::new(3);
        assert_eq!((d.difficulty, d.specialty), (0, Some(0)));
        assert_eq!(d.current(), None, "the stage wheel is up");
        d.stage = Some(1);
        assert_eq!(d.current(), Some(Step::Diet));
        assert!(!d.answered(), "a diet has to be chosen");
        d.diet = Some(2);
        assert!(d.answered());
        d.step = 2;
        assert_eq!(d.current(), Some(Step::Name));
        d.stage = Some(3);
        d.step = 0;
        assert_eq!(d.current(), Some(Step::Creature));
        assert!(!d.answered(), "a creature has to be chosen");
        d.pick = Some(Pick::Maxis { png: key(), name: "Blob".into() });
        assert!(d.answered());
        d.step = 3;
        assert_eq!(d.current(), Some(Step::Assets));
        assert!(!d.answered(), "a city hall and a vehicle have to be chosen");
        d.building = Some(asset(SetupKind::BuildingCityHall));
        d.vehicle = Some(asset(SetupKind::VehicleMilitaryLand));
        assert!(d.answered());
        d.theme_open = true;
        assert_eq!(d.steps().len(), 6, "the theme step joins once the Creature stage is unlocked");
    }

    #[test]
    fn a_draft_is_ready_once_every_panel_is_answered() {
        let mut d = Draft::new(3);
        assert!(!d.ready());
        d.stage = Some(0);
        d.planet = "Gorgon".into();
        assert!(!d.ready(), "the Cell diet is required");
        d.diet = Some(1);
        assert!(d.ready());
        d.stage = Some(2);
        assert!(d.ready(), "a Tribal start asks the diet, not a creature");
        d.stage = Some(3);
        assert!(!d.ready(), "a Civilization start needs its creature");
        d.pick = Some(Pick::Maxis { png: key(), name: "Blob".into() });
        assert!(!d.ready(), "and its city hall and vehicle");
        d.building = Some(asset(SetupKind::BuildingCityHall));
        d.vehicle = Some(asset(SetupKind::VehicleMilitaryLand));
        assert!(d.ready());
        d.specialty = None;
        assert!(!d.ready(), "and a specialty");
        d.specialty = Some(2);
        assert!(d.ready());
        d.stage = Some(4);
        assert!(d.ready(), "a Space start needs no specialty");
        d.planet = "  ".into();
        assert!(!d.ready());
        assert_eq!(d.setup().difficulty, 0);
    }

    #[test]
    fn planet_names_are_typed_up_to_the_limit() {
        let key = |k: Key| KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: k,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        let mut name = String::new();
        edit_name(&mut name, &key(Key::Character("Ab".into())));
        edit_name(&mut name, &key(Key::Space));
        edit_name(&mut name, &key(Key::Backspace));
        assert_eq!(name, "Ab");
        for _ in 0..40 {
            edit_name(&mut name, &key(Key::Character("x".into())));
        }
        assert_eq!(name.chars().count(), MAX_PLANET_NAME);
    }

    /// A stored creature picked on the creature panel starts the stage.
    #[test]
    fn a_picked_stored_creature_starts_the_stage() {
        let creature = store::StoredCreature::default();
        let pick = Pick::Stored { name: "Mine".into(), creature: Box::new(creature) };
        let mut fe = FrontEnd { base: Some(std::path::PathBuf::from("C:/nowhere/save.json")), ..Default::default() };
        let mut d = Draft::new(1);
        d.stage = Some(3);
        d.planet = "Ulmo".into();
        d.pick = Some(pick);
        let mut design = RigidDesign::default();
        design.name = "Town Hall".into();
        d.building = Some(Asset { design, ..asset(SetupKind::BuildingCityHall) });
        let mut run = RunProgress::default();
        let mut save = crate::persistence::SaveState::default();
        let mut editor = crate::editor::EditorState::default();
        let mut next = NextState::<GameStage>::default();
        start(&mut fe, &d, &mut run, &mut save, &crate::Originals::default(), &mut editor, &mut next);
        assert_eq!(run.creature_creation.name, "Mine");
        assert!(editor.visuals_dirty);
        // The chosen city hall is the player's design of the stage.
        assert_eq!(run.rigid.designs.get(SetupKind::BuildingCityHall.setup_name()).map(|d| d.name.as_str()), Some("Town Hall"));
        assert!(!STAGE_MODES[0].creature, "a Cell start has no creature step: the pick is ignored");
    }

    /// Starting a game writes nothing itself and enters the chosen stage with
    /// the draft's setup in the run.
    #[test]
    fn starting_a_later_stage_keeps_the_setup() {
        let mut fe = FrontEnd { base: Some(std::path::PathBuf::from("C:/nowhere/save.json")), ..Default::default() };
        let mut d = Draft::new(2);
        d.stage = Some(2);
        d.planet = "Ulmo".into();
        d.difficulty = 2;
        fe.new_game = Some(d.clone());
        let mut run = RunProgress::default();
        let mut save = crate::persistence::SaveState::default();
        let mut editor = crate::editor::EditorState::default();
        let mut next = NextState::<GameStage>::default();
        start(&mut fe, &d, &mut run, &mut save, &crate::Originals::default(), &mut editor, &mut next);
        assert_eq!(run.current_stage(), GameStage::Tribal);
        assert_eq!(run.game_setup.planet_name, "Ulmo");
        assert_eq!(run.game_setup.difficulty, 2);
        assert_eq!(run.cell.difficulty, 2);
        assert_eq!(save.path, std::path::PathBuf::from("C:/nowhere/save-slot2.json"));
        assert!(fe.new_game.is_none());
        assert!(matches!(next, NextState::Pending(GameStage::Tribal)));
    }
}
