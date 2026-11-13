//! Installed data of the retail Cell Creator, read at runtime (nothing retail
//! is embedded):
//!
//! * editor setups (`editor_setup~` 0x40600100): `CellTemplate` (parent of all),
//!   `CellEditorLevel0..4` (pedestal model per Cell level, parts palette 1) and
//!   `CellEditorIntro` (palette 2, budget 35);
//! * the parts palettes (`palettes~` 0x406B6A00!N): pages in group
//!   0x406B6C00|N, part lists in 0x406B6D00|N (row/column per item), categories
//!   in 0x406B6E00|N. Palette 1 (every in-game level) is one page of twelve
//!   parts in four rows: mouths, weapons, movement, eyes;
//! * the part PROPs (`cell_parts~` 0x40616000) through their templates
//!   (`CellEditorTemplates`): price, complexity, rig block type, cell
//!   capabilities, left/right/centre (symmetry) files, scale limits;
//! * the ability ("verb") definitions of the Cell editor (`ui_verbs~`
//!   0x01B68DB4 `cl_*`): name, description, icon, level display;
//! * localized strings (`Text.package` locale tables).
//!
//! Property names are from SporeModder-FX `reg_property.txt`.

use std::collections::HashMap;

use crate::{
    asset_catalog::{ResourceIndex, CELL_PARTS, PROP},
    dbpf::ResourceKey,
    prop::{Property, PropertyList},
    world_objects::name_id,
};

pub const EDITOR_SETUP_GROUP: u32 = 0x4060_0100;
#[allow(dead_code)]
pub const PALETTE_GROUP: u32 = 0x406B_6A00;
pub const PAGE_GROUP_BASE: u32 = 0x406B_6C00;
pub const ITEMS_GROUP_BASE: u32 = 0x406B_6D00;
/// `editor_rigblocks~`: editor models such as the Cell pedestals.
pub const EDITOR_MODEL_GROUP: u32 = 0x4060_6000;
/// `ui_verbs~`: creature/cell ability definitions.
pub const VERB_GROUP: u32 = 0x01B6_8DB4;
/// `cellparticons~`: palette icons, instance = part PROP instance.
pub const PART_ICON_GROUP: u32 = 0xB1B0_F42E;
pub const PNG: u32 = 0x2F7D_0004;
const LOCALE_TABLE: u32 = 0x02FA_C0B6;
/// Locale table of the editor setups (`editorName`, `editorNamePrompt`...).
pub const EDITOR_STRINGS: u32 = 0x6E77_2A58;

/// Palette used by every in-game Cell editor level (`CellEditorLevel0..4`).
pub const GAME_PALETTE: u32 = 1;

#[allow(dead_code)]
pub mod prop_id {
    pub const PARENT: u32 = 0x00B2_CCCB;
    pub const DESCRIPTION: u32 = 0x00B2_CCCA;
    pub const MODEL_MESH_LOD0: u32 = 0x00F9_EFBB;
    pub const MODEL_SCALE: u32 = 0x00FB_A611;
    pub const MODEL_OFFSET: u32 = 0x00FB_A610;
    pub const BLOCK_NAME: u32 = 0x8F6F_C401;
    pub const PRICE: u32 = 0x0216_6464;
    pub const COMPLEXITY: u32 = 0x3C65_2302;
    pub const RIG_BLOCK_TYPE: u32 = 0x0186_609D;
    pub const MIN_SCALE: u32 = 0xF023_ED73;
    pub const MAX_SCALE: u32 = 0xF023_ED79;
    pub const SELL_BACK: u32 = 0x31C7_C1F3;
    pub const LEFT_FILE: u32 = 0x0F48_EB09;
    pub const RIGHT_FILE: u32 = 0x18C1_DBE0;
    pub const CENTER_FILE: u32 = 0x3A3B_9D21;
    pub const ALLOW_ON_TOP: u32 = 0xD11C_BF2E;
    pub const REMAIN_UPRIGHT: u32 = 0x0201_636B;
    pub const POINT_FORWARD: u32 = 0x31B7_B15B;
    pub const SYMMETRY_SNAP: u32 = 0x8FFC_9E66;
    pub const REPLACE_SNAP: u32 = 0x0492_18C6;
    pub const SNAP_REPLACE_TYPES: u32 = 0x048E_2A2E;
    pub const CELL_GAME_SPEED: u32 = 0x0667_83B4;
    pub const CELL_MOVEMENT: u32 = 0x11B7_9302;
    pub const CELL_WEAPON: u32 = 0x11B7_9303;
    pub const CELL_STALK_EYE: u32 = 0x0655_4604;
    // Editor setups.
    pub const EDITOR_PEDESTAL: u32 = 0xB00D_B7BD;
    pub const EDITOR_PARTS_PALETTE: u32 = 0xF5CB_E065;
    pub const EDITOR_PAINT_PALETTE: u32 = 0x7A92_6123;
    pub const EDITOR_INITIAL_BUDGET: u32 = 0xB1B7_6A19;
    pub const EDITOR_MAX_BUDGET: u32 = 0x065E_9BE8;
    pub const EDITOR_COMPLEXITY_LIMIT: u32 = 0x9187_AEE3;
    pub const EDITOR_MAX_BAKED_BLOCKS: u32 = 0x04DB_C51F;
    pub const EDITOR_NAME_PROMPT: u32 = 0x0537_1343;
    pub const EDITOR_USE_SYMMETRY: u32 = 0x300D_D020;
    pub const EDITOR_LIGHT: u32 = 0xB00D_B8C8;
    pub const EDITOR_SUBTRACT_ON_LOAD: u32 = 0x126E_AAE3;
    // Palettes.
    pub const PAGE_SEQUENCE: u32 = 0x0959_1F67;
    pub const PAGE_PARENT_PAGE: u32 = 0x1F7D_3CF3;
    pub const PAGE_ITEMS: u32 = 0xEEEA_D734;
    pub const PAGE_ITEM_ROWS: u32 = 0xF8E9_9C4F;
    pub const PAGE_ITEM_COLUMNS: u32 = 0x22C4_7931;
    pub const PAGE_NUM_ROWS: u32 = 0x30A9_5517;
    pub const PAGE_NUM_COLUMNS: u32 = 0x033A_8EC2;
    pub const PAGE_ITEM_WIDTH_FRACTION: u32 = 0x05AD_C172;
    // Verbs.
    pub const ABILITY_NAME: u32 = 0x01B7_C544;
    pub const ABILITY_DESCRIPTION: u32 = 0x3249_A320;
    pub const ABILITY_REQ_ANY_PARTS: u32 = 0x01B7_DD74;
    pub const VERB_CATEGORY: u32 = 0x04AB_BB0D;
    pub const VERB_IMAGE: u32 = 0x0626_EFE0;
    pub const VERB_TRAY_IMAGE: u32 = 0x062C_C8D9;
    pub const VERB_SHOW_LEVEL: u32 = 0x04C1_AB51;
    pub const VERB_MAX_LEVEL: u32 = 0x0667_8640;
    // Lighting (`CellEditor` light setup).
    pub const LIGHT_SUN_DIR: u32 = 0x0100_EAB8;
    pub const LIGHT_SUN_COLOR: u32 = 0x0100_EAB9;
    pub const LIGHT_SUN_STRENGTH: u32 = 0x0100_EABA;
    pub const EXPOSURE: u32 = 0x0100_EAC4;
    pub const SH_COEFFS: u32 = 0x0100_EAC5;
}

