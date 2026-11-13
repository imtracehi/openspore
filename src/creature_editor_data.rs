//! Installed data of the retail Creature Creator (and of the other editors
//! that share its engine), read at runtime; nothing retail is embedded except
//! the code immediates the editor's own code uses (play-mode button and
//! animation ids, see [`PLAY_ANIM_BUTTONS`]).
//!
//! Sources (`notes/creator-re-notes.md` sections 0, 1.3, 1.6, 4):
//! * editor setups (`editor_setup~` 0x40600100) after inheritance:
//!   `CreatureEditorExtraLarge` (Creature stage) -> `CreatureEditorSetup` ->
//!   `GlobalTemplate`; `CellToCreatureEditor` (first editor after the Cell
//!   stage, budget 145); the Tribal/Civ/Space outfitters;
//! * palettes (`palettes~` 0x406B6A00!N): categories in 0x406B6E00|N, pages in
//!   0x406B6C00|N, item lists in 0x406B6D00|N. Every item list whose
//!   `palettePagePartParentPage` is a page contributes its (row, column,
//!   item) triples: the lists are merged (the `_scc_*` lists are the demo
//!   tiers, the full game shows their union, `PaletteMain::ReadProp`
//!   0x5C6430). Columns are upgrade levels;
//! * part PROPs (`creature_parts~` 0x40626000) through their templates:
//!   price, complexity, bones, bake level, unlock level/find percentage,
//!   scale limits, capabilities;
//! * editor cameras (`camera_properties~` 0x40410100), lights (0x40200100),
//!   Test Drive backgrounds (`creatureeditor_backgrounds~` 0x40607000 + set)
//!   and the play-mode camera tuning.

use std::collections::HashMap;

use crate::{
    asset_catalog::{ResourceIndex, CREATURE_PARTS, PROP},
    cell_editor_data::{int, ints, key_list, key_of, number, prop_key, EditorLight, Locale, EDITOR_SETUP_GROUP},
    dbpf::ResourceKey,
    prop::PropertyList,
    world_objects::name_id,
};

pub const PALETTE_GROUP: u32 = 0x406B_6A00;
/// The `editor` locale table.
pub const EDITOR_LOCALE: u32 = 0x496B_FB26;
pub const PAGE_GROUP_BASE: u32 = 0x406B_6C00;
pub const ITEMS_GROUP_BASE: u32 = 0x406B_6D00;
pub const CATEGORY_GROUP_BASE: u32 = 0x406B_6E00;
/// Palette modules (`palettePalettePages` lists).
pub const MODULE_GROUP_BASE: u32 = 0x406B_6B00;
pub const CAMERA_GROUP: u32 = 0x4041_0100;
pub const LIGHT_GROUP: u32 = 0x4020_0100;
pub const BACKGROUND_GROUP_BASE: u32 = 0x4060_7000;
/// `CreaturePartIcons`: palette icons, instance = part PROP instance (the
/// thumbnail group of palette !0C).
pub const PART_ICON_GROUP: u32 = 0x0223_1C8B;
/// `editors_creature~`: category tab and page icons.
pub const EDITORS_CREATURE_GROUP: u32 = 0x100D_977C;
pub const PNG: u32 = 0x2F7D_0004;

/// Creature palettes: parts `!0C`, paint `!0D`, Cell-to-Creature parts `!0E`.
pub const CREATURE_PARTS_PALETTE: u32 = 0x0C;
pub const CREATURE_PAINT_PALETTE: u32 = 0x0D;
pub const CELL_TO_CREATURE_PALETTE: u32 = 0x0E;

#[allow(dead_code)]
pub mod prop_id {
    // Part PROPs.
    pub const BLOCK_NAME: u32 = 0x8F6F_C401;
    pub const MODEL_MESH_LOD0: u32 = 0x00F9_EFBB;
    pub const PRICE: u32 = 0x0216_6464;
    pub const COMPLEXITY: u32 = 0x3C65_2302;
    pub const BONES: u32 = 0x51FA_9DFA;
    pub const BAKE_LEVEL: u32 = 0x0446_0B63;
    pub const UNLOCK_LEVEL: u32 = 0x03A2_89AC;
    pub const FIND_PERCENTAGE: u32 = 0x03A2_89C3;
    pub const MIN_SCALE: u32 = 0xF023_ED73;
    pub const MAX_SCALE: u32 = 0xF023_ED79;
    pub const SELL_BACK: u32 = 0x31C7_C1F3;
    pub const ORIENT_TO_SURFACES: u32 = 0x2FF3_25CD;
    pub const PREFER_SYMMETRY_PLANE: u32 = 0x5C5E_51EC;
    pub const SYMMETRY_SNAP_DELTA: u32 = 0x8FFC_9E66;
    pub const ACTS_LIKE_FOOT: u32 = 0x04A3_3896;
    pub const ACTS_LIKE_GRASPER: u32 = 0x04A3_386D;
    // Editor setups.
    pub const PARTS_PALETTE: u32 = 0xF5CB_E065;
    pub const PAINT_PALETTE: u32 = 0x7A92_6123;
    pub const INITIAL_BUDGET: u32 = 0xB1B7_6A19;
    pub const MAX_BUDGET: u32 = 0x065E_9BE8;
    pub const COMPLEXITY_LIMIT: u32 = 0x9187_AEE3;
    pub const BONE_LIMIT: u32 = 0x047C_1D1B;
    pub const MAX_BAKED_BLOCKS: u32 = 0x04DB_C51F;
    pub const MAX_BLOCK_COUNT: u32 = 0x0684_A675;
    /// `editorViewableComplexityFlags`: which limits the meter shows (1 block
    /// score, 2 bones, 4 baked blocks, 8 geometry score).
    pub const VIEWABLE_COMPLEXITY_FLAGS: u32 = 0x05FA_1567;
    /// `editorMaxGeomScore` (vehicles 30, buildings 35).
    pub const MAX_GEOM_SCORE: u32 = 0x066F_72AE;
    /// `editorModelTypes`: the creation types the editor builds (more than
    /// one: the type chooser of `EditorSharedUI`).
    pub const MODEL_TYPES: u32 = 0xB035_1B13;
    /// `GeomInfo` PROPs: `editorGeomInfoKeys` (part PROPs) and
    /// `editorGeomInfoScores` (their geometry scores).
    pub const GEOM_INFO_KEYS: u32 = 0x066F_235B;
    pub const GEOM_INFO_SCORES: u32 = 0x066F_235C;
    pub const CAMERA_UI: u32 = 0x300D_B745;
    pub const LIGHT: u32 = 0xB00D_B8C8;
    pub const PAINT_LIGHT: u32 = 0x0141_40DD;
    pub const PLAY_ENTRY_EFFECT: u32 = 0x92DD_FC3E;
    pub const PLAY_EXIT_EFFECT: u32 = 0x92DD_FC3F;
    pub const SKYBOX_EFFECT: u32 = 0x026F_337B;
    pub const BACKGROUND_MODEL: u32 = 0x021B_35A0;
    pub const PEDESTAL_MODEL: u32 = 0xB00D_B7BD;
    pub const TEST_ENVIRONMENT: u32 = 0x026F_3355;
    pub const DAIS_TYPE: u32 = 0x0673_31DA;
    pub const DAIS_RADIUS: u32 = 0x0673_3247;
    pub const BOUND_SIZE: u32 = 0x700D_B77D;
    pub const MIN_HEIGHT: u32 = 0x44C7_F29F;
    pub const MAX_HEIGHT: u32 = 0x2704_959D;
    pub const STARTUP_MODEL: u32 = 0x503D_2F60;
    pub const CURRENCY_CHAR: u32 = 0x05DA_A925;
    pub const NAME_PROMPT: u32 = 0x0537_1343;
    pub const DEFAULT_NAME: u32 = 0x04D9_2F82;
    pub const EDITOR_NAME: u32 = 0x703E_542F;
    pub const SUBTRACT_ON_LOAD: u32 = 0x126E_AAE3;
    pub const BACKGROUND_SET: u32 = 0x0673_A6E1;
    pub const USE_SYMMETRY: u32 = 0x300D_D020;
    pub const USE_SPINE: u32 = 0x100D_E9E3;
    pub const USE_SKIN: u32 = 0x300D_E90B;
    pub const SHOW_VERTEBRAE: u32 = 0x1022_ADB0;
    pub const ONLY_EDIT_FROM_PALETTE: u32 = 0x3022_C4B9;
    pub const BASE_UNLOCKS: u32 = 0x057E_9439;
    pub const MANIPULATORS: u32 = 0x1011_9203;
    pub const IDLE_ACTIVATION: u32 = 0x0525_EA2E;
    pub const BUDGET_TOOLTIP: u32 = 0x0652_38B1;
    pub const VERB_TRAY_COLLECTION: u32 = 0x04AA_3989;
    /// `verbTrayIconID` of a verb tray: the round icon at the tray's right end.
    pub const VERB_TRAY_ICON_ID: u32 = 0x0628_6814;
    pub const MOUSE_WHEEL_TIMEOUT: u32 = 0x4A6E_A1A3;
    pub const MOUSE_WHEEL_DISTANCE: u32 = 0x3338_2A22;
    // Palettes.
    pub const PALETTE_PAGES: u32 = 0xF21E_733C;
    pub const PALETTE_THUMBNAIL_GROUP: u32 = 0x0223_3661;
    pub const PAGE_SEQUENCE: u32 = 0x0959_1F67;
    pub const PAGE_PARENT_PAGE: u32 = 0x1F7D_3CF3;
    pub const PAGE_PARENT_CATEGORY: u32 = 0xC840_67F7;
    pub const PAGE_ITEMS: u32 = 0xEEEA_D734;
    pub const PAGE_ITEM_ROWS: u32 = 0xF8E9_9C4F;
    pub const PAGE_ITEM_COLUMNS: u32 = 0x22C4_7931;
    pub const PAGE_NUM_ROWS: u32 = 0x30A9_5517;
    pub const PAGE_NUM_COLUMNS: u32 = 0x033A_8EC2;
    pub const PAGE_ITEM_WIDTH_FRACTION: u32 = 0x05AD_C172;
    pub const PAGE_ITEM_MAX_HEIGHT_FRACTION: u32 = 0x05B9_A649;
    pub const PAGE_ICON: u32 = 0xE6A3_1466;
    pub const DESCRIPTION: u32 = 0x00B2_CCCA;
    pub const CATEGORY_NAME: u32 = 0x2E19_42A8;
    pub const CATEGORY_SEQUENCE: u32 = 0x35EE_B8B5;
    pub const CATEGORY_ICONS: u32 = 0x044F_6C09;
    // Cameras.
    pub const CAMERA_ZOOM_SCALE: u32 = 0x00C7_C4F8;
    pub const CAMERA_ROTATE_SCALE: u32 = 0x00C7_C4F9;
    pub const CAMERA_TRANSLATE_SCALE: u32 = 0x00C7_C4FA;
    pub const CAMERA_ZOOM: u32 = 0x00C7_C4FB;
    pub const CAMERA_PITCH: u32 = 0x00C7_C4FC;
    pub const CAMERA_HEADING: u32 = 0x00C7_C4FD;
    pub const CAMERA_OFFSET_X: u32 = 0x6FDA_2E1C;
    pub const CAMERA_OFFSET_Y: u32 = 0x8FDA_2E23;
    pub const CAMERA_MIN_ZOOM: u32 = 0x00FE_23B2;
    pub const CAMERA_MAX_ZOOM: u32 = 0x00FE_2437;
    pub const CAMERA_MIN_PITCH: u32 = 0x00FE_243B;
    pub const CAMERA_MAX_PITCH: u32 = 0x00FE_243F;
    pub const CAMERA_NEAR: u32 = 0x0110_2B20;
    pub const CAMERA_FAR: u32 = 0x0110_2B2F;
    pub const CAMERA_FOV: u32 = 0x044C_6220;
    // Test Drive backgrounds.
    pub const BG_ENTRY_EFFECT: u32 = 0x03FD_7424;
    pub const BG_THUMBNAIL: u32 = 0x03FE_BC27;
    pub const BG_ORDER: u32 = 0x040E_7F66;
    pub const BG_LIGHTING: u32 = 0x056B_F5D7;
    // Play mode setup (`PlayModeSetup`).
    pub const PLAY_MODE_LIGHT: u32 = 0x03E2_FEE2;
    pub const PLAY_MODE_COLLISION: u32 = 0x03E9_6904;
    pub const CROSS_FADE_SNAP: u32 = 0x03FE_B3CF;
}