/// Rig block class (`modelRigBlockType` instance) of a Cell part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartClass {
    Mouth,
    Eye,
    Movement,
    Weapon,
    Other,
}

impl PartClass {
    fn from_hash(h: u32) -> Self {
        if h == name_id("mouth") {
            Self::Mouth
        } else if h == name_id("eye") {
            Self::Eye
        } else if h == name_id("movement") {
            Self::Movement
        } else if h == name_id("weapon") {
            Self::Weapon
        } else {
            Self::Other
        }
    }
}

/// Retail `UnlockType` slot (`mUnlockedParts[13]` index) of a part, from its rig
/// block subtype. RecordUnlock's table (cell-re notes 6) pairs 2 jaw, 3 filter,
/// 4 proboscis (poker), 5 chemical, 6 electric, 7 spike, 8 cilia, 9 flagella,
/// 10 jet and 11 the stalk eye with the palette items.
pub fn unlock_slot(class: PartClass, subtype: u32) -> Option<usize> {
    let s = |n: &str| subtype == name_id(n);
    Some(match class {
        PartClass::Mouth if s("jaw") => 2,
        PartClass::Mouth if s("filter") => 3,
        PartClass::Mouth if s("poker") => 4,
        PartClass::Weapon if s("chemical") => 5,
        PartClass::Weapon if s("electric") => 6,
        PartClass::Weapon if s("spike") => 7,
        PartClass::Movement if s("cilia") => 8,
        PartClass::Movement if s("flagella") => 9,
        PartClass::Movement if s("jet") => 10,
        PartClass::Eye if s("stalk") => 11,
        _ => return None,
    })
}

/// Unlock slots with no pickup cell in the install (`cell-data-model.md`:
/// only mandible, spike, cilia, jet, chemical, electric and proboscis can be
/// found), so they must be owned from the start: filter mouth, flagella and
/// the stalk eye. ADAPTED: the retail new-game initialisation of
/// `mUnlockedParts` is not decoded; the retail frame of a fresh lineage shows
/// exactly these parts (plus the beady/button eyes, which have no slot)
/// available.
pub const START_SLOTS: [usize; 3] = [3, 9, 11];

/// One part placed on a palette page.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteItem {
    pub part: u32,
    pub row: u32,
    pub column: u32,
}

#[derive(Clone, Debug)]
pub struct PalettePage {
    pub key: ResourceKey,
    pub sequence: u32,
    pub items: Vec<PaletteItem>,
    pub rows: u32,
    pub columns: u32,
    /// `palettePageItemPercentageWidth` of the page template.
    pub item_width_fraction: f32,
}

/// What the editor needs of one Cell part definition.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct CellPartInfo {
    pub key: ResourceKey,
    pub name: String,
    pub price: i32,
    pub complexity: i32,
    pub class: PartClass,
    pub subtype: u32,
    pub unlock: Option<usize>,
    /// PROP instances of the symmetry files (`modelLeftFile`, `modelRightFile`,
    /// `modelCenterFile`); absent when the part has no variants.
    pub left: Option<u32>,
    pub right: Option<u32>,
    pub center: Option<u32>,
    pub game_speed: i32,
    pub allow_on_top: bool,
    pub remain_upright: bool,
    pub point_forward: bool,
    pub symmetry_snap: f32,
    pub replace_snap: f32,
    /// Rig classes this part replaces when dropped onto them.
    pub snap_replace: Vec<PartClass>,
    pub min_scale: f32,
    pub max_scale: f32,
    pub sell_back: f32,
    pub icon: ResourceKey,
    /// RW4 instance of `modelMeshLOD0` (what `PartInstance::instance` holds).
    pub model: u32,
}

/// A Cell ability shown in the verb trays and the palette rollovers.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct VerbInfo {
    pub key: ResourceKey,
    /// `verbIconCategory` instance (e.g. `cell_herbivore`, `sense`).
    pub category: u32,
    pub name: String,
    pub description: String,
    /// `verbIconImageID` instance; the PNG lives in one of the UI groups.
    pub image: u32,
    pub tray_image: Option<u32>,
    pub show_level: bool,
    pub max_level: Option<i32>,
    /// PROP instances of the parts that grant it.
    pub parts: Vec<u32>,
}

/// Values of one editor setup after inheritance.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct EditorSetup {
    pub name: String,
    pub pedestal: Option<ResourceKey>,
    pub parts_palette: Option<u32>,
    pub paint_palette: Option<u32>,
    pub initial_budget: Option<i32>,
    pub max_budget: Option<i32>,
    pub complexity_limit: Option<i32>,
    pub max_baked_blocks: Option<i32>,
    pub use_symmetry: bool,
    pub subtract_cost_on_load: bool,
    pub light: Option<u32>,
}

/// Editor lighting of a `CellEditor`-style light PROP.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct EditorLight {
    /// Spore space (Z up).
    pub sun_dir: [f32; 3],
    pub sun_color: [f32; 3],
    pub sun_strength: f32,
    pub exposure: f32,
    /// First spherical-harmonics coefficient (ambient colour).
    pub ambient: [f32; 3],
    /// All `shCoeffs` (0x0100EAC5) of the light PROP (RGB each; 25 installed).
    pub sh: Vec<[f32; 3]>,
}

#[derive(bevy::prelude::Resource, Clone, Debug, Default)]
pub struct CellEditorData {
    pub loaded: bool,
    pub template: EditorSetup,
    pub levels: Vec<EditorSetup>,
    pub intro: EditorSetup,
    pub palettes: HashMap<u32, Vec<PalettePage>>,
    pub parts: HashMap<u32, CellPartInfo>,
    /// Model instance -> definition, for saved parts without a definition id
    /// (palette items win over other definitions sharing a model).
    pub by_model: HashMap<u32, u32>,
    pub verbs: Vec<VerbInfo>,
    /// Verb categories of each non-header tray of `CLE_VerbTrayCollection`
    /// (the diet tray is the header).
    pub trays: Vec<Vec<u32>>,
    pub light: Option<EditorLight>,
    /// `CellEditorPaintMode` (`editorPaintModeLight`): the Paint mode light.
    pub paint_light: Option<EditorLight>,
    /// Time of each sound event of the part clips (`cell_editor::clip_sound_times`).
    pub part_sound_times: HashMap<String, f32>,
    pub name_prompt: Option<String>,
    pub speed_caption: Option<String>,
    pub speed_tooltip: Option<String>,
    /// Hint `edi-errornomouth` (HintsEDI 0x1F83D13C): the refusal to save a
    /// cell without a mouth.
    pub hint_needs_mouth: Option<String>,
    /// Palette rollover refusals of the editor constants table 0x496BFB26:
    /// 0x101 "Too expensive" (price over the DNA, `0x59E8A1` result 2) and
    /// 0x102 "Too many parts" (a complexity / part budget slot, result 3).
    pub reason_expensive: Option<String>,
    pub reason_parts: Option<String>,
    pub speed_max_dots: i32,
    /// Paint palette (styles, colour row), see `cell_paint`.
    pub paint: crate::cell_paint::CellPaintData,
    /// Hint `edi-cameramoved` ("Click and drag the background to rotate the
    /// camera"): its seconds before the first showing, visible time and the
    /// pause before it shows again (`EDI-TemplateBasic`).
    pub camera_hint: HintTiming,
    pub errors: Vec<String>,
}

/// Timing of an editor hint (`hintWait`, `hintLife`, `hintPeriod`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HintTiming {
    pub wait: f32,
    pub life: f32,
    pub period: f32,
}

impl Default for HintTiming {
    fn default() -> Self {
        Self {
            wait: 5.0,
            life: 5.0,
            period: 5.0,
        }
    }
}

impl HintTiming {
    /// Whether the hint is up `clock` seconds after the editor opened (it
    /// waits, shows for its life, rests for its period, shows again...).
    pub fn showing(&self, clock: f32) -> bool {
        let t = clock - self.wait;
        t >= 0.0 && t % (self.life + self.period).max(1e-3) < self.life
    }
}