/// Which editor setup a session runs (`cEditor::mEditorName`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SetupKind {
    /// `CreatureEditorExtraLarge`: the Creature stage's editor (mating) and the
    /// free creator.
    #[default]
    Creature,
    /// `CellToCreatureEditor`: the "Early Creature Creator" after the Cell
    /// stage (legs + creature versions of the cell parts, budget 145).
    CellToCreature,
    Tribal,
    Civ,
    Space,
    /// The rigid-block editors (`VehicleTemplate`, `BuildingTemplate`,
    /// `UFOEditorSetup`): their setups, palettes and parts load through the
    /// same readers; the session (free placement of blocks, no skin) is
    /// `rigid_editor`. The templates carry every part page; the per-class
    /// setups below (children of the templates) carry the class's own
    /// palette, name prompt and studio.
    Vehicle,
    Building,
    Ufo,
    /// Civ vehicle classes: military / economic / cultural (the "Religious"
    /// vehicles) by land / sea / air.
    VehicleMilitaryLand,
    VehicleMilitaryWater,
    VehicleMilitaryAir,
    VehicleEconomicLand,
    VehicleEconomicWater,
    VehicleEconomicAir,
    VehicleCulturalLand,
    VehicleCulturalWater,
    VehicleCulturalAir,
    /// Civ building types.
    BuildingCityHall,
    BuildingHouse,
    BuildingFactory,
    BuildingEntertainment,
}

impl SetupKind {
    /// Rigid-block editors: no creature, skin or spine.
    pub fn is_rigid(self) -> bool {
        !matches!(self, Self::Creature | Self::CellToCreature | Self::Tribal | Self::Civ | Self::Space)
    }
    /// A vehicle editor (the template or one of the nine Civ classes).
    pub fn is_vehicle(self) -> bool {
        self.is_rigid() && !self.is_building() && self != Self::Ufo
    }
    /// A building editor (the template or one of the four Civ types).
    pub fn is_building(self) -> bool {
        matches!(
            self,
            Self::Building | Self::BuildingCityHall | Self::BuildingHouse | Self::BuildingFactory | Self::BuildingEntertainment
        )
    }
    /// Part PROP group the editor sells (`building_parts~` or
    /// `vehicle_parts~`; the UFO editor sells vehicle parts).
    pub fn part_group(self) -> u32 {
        if self.is_building() {
            crate::asset_catalog::BUILDING_PARTS
        } else {
            crate::asset_catalog::VEHICLE_PARTS
        }
    }
    /// The Tribal / Civ / Space outfitters (`editorOnlyEditFromPalette`).
    pub fn is_outfitter(self) -> bool {
        matches!(self, Self::Tribal | Self::Civ | Self::Space)
    }
    pub const ALL: [SetupKind; 21] = [
        Self::Creature,
        Self::CellToCreature,
        Self::Tribal,
        Self::Civ,
        Self::Space,
        Self::Vehicle,
        Self::Building,
        Self::Ufo,
        Self::VehicleMilitaryLand,
        Self::VehicleMilitaryWater,
        Self::VehicleMilitaryAir,
        Self::VehicleEconomicLand,
        Self::VehicleEconomicWater,
        Self::VehicleEconomicAir,
        Self::VehicleCulturalLand,
        Self::VehicleCulturalWater,
        Self::VehicleCulturalAir,
        Self::BuildingCityHall,
        Self::BuildingHouse,
        Self::BuildingFactory,
        Self::BuildingEntertainment,
    ];
    pub fn setup_name(self) -> &'static str {
        match self {
            Self::Creature => "CreatureEditorExtraLarge",
            Self::CellToCreature => "CellToCreatureEditor",
            Self::Tribal => "TribalAccessoriesEditorSetup",
            Self::Civ => "CivAccessoriesEditorSetup",
            Self::Space => "SpaceAccessoriesEditorSetup",
            Self::Vehicle => "VehicleTemplate",
            Self::Building => "BuildingTemplate",
            Self::Ufo => "UFOEditorSetup",
            Self::VehicleMilitaryLand => "VehicleMilitaryLand",
            Self::VehicleMilitaryWater => "VehicleMilitaryWater",
            Self::VehicleMilitaryAir => "VehicleMilitaryAir",
            Self::VehicleEconomicLand => "VehicleEconomicLand",
            Self::VehicleEconomicWater => "VehicleEconomicWater",
            Self::VehicleEconomicAir => "VehicleEconomicAir",
            Self::VehicleCulturalLand => "VehicleCulturalLand",
            Self::VehicleCulturalWater => "VehicleCulturalWater",
            Self::VehicleCulturalAir => "VehicleCulturalAir",
            Self::BuildingCityHall => "BuildingCityHall",
            Self::BuildingHouse => "BuildingHouse",
            Self::BuildingFactory => "BuildingFactory",
            Self::BuildingEntertainment => "BuildingEntertainment",
        }
    }
}

/// Values of one editor setup after inheritance (setup -> parents ->
/// `GlobalTemplate`, resolved by `ResourceIndex::properties`).
#[derive(Clone, Debug, Default)]
pub struct CreatureSetup {
    pub name: String,
    pub instance: u32,
    pub parts_palette: Option<u32>,
    pub paint_palette: Option<u32>,
    pub initial_budget: Option<i32>,
    pub max_budget: Option<i32>,
    pub complexity_limit: Option<i32>,
    pub bone_limit: Option<i32>,
    pub max_baked_blocks: Option<i32>,
    /// `editorMaxBlockCount` (`GlobalTemplate` 250): rigblocks in a creation.
    pub max_block_count: Option<i32>,
    /// `editorMaxGeomScore` (0 or missing: no geometry limit).
    pub max_geom_score: Option<f32>,
    /// `editorViewableComplexityFlags` (creatures 7, vehicles / buildings
    /// 0xB, outfitters 1).
    pub complexity_flags: Option<u32>,
    /// `editorModelTypes` instances (`VehicleTemplate` lists ten, a Civ
    /// class setup its own one).
    pub model_types: Vec<u32>,
    /// `editorCameraUI` instance (a `camera_properties~` PROP).
    pub camera: Option<u32>,
    pub light: Option<u32>,
    pub paint_light: Option<u32>,
    /// `editorPlayModeEntryEffectID` / `ExitEffectID` (the exit effect is also
    /// the Build-mode dais look created at editor start).
    pub play_entry_effect: Option<u32>,
    pub play_exit_effect: Option<u32>,
    pub skybox_effect: Option<u32>,
    pub background_model: Option<u32>,
    pub pedestal_model: Option<u32>,
    /// `editorSwatchPedestalModel`: the vehicle editors name their studio base
    /// only here (`ve_editor_landbase`), not in `editorPedestalModel`.
    pub swatch_pedestal_model: Option<u32>,
    pub test_environment: Option<u32>,
    pub dais_type: Option<u32>,
    pub dais_radius: Option<f32>,
    pub bound_size: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,
    pub startup_models: Vec<ResourceKey>,
    pub currency_char: Option<char>,
    pub name_prompt: Option<String>,
    pub default_name: Option<String>,
    pub editor_name: Option<String>,
    pub budget_tooltip: Option<String>,
    pub subtract_cost_on_load: bool,
    pub background_set: i32,
    pub use_symmetry: bool,
    pub use_spine: bool,
    pub use_skin: bool,
    pub show_vertebrae: bool,
    pub only_edit_from_palette: bool,
    /// `editorBaseConfigUnlocks`: (palette category, count) pairs.
    pub base_unlocks: Vec<(u32, u32)>,
    /// `editorEnabledManipulators` instances (e.g. `cSPEditorManipulationLimb`).
    pub manipulators: Vec<u32>,
    pub idle_activation_ms: Option<f32>,
    pub verb_trays: Option<u32>,
    pub mouse_wheel_timeout: Option<f32>,
    pub mouse_wheel_distance: Option<f32>,
}

impl CreatureSetup {
    pub fn has_manipulator(&self, name: &str) -> bool {
        self.manipulators.contains(&name_id(name))
    }
    /// The spine can be edited: `editorUseSpine` and the Spine manipulator
    /// (the outfitters enable Pinning / DeformHandle / RotationHandle only).
    pub fn edits_spine(&self) -> bool {
        self.use_spine && (self.manipulators.is_empty() || self.has_manipulator("cSPEditorManipulationSpine"))
    }
}

/// One palette category (a tab).
#[derive(Clone, Debug, Default)]
pub struct PaletteCategory {
    pub instance: u32,
    pub name: String,
    pub sequence: u32,
    /// `paletteCategoryIconList`: norm, off, hl, act, on-norm, on-off, on-hl,
    /// on-act (and a 9th copy of norm).
    pub icons: Vec<ResourceKey>,
    /// Indices into [`Palette::pages`], in page order.
    pub pages: Vec<usize>,
}