/// Hint id of the camera hint.
pub const CAMERA_HINT: &str = "edi-cameramoved";
const HINT_GROUP: u32 = 0x0522_DE06;

pub fn prop_key(group: u32, instance: u32) -> ResourceKey {
    ResourceKey {
        type_id: PROP,
        group_id: group,
        instance_id: instance,
    }
}

/// Every key of a key-array property (12-byte items: instance, type, group).
pub fn keys(p: &Property) -> Vec<ResourceKey> {
    p.values
        .iter()
        .filter(|v| v.len() >= 12)
        .map(|v| {
            let w = |i: usize| u32::from_le_bytes(v[i..i + 4].try_into().unwrap());
            ResourceKey {
                instance_id: w(0),
                type_id: w(4),
                group_id: w(8),
            }
        })
        .collect()
}

/// Every value of an int32/uint32 property (big endian).
pub fn ints(p: &Property) -> Vec<i64> {
    p.values
        .iter()
        .filter(|v| v.len() >= 4)
        .map(|v| {
            let b: [u8; 4] = v[..4].try_into().unwrap();
            if p.kind == 10 {
                u32::from_be_bytes(b) as i64
            } else {
                i32::from_be_bytes(b) as i64
            }
        })
        .collect()
}

pub(crate) fn number(list: &PropertyList, id: u32) -> Option<f32> {
    list.0.get(&id).and_then(|p| p.number())
}

pub(crate) fn int(list: &PropertyList, id: u32) -> Option<i32> {
    number(list, id).map(|v| v.round() as i32)
}

fn flag(list: &PropertyList, id: u32) -> bool {
    number(list, id).is_some_and(|v| v != 0.0)
}

pub(crate) fn key_of(list: &PropertyList, id: u32) -> Option<ResourceKey> {
    list.0.get(&id).and_then(|p| p.key())
}

pub(crate) fn key_list(list: &PropertyList, id: u32) -> Vec<ResourceKey> {
    list.0.get(&id).map(keys).unwrap_or_default()
}

/// Localized strings with a small cache of parsed locale tables.
#[derive(Default)]
pub struct Locale {
    tables: HashMap<u32, HashMap<u32, String>>,
}

impl Locale {
    pub fn get(&mut self, index: &ResourceIndex, table: u32, id: u32) -> Option<String> {
        let entries = self.tables.entry(table).or_insert_with(|| {
            let mut records: Vec<_> = index
                .records
                .keys()
                .filter(|k| k.type_id == LOCALE_TABLE && k.instance_id == table)
                .copied()
                .collect();
            records.sort_by_key(|k| k.group_id);
            let mut map = HashMap::new();
            for k in records {
                if let Ok(bytes) = index.read(k) {
                    for (id, text) in crate::decode::parse_locale_table(&bytes) {
                        map.entry(id).or_insert(text);
                    }
                }
            }
            map
        });
        entries.get(&id).cloned()
    }

    /// Localized text property (`0x22`), falling back to its embedded text.
    pub fn text(&mut self, index: &ResourceIndex, list: &PropertyList, id: u32) -> Option<String> {
        let (table, text_id, fallback) = list.0.get(&id)?.text()?;
        self.get(index, table, text_id)
            .or(Some(fallback))
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty() && !t.starts_with('~'))
    }
}

fn setup(index: &ResourceIndex, name: &str) -> Result<EditorSetup, String> {
    let p = index.properties(prop_key(EDITOR_SETUP_GROUP, name_id(name)))?;
    // Instance-only references (group 0) name the pedestal PROP in the editor
    // model group, like the retail editor resolves `editorPedestalModel`.
    let pedestal = key_of(&p, prop_id::EDITOR_PEDESTAL).map(|k| prop_key(EDITOR_MODEL_GROUP, k.instance_id));
    let palette = |id| key_of(&p, id).map(|k| k.instance_id);
    Ok(EditorSetup {
        name: name.to_string(),
        pedestal,
        parts_palette: palette(prop_id::EDITOR_PARTS_PALETTE),
        paint_palette: palette(prop_id::EDITOR_PAINT_PALETTE),
        initial_budget: int(&p, prop_id::EDITOR_INITIAL_BUDGET),
        max_budget: int(&p, prop_id::EDITOR_MAX_BUDGET),
        complexity_limit: int(&p, prop_id::EDITOR_COMPLEXITY_LIMIT),
        max_baked_blocks: int(&p, prop_id::EDITOR_MAX_BAKED_BLOCKS),
        use_symmetry: flag(&p, prop_id::EDITOR_USE_SYMMETRY),
        subtract_cost_on_load: flag(&p, prop_id::EDITOR_SUBTRACT_ON_LOAD),
        light: key_of(&p, prop_id::EDITOR_LIGHT).map(|k| k.instance_id),
    })
}

/// Pedestal model of Cell editor level `level` (`CellEditorLevel0..4`), for
/// scenes that draw the Creator's backdrop without loading the whole editor
/// data (the HUD portrait).
pub fn level_pedestal(index: &ResourceIndex, level: usize) -> Option<ResourceKey> {
    setup(index, &format!("CellEditorLevel{level}")).ok().and_then(|s| s.pedestal)
}

/// Pages of palette `id`, sorted by their sequence numbers.
pub fn palette_pages(index: &ResourceIndex, id: u32) -> Vec<PalettePage> {
    let items_group = ITEMS_GROUP_BASE | (id & 0xFF);
    let page_group = PAGE_GROUP_BASE | (id & 0xFF);
    let mut lists: Vec<ResourceKey> = index
        .records
        .keys()
        .filter(|k| k.type_id == PROP && k.group_id == items_group)
        .copied()
        .collect();
    lists.sort_by_key(|k| k.instance_id);
    let mut pages = Vec::new();
    for list in lists {
        let Ok(p) = index.properties(list) else {
            continue;
        };
        let Some(parent) = key_of(&p, prop_id::PAGE_PARENT_PAGE) else {
            continue;
        };
        let page_key = prop_key(page_group, parent.instance_id);
        let page = index.properties(page_key).unwrap_or_default();
        let parts = key_list(&p, prop_id::PAGE_ITEMS);
        let rows = p.0.get(&prop_id::PAGE_ITEM_ROWS).map(ints).unwrap_or_default();
        let columns = p.0.get(&prop_id::PAGE_ITEM_COLUMNS).map(ints).unwrap_or_default();
        let items = parts
            .iter()
            .enumerate()
            .map(|(i, k)| PaletteItem {
                part: k.instance_id,
                row: rows.get(i).copied().unwrap_or(0).max(0) as u32,
                column: columns.get(i).copied().unwrap_or(0).max(0) as u32,
            })
            .collect();
        pages.push(PalettePage {
            key: page_key,
            sequence: int(&page, prop_id::PAGE_SEQUENCE).unwrap_or(0).max(0) as u32,
            items,
            rows: int(&page, prop_id::PAGE_NUM_ROWS).unwrap_or(4).max(1) as u32,
            columns: int(&page, prop_id::PAGE_NUM_COLUMNS).unwrap_or(3).max(1) as u32,
            item_width_fraction: number(&page, prop_id::PAGE_ITEM_WIDTH_FRACTION).unwrap_or(0.22),
        });
    }
    pages.sort_by_key(|p| (p.sequence, p.key.instance_id));
    pages
}

pub fn part_info(index: &ResourceIndex, locale: &mut Locale, instance: u32) -> Result<CellPartInfo, String> {
    let key = prop_key(CELL_PARTS, instance);
    let p = index.properties(key)?;
    // `modelRigBlockType` is written `cell!<class>.<subtype>`.
    let (class_hash, subtype) = key_of(&p, prop_id::RIG_BLOCK_TYPE)
        .map(|k| (k.instance_id, k.type_id))
        .unwrap_or((0, 0));
    let class = PartClass::from_hash(class_hash);
    let file = |id| key_of(&p, id).map(|k| k.instance_id).filter(|i| *i != 0);
    let snap_replace = key_list(&p, prop_id::SNAP_REPLACE_TYPES)
        .into_iter()
        .map(|k| PartClass::from_hash(k.instance_id))
        .collect();
    let name = locale
        .text(index, &p, prop_id::BLOCK_NAME)
        .map(|n| n.replace("(PLACEHOLDER)", "").trim().to_string())
        .unwrap_or_else(|| format!("Part {instance:08X}"));
    Ok(CellPartInfo {
        key,
        name,
        price: int(&p, prop_id::PRICE).unwrap_or(0).max(0),
        complexity: int(&p, prop_id::COMPLEXITY).unwrap_or(0).max(0),
        class,
        subtype,
        unlock: unlock_slot(class, subtype),
        left: file(prop_id::LEFT_FILE),
        right: file(prop_id::RIGHT_FILE),
        center: file(prop_id::CENTER_FILE),
        game_speed: int(&p, prop_id::CELL_GAME_SPEED).unwrap_or(0).max(0),
        allow_on_top: flag(&p, prop_id::ALLOW_ON_TOP),
        remain_upright: flag(&p, prop_id::REMAIN_UPRIGHT),
        point_forward: flag(&p, prop_id::POINT_FORWARD),
        symmetry_snap: number(&p, prop_id::SYMMETRY_SNAP).unwrap_or(0.03),
        replace_snap: number(&p, prop_id::REPLACE_SNAP).unwrap_or(0.0),
        snap_replace,
        min_scale: number(&p, prop_id::MIN_SCALE).unwrap_or(0.5).max(0.05),
        max_scale: number(&p, prop_id::MAX_SCALE).unwrap_or(1.5).max(0.05),
        sell_back: number(&p, prop_id::SELL_BACK).unwrap_or(1.0),
        icon: ResourceKey {
            type_id: PNG,
            group_id: PART_ICON_GROUP,
            instance_id: instance,
        },
        model: key_of(&p, prop_id::MODEL_MESH_LOD0).map(|k| k.instance_id).unwrap_or(0),
    })
}