impl PaletteCategory {
    /// Tab image for a state: `selected` uses the `on-` half of the list.
    pub fn icon(&self, selected: bool, hover: bool, pressed: bool, disabled: bool) -> Option<ResourceKey> {
        let i = match (disabled, pressed, hover) {
            (true, _, _) => 1,
            (_, true, _) => 3,
            (_, _, true) => 2,
            _ => 0,
        } + if selected { 4 } else { 0 };
        self.icons.get(i).or(self.icons.first()).copied()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaletteItem {
    pub part: u32,
    /// PROP group of the part (`creature_parts~`, or the outfitter groups).
    pub group: u32,
    pub row: u32,
    pub column: u32,
}

/// A palette page with every item list merged into it.
#[derive(Clone, Debug, Default)]
pub struct PalettePage {
    pub instance: u32,
    pub description: String,
    /// Parent category instance.
    pub category: u32,
    pub sequence: u32,
    pub rows: u32,
    pub columns: u32,
    pub item_width_fraction: f32,
    pub item_max_height_fraction: f32,
    pub icon: Option<ResourceKey>,
    pub items: Vec<PaletteItem>,
}

impl PalettePage {
    /// Rows actually used (the grid's height in items).
    pub fn used_rows(&self) -> u32 {
        self.items.iter().map(|i| i.row + 1).max().unwrap_or(0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Palette {
    pub id: u32,
    /// `palettePaletteThumbnailGroupID`: group of the item icons (instance =
    /// part PROP instance).
    pub thumbnail_group: Option<u32>,
    pub categories: Vec<PaletteCategory>,
    pub pages: Vec<PalettePage>,
}

impl Palette {
    /// Palette icon of a part (the palette's thumbnail group).
    pub fn icon(&self, part: u32) -> ResourceKey {
        ResourceKey { type_id: PNG, group_id: self.thumbnail_group.unwrap_or(PART_ICON_GROUP), instance_id: part }
    }
    pub fn page(&self, category: usize, page: usize) -> Option<&PalettePage> {
        let c = self.categories.get(category)?;
        self.pages.get(*c.pages.get(page)?)
    }
    pub fn page_count(&self, category: usize) -> usize {
        self.categories.get(category).map_or(0, |c| c.pages.len())
    }
    /// (category, page) holding a part.
    pub fn locate(&self, part: u32) -> Option<(usize, usize)> {
        for (ci, c) in self.categories.iter().enumerate() {
            for (pi, &p) in c.pages.iter().enumerate() {
                if self.pages[p].items.iter().any(|i| i.part == part) {
                    return Some((ci, pi));
                }
            }
        }
        None
    }
}

/// Creature capabilities shown as abilities (verb categories of the
/// `CRE_*VerbTray` trays). `(verb category, capability property)`.
pub const ABILITY_CAPABILITIES: &[(&str, u32)] = &[
    ("bite", 0xB1C3_E5B7),
    ("charge", 0xB1C3_E5B8),
    ("strike", 0xB1C3_E5C0),
    ("spit", 0xB1C3_E5B9),
    ("sing", 0xB1C3_E5C2),
    ("dance", 0xB1C3_E5C1),
    ("charm", 0x91A5_70BA),
    ("pose", 0xB1C3_E5C4),
    ("call", 0xF354_A87A),
    ("jump", 0x330C_117A),
    ("grasp", 0xB00F_0FE2),
    ("glide", 0x04F4_E1B4),
    ("stealth", 0x51C3_E5B4),
    ("sprint", 0x3386_C531),
    ("sense", 0xB3E3_0313),
    ("speed", 0xB354_A87C),
    ("health", 0x31C3_E5B2),
    // The tribal accessories (`modelCapabilityTribe*`, the `TAE_` trays).
    ("attack", 0x055D_370E),
    ("social", 0x055D_3747),
    ("armor", 0x055D_374C),
    ("gather", 0x055D_3750),
    ("fishing", 0x055D_3754),
    // The vehicle parts (`modelCapabilitySpeed / Defense / Power` and the
    // three type powers; the `VHE_` trays, `rigid_stats`).
    ("vehicle_speed", 0x11B7_8A70),
    ("vehicle_defense", 0x11B7_8A71),
    ("vehicle_power", 0x11B7_8A72),
    ("vehicle_militarypower", 0x0632_9468),
    ("vehicle_religiouspower", 0x0632_9469),
    ("vehicle_economicpower", 0x0632_946A),
];
/// Diet capabilities (header tray).
pub const CAP_CARNIVORE: u32 = 0x022E_7847;
pub const CAP_HERBIVORE: u32 = 0x022E_785C;
pub const CAP_MOUTH: u32 = 0xB00F_0FEC;

/// What the editor needs of one creature part definition.
#[derive(Clone, Debug)]
pub struct CreaturePartInfo {
    pub key: ResourceKey,
    pub name: String,
    pub price: i32,
    pub complexity: i32,
    pub bones: i32,
    pub bake_level: i32,
    /// Geometry score from the install's `GeomInfo` PROP of the part's group
    /// (`IBakeManager` vtable +0x6C = FUN_00417140; 0 when not listed).
    pub geom: f32,
    pub unlock_level: i32,
    pub find_percentage: f32,
    pub min_scale: f32,
    pub max_scale: f32,
    pub sell_back: f32,
    pub icon: ResourceKey,
    /// RW4 instance of `modelMeshLOD0` (0 for limb assemblies).
    pub model: u32,
    /// Ability levels (verb category from [`ABILITY_CAPABILITIES`], level).
    pub abilities: Vec<(&'static str, i32)>,
    pub carnivore: i32,
    pub herbivore: i32,
    pub mouth: bool,
    /// Pinning rules (part templates, `notes/creator-re-notes.md` 2.1):
    /// `modelOrientToSurfaces` (stick out of the skin along its normal).
    pub orient_to_surface: bool,
    /// `modelPreferToBeOnPlaneOfSymmetry` (mouths).
    pub prefer_symmetry_plane: bool,
    /// `modelSymmetrySnapDelta` (limb segments 0.06), if authored.
    pub symmetry_snap_delta: Option<f32>,
    /// `modelActsLikeFoot` / `modelActsLikeGrasper`.
    pub acts_like_foot: bool,
    pub acts_like_grasper: bool,
    /// Available when a Creature stage begins (`editorBaseConfigUnlocks`
    /// rows of the first column, or an item of the Cell-to-Creature palette);
    /// set by [`CreatureEditorData::load`].
    pub starts_unlocked: bool,
}

#[cfg(test)]
impl CreaturePartInfo {
    /// A plain test part of the given block score.
    pub fn test(complexity: i32) -> Self {
        let k = ResourceKey { type_id: 0, group_id: 0, instance_id: 7 };
        Self {
            key: k,
            name: "part".into(),
            price: 25,
            complexity,
            bones: 0,
            bake_level: 0,
            geom: 0.0,
            unlock_level: 1,
            find_percentage: 1.0,
            min_scale: 0.5,
            max_scale: 2.0,
            sell_back: 1.0,
            icon: k,
            model: 1,
            abilities: vec![],
            carnivore: 0,
            herbivore: 0,
            mouth: false,
            orient_to_surface: false,
            prefer_symmetry_plane: false,
            symmetry_snap_delta: None,
            acts_like_foot: false,
            acts_like_grasper: false,
            starts_unlocked: false,
        }
    }
}

/// `camera_properties~` editor camera (`EditorCamera::Initialize` 0x5A25E0).
/// Angles in degrees as authored; FOV in radians (default 1 rad).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorCameraProps {
    pub zoom: f32,
    pub min_zoom: f32,
    pub max_zoom: f32,
    pub pitch: f32,
    pub min_pitch: f32,
    pub max_pitch: f32,
    pub heading: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub near: f32,
    pub far: f32,
    pub fov: f32,
    pub zoom_scale: f32,
    pub rotate_scale: f32,
    pub translate_scale: f32,
}

impl Default for EditorCameraProps {
    /// `EditorCameraCreatureUI` (A56276DA) as installed; used only when the
    /// install lacks the PROP.
    fn default() -> Self {
        Self {
            zoom: 5.0,
            min_zoom: 2.5,
            max_zoom: 7.5,
            pitch: -8.6,
            min_pitch: -55.0,
            max_pitch: 5.0,
            heading: -50.0,
            offset_x: 1.2,
            offset_y: 0.65,
            near: 0.2,
            far: 0.0,
            fov: 1.0,
            zoom_scale: 1.0,
            rotate_scale: 1.0,
            translate_scale: 1.0,
        }
    }
}

/// One Test Drive background (`creatureeditor_backgrounds~`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayBackground {
    pub instance: u32,
    pub order: i32,
    pub entry_effect: Option<u32>,
    pub thumbnail: Option<u32>,
    pub lighting: Option<u32>,
}

/// `PlayModeCameraTuning` (40600100!CEADEB40) and `CreatureTestMode~`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayCameraTuning {
    pub initial_distance: f32,
    /// Degrees.
    pub initial_phi: f32,
    pub initial_theta: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub min_pitch: f32,
    pub max_pitch: f32,
    pub near: f32,
    pub sensitivity: [f32; 3],
}

impl Default for PlayCameraTuning {
    fn default() -> Self {
        Self {
            initial_distance: 6.5,
            initial_phi: 18.6,
            initial_theta: 27.0,
            min_distance: 3.0,
            max_distance: 9.5,
            min_pitch: 0.0,
            max_pitch: 50.0,
            near: 0.25,
            sensitivity: [0.7, 0.4, 0.0006],
        }
    }
}

/// A Test Drive animation button (`PlayModeActorModule` 0x639BC0 /
/// `HandleUIButton` 0x63AA80, dances 0x63B320). Control id, TLSA request
/// ids (a random one of several for Roar; Sit toggles start/stop) and the
/// retail name of the first request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayAnimButton {
    pub control: u32,
    pub anims: &'static [u32],
    pub name: &'static str,
    pub kind: PlayAnimKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayAnimKind {
    /// Plays once, then the editor idle.
    Action,
    /// Toggles between its two requests (Sit start / stop).
    Toggle,
    /// Loops until another button (dances, walk, run).
    Loop,
}

/// `csa_idle_gen_editor`: queued after every action (loop).
pub const IDLE_ANIM: u32 = 0x0433_0667;
/// `csa_react_envir_01..06`: a random one (not the previous) after a
/// background change (`EditorPlayMode::GetNextEnvironmentReactionAnimID`).
pub const ENVIRONMENT_REACTIONS: [u32; 6] =
    [0x0431_DF93, 0x0431_DF9A, 0x0431_DF9F, 0x0431_DFA6, 0x044D_798C, 0x044D_7999];

pub const PLAY_ANIM_BUTTONS: &[PlayAnimButton] = &[
    PlayAnimButton { control: 0x03E9_9D88, anims: &[0x042D_D0E8], name: "csa_actn_jumphit", kind: PlayAnimKind::Action },
    PlayAnimButton { control: 0x03E9_9D94, anims: &[0x042D_D0E2], name: "csa_actn_backflip", kind: PlayAnimKind::Action },
    PlayAnimButton { control: 0x03E9_9E98, anims: &[0x0407_9846, 0x0407_9859], name: "csa_sit_start", kind: PlayAnimKind::Toggle },
    PlayAnimButton {
        control: 0x03E9_9D8C,
        anims: &[0x0433_0328, 0x042D_D0EE, 0x0437_3686],
        name: "csa_actn_battleroar",
        kind: PlayAnimKind::Action,
    },
    PlayAnimButton { control: 0x03E9_9DE4, anims: &[0x042D_D0F4], name: "ed_matecall", kind: PlayAnimKind::Action },
    PlayAnimButton { control: 0x03E9_9D90, anims: &[0x042D_D0D5], name: "csa_actn_punch", kind: PlayAnimKind::Action },
    PlayAnimButton { control: 0x03FC_42E4, anims: &[0x042D_D196], name: "csa_emo_happy", kind: PlayAnimKind::Action },
    PlayAnimButton { control: 0x03FC_4300, anims: &[0x042D_D19C], name: "csa_emo_angry", kind: PlayAnimKind::Action },
    PlayAnimButton { control: 0x03FC_4304, anims: &[0x042D_D1A9], name: "csa_emo_sad", kind: PlayAnimKind::Action },
    PlayAnimButton { control: 0x03FC_4310, anims: &[0x042D_D1AE], name: "csa_emo_laugh", kind: PlayAnimKind::Action },
    PlayAnimButton { control: 0x03FC_4314, anims: &[0x042D_D1B3], name: "csa_emo_scared", kind: PlayAnimKind::Action },
    PlayAnimButton { control: 0x03FC_431C, anims: &[0x042D_D1B8], name: "csa_emo_swoon", kind: PlayAnimKind::Action },
    PlayAnimButton { control: 0x0703_4071, anims: &[0x06F8_A609], name: "BP1_cycle_walk", kind: PlayAnimKind::Loop },
    PlayAnimButton { control: 0x0703_4070, anims: &[0x06F8_A61B], name: "BP1_cycle_run", kind: PlayAnimKind::Loop },
    // Dances (looping).
    PlayAnimButton { control: 0x03E8_30F4, anims: &[0x03FB_CE75], name: "csa_dance_01", kind: PlayAnimKind::Loop },
    PlayAnimButton { control: 0x03E8_30FC, anims: &[0x03FB_CE97], name: "csa_dance_03", kind: PlayAnimKind::Loop },
    PlayAnimButton { control: 0x03E8_3104, anims: &[0x03FB_CEA4], name: "csa_dance_04", kind: PlayAnimKind::Loop },
    PlayAnimButton { control: 0x03E8_313C, anims: &[0x03FB_CEB5], name: "csa_dance_05", kind: PlayAnimKind::Loop },
    PlayAnimButton { control: 0x03E8_3154, anims: &[0x0411_4B22], name: "csa_dance_06", kind: PlayAnimKind::Loop },
    PlayAnimButton { control: 0x03E8_3170, anims: &[0x0411_4B5E], name: "csa_dance_07", kind: PlayAnimKind::Loop },
];

/// Test Drive background slot buttons 0..3 (`PlayModeBackgrounds::Load`
/// 0x62FCC0).
pub const BACKGROUND_BUTTONS: [u32; 4] = [0x0445_B018, 0x0445_B318, 0x0445_B340, 0x0445_B388];

/// Complexity of a creation against the setup's limits (`StdEditorLimits`
/// indices 1..5: block score, bones, baked blocks, geometry score, block
/// count; sums of `FUN_004AD240` over the rigblocks, ratio `FUN_005745C0`,
/// VERIFIED `notes/editors-final-notes.md` 1). A limit of 0 or less is "no
/// limit" (retail turns 0 into -1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complexity {
    pub blocks: i32,
    pub bones: i32,
    pub baked: i32,
    pub block_limit: i32,
    pub bone_limit: i32,
    pub baked_limit: i32,
    /// Sum of the blocks' `GeomInfo` scores and `editorMaxGeomScore`.
    pub geom: f32,
    pub geom_limit: f32,
    /// Number of rigblocks and `editorMaxBlockCount`.
    pub count: i32,
    pub count_limit: i32,
    /// `editorViewableComplexityFlags` (bit 0 block, 1 bones, 2 baked, 3 geometry).
    pub flags: u32,
}

impl Complexity {
    /// Fill of the meter (`FUN_005745C0`): the fullest of the ratios the
    /// setup's flags show, and the block count's ratio, which is always
    /// shown.
    pub fn ratio(&self) -> f32 {
        let r = |v: f32, l: f32| if l > 0.0 { v / l } else { 0.0 };
        let shown = |bit: u32| self.flags & bit != 0;
        let mut m = 0.0f32;
        if shown(1) {
            m = m.max(r(self.blocks as f32, self.block_limit as f32));
        }
        if shown(2) {
            m = m.max(r(self.bones as f32, self.bone_limit as f32));
        }
        if shown(4) {
            m = m.max(r(self.baked as f32, self.baked_limit as f32));
        }
        if shown(8) {
            m = m.max(r(self.geom, self.geom_limit));
        }
        m.max(r(self.count as f32, self.count_limit as f32))
    }
    /// Over a limit (shown or not).
    pub fn over(&self) -> bool {
        let over = |v: i32, l: i32| l > 0 && v > l;
        over(self.blocks, self.block_limit)
            || over(self.bones, self.bone_limit)
            || over(self.baked, self.baked_limit)
            || over(self.count, self.count_limit)
            || (self.geom_limit > 0.0 && self.geom > self.geom_limit)
    }
    /// Room left in a limit (`StdEditorLimits::SetValue(i, limit - used)`;
    /// unlimited: `i32::MAX`).
    fn room(used: i32, limit: i32) -> i32 {
        if limit > 0 { limit - used } else { i32::MAX }
    }
    /// Can a palette part be bought (`StdEditorLimits` vtable +0x10 =
    /// FUN_0059E580, VERIFIED): every limit needs room for **two** of the part
    /// (a mirrored pair): its block score, bones, one baked block unless its
    /// bake level is above 1, two blocks, and the geometry score rounded up.
    pub fn has_room_for(&self, info: &CreaturePartInfo) -> bool {
        let n_baked = if info.bake_level > 1 { 0 } else { 1 };
        let geom_room = if self.geom_limit > 0.0 { (self.geom_limit - self.geom).floor() as i32 } else { i32::MAX };
        info.complexity * 2 <= Self::room(self.blocks, self.block_limit)
            && info.bones * 2 <= Self::room(self.bones, self.bone_limit)
            && n_baked * 2 <= Self::room(self.baked, self.baked_limit)
            && 2 <= Self::room(self.count, self.count_limit)
            && (info.geom * 2.0).ceil() as i32 <= geom_room
    }
}

/// Complexity score of a vertebra (`ce_vertebra` `modelComplexityScore`).
pub const VERTEBRA_COMPLEXITY: i32 = 2;

#[derive(bevy::prelude::Resource, Clone, Debug, Default)]
pub struct CreatureEditorData {
    pub loaded: bool,
    pub setups: HashMap<SetupKind, CreatureSetup>,
    pub palettes: HashMap<u32, Palette>,
    pub parts: HashMap<u32, CreaturePartInfo>,
    /// Model instance -> definition (palette definitions first).
    pub by_model: HashMap<u32, u32>,
    pub cameras: HashMap<u32, EditorCameraProps>,
    pub lights: HashMap<u32, EditorLight>,
    /// Background set -> backgrounds sorted by order.
    pub backgrounds: HashMap<i32, Vec<PlayBackground>>,
    pub play_camera: PlayCameraTuning,
    pub play_light: Option<u32>,
    pub cross_fade_effect: Option<u32>,
    /// Verb categories of each non-header tray of the setup's verb tray
    /// collection (`CRE_VerbTrayCollection`).
    pub trays: Vec<Vec<u32>>,
    /// The same for every setup's own collection (`TAE_VerbTrayCollection`
    /// for the Tribal outfitter, `CAE_` (empty) for the Civ one).
    pub setup_trays: HashMap<SetupKind, Vec<Vec<u32>>>,
    /// `verbTrayIconID` (image instance) of each tray of `trays` / `setup_trays`, in the same order.
    pub tray_icons: Vec<Option<u32>>,
    pub setup_tray_icons: HashMap<SetupKind, Vec<Option<u32>>>,
    /// `HintsEDI` "Your creation needs a mouth" (0x1F83D13C).
    pub hint_needs_mouth: Option<String>,
    /// Palette refusal reasons of FUN_0059E580 (`editor` locale 0x496BFB26:
    /// 0x201 over budget, 0x202 too complex; 0x101 / 0x102 are their titles).
    pub reason_budget: Option<String>,
    pub reason_complexity: Option<String>,
    pub errors: Vec<String>,
}

fn flag(list: &PropertyList, id: u32) -> bool {
    number(list, id).is_some_and(|v| v != 0.0)
}

/// Instance of a key property, `None` for 0 / 0xFFFFFFFF.
fn instance(list: &PropertyList, id: u32) -> Option<u32> {
    key_of(list, id).map(|k| k.instance_id).filter(|i| *i != 0 && *i != u32::MAX)
}

fn setup(index: &ResourceIndex, locale: &mut Locale, kind: SetupKind) -> Result<CreatureSetup, String> {
    let name = kind.setup_name();
    let instance_id = name_id(name);
    let p = index.properties(prop_key(EDITOR_SETUP_GROUP, instance_id))?;
    let unlocks = p.0.get(&prop_id::BASE_UNLOCKS).map(ints).unwrap_or_default();
    Ok(CreatureSetup {
        name: name.to_string(),
        instance: instance_id,
        parts_palette: instance(&p, prop_id::PARTS_PALETTE),
        paint_palette: instance(&p, prop_id::PAINT_PALETTE),
        initial_budget: int(&p, prop_id::INITIAL_BUDGET),
        max_budget: int(&p, prop_id::MAX_BUDGET),
        complexity_limit: int(&p, prop_id::COMPLEXITY_LIMIT),
        bone_limit: int(&p, prop_id::BONE_LIMIT),
        max_baked_blocks: int(&p, prop_id::MAX_BAKED_BLOCKS),
        max_block_count: int(&p, prop_id::MAX_BLOCK_COUNT),
        max_geom_score: number(&p, prop_id::MAX_GEOM_SCORE),
        complexity_flags: p.0.get(&prop_id::VIEWABLE_COMPLEXITY_FLAGS).map(ints).and_then(|v| v.first().copied()).map(|v| v as u32),
        model_types: key_list(&p, prop_id::MODEL_TYPES).into_iter().map(|k| k.instance_id).collect(),
        camera: instance(&p, prop_id::CAMERA_UI),
        light: instance(&p, prop_id::LIGHT),
        paint_light: instance(&p, prop_id::PAINT_LIGHT),
        play_entry_effect: instance(&p, prop_id::PLAY_ENTRY_EFFECT),
        play_exit_effect: instance(&p, prop_id::PLAY_EXIT_EFFECT),
        skybox_effect: instance(&p, prop_id::SKYBOX_EFFECT),
        background_model: instance(&p, prop_id::BACKGROUND_MODEL),
        pedestal_model: instance(&p, prop_id::PEDESTAL_MODEL),
        swatch_pedestal_model: instance(&p, name_id("editorSwatchPedestalModel")),
        test_environment: instance(&p, prop_id::TEST_ENVIRONMENT),
        dais_type: p.0.get(&prop_id::DAIS_TYPE).map(ints).and_then(|v| v.first().copied()).map(|v| v as u32),
        dais_radius: number(&p, prop_id::DAIS_RADIUS),
        bound_size: number(&p, prop_id::BOUND_SIZE),
        min_height: number(&p, prop_id::MIN_HEIGHT),
        max_height: number(&p, prop_id::MAX_HEIGHT),
        startup_models: key_list(&p, prop_id::STARTUP_MODEL),
        currency_char: p
            .0
            .get(&prop_id::CURRENCY_CHAR)
            .map(ints)
            .and_then(|v| v.first().copied())
            .and_then(|c| char::from_u32(c as u32)),
        name_prompt: locale.text(index, &p, prop_id::NAME_PROMPT),
        default_name: locale.text(index, &p, prop_id::DEFAULT_NAME),
        editor_name: locale.text(index, &p, prop_id::EDITOR_NAME),
        budget_tooltip: locale.text(index, &p, prop_id::BUDGET_TOOLTIP),
        subtract_cost_on_load: flag(&p, prop_id::SUBTRACT_ON_LOAD),
        background_set: int(&p, prop_id::BACKGROUND_SET).unwrap_or(0),
        use_symmetry: flag(&p, prop_id::USE_SYMMETRY),
        use_spine: flag(&p, prop_id::USE_SPINE),
        use_skin: flag(&p, prop_id::USE_SKIN),
        show_vertebrae: flag(&p, prop_id::SHOW_VERTEBRAE),
        only_edit_from_palette: flag(&p, prop_id::ONLY_EDIT_FROM_PALETTE),
        base_unlocks: unlocks.chunks_exact(2).map(|c| (c[0] as u32, c[1] as u32)).collect(),
        manipulators: key_list(&p, prop_id::MANIPULATORS).into_iter().map(|k| k.instance_id).collect(),
        idle_activation_ms: number(&p, prop_id::IDLE_ACTIVATION),
        verb_trays: instance(&p, prop_id::VERB_TRAY_COLLECTION),
        mouse_wheel_timeout: number(&p, prop_id::MOUSE_WHEEL_TIMEOUT),
        mouse_wheel_distance: number(&p, prop_id::MOUSE_WHEEL_DISTANCE),
    })
}

/// Read palette `id`: the pages its modules list (`palettePalettePages` of
/// every PROP in 0x406B6B00|N; all pages of 0x406B6C00|N when there is no
/// module), their categories and the merged item lists (item lists live in
/// the items group of the page's own group, e.g. 0x406B6D0A for the
/// Cell-to-Creature pages in 0x406B6C0A).
pub fn palette(index: &ResourceIndex, locale: &mut Locale, id: u32) -> Palette {
    let low = id & 0xFF;
    let keys_in = |group: u32| {
        let mut v: Vec<ResourceKey> =
            index.records.keys().filter(|k| k.type_id == PROP && k.group_id == group).copied().collect();
        v.sort_by_key(|k| k.instance_id);
        v
    };
    let mut out = Palette { id, ..Default::default() };
    if let Ok(p) = index.properties(prop_key(PALETTE_GROUP, id)) {
        out.thumbnail_group = key_of(&p, prop_id::PALETTE_THUMBNAIL_GROUP).map(|k| k.group_id).filter(|g| *g != 0);
    }
    let mut page_keys: Vec<ResourceKey> = Vec::new();
    for module in keys_in(MODULE_GROUP_BASE | low) {
        let Ok(p) = index.properties(module) else { continue };
        for k in key_list(&p, prop_id::PALETTE_PAGES) {
            let k = prop_key(if k.group_id == 0 { PAGE_GROUP_BASE | low } else { k.group_id }, k.instance_id);
            if !page_keys.contains(&k) {
                page_keys.push(k);
            }
        }
    }
    if page_keys.is_empty() {
        page_keys = keys_in(PAGE_GROUP_BASE | low);
    }
    let mut item_groups: Vec<u32> = Vec::new();
    let mut category_keys: Vec<ResourceKey> = Vec::new();
    for key in page_keys {
        let Ok(p) = index.properties(key) else { continue };
        let description = p
            .0
            .get(&prop_id::DESCRIPTION)
            .and_then(|v| v.text())
            .map(|t| t.2)
            .unwrap_or_default();
        let category = key_of(&p, prop_id::PAGE_PARENT_CATEGORY).filter(|k| k.instance_id != 0 && k.instance_id != u32::MAX);
        if let Some(c) = category {
            let c = prop_key(if c.group_id == 0 { CATEGORY_GROUP_BASE | (key.group_id & 0xFF) } else { c.group_id }, c.instance_id);
            if !category_keys.contains(&c) {
                category_keys.push(c);
            }
        }
        let items = ITEMS_GROUP_BASE | (key.group_id & 0xFF);
        if !item_groups.contains(&items) {
            item_groups.push(items);
        }
        out.pages.push(PalettePage {
            instance: key.instance_id,
            description,
            category: category.map_or(0, |c| c.instance_id),
            sequence: int(&p, prop_id::PAGE_SEQUENCE).unwrap_or(0).max(0) as u32,
            rows: int(&p, prop_id::PAGE_NUM_ROWS).unwrap_or(10).max(1) as u32,
            columns: int(&p, prop_id::PAGE_NUM_COLUMNS).unwrap_or(4).max(1) as u32,
            item_width_fraction: number(&p, prop_id::PAGE_ITEM_WIDTH_FRACTION).unwrap_or(0.25),
            item_max_height_fraction: number(&p, prop_id::PAGE_ITEM_MAX_HEIGHT_FRACTION).unwrap_or(0.1),
            icon: key_of(&p, prop_id::PAGE_ICON).filter(|k| k.instance_id != 0).map(|k| ResourceKey { type_id: PNG, ..k }),
            items: vec![],
        });
    }
    for key in category_keys {
        let Ok(p) = index.properties(key) else { continue };
        out.categories.push(PaletteCategory {
            instance: key.instance_id,
            name: locale.text(index, &p, prop_id::CATEGORY_NAME).unwrap_or_default(),
            sequence: int(&p, prop_id::CATEGORY_SEQUENCE).unwrap_or(0).max(0) as u32,
            icons: key_list(&p, prop_id::CATEGORY_ICONS)
                .into_iter()
                .map(|k| ResourceKey { type_id: PNG, ..k })
                .collect(),
            pages: vec![],
        });
    }
    // Merge every item list into its parent page.
    for group in item_groups {
        for key in keys_in(group) {
            let Ok(p) = index.properties(key) else { continue };
            let Some(parent) = instance(&p, prop_id::PAGE_PARENT_PAGE) else { continue };
            let Some(page) = out.pages.iter_mut().find(|pg| pg.instance == parent) else {
                continue;
            };
            let parts = key_list(&p, prop_id::PAGE_ITEMS);
            let rows = p.0.get(&prop_id::PAGE_ITEM_ROWS).map(ints).unwrap_or_default();
            let columns = p.0.get(&prop_id::PAGE_ITEM_COLUMNS).map(ints).unwrap_or_default();
            for (i, k) in parts.iter().enumerate() {
                let item = PaletteItem {
                    part: k.instance_id,
                    group: if k.group_id == 0 { CREATURE_PARTS } else { k.group_id },
                    row: rows.get(i).copied().unwrap_or(0).max(0) as u32,
                    column: columns.get(i).copied().unwrap_or(0).max(0) as u32,
                };
                // A later list never doubles a cell already filled.
                if !page.items.iter().any(|x| x.row == item.row && x.column == item.column) {
                    page.items.push(item);
                }
            }
        }
    }
    for page in &mut out.pages {
        page.items.sort_by_key(|i| (i.row, i.column));
    }
    out.pages.sort_by_key(|p| (p.sequence, p.instance));
    out.categories.sort_by_key(|c| (c.sequence, c.instance));
    // Pages without a category (the Cell-to-Creature "Legs" and "Cell"
    // pages) form a tab each.
    for (pi, page) in out.pages.iter().enumerate() {
        match out.categories.iter_mut().find(|c| page.category != 0 && c.instance == page.category) {
            Some(c) => c.pages.push(pi),
            None => out.categories.push(PaletteCategory {
                instance: page.instance,
                name: page.description.clone(),
                sequence: u32::MAX,
                icons: page.icon.into_iter().collect(),
                pages: vec![pi],
            }),
        }
    }
    out.categories.retain(|c| !c.pages.is_empty());
    out
}

pub fn part_info(index: &ResourceIndex, locale: &mut Locale, group: u32, instance_id: u32) -> Result<CreaturePartInfo, String> {
    let key = prop_key(group, instance_id);
    let p = index.properties(key)?;
    let level = |id: u32| int(&p, id).unwrap_or(0);
    let name = locale
        .text(index, &p, prop_id::BLOCK_NAME)
        .map(|n| n.replace("(PLACEHOLDER)", "").trim().to_string())
        .unwrap_or_else(|| format!("Part {instance_id:08X}"));
    Ok(CreaturePartInfo {
        key,
        name,
        price: level(prop_id::PRICE).max(0),
        complexity: level(prop_id::COMPLEXITY).max(0),
        bones: level(prop_id::BONES).max(0),
        bake_level: level(prop_id::BAKE_LEVEL).max(0),
        geom: 0.0,
        unlock_level: level(prop_id::UNLOCK_LEVEL).max(0),
        find_percentage: number(&p, prop_id::FIND_PERCENTAGE).unwrap_or(1.0),
        min_scale: number(&p, prop_id::MIN_SCALE).unwrap_or(0.5).max(0.05),
        max_scale: number(&p, prop_id::MAX_SCALE).unwrap_or(2.0).max(0.05),
        sell_back: number(&p, prop_id::SELL_BACK).unwrap_or(1.0),
        icon: ResourceKey { type_id: PNG, group_id: PART_ICON_GROUP, instance_id },
        model: key_of(&p, prop_id::MODEL_MESH_LOD0).map(|k| k.instance_id).unwrap_or(0),
        abilities: ABILITY_CAPABILITIES
            .iter()
            .filter_map(|(verb, id)| Some((*verb, level(*id))).filter(|(_, l)| *l > 0))
            .collect(),
        carnivore: level(CAP_CARNIVORE),
        herbivore: level(CAP_HERBIVORE),
        mouth: level(CAP_MOUTH) > 0,
        orient_to_surface: flag(&p, prop_id::ORIENT_TO_SURFACES),
        prefer_symmetry_plane: flag(&p, prop_id::PREFER_SYMMETRY_PLANE),
        symmetry_snap_delta: number(&p, prop_id::SYMMETRY_SNAP_DELTA).filter(|d| d.is_finite() && *d > 0.0),
        acts_like_foot: flag(&p, prop_id::ACTS_LIKE_FOOT),
        acts_like_grasper: flag(&p, prop_id::ACTS_LIKE_GRASPER),
        starts_unlocked: false,
    })
}

/// Parts available when a stage begins: in each category of the setup's
/// parts palette, the first-column item of the first `count` rows
/// (`editorBaseConfigUnlocks` (category, count) pairs; INFERRED meaning,
/// `notes/creator-re-notes.md` 1.7). Rows count across the category's pages.
pub fn base_unlocked_parts(palette: &Palette, base_unlocks: &[(u32, u32)]) -> Vec<u32> {
    let mut out = Vec::new();
    for c in &palette.categories {
        let Some(&(_, count)) = base_unlocks.iter().find(|(cat, _)| *cat == c.instance) else {
            continue;
        };
        let mut row_base = 0;
        for &pi in &c.pages {
            let page = &palette.pages[pi];
            for item in &page.items {
                if item.column == 0 && row_base + item.row < count {
                    out.push(item.part);
                }
            }
            row_base += page.used_rows();
        }
    }
    out
}

pub fn camera(index: &ResourceIndex, instance_id: u32) -> Option<EditorCameraProps> {
    let p = index.properties(prop_key(CAMERA_GROUP, instance_id)).ok()?;
    let d = EditorCameraProps::default();
    let n = |id: u32, v: f32| number(&p, id).filter(|x| x.is_finite()).unwrap_or(v);
    Some(EditorCameraProps {
        zoom: n(prop_id::CAMERA_ZOOM, d.zoom),
        min_zoom: n(prop_id::CAMERA_MIN_ZOOM, d.min_zoom),
        max_zoom: n(prop_id::CAMERA_MAX_ZOOM, d.max_zoom),
        pitch: n(prop_id::CAMERA_PITCH, d.pitch),
        min_pitch: n(prop_id::CAMERA_MIN_PITCH, d.min_pitch),
        max_pitch: n(prop_id::CAMERA_MAX_PITCH, d.max_pitch),
        heading: n(prop_id::CAMERA_HEADING, d.heading),
        offset_x: n(prop_id::CAMERA_OFFSET_X, d.offset_x),
        offset_y: n(prop_id::CAMERA_OFFSET_Y, d.offset_y),
        near: n(prop_id::CAMERA_NEAR, d.near),
        far: n(prop_id::CAMERA_FAR, 0.0),
        // `mfInitialFOV` defaults to 1 rad (57.29578 deg) when absent.
        fov: n(prop_id::CAMERA_FOV, 1.0),
        zoom_scale: n(prop_id::CAMERA_ZOOM_SCALE, 1.0),
        rotate_scale: n(prop_id::CAMERA_ROTATE_SCALE, 1.0),
        translate_scale: n(prop_id::CAMERA_TRANSLATE_SCALE, 1.0),
    })
}

fn backgrounds(index: &ResourceIndex, set: i32) -> Vec<PlayBackground> {
    let group = BACKGROUND_GROUP_BASE + set.max(0) as u32;
    let mut out: Vec<PlayBackground> = index
        .records
        .keys()
        .filter(|k| k.type_id == PROP && k.group_id == group)
        .copied()
        .filter_map(|k| {
            let p = index.properties(k).ok()?;
            Some(PlayBackground {
                instance: k.instance_id,
                order: int(&p, prop_id::BG_ORDER).unwrap_or(0),
                entry_effect: instance(&p, prop_id::BG_ENTRY_EFFECT),
                thumbnail: instance(&p, prop_id::BG_THUMBNAIL),
                lighting: instance(&p, prop_id::BG_LIGHTING),
            })
        })
        .collect();
    out.sort_by_key(|b| (b.order, b.instance));
    out
}

/// Verb categories per tray of a verb tray collection (header excluded).
fn trays(index: &ResourceIndex, collection: u32) -> Vec<Vec<u32>> {
    const VERB_TRAYS: u32 = 0x04AA_3838;
    const VERB_ICONS: u32 = 0x04A2_0FEB;
    let group = crate::cell_editor_data::VERBTRAY_GROUP;
    let Ok(c) = index.properties(prop_key(group, collection)) else {
        return vec![];
    };
    key_list(&c, VERB_TRAYS)
        .into_iter()
        .filter(|k| k.group_id != name_id("EditorHeaderTray"))
        .filter_map(|k| index.properties(prop_key(group, k.instance_id)).ok())
        .map(|tray| key_list(&tray, VERB_ICONS).into_iter().map(|k| k.instance_id).collect())
        .collect()
}

/// `verbTrayIconID` of each non-header tray of a verb tray collection (the round icon of the tray,
/// `stats_attack_icon` ... in the editor verb icon group; same order as [`trays`]).
fn tray_icons(index: &ResourceIndex, collection: u32) -> Vec<Option<u32>> {
    const VERB_TRAYS: u32 = 0x04AA_3838;
    let group = crate::cell_editor_data::VERBTRAY_GROUP;
    let Ok(c) = index.properties(prop_key(group, collection)) else {
        return vec![];
    };
    key_list(&c, VERB_TRAYS)
        .into_iter()
        .filter(|k| k.group_id != name_id("EditorHeaderTray"))
        .filter_map(|k| index.properties(prop_key(group, k.instance_id)).ok())
        .map(|tray| instance(&tray, prop_id::VERB_TRAY_ICON_ID))
        .collect()
}

impl CreatureEditorData {
    /// Read everything from the install. Missing pieces are recorded in
    /// `errors`; the editor still opens with what resolved.
    pub fn load(index: &ResourceIndex) -> Self {
        let mut out = Self { loaded: true, ..Self::default() };
        if index.records.is_empty() {
            return out;
        }
        let mut locale = Locale::default();
        for kind in SetupKind::ALL {
            match setup(index, &mut locale, kind) {
                Ok(s) => {
                    out.setups.insert(kind, s);
                }
                Err(e) => out.errors.push(format!("{}: {e}", kind.setup_name())),
            }
        }
        let mut palette_ids: Vec<u32> = out.setups.values().filter_map(|s| s.parts_palette).collect();
        palette_ids.sort();
        palette_ids.dedup();
        for id in palette_ids {
            out.palettes.insert(id, palette(index, &mut locale, id));
        }
        let mut wanted: Vec<(u32, u32)> = out
            .palettes
            .values()
            .flat_map(|p| p.pages.iter().flat_map(|pg| pg.items.iter().map(|i| (i.part, i.group))))
            .collect();
        wanted.sort();
        wanted.dedup();
        let palette_parts: Vec<u32> = wanted.iter().map(|w| w.0).collect();
        for (instance_id, group) in wanted {
            match part_info(index, &mut locale, group, instance_id) {
                Ok(info) => {
                    out.parts.insert(instance_id, info);
                }
                Err(e) => out.errors.push(format!("part {instance_id:08X}: {e}")),
            }
        }
        // Saved creatures may use any creature part definition.
        let mut all: Vec<u32> = index
            .records
            .keys()
            .filter(|k| k.type_id == PROP && k.group_id == CREATURE_PARTS)
            .map(|k| k.instance_id)
            .collect();
        all.sort();
        for instance_id in all {
            if !out.parts.contains_key(&instance_id) {
                if let Ok(info) = part_info(index, &mut locale, CREATURE_PARTS, instance_id) {
                    out.parts.insert(instance_id, info);
                }
            }
        }
        let geom = geom_info(index);
        for info in out.parts.values_mut() {
            info.geom = geom.get(&(info.key.group_id, info.key.instance_id)).copied().unwrap_or(0.0);
        }
        // Start-of-stage availability: the Creature setup's base rows, and the
        // whole Cell-to-Creature palette (the early creator offers every item).
        let mut free: Vec<u32> = Vec::new();
        if let Some(s) = out.setups.get(&SetupKind::Creature) {
            if let Some(p) = s.parts_palette.and_then(|id| out.palettes.get(&id)) {
                free.extend(base_unlocked_parts(p, &s.base_unlocks));
            }
        }
        if let Some(p) = out.parts_palette(SetupKind::CellToCreature) {
            free.extend(p.pages.iter().flat_map(|pg| pg.items.iter().map(|i| i.part)));
        }
        // The outfitters sell every palette item (`itemUnlockFindPercentage`
        // 1 on the accessories: `AddUnlockableItemFromProp` unlocks them).
        for kind in SetupKind::ALL.into_iter().filter(|k| k.is_outfitter()) {
            if let Some(p) = out.parts_palette(kind) {
                free.extend(
                    p.pages
                        .iter()
                        .flat_map(|pg| pg.items.iter())
                        .filter(|i| out.parts.get(&i.part).is_some_and(|x| x.find_percentage >= 1.0))
                        .map(|i| i.part),
                );
            }
        }
        // The rigid-block editors sell every palette item. ADAPTED: the
        // retail unlock rule of the vehicle / building / UFO parts is not
        // traced (the old assembly editors offered every catalog part).
        for kind in SetupKind::ALL.into_iter().filter(|k| k.is_rigid()) {
            if let Some(p) = out.parts_palette(kind) {
                free.extend(p.pages.iter().flat_map(|pg| pg.items.iter().map(|i| i.part)));
            }
        }
        for part in free {
            if let Some(info) = out.parts.get_mut(&part) {
                info.starts_unlocked = true;
            }
        }
        let mut by_model: Vec<(u32, bool, u32)> = out
            .parts
            .values()
            .filter(|i| i.model != 0)
            .map(|i| (i.model, !palette_parts.contains(&i.key.instance_id), i.key.instance_id))
            .collect();
        by_model.sort();
        for (model, _, definition) in by_model {
            out.by_model.entry(model).or_insert(definition);
        }
        let mut cameras: Vec<u32> = out.setups.values().filter_map(|s| s.camera).collect();
        cameras.sort();
        cameras.dedup();
        for c in cameras {
            match camera(index, c) {
                Some(props) => {
                    out.cameras.insert(c, props);
                }
                None => out.errors.push(format!("camera {c:08X} missing")),
            }
        }
        let mut sets: Vec<i32> = out.setups.values().map(|s| s.background_set).collect();
        sets.sort();
        sets.dedup();
        for set in sets {
            out.backgrounds.insert(set, backgrounds(index, set));
        }
        if let Ok(p) = index.properties(prop_key(EDITOR_SETUP_GROUP, name_id("PlayModeSetup"))) {
            out.play_light = instance(&p, prop_id::PLAY_MODE_LIGHT);
            out.cross_fade_effect = instance(&p, prop_id::CROSS_FADE_SNAP);
        }
        let mut tuning = PlayCameraTuning::default();
        if let Ok(p) = index.properties(prop_key(EDITOR_SETUP_GROUP, name_id("PlayModeCameraTuning"))) {
            let n = |name: &str, v: f32| number(&p, name_id(name)).unwrap_or(v);
            tuning.initial_distance = n("initialDistance", tuning.initial_distance);
            tuning.initial_phi = n("InitialPHI", tuning.initial_phi);
            tuning.initial_theta = n("InitialTheta", tuning.initial_theta);
            // `cameraMouseSensitivityX/Y/Z` have registered ids.
            tuning.sensitivity = [
                number(&p, 0xF06D_1E9B).unwrap_or(tuning.sensitivity[0]),
                number(&p, 0x306D_1E9E).unwrap_or(tuning.sensitivity[1]),
                number(&p, 0x506D_1EA0).unwrap_or(tuning.sensitivity[2]),
            ];
        }
        if let Ok(p) = index.properties(prop_key(CAMERA_GROUP, name_id("CreatureTestMode~"))) {
            let n = |name: &str, v: f32| number(&p, name_id(name)).unwrap_or(v);
            let _ = n;
            // `cameraMinDistance` / `cameraMaxDistance` (registered ids).
            tuning.min_distance = number(&p, 0x303E_B34D).unwrap_or(tuning.min_distance);
            tuning.max_distance = number(&p, 0x703E_B357).unwrap_or(tuning.max_distance);
            tuning.min_pitch = number(&p, 0x07B6_8967).or(number(&p, prop_id::CAMERA_MIN_PITCH)).unwrap_or(tuning.min_pitch);
            tuning.max_pitch = number(&p, 0x07B6_8979).or(number(&p, prop_id::CAMERA_MAX_PITCH)).unwrap_or(tuning.max_pitch);
            tuning.near = number(&p, prop_id::CAMERA_NEAR).unwrap_or(tuning.near);
        }
        out.play_camera = tuning;
        let mut light_ids: Vec<u32> = out
            .setups
            .values()
            .flat_map(|s| [s.light, s.paint_light])
            .chain(out.backgrounds.values().flatten().map(|b| b.lighting))
            .chain([out.play_light])
            .flatten()
            .collect();
        light_ids.sort();
        light_ids.dedup();
        for l in light_ids {
            if let Some(light) = crate::cell_editor_data::light(index, l) {
                out.lights.insert(l, light);
            }
        }
        let collection = out
            .setups
            .get(&SetupKind::Creature)
            .and_then(|s| s.verb_trays)
            .unwrap_or_else(|| name_id("CRE_VerbTrayCollection"));
        out.trays = trays(index, collection);
        out.tray_icons = tray_icons(index, collection);
        let per_setup: Vec<(SetupKind, Vec<Vec<u32>>)> = out
            .setups
            .iter()
            .filter_map(|(k, s)| Some((*k, trays(index, s.verb_trays?))))
            .collect();
        out.setup_trays.extend(per_setup);
        let per_setup_icons: Vec<(SetupKind, Vec<Option<u32>>)> = out
            .setups
            .iter()
            .filter_map(|(k, s)| Some((*k, tray_icons(index, s.verb_trays?))))
            .collect();
        out.setup_tray_icons.extend(per_setup_icons);
        out.hint_needs_mouth = locale.get(index, name_id("HintsEDI"), 0x1F83_D13C);
        // The entries carry the authoring label "(NOT IN USE)"; the text after
        // it is the retail sentence.
        let reason = |t: Option<String>| t.map(|t| t.trim_start_matches("(NOT IN USE)").trim().to_string()).filter(|t| !t.is_empty());
        out.reason_budget = reason(locale.get(index, EDITOR_LOCALE, 0x201));
        out.reason_complexity = reason(locale.get(index, EDITOR_LOCALE, 0x202));
        out
    }

    /// Verb categories of the setup's trays (header excluded).
    pub fn trays_of(&self, kind: SetupKind) -> &[Vec<u32>] {
        self.setup_trays.get(&kind).map_or(self.trays.as_slice(), |t| t.as_slice())
    }

    /// `verbTrayIconID` instances of the setup's trays (same order as [`Self::trays_of`]).
    pub fn tray_icons_of(&self, kind: SetupKind) -> &[Option<u32>] {
        self.setup_tray_icons.get(&kind).map_or(self.tray_icons.as_slice(), |t| t.as_slice())
    }

    pub fn setup(&self, kind: SetupKind) -> Option<&CreatureSetup> {
        self.setups.get(&kind)
    }

    /// Parts palette of a setup.
    pub fn parts_palette(&self, kind: SetupKind) -> Option<&Palette> {
        self.setup(kind).and_then(|s| s.parts_palette).and_then(|id| self.palettes.get(&id))
    }

    pub fn camera_of(&self, kind: SetupKind) -> EditorCameraProps {
        self.setup(kind)
            .and_then(|s| s.camera)
            .and_then(|c| self.cameras.get(&c))
            .copied()
            .unwrap_or_default()
    }

    pub fn light_of(&self, id: Option<u32>) -> Option<&EditorLight> {
        id.and_then(|l| self.lights.get(&l))
    }

    pub fn backgrounds_of(&self, kind: SetupKind) -> &[PlayBackground] {
        let set = self.setup(kind).map_or(0, |s| s.background_set);
        self.backgrounds.get(&set).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Definition of a saved part: its definition id, else its model's.
    pub fn definition_of(&self, part: &crate::game::PartInstance) -> Option<u32> {
        part.definition_id
            .filter(|d| self.parts.contains_key(d))
            .or_else(|| self.by_model.get(&part.instance).copied())
    }

    pub fn info_of(&self, part: &crate::game::PartInstance) -> Option<&CreaturePartInfo> {
        self.definition_of(part).and_then(|d| self.parts.get(&d))
    }

    /// Complexity of the mounted parts plus the spine's vertebrae against a
    /// setup's limits. A symmetric pair counts both parts (two rigblocks, as
    /// retail's doubled virtual twin in FUN_004AD240).
    pub fn complexity(&self, kind: SetupKind, parts: &[crate::game::PartInstance], vertebrae: usize) -> Complexity {
        let s = self.setup(kind);
        // Rigid blocks have no spine.
        let vertebrae = if kind.is_rigid() { 0 } else { vertebrae };
        let mut c = Complexity {
            block_limit: s.and_then(|s| s.complexity_limit).unwrap_or(280),
            bone_limit: s.and_then(|s| s.bone_limit).unwrap_or(128),
            baked_limit: s.and_then(|s| s.max_baked_blocks).unwrap_or(65),
            geom_limit: s.and_then(|s| s.max_geom_score).unwrap_or(0.0),
            count_limit: s.and_then(|s| s.max_block_count).unwrap_or(250),
            flags: s.and_then(|s| s.complexity_flags).unwrap_or(7),
            blocks: vertebrae as i32 * VERTEBRA_COMPLEXITY,
            bones: vertebrae as i32,
            // A vertebra (`ce_vertebra`, bake level 0) counts as a baked block.
            baked: vertebrae as i32,
            count: vertebrae as i32,
            geom: 0.0,
        };
        // A rigid limb's segment parts are counted in their assembly item.
        for p in parts.iter().filter(|p| p.edits.segment == 0) {
            c.count += 1;
            if let Some(info) = self.info_of(p) {
                c.blocks += info.complexity;
                c.bones += info.bones;
                // FUN_004AD240: a block counts as baked unless its bake level is 2.
                c.baked += i32::from(info.bake_level != 2);
                c.geom += info.geom;
            }
        }
        c
    }
}

/// `GeomInfo` PROPs (`IBakeManager` setup FUN_00403B20: groups
/// 0x40610100..0x40660100 = cell / creature / building / vehicle / empty /
/// flora configs, instance `GeomInfo`): part PROP key -> geometry score. The
/// lookup (FUN_00417140) picks the map by the part group's third byte
/// (0x61..0x66).
pub fn geom_info(index: &ResourceIndex) -> HashMap<(u32, u32), f32> {
    let mut out = HashMap::new();
    for b in 0x61u32..=0x66 {
        let group = 0x4000_0100 | (b << 16);
        let Ok(p) = index.properties(prop_key(group, name_id("GeomInfo"))) else {
            continue;
        };
        let keys = key_list(&p, prop_id::GEOM_INFO_KEYS);
        let scores: Vec<f32> = p
            .0
            .get(&prop_id::GEOM_INFO_SCORES)
            .map(|v| v.values.iter().filter_map(|b| Some(f32::from_be_bytes(b.get(..4)?.try_into().ok()?))).collect())
            .unwrap_or_default();
        if keys.len() != scores.len() {
            continue;
        }
        for (k, s) in keys.iter().zip(scores) {
            out.insert((k.group_id, k.instance_id), s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complexity_meter_shows_the_fullest_limit() {
        let c = Complexity {
            blocks: 140,
            bones: 32,
            baked: 13,
            block_limit: 280,
            bone_limit: 128,
            baked_limit: 65,
            count: 20,
            count_limit: 250,
            flags: 7,
            ..Default::default()
        };
        assert!((c.ratio() - 0.5).abs() < 1e-6);
        assert!(!c.over());
        let o = Complexity { bones: 130, ..c };
        assert!(o.over());
        // Vehicles (flags 0xB): the geometry score shows, the baked count not.
        let v = Complexity { geom: 24.0, geom_limit: 30.0, baked: 64, flags: 0xB, bones: 0, ..c };
        assert!((v.ratio() - 0.8).abs() < 1e-6);
        // The block count always shows (outfitters: flags 1).
        let o = Complexity { blocks: 10, block_limit: 300, count: 200, flags: 1, ..c };
        assert!((o.ratio() - 0.8).abs() < 1e-6);
        // A purchase needs room for a pair (FUN_0059E580).
        let mut info = CreaturePartInfo::test(70);
        assert!(c.has_room_for(&info), "140 + 2 x 70 = 280");
        info.complexity = 71;
        assert!(!c.has_room_for(&info));
        info.complexity = 1;
        assert!(!v.has_room_for(&info), "baked 64 of 65: no room for a pair");
        let v = Complexity { baked: 13, ..v };
        info.geom = 3.1;
        assert!(!v.has_room_for(&info), "ceil(6.2) = 7 > floor(30 - 24) = 6");
        info.geom = 3.0;
        assert!(v.has_room_for(&info));
    }

    #[test]
    fn category_icons_follow_the_retail_state_order() {
        let k = |i: u32| ResourceKey { type_id: PNG, group_id: EDITORS_CREATURE_GROUP, instance_id: i };
        let c = PaletteCategory { icons: (0..9).map(k).collect(), ..Default::default() };
        assert_eq!(c.icon(false, false, false, false), Some(k(0)));
        assert_eq!(c.icon(false, true, false, false), Some(k(2)));
        assert_eq!(c.icon(true, false, false, false), Some(k(4)));
        assert_eq!(c.icon(true, true, false, false), Some(k(6)));
        assert_eq!(c.icon(false, false, true, false), Some(k(3)));
    }

    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_rigid_editor_setups_and_palettes_load() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let data = CreatureEditorData::load(&index);
        assert!(data.errors.is_empty(), "{:?}", data.errors);
        for (kind, palette_id, initial, max, complexity, currency, group) in [
            (SetupKind::Vehicle, 0x3B, 5000, 9999, 300, '\u{2691}', crate::asset_catalog::VEHICLE_PARTS),
            (SetupKind::Building, 0x28, 50000, 99999, 250, '\u{268C}', crate::asset_catalog::BUILDING_PARTS),
            (SetupKind::Ufo, 0x46, 10000, 99999, 300, '\u{2691}', crate::asset_catalog::VEHICLE_PARTS),
        ] {
            let s = data.setup(kind).unwrap();
            assert!(kind.is_rigid());
            assert_eq!(s.parts_palette, Some(palette_id), "{kind:?}");
            assert_eq!(s.initial_budget, Some(initial), "{kind:?}");
            assert_eq!(s.max_budget, Some(max), "{kind:?}");
            assert_eq!(s.complexity_limit, Some(complexity), "{kind:?}");
            assert_eq!(s.currency_char, Some(currency), "{kind:?}");
            assert!(!s.use_spine && !s.use_skin, "{kind:?}: rigid blocks have no spine or skin");
            assert!(s.camera.is_some() && s.light.is_some(), "{kind:?}");
            let pal = data.parts_palette(kind).unwrap();
            let names: Vec<&str> = pal.categories.iter().map(|c| c.name.as_str()).collect();
            let items: Vec<_> = pal.pages.iter().flat_map(|p| p.items.iter()).collect();
            println!("{kind:?}: {names:?}, {} items, camera {:?}", items.len(), data.camera_of(kind));
            assert!(!items.is_empty() && items.iter().all(|i| i.group == group || i.group == crate::asset_catalog::VEHICLE_PARTS), "{kind:?}");
            let priced = items.iter().filter(|i| data.parts.get(&i.part).is_some_and(|p| p.price > 0 && p.model != 0)).count();
            assert!(priced * 10 >= items.len() * 9, "{kind:?}: {priced} of {} items resolve", items.len());
            // Complexity meter: geometry score shown and limited (VERIFIED data).
            assert_eq!(s.complexity_flags, Some(0xB), "{kind:?}");
            assert_eq!(s.max_block_count, Some(250), "{kind:?}");
            let geom = items.iter().filter(|i| data.parts.get(&i.part).is_some_and(|p| p.geom > 0.0)).count();
            println!("{kind:?}: geom limit {:?}, model types {:08X?}, {geom} of {} items scored", s.max_geom_score, s.model_types, items.len());
            assert!(geom * 10 >= items.len() * 9, "{kind:?}: {geom} of {} items have a geometry score", items.len());
        }
        assert_eq!(data.setup(SetupKind::Vehicle).unwrap().max_geom_score, Some(30.0));
        assert_eq!(data.setup(SetupKind::Building).unwrap().max_geom_score, Some(35.0));
        assert_eq!(data.setup(SetupKind::Vehicle).unwrap().model_types.len(), 10);
        assert_eq!(data.setup(SetupKind::Building).unwrap().model_types.len(), 5);
        for kind in SetupKind::ALL.into_iter().filter(|k| k.is_rigid()) {
            println!("{kind:?}: model types {:08X?}", data.setup(kind).unwrap().model_types);
        }
        assert_eq!(data.setup(SetupKind::Creature).unwrap().complexity_flags, Some(7));
        assert_eq!(data.setup(SetupKind::Tribal).unwrap().complexity_flags, Some(1));
        println!("reasons: {:?} / {:?}", data.reason_budget, data.reason_complexity);
        assert!(data.reason_complexity.is_some());
    }

    #[test]
    fn play_buttons_cover_the_retail_actions_and_dances() {
        assert_eq!(PLAY_ANIM_BUTTONS.len(), 20);
        let roar = PLAY_ANIM_BUTTONS.iter().find(|b| b.control == 0x03E9_9D8C).unwrap();
        assert_eq!(roar.anims.len(), 3);
        let sit = PLAY_ANIM_BUTTONS.iter().find(|b| b.kind == PlayAnimKind::Toggle).unwrap();
        assert_eq!(sit.anims, &[0x0407_9846, 0x0407_9859]);
        assert_eq!(PLAY_ANIM_BUTTONS.iter().filter(|b| b.name.starts_with("csa_dance")).count(), 6);
    }

    /// The Arms & Legs pages have a spacer row in the retail data: 7 rows, arms in rows 0-2, legs in
    /// rows 4-6. Retail keeps the gap: `PalettePage::ReadItemsModule` (0x5C80E0) files an item in slot
    /// `rows * column + row` of the page's slot table and `PalettePageUI::GetItemArea` (0x5C8E60)
    /// places slot `i` at column `i / rows`, row `i % rows`, so an empty row stays empty (the capture
    /// audit's "empty row 4" is this data, not a paging error).
    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_arms_and_legs_pages_keep_the_retail_spacer_row() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let data = CreatureEditorData::load(&index);
        let pal = data.parts_palette(SetupKind::Creature).unwrap();
        let tab = pal.categories.iter().position(|c| c.name == "Arms & Legs").expect("Arms & Legs tab");
        assert_eq!(pal.categories[tab].pages.len(), 2);
        for page in 0..2 {
            let p = pal.page(tab, page).unwrap();
            assert_eq!(p.rows, 7, "page {page}");
            let rows: std::collections::BTreeSet<u32> = p.items.iter().map(|i| i.row).collect();
            assert!(rows.contains(&0) && rows.contains(&4), "page {page}: {rows:?}");
            assert!(!rows.contains(&3), "page {page}: row 3 is the spacer ({rows:?})");
        }
    }

    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_creature_editor_data_matches_the_retail_setups() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let data = CreatureEditorData::load(&index);
        assert!(data.errors.is_empty(), "{:?}", data.errors);
        let creature = data.setup(SetupKind::Creature).unwrap();
        assert_eq!(creature.parts_palette, Some(CREATURE_PARTS_PALETTE));
        assert_eq!(creature.paint_palette, Some(CREATURE_PAINT_PALETTE));
        assert_eq!(creature.max_budget, Some(9999));
        assert_eq!(creature.complexity_limit, Some(280));
        assert_eq!(creature.bone_limit, Some(128));
        assert_eq!(creature.max_baked_blocks, Some(65));
        assert_eq!(creature.camera, Some(name_id("EditorCameraCreatureUI_Extra_Large")));
        assert_eq!(creature.play_entry_effect, Some(name_id("editor_test_mode_entry_extra_large")));
        assert_eq!(creature.play_exit_effect, Some(name_id("editor_part_mode_entry_extra_large")));
        assert_eq!(creature.skybox_effect, Some(name_id("CR_direction_particles")));
        assert_eq!(creature.light, Some(name_id("CreatureEditor")));
        assert_eq!(creature.currency_char, Some('\u{268B}'));
        assert_eq!(creature.startup_models.len(), 30);
        assert_eq!(creature.dais_radius, Some(1.25));
        assert!(creature.use_spine && creature.use_symmetry && creature.use_skin);
        assert!(creature.has_manipulator("cSPEditorManipulationLimb"));
        assert_eq!(creature.base_unlocks.len(), 7);
        let c2c = data.setup(SetupKind::CellToCreature).unwrap();
        assert_eq!(c2c.initial_budget, Some(145));
        assert_eq!(c2c.max_budget, Some(999));
        assert_eq!(c2c.parts_palette, Some(CELL_TO_CREATURE_PALETTE));
        assert!(c2c.subtract_cost_on_load);
        let tribal = data.setup(SetupKind::Tribal).unwrap();
        assert!(tribal.only_edit_from_palette);
        assert_eq!(tribal.initial_budget, Some(200));
        // Camera: zoom 5 (2.5..7.5), pitch -8.6 (-55..5), heading -50, FOV 1 rad.
        let cam = data.camera_of(SetupKind::Creature);
        assert_eq!((cam.zoom, cam.min_zoom, cam.max_zoom), (5.0, 2.5, 7.5));
        assert_eq!((cam.pitch, cam.min_pitch, cam.max_pitch, cam.heading), (-8.6, -55.0, 5.0, -50.0));
        assert_eq!((cam.offset_x, cam.offset_y, cam.near, cam.fov), (1.2, 0.65, 0.2, 1.0));
        // Palette !0C: seven categories in retail order, 252 merged items.
        let pal = data.parts_palette(SetupKind::Creature).unwrap();
        let names: Vec<&str> = pal.categories.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Mouths", "Eyes & Senses", "Arms & Legs", "Graspers", "Feet", "Weapons", "Details"]);
        assert_eq!(pal.categories[2].pages.len(), 2, "Arms & Legs has two pages");
        // 252 items in the base game; installed booster packs (Creepy & Cute)
        // add their rows to the same pages.
        let total: usize = pal.pages.iter().map(|p| p.items.len()).sum();
        assert!(total >= 252, "{total}");
        for p in &pal.pages {
            println!("page {} ({}): {} items, {} rows", p.description, p.instance, p.items.len(), p.used_rows());
        }
        assert!(pal.categories.iter().all(|c| c.icons.len() >= 8));
        let mouths = pal.page(0, 0).unwrap();
        assert_eq!(mouths.columns, 4);
        assert!(mouths.items.iter().all(|i| i.column < 4));
        // Columns are upgrade levels with the retail prices.
        let jaw = &data.parts[&name_id("ce_mouth_jaw_carnivore_01")];
        assert_eq!((jaw.price, jaw.complexity, jaw.bones), (25, 20, 9));
        assert!(jaw.abilities.contains(&("bite", 2)) && jaw.carnivore > 0 && jaw.mouth);
        let prices: Vec<i32> = (0..4)
            .filter_map(|c| mouths.items.iter().find(|i| i.row == 0 && i.column == c))
            .map(|i| data.parts[&i.part].price)
            .collect();
        assert_eq!(prices.len(), 4);
        assert!(prices.windows(2).all(|w| w[0] < w[1]), "{prices:?}");
        // Cell-to-Creature: Legs + Cell.
        let c2c_pal = data.parts_palette(SetupKind::CellToCreature).unwrap();
        let c2c_total: usize = c2c_pal.pages.iter().map(|p| p.items.len()).sum();
        assert_eq!(c2c_total, 15);
        // Test Drive backgrounds: default, sci-fi, doom, black with lights.
        let bgs = data.backgrounds_of(SetupKind::Creature);
        // (installed booster packs add more, paged four at a time)
        assert!(bgs.len() >= 4, "{bgs:?}");
        assert_eq!(bgs[0].entry_effect, Some(name_id("editor_test_mode_return")));
        let black = bgs.iter().find(|b| b.instance == name_id("blackbg")).unwrap();
        assert_eq!(black.lighting, Some(name_id("CreatureEditor")));
        assert!(data.lights.contains_key(&name_id("CreatureEditor")));
        assert_eq!(data.play_camera.initial_distance, 6.5);
        assert!(data.trays.len() >= 4, "{:?}", data.trays);
        // `verbTrayIconID` of the Generic / Attack / Social / Movement / Health trays.
        assert_eq!(
            data.tray_icons,
            ["stats_movement_icon", "stats_attack_icon", "stats_social_icon", "stats_motion_icon", "stats_health_icon"]
                .map(|n| Some(name_id(n)))
                .to_vec()
        );
    }
}