fn verbs(index: &ResourceIndex, locale: &mut Locale, parts: &HashMap<u32, CellPartInfo>) -> Vec<VerbInfo> {
    let mut keys: Vec<ResourceKey> = index
        .records
        .keys()
        .filter(|k| k.type_id == PROP && k.group_id == VERB_GROUP)
        .copied()
        .collect();
    keys.sort_by_key(|k| k.instance_id);
    let mut out = Vec::new();
    for key in keys {
        let Ok(p) = index.properties(key) else {
            continue;
        };
        let required: Vec<u32> = key_list(&p, prop_id::ABILITY_REQ_ANY_PARTS)
            .into_iter()
            .map(|k| k.instance_id)
            .collect();
        // Cell abilities are the ones granted by Cell parts.
        if !required.iter().any(|i| parts.contains_key(i)) {
            continue;
        }
        let image = key_of(&p, prop_id::VERB_IMAGE).map(|k| k.instance_id).unwrap_or(0);
        out.push(VerbInfo {
            key,
            category: key_of(&p, prop_id::VERB_CATEGORY).map(|k| k.instance_id).unwrap_or(0),
            name: locale.text(index, &p, prop_id::ABILITY_NAME).unwrap_or_default(),
            description: locale.text(index, &p, prop_id::ABILITY_DESCRIPTION).unwrap_or_default(),
            image,
            tray_image: key_of(&p, prop_id::VERB_TRAY_IMAGE).map(|k| k.instance_id),
            show_level: flag(&p, prop_id::VERB_SHOW_LEVEL),
            max_level: int(&p, prop_id::VERB_MAX_LEVEL),
            parts: required,
        });
    }
    out
}

/// `verbtrays` group of the verb tray PROPs.
pub const VERBTRAY_GROUP: u32 = 0xAF02_8F41;

/// Verb categories of the Cell editor's trays (`CLE_VerbTrayCollection`
/// `verbTrays`, each tray's `verbIcons`). A tray referenced through the
/// `EditorHeaderTray` group is the header (the diet) and is skipped.
fn trays(index: &ResourceIndex) -> Vec<Vec<u32>> {
    const VERB_TRAYS: u32 = 0x04AA_3838;
    const VERB_ICONS: u32 = 0x04A2_0FEB;
    let Ok(collection) = index.properties(prop_key(VERBTRAY_GROUP, name_id("CLE_VerbTrayCollection"))) else {
        return vec![];
    };
    key_list(&collection, VERB_TRAYS)
        .into_iter()
        .filter(|k| k.group_id != name_id("EditorHeaderTray"))
        .filter_map(|k| index.properties(prop_key(VERBTRAY_GROUP, k.instance_id)).ok())
        .map(|tray| key_list(&tray, VERB_ICONS).into_iter().map(|k| k.instance_id).collect())
        .collect()
}

pub(crate) fn light(index: &ResourceIndex, instance: u32) -> Option<EditorLight> {
    // `editorLight` names an instance; the light PROPs live in group 0x40200100.
    let p = index.properties(prop_key(0x4020_0100, instance)).ok()?;
    let v3 = |id: u32| p.0.get(&id).and_then(|v| v.vector3().or_else(|| v.color_rgb()));
    let ambient = p
        .0
        .get(&prop_id::SH_COEFFS)
        .and_then(|v| v.values.first())
        .filter(|v| v.len() >= 12)
        .map(|v| [0, 4, 8].map(|o| f32::from_le_bytes(v[o..o + 4].try_into().unwrap())))
        .unwrap_or([1.0; 3]);
    // The loader (0x70AF40) fills the sun slot only when `lightSunColor` is
    // present: the Paint light has none, so it has no sun.
    let has_sun = p.0.contains_key(&prop_id::LIGHT_SUN_COLOR);
    let sh = p
        .0
        .get(&prop_id::SH_COEFFS)
        .map(|v| {
            v.values
                .iter()
                .filter(|v| v.len() >= 12)
                .map(|v| [0, 4, 8].map(|o| f32::from_le_bytes(v[o..o + 4].try_into().unwrap())))
                .collect()
        })
        .unwrap_or_default();
    Some(EditorLight {
        sun_dir: v3(prop_id::LIGHT_SUN_DIR).unwrap_or([0.0, 0.0, 1.0]),
        sun_color: v3(prop_id::LIGHT_SUN_COLOR).unwrap_or([1.0; 3]),
        sun_strength: if has_sun { number(&p, prop_id::LIGHT_SUN_STRENGTH).unwrap_or(1.0) } else { 0.0 },
        exposure: number(&p, prop_id::EXPOSURE).unwrap_or(1.0),
        ambient,
        sh,
    })
}

impl CellEditorData {
    /// Read everything from the install. Missing pieces are recorded in
    /// `errors`; the editor still opens with what resolved.
    pub fn load(index: &ResourceIndex) -> Self {
        let mut out = Self {
            loaded: true,
            speed_max_dots: 5,
            ..Self::default()
        };
        if index.records.is_empty() {
            return out;
        }
        let mut locale = Locale::default();
        match setup(index, "CellTemplate") {
            Ok(s) => out.template = s,
            Err(e) => out.errors.push(format!("CellTemplate: {e}")),
        }
        for level in 0..5 {
            let name = format!("CellEditorLevel{level}");
            match setup(index, &name) {
                Ok(s) => out.levels.push(s),
                Err(e) => {
                    out.errors.push(format!("{name}: {e}"));
                    out.levels.push(out.template.clone());
                }
            }
        }
        match setup(index, "CellEditorIntro") {
            Ok(s) => out.intro = s,
            Err(e) => out.errors.push(format!("CellEditorIntro: {e}")),
        }
        let mut palette_ids: Vec<u32> = out
            .levels
            .iter()
            .chain([&out.intro, &out.template])
            .filter_map(|s| s.parts_palette)
            .collect();
        palette_ids.sort();
        palette_ids.dedup();
        for id in palette_ids {
            out.palettes.insert(id, palette_pages(index, id));
        }
        // Every palette item plus its symmetry files.
        let mut wanted: Vec<u32> = out
            .palettes
            .values()
            .flatten()
            .flat_map(|p| p.items.iter().map(|i| i.part))
            .collect();
        let mut i = 0;
        while i < wanted.len() {
            let instance = wanted[i];
            i += 1;
            if out.parts.contains_key(&instance) {
                continue;
            }
            match part_info(index, &mut locale, instance) {
                Ok(info) => {
                    for f in [info.left, info.right, info.center].into_iter().flatten() {
                        if !wanted.contains(&f) {
                            wanted.push(f);
                        }
                    }
                    out.parts.insert(instance, info);
                }
                Err(e) => out.errors.push(format!("part {instance:08X}: {e}")),
            }
        }
        // Saved parts may use any Cell part definition (older saves, starting
        // parts): describe every installed one too.
        let palette_parts: Vec<u32> = out.parts.keys().copied().collect();
        let mut all: Vec<u32> = index
            .records
            .keys()
            .filter(|k| k.type_id == PROP && k.group_id == CELL_PARTS)
            .map(|k| k.instance_id)
            .collect();
        all.sort();
        for instance in all {
            if !out.parts.contains_key(&instance) {
                if let Ok(info) = part_info(index, &mut locale, instance) {
                    out.parts.insert(instance, info);
                }
            }
        }
        let mut by_model: Vec<(u32, u32, bool)> = out
            .parts
            .values()
            .filter(|i| i.model != 0)
            .map(|i| (i.model, i.key.instance_id, palette_parts.contains(&i.key.instance_id)))
            .collect();
        // Palette definitions first, then the lowest instance.
        by_model.sort_by_key(|(m, i, palette)| (*m, !*palette, *i));
        for (model, definition, _) in by_model {
            out.by_model.entry(model).or_insert(definition);
        }
        out.verbs = verbs(index, &mut locale, &out.parts);
        out.trays = trays(index);
        out.light = out
            .levels
            .first()
            .and_then(|s| s.light)
            .or(out.template.light)
            .and_then(|l| light(index, l));
        out.paint_light = light(index, name_id("CellEditorPaintMode"));
        out.part_sound_times = crate::cell_editor::clip_sound_times(index);
        if let Ok(p) = index.properties(prop_key(HINT_GROUP, name_id(CAMERA_HINT))) {
            let default = HintTiming::default();
            out.camera_hint = HintTiming {
                wait: number(&p, 0x0522_DE0C).unwrap_or(default.wait),
                life: number(&p, 0x0522_DE0B).unwrap_or(default.life),
                period: number(&p, 0x0522_DE0A).unwrap_or(default.period),
            };
        }
        out.paint = crate::cell_paint::CellPaintData::load(index);
        out.errors.extend(out.paint.errors.iter().cloned());
        out.name_prompt = locale.get(index, EDITOR_STRINGS, 0x0537_18CE);
        out.speed_caption = locale.get(index, EDITOR_STRINGS, 0x0000_0001);
        out.speed_tooltip = locale.get(index, EDITOR_STRINGS, 0x0000_0011);
        out.hint_needs_mouth = locale.get(index, name_id("HintsEDI"), 0x1F83_D13C);
        out.reason_expensive = locale.get(index, crate::creature_editor_data::EDITOR_LOCALE, 0x101);
        out.reason_parts = locale.get(index, crate::creature_editor_data::EDITOR_LOCALE, 0x102);
        out
    }

    /// Setup of a Cell level (0..=4), falling back to the template.
    pub fn level(&self, level: usize) -> &EditorSetup {
        self.levels.get(level.min(4)).unwrap_or(&self.template)
    }

    /// Definition of a saved part: its definition id, else its model's.
    pub fn definition_of(&self, part: &crate::game::PartInstance) -> Option<u32> {
        part.definition_id
            .filter(|d| self.parts.contains_key(d))
            .or_else(|| self.by_model.get(&part.instance).copied())
    }

    pub fn info_of(&self, part: &crate::game::PartInstance) -> Option<&CellPartInfo> {
        self.definition_of(part).and_then(|d| self.parts.get(&d))
    }

    pub fn palette(&self, id: u32) -> &[PalettePage] {
        self.palettes.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn complexity_limit(&self, level: usize) -> i32 {
        self.level(level)
            .complexity_limit
            .or(self.template.complexity_limit)
            .unwrap_or(300)
    }

    pub fn max_blocks(&self, level: usize) -> i32 {
        self.level(level)
            .max_baked_blocks
            .or(self.template.max_baked_blocks)
            .unwrap_or(65)
    }

    /// The file used for a placement: the centre variant on the centreline,
    /// otherwise the left/right file of the family, else the part itself.
    pub fn family_file(&self, part: u32, side: Side) -> u32 {
        let Some(info) = self.parts.get(&part) else {
            return part;
        };
        // A `-symmetric` palette item inherits its family's files.
        let pick = match side {
            Side::Center => info.center.or(info.left),
            Side::Left => info.left,
            Side::Right => info.right.or(info.left),
        };
        pick.filter(|f| self.parts.contains_key(f)).unwrap_or(part)
    }

    /// Abilities granted by a part (the verbs whose required parts list it).
    pub fn verbs_of(&self, part: u32) -> Vec<&VerbInfo> {
        let family = self.parts.get(&part);
        self.verbs
            .iter()
            .filter(|v| {
                v.parts.contains(&part)
                    || family.is_some_and(|f| {
                        [f.left, f.right, f.center]
                            .into_iter()
                            .flatten()
                            .any(|i| v.parts.contains(&i))
                    })
            })
            .collect()
    }
}

/// Which side of the body a placement is on (Bevy space: the cell faces -Z,
/// its right side is +X).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Center,
    Right,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlock_slots_follow_the_retail_record_unlock_table() {
        let h = name_id;
        assert_eq!(unlock_slot(PartClass::Mouth, h("jaw")), Some(2));
        assert_eq!(unlock_slot(PartClass::Mouth, h("filter")), Some(3));
        assert_eq!(unlock_slot(PartClass::Mouth, h("poker")), Some(4));
        assert_eq!(unlock_slot(PartClass::Weapon, h("chemical")), Some(5));
        assert_eq!(unlock_slot(PartClass::Weapon, h("electric")), Some(6));
        assert_eq!(unlock_slot(PartClass::Weapon, h("spike")), Some(7));
        assert_eq!(unlock_slot(PartClass::Movement, h("cilia")), Some(8));
        assert_eq!(unlock_slot(PartClass::Movement, h("flagella")), Some(9));
        assert_eq!(unlock_slot(PartClass::Movement, h("jet")), Some(10));
        assert_eq!(unlock_slot(PartClass::Eye, h("stalk")), Some(11));
        assert_eq!(unlock_slot(PartClass::Eye, h("ball")), None);
    }

    #[test]
    fn hint_timing_waits_shows_rests_and_repeats() {
        let t = HintTiming {
            wait: 5.0,
            life: 5.0,
            period: 5.0,
        };
        assert!(!t.showing(0.0) && !t.showing(4.9));
        assert!(t.showing(5.0) && t.showing(9.9));
        assert!(!t.showing(10.0) && !t.showing(14.9));
        assert!(t.showing(15.0), "shows again after its period");
    }

    #[test]
    fn key_and_int_arrays_decode_their_byte_orders() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        raw.extend_from_slice(&PROP.to_le_bytes());
        raw.extend_from_slice(&CELL_PARTS.to_le_bytes());
        let p = Property {
            kind: 0x20,
            array: true,
            values: vec![raw],
        };
        assert_eq!(keys(&p)[0], prop_key(CELL_PARTS, 0x1234_5678));
        let i = Property {
            kind: 9,
            array: true,
            values: vec![3i32.to_be_bytes().to_vec(), (-2i32).to_be_bytes().to_vec()],
        };
        assert_eq!(ints(&i), vec![3, -2]);
    }

    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_cell_editor_data_matches_the_retail_palette() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let data = CellEditorData::load(&index);
        assert!(data.errors.is_empty(), "{:?}", data.errors);
        assert_eq!(data.level(0).parts_palette, Some(GAME_PALETTE));
        assert_eq!(data.intro.initial_budget, Some(35));
        assert_eq!(data.complexity_limit(0), 300);
        assert_eq!(data.camera_hint, HintTiming { wait: 5.0, life: 5.0, period: 5.0 });
        let page = &data.palette(GAME_PALETTE)[0];
        assert_eq!(page.items.len(), 12);
        let name = |i: usize| data.parts[&page.items[i].part].name.clone();
        assert_eq!(name(0), "Filter Mouth");
        assert_eq!(name(1), "Jaw");
        assert_eq!(name(2), "Proboscis");
        assert_eq!(name(10), "Stalk Eye");
        let price = |i: usize| data.parts[&page.items[i].part].price;
        assert_eq!((price(0), price(2), price(3), price(9)), (15, 25, 10, 5));
        // Mouths and weapons replace each other within their snap delta.
        let (jaw, poker) = (&data.parts[&page.items[1].part], &data.parts[&page.items[2].part]);
        assert_eq!(jaw.snap_replace, vec![PartClass::Mouth, PartClass::Weapon]);
        assert!((jaw.replace_snap - 0.2).abs() < 1e-3 && (poker.replace_snap - 0.9).abs() < 1e-3);
        let stalk = &data.parts[&page.items[10].part];
        assert!(stalk.center.is_some() && stalk.right.is_some());
        assert!(data.verbs.iter().any(|v| v.name == "Herbivore"));
        assert!(data.level(4).pedestal.is_some());
        // The retail refusal strings (editor constants table 0x496BFB26, HintsEDI).
        assert_eq!(data.reason_expensive.as_deref(), Some("Too expensive"));
        assert_eq!(data.reason_parts.as_deref(), Some("Too many parts"));
        assert_eq!(data.hint_needs_mouth.as_deref(), Some("Your creation needs a mouth"));
    }
}
