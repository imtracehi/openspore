//! Retail Cell-stage data model: typed decoders for the twelve Cell resource
//! types and the 128x128 advection maps, plus [`CellDatabase`], the complete
//! cross-referenced set as installed.
//!
//! Layouts are the serialized structs of Spore ModAPI
//! `Simulator/Cell/cCellResource.h` with every pointer field stored as a
//! file-relative offset, read in the field order of SporeModder-FX
//! `file/cell/Cell*File.java`. Where those two sources disagree the installed
//! data decides, and the field says so:
//! - the cell record stores `loot` at 0xD4 and `explosionTable` at 0xD8 (the
//!   SMFX order; ModAPI names them the other way round, but every installed
//!   creature's 0xD4 is a `loot_*` part-drop table and every rock's 0xD8 is a
//!   `loot_*_chunks` table);
//! - world advect `strength`/`variance`/`period` and the marker
//!   `encounterScale` are floats (ModAPI declares `int`; the bytes are floats).
//!
//! References stay instance IDs (0 = none) and resolve through the database.
//! This module is pure original data: it never invents values. Gameplay lives
//! in `cell_engine`.

use crate::{
    asset_catalog::ResourceIndex,
    dbpf::{self, ResourceKey},
};
use bevy::prelude::Resource;
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const TYPE_GLOBALS: u32 = 0x2A3C_E5B7;
pub const TYPE_WORLD: u32 = 0x9B8E_862F;
pub const TYPE_POPULATE: u32 = 0xDA14_1C1B;
pub const TYPE_CELL: u32 = 0xDFAD_9F51;
pub const TYPE_LOOT_TABLE: u32 = 0xD92A_F091;
pub const TYPE_LOOK_TABLE: u32 = 0x8C04_2499;
pub const TYPE_LOOK_ALGORITHM: u32 = 0xDBA3_5AE2;
pub const TYPE_EFFECT_MAP: u32 = 0x433F_B70C;
pub const TYPE_BACKGROUND_MAP: u32 = 0x612B_3191;
pub const TYPE_POWERS: u32 = 0x754B_E343;
pub const TYPE_RANDOM_CREATURE: u32 = 0xF9C3_D770;
pub const TYPE_STRUCTURE: u32 = 0x4B9E_F6DC;
/// `.advect` (SporeModder-FX `reg_type.txt`): a raw 128x128 `Vector3` field.
pub const TYPE_ADVECT: u32 = 0x0480_5684;
/// Every installed Cell record lives in group 0 (`PatchData.package`).
pub const CELL_DATA_GROUP: u32 = 0;
/// `.cll` cell creations referenced by structure `creature` attachments
/// (group 0 per ModAPI `GetModelKeyForCellResource`).
pub const TYPE_CELL_CREATION: u32 = 0x3D97_A8E4;
/// Locale table holding the cell `name.localeInstanceID` strings
/// (SporeModder-FX `CellFile.cLocalizedString`; verified in `Text.package`).
pub const CELL_NAME_LOCALE_TABLE: u32 = 0x04DC_2F94;
const LOCALE_TEXT: u32 = 0x02FA_C0B6;

/// Side of the square advect vector field.
pub const ADVECT_SIDE: usize = 128;

// ---------------------------------------------------------------------------
// Byte reader / writer
// ---------------------------------------------------------------------------

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| {
                format!(
                    "truncated at {:#x}+{n} of {:#x} bytes",
                    self.at,
                    self.bytes.len()
                )
            })?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }
    fn seek(&mut self, at: usize) -> Result<(), String> {
        if at > self.bytes.len() {
            return Err(format!("offset {at:#x} beyond {:#x} bytes", self.bytes.len()));
        }
        self.at = at;
        Ok(())
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    fn f32(&mut self) -> Result<f32, String> {
        let at = self.at;
        let value = f32::from_bits(self.u32()?);
        if value.is_finite() {
            Ok(value)
        } else {
            Err(format!("non-finite float at {at:#x}"))
        }
    }
    /// A one-byte C++ `bool`; serialized values are exactly 0 or 1.
    fn bool8(&mut self) -> Result<bool, String> {
        let at = self.at;
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(format!("bool byte {other} at {at:#x}")),
        }
    }
    fn skip(&mut self, n: usize) -> Result<(), String> {
        self.take(n).map(|_| ())
    }
    fn finish(&self) -> Result<(), String> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(format!(
                "{} unread trailing bytes",
                self.bytes.len() - self.at
            ))
        }
    }
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.u32(v as u32);
    }
    fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    fn bool8(&mut self, v: bool) {
        self.0.push(v as u8);
    }
    fn pad(&mut self, n: usize) {
        self.0.extend(std::iter::repeat_n(0u8, n));
    }
}

/// Count + offset pair of a serialized array. An empty array may be written
/// with offset 0 or with the offset it would have had.
fn array_at(count: u32, offset: u32, expected: usize, what: &str) -> Result<usize, String> {
    if count > 65_536 {
        return Err(format!("{what}: implausible count {count}"));
    }
    if count == 0 && (offset == 0 || offset as usize == expected) {
        return Ok(expected);
    }
    if offset as usize != expected {
        return Err(format!(
            "{what}: array offset {offset:#x}, expected {expected:#x}"
        ));
    }
    Ok(expected)
}

fn hex<S: Serializer>(id: &u32, serializer: S) -> Result<S::Ok, S::Error> {
    if *id == 0 {
        serializer.serialize_none()
    } else {
        serializer.serialize_str(&format!("0x{id:08X}"))
    }
}

// ---------------------------------------------------------------------------
// Enumerations (exact ModAPI values; unknown raw values are decode errors)
// ---------------------------------------------------------------------------

macro_rules! raw_enum {
    ($(#[$meta:meta])* pub enum $name:ident : $what:literal {
        $($variant:ident = $value:literal => $label:literal),+ $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name { $($variant),+ }
        impl $name {
            #[allow(dead_code)]
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn from_raw(raw: i32) -> Result<Self, String> {
                match raw {
                    $($value => Ok(Self::$variant),)+
                    other => Err(format!("unknown {} {other}", $what)),
                }
            }
            pub fn raw(self) -> i32 {
                match self { $(Self::$variant => $value),+ }
            }
            pub fn label(self) -> &'static str {
                match self { $(Self::$variant => $label),+ }
            }
        }
        impl Default for $name {
            /// The first listed value (the "none" value where one exists).
            fn default() -> Self {
                Self::ALL[0]
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.label())
            }
        }
    };
}

raw_enum! {
    /// `cCellCellResource::cAIData::Type`.
    pub enum AiType: "AI type" {
        None = -1 => "none",
        Player = 0 => "player",
        Parts = 0x1001 => "parts",
        Straight = 0x1002 => "straight",
        SlowSeek = 0x1003 => "slowSeek",
        Orbit = 0x1004 => "orbit",
        Chomp = 0x1005 => "chomp",
        Eater = 0x1006 => "eater",
        Parasite = 0x1007 => "parasite",
        Caterpillar = 0x1008 => "caterpillar",
        Sleeper = 0x1009 => "sleeper",
        Waterballoon = 0x100A => "waterballoon",
        Mosquito = 0x100B => "mosquito",
        Dragon = 0x100C => "dragon",
        Poolball = 0x100D => "poolball",
        Leak = 0x100E => "leak",
    }
}

raw_enum! {
    /// `cAIData::FoodType` (what the cell eats).
    pub enum FoodType: "food type" {
        Null = -1 => "null",
        None = 0 => "none",
        Rock = 1 => "rock",
        Solid = 2 => "solid",
        Liquid = 3 => "liquid",
        Air = 4 => "air",
    }
}

raw_enum! {
    /// `cCellCellResource::CellType` (what the cell *is* as food/object).
    pub enum CellType: "cell type" {
        None = 0 => "none",
        Rock = 1 => "rock",
        Solid = 2 => "solid",
        Liquid = 3 => "liquid",
        Air = 4 => "air",
        Poison = 5 => "poison",
        Unlock = 6 => "unlock",
        Nanite = 7 => "nanite",
    }
}

raw_enum! {
    /// `cCellCellResource::Sound`.
    pub enum CellSound: "cell sound" {
        None = 0 => "none",
        Creature = 1 => "creature",
        Plant = 2 => "plant",
        Rock = 3 => "rock",
        Bubble = 4 => "bubble",
    }
}

raw_enum! {
    /// `cCellCellResource::Density` (indexes the Globals density constants).
    pub enum Density: "density" {
        None = 0 => "none",
        Liquid = 1 => "liquid",
        Air = 2 => "air",
        Solid = 3 => "solid",
        Rock = 4 => "rock",
    }
}

raw_enum! {
    /// `cCellCellResource::UnlockType` (the part category an Unlock cell gives).
    pub enum UnlockType: "unlock type" {
        None = 0 => "none",
        Mandible = 2 => "mandible",
        Filter = 3 => "filter",
        Proboscis = 4 => "proboscis",
        Chemical = 5 => "chemical",
        Electric = 6 => "electric",
        Spike = 7 => "spike",
        Cilia = 8 => "cilia",
        Flagella = 9 => "flagella",
        Jet = 10 => "jet",
        Body = 12 => "body",
    }
}

raw_enum! {
    /// `cCellPopulateResource::cMarker::Type`.
    pub enum MarkerType: "marker type" {
        Distribute = 0 => "distribute",
        Cluster = 1 => "cluster",
        Plant = 2 => "plant",
        Encounter = 3 => "encounter",
    }
}

raw_enum! {
    /// `cCellPopulateResource::cMarker::PlantType`.
    pub enum PlantType: "plant type" {
        Type1 = 0 => "type1",
        Type2 = 1 => "type2",
        Type3 = 2 => "type3",
        Type4 = 3 => "type4",
        Type5 = 4 => "type5",
        Type6 = 5 => "type6",
    }
}

raw_enum! {
    /// `cCellLootTableResource::cLootTableEntry::Type`.
    pub enum LootEntryType: "loot entry type" {
        Nothing = 0 => "nothing",
        Cell = 1 => "cell",
        Table = 2 => "table",
    }
}

raw_enum! {
    /// `cCellLookTableResource::cLookTableEntry::Type` (what an eye looks at).
    pub enum LookTarget: "look target" {
        Up = 0 => "up",
        Forward = 1 => "forward",
        Left = 2 => "left",
        Right = 3 => "right",
        PursueTarget = 4 => "pursue_target",
        FleeTarget = 5 => "flee_target",
        Object = 6 => "object",
        Food = 7 => "food",
        Predator = 8 => "predator",
        Destination = 9 => "destination",
        Cursor = 10 => "cursor",
        Player = 11 => "player",
    }
}

raw_enum! {
    /// `cCellLookAlgorithmResource::cLookAlgorithmEntry::Type`.
    pub enum LookSubject: "look subject" {
        Player = 0 => "player",
        Npc = 1 => "npc",
        Epic = 2 => "epic",
    }
}

raw_enum! {
    /// `cCellLookAlgorithmResource::cLookAlgorithmEntry::Action`.
    pub enum LookAction: "look action" {
        Other = -1 => "other",
        Idle = 0 => "idle",
        Moving = 0x12 => "moving",
        MoveScared = 0x13 => "move_scared",
        MoveMad = 0x14 => "move_mad",
        Scared = 0x15 => "scared",
        Mad = 0x16 => "mad",
    }
}

raw_enum! {
    /// `cCellEffectMapResource::cEffectMapEntry::Type`.
    pub enum EffectMapType: "effect map type" {
        Foreground = 0 => "foreground",
        Background = 1 => "background",
        Tracer = 2 => "tracer",
        TracerBackground = 3 => "tracer_background",
        Skybox = 4 => "skybox",
        TileBackground = 5 => "tile_background",
    }
}

raw_enum! {
    /// `cCellStructureResource::cSPAttachment::Type`.
    pub enum AttachmentType: "attachment type" {
        Model = 0 => "model",
        Effect = 1 => "effect",
        Structure = 2 => "structure",
        Creature = 3 => "creature",
        RandomCreature = 4 => "random_creature",
        PlayerCreature = 5 => "player_creature",
        Debug = 6 => "debug",
    }
}

raw_enum! {
    /// `cCellRandomCreatureResource::cRandomCreatureEntry::Type`.
    pub enum RandomCreatureType: "random creature type" {
        Creature = 0 => "creature",
        Pollinated = 1 => "pollinated",
    }
}

/// `CellStageScale`: the size ladder (`_0, _1, _2, _4, _10 ... _1M`) or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellScale(pub i32);

impl Default for CellScale {
    fn default() -> Self {
        Self::NONE
    }
}

impl CellScale {
    pub const NONE: Self = Self(-1);
    /// Largest ladder index (`_1M`).
    pub const MAX: i32 = 19;
    const LABELS: [&'static str; 20] = [
        "0", "1", "2", "4", "10", "20", "40", "100", "200", "400", "1K", "2K", "4K", "10K", "20K",
        "40K", "100K", "200K", "400K", "1M",
    ];
    const NOMINAL: [f32; 20] = [
        0.0, 1.0, 2.0, 4.0, 10.0, 20.0, 40.0, 100.0, 200.0, 400.0, 1e3, 2e3, 4e3, 1e4, 2e4, 4e4,
        1e5, 2e5, 4e5, 1e6,
    ];
    pub fn from_raw(raw: i32) -> Result<Self, String> {
        if (-1..=Self::MAX).contains(&raw) {
            Ok(Self(raw))
        } else {
            Err(format!("unknown CellStageScale {raw}"))
        }
    }
    pub fn is_none(self) -> bool {
        self.0 < 0
    }
    pub fn index(self) -> i32 {
        self.0
    }
    pub fn label(self) -> &'static str {
        if self.is_none() {
            "none"
        } else {
            Self::LABELS[self.0 as usize]
        }
    }
    /// The size the enum name spells (`_10` -> 10). Whether the engine uses
    /// these literally is a question for the RE notes; see `cell_engine`.
    pub fn nominal(self) -> Option<f32> {
        (!self.is_none()).then(|| Self::NOMINAL[self.0 as usize])
    }
    /// Ladder step `by` levels away, clamped to the ladder (none stays none).
    pub fn offset(self, by: i32) -> Self {
        if self.is_none() {
            self
        } else {
            Self((self.0 + by).clamp(0, Self::MAX))
        }
    }
}

impl Serialize for CellScale {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.label())
    }
}

/// `cAIData::MovementStyle`: flag bits flagella=1, cilia=2, jet=4,
/// turn-in-place=8 (combinations are named in ModAPI).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct MovementStyle(pub u32);

impl MovementStyle {
    pub const FLAGELLA: u32 = 1;
    pub const CILIA: u32 = 2;
    pub const JET: u32 = 4;
    pub const TURN_IN_PLACE: u32 = 8;
    pub fn from_raw(raw: i32) -> Result<Self, String> {
        if (0..=15).contains(&raw) {
            Ok(Self(raw as u32))
        } else {
            Err(format!("unknown movement style {raw}"))
        }
    }
    pub fn flagella(self) -> bool {
        self.0 & Self::FLAGELLA != 0
    }
    pub fn cilia(self) -> bool {
        self.0 & Self::CILIA != 0
    }
    pub fn jet(self) -> bool {
        self.0 & Self::JET != 0
    }
    pub fn turn_in_place(self) -> bool {
        self.0 & Self::TURN_IN_PLACE != 0
    }
    pub fn label(self) -> String {
        if self.0 == 0 {
            return "default".into();
        }
        let mut parts = Vec::new();
        for (bit, name) in [
            (Self::FLAGELLA, "flagella"),
            (Self::CILIA, "cillia"),
            (Self::JET, "jet"),
            (Self::TURN_IN_PLACE, "turn_in_place"),
        ] {
            if self.0 & bit != 0 {
                parts.push(name);
            }
        }
        parts.join("_")
    }
}

impl Serialize for MovementStyle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.label())
    }
}

// ---------------------------------------------------------------------------
// Globals (0x114 bytes)
// ---------------------------------------------------------------------------

/// `cCellGlobalsResource` — field names are the ModAPI names.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellGlobals {
    pub game_mode: i32,
    /// `world_1..world_5`.
    #[serde(serialize_with = "hex_array")]
    pub worlds: [u32; 5],
    /// `worldBackground_1..worldBackground_5`.
    #[serde(serialize_with = "hex_array")]
    pub world_backgrounds: [u32; 5],
    #[serde(serialize_with = "hex")]
    pub world_random: u32,
    #[serde(serialize_with = "hex")]
    pub world_random_bg: u32,
    #[serde(serialize_with = "hex")]
    pub start_cell: u32,
    #[serde(serialize_with = "hex")]
    pub starting_cell_key: u32,
    #[serde(serialize_with = "hex")]
    pub effect_map_entry: u32,
    #[serde(serialize_with = "hex")]
    pub background_map_entry: u32,
    pub flow_multiplier: f32,
    pub npc_speed_multiplier: f32,
    pub npc_turn_speed_multiplier_jet: f32,
    pub npc_turn_speed_multiplier_flagella: f32,
    pub npc_turn_speed_multiplier_cilia: f32,
    pub density_rock: f32,
    pub density_solid: f32,
    pub density_liquid: f32,
    pub density_air: f32,
    pub background_distance: f32,
    pub min_drag_collision_speed: f32,
    pub min_impact_collision_speed: f32,
    pub cilia_speed_as_jet: f32,
    pub flagella_speed_as_jet: f32,
    pub flagella_speed_as_cilia: f32,
    pub cilia_speed_as_flagella: f32,
    #[serde(serialize_with = "hex")]
    pub key_look_algorithm: u32,
    pub beach_distance: f32,
    pub finish_line_distance: f32,
    pub no_part_speed: f32,
    pub flagella_ramp_min_factor: f32,
    pub flagella_ramp_time: f32,
    pub flagella_ramp_reset_angle: f32,
    pub flagella_turn_speed_ramp_start: f32,
    pub flagella_turn_speed_ramp_end: f32,
    pub flagella_turn_speed_min: f32,
    pub flagella_turn_speed_max: f32,
    pub cilia_turn_speed: f32,
    pub jet_turn_speed: f32,
    pub start_level_no_creature_radius: f32,
    pub start_level_no_anything_radius: f32,
    #[serde(rename = "numHighLOD_FG")]
    pub num_high_lod_fg: i32,
    #[serde(rename = "numHighLOD_BG")]
    pub num_high_lod_bg: i32,
    pub percent_animal_food: f32,
    pub percent_plant_food: f32,
    pub field_208: i32,
    pub control_method: i32,
    pub editor_method: i32,
    pub tutorial_method: i32,
    pub ending_method: i32,
    /// 1 = a cell with fewer than 3 eyes is blind (ModAPI `EyeMethod`).
    pub eye_method: i32,
    pub time_to_goldy_cinematic: f32,
    pub mission_time: f32,
    pub mission_reset_time: f32,
    pub escape_min_distance: f32,
    pub escape_max_distance: f32,
    pub escape_delay_medium: f32,
    pub escape_timer_hard: f32,
    pub non_animating_cilia_movement_factor: f32,
    pub non_animating_jet_movement_factor: f32,
    pub mate_trigger_distance: f32,
    pub mate_spawn_distance: f32,
}

fn hex_array<S: Serializer, const N: usize>(ids: &[u32; N], serializer: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(N))?;
    for id in ids {
        if *id == 0 {
            seq.serialize_element(&Option::<String>::None)?;
        } else {
            seq.serialize_element(&format!("0x{id:08X}"))?;
        }
    }
    seq.end()
}

impl CellGlobals {
    pub const SIZE: usize = 0x114;

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != Self::SIZE {
            return Err(format!("globals: {} bytes, expected 0x114", bytes.len()));
        }
        let mut r = Reader::new(bytes);
        let game_mode = r.i32()?;
        let mut worlds = [0; 5];
        for w in &mut worlds {
            *w = r.u32()?;
        }
        let mut world_backgrounds = [0; 5];
        for w in &mut world_backgrounds {
            *w = r.u32()?;
        }
        let g = Self {
            game_mode,
            worlds,
            world_backgrounds,
            world_random: r.u32()?,
            world_random_bg: r.u32()?,
            start_cell: r.u32()?,
            starting_cell_key: r.u32()?,
            effect_map_entry: r.u32()?,
            background_map_entry: r.u32()?,
            flow_multiplier: r.f32()?,
            npc_speed_multiplier: r.f32()?,
            npc_turn_speed_multiplier_jet: r.f32()?,
            npc_turn_speed_multiplier_flagella: r.f32()?,
            npc_turn_speed_multiplier_cilia: r.f32()?,
            density_rock: r.f32()?,
            density_solid: r.f32()?,
            density_liquid: r.f32()?,
            density_air: r.f32()?,
            background_distance: r.f32()?,
            min_drag_collision_speed: r.f32()?,
            min_impact_collision_speed: r.f32()?,
            cilia_speed_as_jet: r.f32()?,
            flagella_speed_as_jet: r.f32()?,
            flagella_speed_as_cilia: r.f32()?,
            cilia_speed_as_flagella: r.f32()?,
            key_look_algorithm: r.u32()?,
            beach_distance: r.f32()?,
            finish_line_distance: r.f32()?,
            no_part_speed: r.f32()?,
            flagella_ramp_min_factor: r.f32()?,
            flagella_ramp_time: r.f32()?,
            flagella_ramp_reset_angle: r.f32()?,
            flagella_turn_speed_ramp_start: r.f32()?,
            flagella_turn_speed_ramp_end: r.f32()?,
            flagella_turn_speed_min: r.f32()?,
            flagella_turn_speed_max: r.f32()?,
            cilia_turn_speed: r.f32()?,
            jet_turn_speed: r.f32()?,
            start_level_no_creature_radius: r.f32()?,
            start_level_no_anything_radius: r.f32()?,
            num_high_lod_fg: r.i32()?,
            num_high_lod_bg: r.i32()?,
            percent_animal_food: r.f32()?,
            percent_plant_food: r.f32()?,
            field_208: r.i32()?,
            control_method: r.i32()?,
            editor_method: r.i32()?,
            tutorial_method: r.i32()?,
            ending_method: r.i32()?,
            eye_method: r.i32()?,
            time_to_goldy_cinematic: r.f32()?,
            mission_time: r.f32()?,
            mission_reset_time: r.f32()?,
            escape_min_distance: r.f32()?,
            escape_max_distance: r.f32()?,
            escape_delay_medium: r.f32()?,
            escape_timer_hard: r.f32()?,
            non_animating_cilia_movement_factor: r.f32()?,
            non_animating_jet_movement_factor: r.f32()?,
            mate_trigger_distance: r.f32()?,
            mate_spawn_distance: r.f32()?,
        };
        r.finish()?;
        Ok(g)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.game_mode);
        for id in self.worlds.iter().chain(&self.world_backgrounds) {
            w.u32(*id);
        }
        for id in [
            self.world_random,
            self.world_random_bg,
            self.start_cell,
            self.starting_cell_key,
            self.effect_map_entry,
            self.background_map_entry,
        ] {
            w.u32(id);
        }
        for v in [
            self.flow_multiplier,
            self.npc_speed_multiplier,
            self.npc_turn_speed_multiplier_jet,
            self.npc_turn_speed_multiplier_flagella,
            self.npc_turn_speed_multiplier_cilia,
            self.density_rock,
            self.density_solid,
            self.density_liquid,
            self.density_air,
            self.background_distance,
            self.min_drag_collision_speed,
            self.min_impact_collision_speed,
            self.cilia_speed_as_jet,
            self.flagella_speed_as_jet,
            self.flagella_speed_as_cilia,
            self.cilia_speed_as_flagella,
        ] {
            w.f32(v);
        }
        w.u32(self.key_look_algorithm);
        for v in [
            self.beach_distance,
            self.finish_line_distance,
            self.no_part_speed,
            self.flagella_ramp_min_factor,
            self.flagella_ramp_time,
            self.flagella_ramp_reset_angle,
            self.flagella_turn_speed_ramp_start,
            self.flagella_turn_speed_ramp_end,
            self.flagella_turn_speed_min,
            self.flagella_turn_speed_max,
            self.cilia_turn_speed,
            self.jet_turn_speed,
            self.start_level_no_creature_radius,
            self.start_level_no_anything_radius,
        ] {
            w.f32(v);
        }
        w.i32(self.num_high_lod_fg);
        w.i32(self.num_high_lod_bg);
        w.f32(self.percent_animal_food);
        w.f32(self.percent_plant_food);
        for v in [
            self.field_208,
            self.control_method,
            self.editor_method,
            self.tutorial_method,
            self.ending_method,
            self.eye_method,
        ] {
            w.i32(v);
        }
        for v in [
            self.time_to_goldy_cinematic,
            self.mission_time,
            self.mission_reset_time,
            self.escape_min_distance,
            self.escape_max_distance,
            self.escape_delay_medium,
            self.escape_timer_hard,
            self.non_animating_cilia_movement_factor,
            self.non_animating_jet_movement_factor,
            self.mate_trigger_distance,
            self.mate_spawn_distance,
        ] {
            w.f32(v);
        }
        w.0
    }

    /// Density constant for a cell density class (ModAPI `density*`).
    pub fn density(&self, density: Density) -> f32 {
        match density {
            Density::None => 0.0,
            Density::Liquid => self.density_liquid,
            Density::Air => self.density_air,
            Density::Solid => self.density_solid,
            Density::Rock => self.density_rock,
        }
    }
}

// ---------------------------------------------------------------------------
// World
// ---------------------------------------------------------------------------

/// `cCellWorldResource::cLevelEntry`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellLevelEntry {
    #[serde(serialize_with = "hex")]
    pub populate: u32,
    pub start_tile: bool,
    pub player_size: CellScale,
}

/// `cCellWorldResource::cAdvectEntry` (floats on disk, see module docs).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellAdvectEntry {
    /// ModAPI `field_0` (a CellStageScale; SMFX writes it as the second
    /// `advect` argument).
    pub scale: CellScale,
    pub player_size: CellScale,
    pub strength: f32,
    pub variance: f32,
    pub period: f32,
    #[serde(serialize_with = "hex")]
    pub advect: u32,
}

/// `cCellWorldResource`: the level ladder of one world plus its currents.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellWorld {
    pub levels: Vec<CellLevelEntry>,
    pub advect: Vec<CellAdvectEntry>,
}

impl CellWorld {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let level_count = r.u32()?;
        let level_offset = r.u32()?;
        let advect_count = r.u32()?;
        let advect_offset = r.u32()?;
        let levels_at = array_at(level_count, level_offset, 16, "world levels")?;
        let advect_at = array_at(
            advect_count,
            advect_offset,
            levels_at + level_count as usize * 12,
            "world advect",
        )?;
        r.seek(levels_at)?;
        let mut levels = Vec::with_capacity(level_count as usize);
        for _ in 0..level_count {
            let populate = r.u32()?;
            let start_tile = r.bool8()?;
            r.skip(3)?;
            levels.push(CellLevelEntry {
                populate,
                start_tile,
                player_size: CellScale::from_raw(r.i32()?)?,
            });
        }
        r.seek(advect_at)?;
        let mut advect = Vec::with_capacity(advect_count as usize);
        for _ in 0..advect_count {
            advect.push(CellAdvectEntry {
                scale: CellScale::from_raw(r.i32()?)?,
                player_size: CellScale::from_raw(r.i32()?)?,
                strength: r.f32()?,
                variance: r.f32()?,
                period: r.f32()?,
                advect: r.u32()?,
            });
        }
        r.finish()?;
        Ok(Self { levels, advect })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(self.levels.len() as u32);
        w.u32(if self.levels.is_empty() { 0 } else { 16 });
        w.u32(self.advect.len() as u32);
        w.u32(if self.advect.is_empty() {
            0
        } else {
            16 + self.levels.len() as u32 * 12
        });
        for level in &self.levels {
            w.u32(level.populate);
            w.bool8(level.start_tile);
            w.pad(3);
            w.i32(level.player_size.0);
        }
        for a in &self.advect {
            w.i32(a.scale.0);
            w.i32(a.player_size.0);
            w.f32(a.strength);
            w.f32(a.variance);
            w.f32(a.period);
            w.u32(a.advect);
        }
        w.0
    }
}

// ---------------------------------------------------------------------------
// Populate
// ---------------------------------------------------------------------------

/// `cCellPopulateResource::cMarker` (0x4C bytes).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellMarker {
    pub kind: MarkerType,
    pub field_0: i32,
    pub field_4: i32,
    pub field_8: i32,
    pub z_offset: f32,
    pub z_offset_max: f32,
    pub field_14: i32,
    #[serde(serialize_with = "hex")]
    pub distribute_cell: u32,
    #[serde(serialize_with = "hex")]
    pub cluster_cell: u32,
    #[serde(serialize_with = "hex")]
    pub encounter_populate: u32,
    pub plant_type: PlantType,
    /// Count for every difficulty; -1 = unused (then the per-difficulty
    /// count applies).
    pub count: f32,
    pub count_easy: f32,
    pub count_med: f32,
    pub count_hard: f32,
    /// Size level of the placed objects.
    pub size: CellScale,
    pub parts: i32,
    pub linear: i32,
    /// Float on disk (ModAPI declares `int`).
    pub encounter_scale: f32,
}

impl CellMarker {
    pub const SIZE: usize = 0x4C;

    /// The cell this marker places (distribute or cluster), if any.
    pub fn cell(&self) -> Option<u32> {
        match self.kind {
            MarkerType::Distribute => Some(self.distribute_cell),
            MarkerType::Cluster => Some(self.cluster_cell),
            _ => None,
        }
        .filter(|id| *id != 0)
    }

    /// `Count(marker)` (0xE5A910, RE 5) for difficulty 0=easy, 1=medium,
    /// 2=hard: that difficulty's own count when it is not -1, else `count`
    /// (-1 -> 0; every count -1 -> 1). `None` when the result is 0.
    pub fn count_for(&self, difficulty: usize) -> Option<f32> {
        let own = match difficulty {
            0 => self.count_easy,
            2 => self.count_hard,
            _ => self.count_med,
        };
        let n = if own != -1.0 {
            own
        } else if self.count != -1.0 {
            self.count
        } else if self.count_easy == -1.0 && self.count_med == -1.0 && self.count_hard == -1.0 {
            1.0
        } else {
            0.0
        };
        (n > 0.0).then_some(n)
    }
}

/// `cCellPopulateResource`: one level's object recipe.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellPopulate {
    pub scale: CellScale,
    #[serde(serialize_with = "hex")]
    pub mask_texture: u32,
    pub markers: Vec<CellMarker>,
}

impl CellPopulate {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let scale = CellScale::from_raw(r.i32()?)?;
        let mask_texture = r.u32()?;
        let count = r.u32()?;
        let offset = r.u32()?;
        r.seek(array_at(count, offset, 16, "populate markers")?)?;
        let mut markers = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let field_0 = r.i32()?;
            let field_4 = r.i32()?;
            let field_8 = r.i32()?;
            let z_offset = r.f32()?;
            let z_offset_max = r.f32()?;
            let field_14 = r.i32()?;
            let distribute_cell = r.u32()?;
            let cluster_cell = r.u32()?;
            let encounter_populate = r.u32()?;
            let plant_type = PlantType::from_raw(r.i32()?)?;
            let kind = MarkerType::from_raw(r.i32()?)?;
            markers.push(CellMarker {
                kind,
                field_0,
                field_4,
                field_8,
                z_offset,
                z_offset_max,
                field_14,
                distribute_cell,
                cluster_cell,
                encounter_populate,
                plant_type,
                count: r.f32()?,
                count_easy: r.f32()?,
                count_med: r.f32()?,
                count_hard: r.f32()?,
                size: CellScale::from_raw(r.i32()?)?,
                parts: r.i32()?,
                linear: r.i32()?,
                encounter_scale: r.f32()?,
            });
        }
        r.finish()?;
        Ok(Self {
            scale,
            mask_texture,
            markers,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.scale.0);
        w.u32(self.mask_texture);
        w.u32(self.markers.len() as u32);
        // Empty marker lists are stored with offset 0 in the install.
        w.u32(if self.markers.is_empty() { 0 } else { 16 });
        for m in &self.markers {
            w.i32(m.field_0);
            w.i32(m.field_4);
            w.i32(m.field_8);
            w.f32(m.z_offset);
            w.f32(m.z_offset_max);
            w.i32(m.field_14);
            w.u32(m.distribute_cell);
            w.u32(m.cluster_cell);
            w.u32(m.encounter_populate);
            w.i32(m.plant_type.raw());
            w.i32(m.kind.raw());
            w.f32(m.count);
            w.f32(m.count_easy);
            w.f32(m.count_med);
            w.f32(m.count_hard);
            w.i32(m.size.0);
            w.i32(m.parts);
            w.i32(m.linear);
            w.f32(m.encounter_scale);
        }
        w.0
    }
}

// ---------------------------------------------------------------------------
// Cell (0x31C bytes)
// ---------------------------------------------------------------------------

/// `cCellCellResource::cAIData` (0xB4 bytes).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellAi {
    #[serde(rename = "type")]
    pub kind: AiType,
    pub awareness_radius: f32,
    pub awareness_radius_food: f32,
    pub awareness_radius_predator: f32,
    pub movement_style: MovementStyle,
    pub flocking: bool,
    pub speed: f32,
    pub chase_speed: f32,
    pub wander_speed: f32,
    pub flee_speed: f32,
    pub fears_nearby_damage_radius: f32,
    pub fears_nearby_death_radius: f32,
    pub fears_nearby_damage_time: f32,
    pub fears_nearby_death_time: f32,
    pub protect_radius: f32,
    pub protect_time: f32,
    pub turn_factor: f32,
    pub axial_movement: bool,
    pub spawn_time: f32,
    pub spawn_rest_time: f32,
    #[serde(serialize_with = "hex")]
    pub spawn_output: u32,
    pub arc_length: f32,
    pub arc_length_secondary: f32,
    pub num_arcs: i32,
    #[serde(serialize_with = "hex")]
    pub key_transformation: u32,
    #[serde(serialize_with = "hex")]
    pub key_projectile: u32,
    pub food: FoodType,
    pub grow_count: i32,
    pub digestion_count: i32,
    pub digestion_time: f32,
    #[serde(serialize_with = "hex")]
    pub digestion_output: u32,
    pub flee_time: f32,
    pub flee_rest_time: f32,
    pub chase_time: f32,
    pub chase_rest_time: f32,
    pub chases_damage: bool,
    pub fears_mouths: bool,
    pub fears_weapons: bool,
    pub fears_electric: bool,
    pub fears_poison: bool,
    pub fears_damage: bool,
    pub ignores_food: bool,
    pub awake_time: f32,
    pub sleep_time: f32,
    pub grow_amount: i32,
    pub hatch_duration: f32,
    pub poison_recharge: f32,
    pub electric_recharge: f32,
    pub electric_recharge_vs_small: f32,
    pub electric_discharge: f32,
}

impl CellAi {
    pub const SIZE: usize = 0xB4;

    fn read(r: &mut Reader) -> Result<Self, String> {
        let kind = AiType::from_raw(r.i32()?)?;
        let awareness_radius = r.f32()?;
        let awareness_radius_food = r.f32()?;
        let awareness_radius_predator = r.f32()?;
        let movement_style = MovementStyle::from_raw(r.i32()?)?;
        let flocking = r.bool8()?;
        r.skip(3)?;
        let speed = r.f32()?;
        let chase_speed = r.f32()?;
        let wander_speed = r.f32()?;
        let flee_speed = r.f32()?;
        let fears_nearby_damage_radius = r.f32()?;
        let fears_nearby_death_radius = r.f32()?;
        let fears_nearby_damage_time = r.f32()?;
        let fears_nearby_death_time = r.f32()?;
        let protect_radius = r.f32()?;
        let protect_time = r.f32()?;
        let turn_factor = r.f32()?;
        let axial_movement = r.bool8()?;
        r.skip(3)?;
        let spawn_time = r.f32()?;
        let spawn_rest_time = r.f32()?;
        let spawn_output = r.u32()?;
        let arc_length = r.f32()?;
        let arc_length_secondary = r.f32()?;
        let num_arcs = r.i32()?;
        let key_transformation = r.u32()?;
        let key_projectile = r.u32()?;
        let food = FoodType::from_raw(r.i32()?)?;
        let grow_count = r.i32()?;
        let digestion_count = r.i32()?;
        let digestion_time = r.f32()?;
        let digestion_output = r.u32()?;
        let flee_time = r.f32()?;
        let flee_rest_time = r.f32()?;
        let chase_time = r.f32()?;
        let chase_rest_time = r.f32()?;
        let chases_damage = r.bool8()?;
        let fears_mouths = r.bool8()?;
        let fears_weapons = r.bool8()?;
        let fears_electric = r.bool8()?;
        let fears_poison = r.bool8()?;
        let fears_damage = r.bool8()?;
        let ignores_food = r.bool8()?;
        r.skip(1)?;
        Ok(Self {
            kind,
            awareness_radius,
            awareness_radius_food,
            awareness_radius_predator,
            movement_style,
            flocking,
            speed,
            chase_speed,
            wander_speed,
            flee_speed,
            fears_nearby_damage_radius,
            fears_nearby_death_radius,
            fears_nearby_damage_time,
            fears_nearby_death_time,
            protect_radius,
            protect_time,
            turn_factor,
            axial_movement,
            spawn_time,
            spawn_rest_time,
            spawn_output,
            arc_length,
            arc_length_secondary,
            num_arcs,
            key_transformation,
            key_projectile,
            food,
            grow_count,
            digestion_count,
            digestion_time,
            digestion_output,
            flee_time,
            flee_rest_time,
            chase_time,
            chase_rest_time,
            chases_damage,
            fears_mouths,
            fears_weapons,
            fears_electric,
            fears_poison,
            fears_damage,
            ignores_food,
            awake_time: r.f32()?,
            sleep_time: r.f32()?,
            grow_amount: r.i32()?,
            hatch_duration: r.f32()?,
            poison_recharge: r.f32()?,
            electric_recharge: r.f32()?,
            electric_recharge_vs_small: r.f32()?,
            electric_discharge: r.f32()?,
        })
    }

    fn write(&self, w: &mut Writer) {
        w.i32(self.kind.raw());
        w.f32(self.awareness_radius);
        w.f32(self.awareness_radius_food);
        w.f32(self.awareness_radius_predator);
        w.i32(self.movement_style.0 as i32);
        w.bool8(self.flocking);
        w.pad(3);
        for v in [
            self.speed,
            self.chase_speed,
            self.wander_speed,
            self.flee_speed,
            self.fears_nearby_damage_radius,
            self.fears_nearby_death_radius,
            self.fears_nearby_damage_time,
            self.fears_nearby_death_time,
            self.protect_radius,
            self.protect_time,
            self.turn_factor,
        ] {
            w.f32(v);
        }
        w.bool8(self.axial_movement);
        w.pad(3);
        w.f32(self.spawn_time);
        w.f32(self.spawn_rest_time);
        w.u32(self.spawn_output);
        w.f32(self.arc_length);
        w.f32(self.arc_length_secondary);
        w.i32(self.num_arcs);
        w.u32(self.key_transformation);
        w.u32(self.key_projectile);
        w.i32(self.food.raw());
        w.i32(self.grow_count);
        w.i32(self.digestion_count);
        w.f32(self.digestion_time);
        w.u32(self.digestion_output);
        w.f32(self.flee_time);
        w.f32(self.flee_rest_time);
        w.f32(self.chase_time);
        w.f32(self.chase_rest_time);
        for b in [
            self.chases_damage,
            self.fears_mouths,
            self.fears_weapons,
            self.fears_electric,
            self.fears_poison,
            self.fears_damage,
            self.ignores_food,
        ] {
            w.bool8(b);
        }
        w.pad(1);
        w.f32(self.awake_time);
        w.f32(self.sleep_time);
        w.i32(self.grow_amount);
        w.f32(self.hatch_duration);
        w.f32(self.poison_recharge);
        w.f32(self.electric_recharge);
        w.f32(self.electric_recharge_vs_small);
        w.f32(self.electric_discharge);
    }
}

/// `cCellCellResource::cEatData`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellEat {
    pub food_value: i32,
    pub hp_value: i32,
    pub bomb: bool,
    pub poison_nova: bool,
}

/// `cCellCellResource`: everything the rules know about one kind of cell
/// object (creatures, food, rocks, bubbles, parts, eggs, poison clouds...).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellRecord {
    #[serde(serialize_with = "hex")]
    pub structure: u32,
    /// Embedded development name (`char16_t[80]`, e.g. `PLACEHOLDER_Goldy`).
    pub name: String,
    #[serde(serialize_with = "hex")]
    pub name_locale: u32,
    pub hp: i32,
    pub fixed_orientation: bool,
    pub flags: i32,
    pub cell_type: CellType,
    pub unlock_type: UnlockType,
    pub density: Density,
    pub sound: CellSound,
    #[serde(rename = "break", serialize_with = "hex")]
    pub break_cell: u32,
    #[serde(serialize_with = "hex")]
    pub pieces: u32,
    #[serde(serialize_with = "hex")]
    pub leak: u32,
    #[serde(serialize_with = "hex")]
    pub expel: u32,
    /// Offset 0xD4 (see module docs: SMFX order, verified on the install).
    #[serde(serialize_with = "hex")]
    pub loot: u32,
    /// Offset 0xD8.
    #[serde(serialize_with = "hex")]
    pub explosion_table: u32,
    #[serde(serialize_with = "hex")]
    pub poison: u32,
    pub ai: CellAi,
    pub ai_hard: CellAi,
    pub ai_easy: CellAi,
    pub friend_group: i32,
    pub wont_attack_player: bool,
    pub wont_attack_player_when_small: bool,
    pub size: [f32; 2],
    pub eat: CellEat,
    pub triggers_escape_mission: bool,
}

impl CellRecord {
    pub const SIZE: usize = 0x31C;

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != Self::SIZE {
            return Err(format!("cell: {} bytes, expected 0x31C", bytes.len()));
        }
        let mut r = Reader::new(bytes);
        let structure = r.u32()?;
        let units: Vec<u16> = r
            .take(160)?
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let name = String::from_utf16_lossy(
            &units.iter().copied().take_while(|c| *c != 0).collect::<Vec<_>>(),
        );
        let name_locale = r.u32()?;
        let hp = r.i32()?;
        let fixed_orientation = r.bool8()?;
        r.skip(3)?;
        let flags = r.i32()?;
        let cell_type = CellType::from_raw(r.i32()?)?;
        let unlock_type = UnlockType::from_raw(r.i32()?)?;
        let density = Density::from_raw(r.i32()?)?;
        let sound = CellSound::from_raw(r.i32()?)?;
        let break_cell = r.u32()?;
        let pieces = r.u32()?;
        let leak = r.u32()?;
        let expel = r.u32()?;
        let loot = r.u32()?;
        let explosion_table = r.u32()?;
        let poison = r.u32()?;
        let ai = CellAi::read(&mut r)?;
        let ai_hard = CellAi::read(&mut r)?;
        let ai_easy = CellAi::read(&mut r)?;
        let friend_group = r.i32()?;
        let wont_attack_player = r.bool8()?;
        let wont_attack_player_when_small = r.bool8()?;
        r.skip(2)?;
        let size = [r.f32()?, r.f32()?];
        let eat = CellEat {
            food_value: r.i32()?,
            hp_value: r.i32()?,
            bomb: r.bool8()?,
            poison_nova: r.bool8()?,
        };
        r.skip(2)?;
        let triggers_escape_mission = r.bool8()?;
        r.skip(3)?;
        r.finish()?;
        Ok(Self {
            structure,
            name,
            name_locale,
            hp,
            fixed_orientation,
            flags,
            cell_type,
            unlock_type,
            density,
            sound,
            break_cell,
            pieces,
            leak,
            expel,
            loot,
            explosion_table,
            poison,
            ai,
            ai_hard,
            ai_easy,
            friend_group,
            wont_attack_player,
            wont_attack_player_when_small,
            size,
            eat,
            triggers_escape_mission,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(self.structure);
        let units: Vec<u16> = self.name.encode_utf16().take(80).collect();
        for i in 0..80 {
            w.0.extend_from_slice(&units.get(i).copied().unwrap_or(0).to_le_bytes());
        }
        w.u32(self.name_locale);
        w.i32(self.hp);
        w.bool8(self.fixed_orientation);
        w.pad(3);
        w.i32(self.flags);
        w.i32(self.cell_type.raw());
        w.i32(self.unlock_type.raw());
        w.i32(self.density.raw());
        w.i32(self.sound.raw());
        for id in [
            self.break_cell,
            self.pieces,
            self.leak,
            self.expel,
            self.loot,
            self.explosion_table,
            self.poison,
        ] {
            w.u32(id);
        }
        self.ai.write(&mut w);
        self.ai_hard.write(&mut w);
        self.ai_easy.write(&mut w);
        w.i32(self.friend_group);
        w.bool8(self.wont_attack_player);
        w.bool8(self.wont_attack_player_when_small);
        w.pad(2);
        w.f32(self.size[0]);
        w.f32(self.size[1]);
        w.i32(self.eat.food_value);
        w.i32(self.eat.hp_value);
        w.bool8(self.eat.bomb);
        w.bool8(self.eat.poison_nova);
        w.pad(2);
        w.bool8(self.triggers_escape_mission);
        w.pad(3);
        w.0
    }

    /// The AI block for a difficulty (0 easy, 1 medium = `ai`, 2 hard).
    pub fn ai_for(&self, difficulty: usize) -> &CellAi {
        match difficulty {
            0 => &self.ai_easy,
            2 => &self.ai_hard,
            _ => &self.ai,
        }
    }
}

// ---------------------------------------------------------------------------
// Loot table
// ---------------------------------------------------------------------------

/// `cCellLootTableResource::cLootTableEntry`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellLootEntry {
    #[serde(rename = "type")]
    pub kind: LootEntryType,
    #[serde(serialize_with = "hex")]
    pub cell: u32,
    #[serde(serialize_with = "hex")]
    pub table: u32,
    pub weight: f32,
    pub count: i32,
    pub count_delta: i32,
    pub level_offset: i32,
}

/// `cCellLootTableResource`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellLootTable {
    pub entries: Vec<CellLootEntry>,
    pub min_radius: f32,
    pub max_radius: f32,
    pub initial_alpha: f32,
    pub expel_force: f32,
    #[serde(serialize_with = "hex")]
    pub effect: u32,
    pub must_have_part: bool,
    pub delay: f32,
}

impl CellLootTable {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let offset = r.u32()?;
        let count = r.u32()?;
        let entries_at = array_at(count, offset, 36, "loot entries")?;
        let min_radius = r.f32()?;
        let max_radius = r.f32()?;
        let initial_alpha = r.f32()?;
        let expel_force = r.f32()?;
        let effect = r.u32()?;
        let must_have_part = r.bool8()?;
        r.skip(3)?;
        let delay = r.f32()?;
        r.seek(entries_at)?;
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(CellLootEntry {
                kind: LootEntryType::from_raw(r.i32()?)?,
                cell: r.u32()?,
                table: r.u32()?,
                weight: r.f32()?,
                count: r.i32()?,
                count_delta: r.i32()?,
                level_offset: r.i32()?,
            });
        }
        r.finish()?;
        Ok(Self {
            entries,
            min_radius,
            max_radius,
            initial_alpha,
            expel_force,
            effect,
            must_have_part,
            delay,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(if self.entries.is_empty() { 0 } else { 36 });
        w.u32(self.entries.len() as u32);
        w.f32(self.min_radius);
        w.f32(self.max_radius);
        w.f32(self.initial_alpha);
        w.f32(self.expel_force);
        w.u32(self.effect);
        w.bool8(self.must_have_part);
        w.pad(3);
        w.f32(self.delay);
        for e in &self.entries {
            w.i32(e.kind.raw());
            w.u32(e.cell);
            w.u32(e.table);
            w.f32(e.weight);
            w.i32(e.count);
            w.i32(e.count_delta);
            w.i32(e.level_offset);
        }
        w.0
    }
}

// ---------------------------------------------------------------------------
// Look table / look algorithm
// ---------------------------------------------------------------------------

/// `cCellLookTableResource::cLookTableEntry`: a weighted eye target.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CellLookEntry {
    #[serde(rename = "type")]
    pub target: LookTarget,
    pub value: f32,
}

/// `cCellLookTableResource`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CellLookTable {
    pub entries: Vec<CellLookEntry>,
}

impl CellLookTable {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let offset = r.u32()?;
        let count = r.u32()?;
        r.seek(array_at(count, offset, 8, "look entries")?)?;
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(CellLookEntry {
                target: LookTarget::from_raw(r.i32()?)?,
                value: r.f32()?,
            });
        }
        r.finish()?;
        Ok(Self { entries })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(8);
        w.u32(self.entries.len() as u32);
        for e in &self.entries {
            w.i32(e.target.raw());
            w.f32(e.value);
        }
        w.0
    }
}

/// `cCellLookAlgorithmResource::cLookAlgorithmEntry`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CellLookAlgorithmEntry {
    #[serde(rename = "type")]
    pub subject: LookSubject,
    pub action: LookAction,
    #[serde(serialize_with = "hex")]
    pub player: u32,
    #[serde(serialize_with = "hex")]
    pub npc: u32,
    #[serde(serialize_with = "hex")]
    pub epic: u32,
}

/// `cCellLookAlgorithmResource`: which look table applies per subject/action.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CellLookAlgorithm {
    pub entries: Vec<CellLookAlgorithmEntry>,
}

impl CellLookAlgorithm {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let offset = r.u32()?;
        let count = r.u32()?;
        r.seek(array_at(count, offset, 8, "look algorithm entries")?)?;
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(CellLookAlgorithmEntry {
                subject: LookSubject::from_raw(r.i32()?)?,
                action: LookAction::from_raw(r.i32()?)?,
                player: r.u32()?,
                npc: r.u32()?,
                epic: r.u32()?,
            });
        }
        r.finish()?;
        Ok(Self { entries })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(8);
        w.u32(self.entries.len() as u32);
        for e in &self.entries {
            w.i32(e.subject.raw());
            w.i32(e.action.raw());
            w.u32(e.player);
            w.u32(e.npc);
            w.u32(e.epic);
        }
        w.0
    }

    /// The look table for a subject and action (the subject's own column).
    pub fn table_for(&self, subject: LookSubject, action: LookAction) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| e.subject == subject && e.action == action)
            .map(|e| match subject {
                LookSubject::Player => e.player,
                LookSubject::Npc => e.npc,
                LookSubject::Epic => e.epic,
            })
            .filter(|id| *id != 0)
    }
}

// ---------------------------------------------------------------------------
// Effect map / background map / powers / random creature
// ---------------------------------------------------------------------------

/// `cCellEffectMapResource::cEffectMapEntry`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellEffectMapEntry {
    #[serde(serialize_with = "hex")]
    pub effect: u32,
    #[serde(rename = "type")]
    pub kind: EffectMapType,
    /// The four ModAPI `field_8..field_14` floats (an increasing range,
    /// e.g. 0.42 0.70 1.60 1.95; -1 = open start).
    pub range: [f32; 4],
    pub field_18: i32,
}

/// `cCellEffectMapResource`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CellEffectMap {
    pub entries: Vec<CellEffectMapEntry>,
}

impl CellEffectMap {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let count = r.u32()?;
        let offset = r.u32()?;
        r.seek(array_at(count, offset, 8, "effect map entries")?)?;
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(CellEffectMapEntry {
                effect: r.u32()?,
                kind: EffectMapType::from_raw(r.i32()?)?,
                range: [r.f32()?, r.f32()?, r.f32()?, r.f32()?],
                field_18: r.i32()?,
            });
        }
        r.finish()?;
        Ok(Self { entries })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(self.entries.len() as u32);
        w.u32(8);
        for e in &self.entries {
            w.u32(e.effect);
            w.i32(e.kind.raw());
            for v in e.range {
                w.f32(v);
            }
            w.i32(e.field_18);
        }
        w.0
    }
}

/// `cCellBackgroundMapResource::cBackgroundMapEntry`: a water colour keyed by
/// an increasing value (0, 0.5, 1.5, 5 ... 100000 in the install).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CellBackgroundColor {
    pub color: [f32; 3],
    #[serde(rename = "key")]
    pub field_c: f32,
}

/// `cCellBackgroundMapResource`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CellBackgroundMap {
    pub entries: Vec<CellBackgroundColor>,
}

impl CellBackgroundMap {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let count = r.u32()?;
        let offset = r.u32()?;
        r.seek(array_at(count, offset, 8, "background map entries")?)?;
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(CellBackgroundColor {
                color: [r.f32()?, r.f32()?, r.f32()?],
                field_c: r.f32()?,
            });
        }
        r.finish()?;
        Ok(Self { entries })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(self.entries.len() as u32);
        w.u32(if self.entries.is_empty() { 0 } else { 8 });
        for e in &self.entries {
            for c in e.color {
                w.f32(c);
            }
            w.f32(e.field_c);
        }
        w.0
    }

    /// Colour at `key` (VERIFIED, 0xE54D30 called by 0xE55080 with the world scale): the entry with
    /// the largest key below `key` and the entry with the smallest key from `key` on bracket it
    /// (every entry is scanned, the order does not matter); the colour is
    /// `lo + (hi - lo) * t` with `t = (key - lo.key) / (hi.key - lo.key + 2^-16)` (0x1041CE0, not
    /// clamped). Without both neighbours (`key` at or below the lowest key, or above the highest)
    /// the routine returns magenta (1, 0, 1), the constants at 0x15A7BEC..0x15A7BF4.
    pub fn color_at(&self, key: f32) -> Option<[f32; 3]> {
        let mut lo: Option<&CellBackgroundColor> = None;
        let mut hi: Option<&CellBackgroundColor> = None;
        for e in &self.entries {
            if e.field_c < key && lo.is_none_or(|l| e.field_c > l.field_c) {
                lo = Some(e);
            }
            if e.field_c >= key && hi.is_none_or(|h| e.field_c < h.field_c) {
                hi = Some(e);
            }
        }
        let (Some(lo), Some(hi)) = (lo, hi) else {
            return Some([1.0, 0.0, 1.0]);
        };
        let t = (key - lo.field_c) / (hi.field_c - lo.field_c + 1.525_878_9e-5);
        Some(std::array::from_fn(|i| lo.color[i] + (hi.color[i] - lo.color[i]) * t))
    }
}

/// `cCellPowersResource`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellPowers {
    pub teleport_cost: i32,
    pub teleport_range: f32,
}

impl CellPowers {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let p = Self {
            teleport_cost: r.i32()?,
            teleport_range: r.f32()?,
        };
        r.finish()?;
        Ok(p)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.i32(self.teleport_cost);
        w.f32(self.teleport_range);
        w.0
    }
}

/// `cCellRandomCreatureResource::cRandomCreatureEntry`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellRandomCreatureEntry {
    #[serde(rename = "type")]
    pub kind: RandomCreatureType,
    #[serde(serialize_with = "hex")]
    pub creature: u32,
    pub weight: f32,
    pub speed_min: i32,
    pub speed_max: i32,
    pub danger_min: i32,
    pub danger_max: i32,
}

/// `cCellRandomCreatureResource`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CellRandomCreature {
    pub entries: Vec<CellRandomCreatureEntry>,
}

impl CellRandomCreature {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let count = r.u32()?;
        let offset = r.u32()?;
        r.seek(array_at(count, offset, 8, "random creature entries")?)?;
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(CellRandomCreatureEntry {
                kind: RandomCreatureType::from_raw(r.i32()?)?,
                creature: r.u32()?,
                weight: r.f32()?,
                speed_min: r.i32()?,
                speed_max: r.i32()?,
                danger_min: r.i32()?,
                danger_max: r.i32()?,
            });
        }
        r.finish()?;
        Ok(Self { entries })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(self.entries.len() as u32);
        w.u32(8);
        for e in &self.entries {
            w.i32(e.kind.raw());
            w.u32(e.creature);
            w.f32(e.weight);
            w.i32(e.speed_min);
            w.i32(e.speed_max);
            w.i32(e.danger_min);
            w.i32(e.danger_max);
        }
        w.0
    }
}

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

/// `cCellStructureResource::cSPAttachment` (0x28 bytes).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellAttachment {
    pub bone: i32,
    #[serde(rename = "type")]
    pub kind: AttachmentType,
    #[serde(serialize_with = "hex")]
    pub structure: u32,
    #[serde(serialize_with = "hex")]
    pub random_creature: u32,
    /// Effect, model or creature instance ID depending on `kind`
    /// (ModAPI `effectID`; SMFX "also model ID and creature ID").
    #[serde(serialize_with = "hex")]
    pub resource: u32,
    pub level_min: i32,
    pub level_max: i32,
    pub color: [f32; 3],
}

/// `cCellStructureResource`: how a cell looks (models, effects, creature,
/// sub-structures) and which effects mark its death/hatching.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellStructure {
    #[serde(serialize_with = "hex")]
    pub on_death: u32,
    #[serde(serialize_with = "hex")]
    pub on_death_small: u32,
    #[serde(serialize_with = "hex")]
    pub on_death_large: u32,
    #[serde(serialize_with = "hex")]
    pub on_hatch: u32,
    #[serde(serialize_with = "hex")]
    pub on_start_hatch: u32,
    pub attachments: Vec<CellAttachment>,
}

impl CellStructure {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let on_death = r.u32()?;
        let on_death_small = r.u32()?;
        let on_death_large = r.u32()?;
        let on_hatch = r.u32()?;
        let on_start_hatch = r.u32()?;
        let offset = r.u32()?;
        let count = r.u32()?;
        // SMFX always writes offset 28, also for an empty list.
        if offset != 28 {
            return Err(format!("structure: attachment offset {offset}, expected 28"));
        }
        if count > 4096 {
            return Err(format!("structure: implausible attachment count {count}"));
        }
        let mut attachments = Vec::with_capacity(count as usize);
        for _ in 0..count {
            attachments.push(CellAttachment {
                bone: r.i32()?,
                kind: AttachmentType::from_raw(r.i32()?)?,
                structure: r.u32()?,
                random_creature: r.u32()?,
                resource: r.u32()?,
                level_min: r.i32()?,
                level_max: r.i32()?,
                color: [r.f32()?, r.f32()?, r.f32()?],
            });
        }
        r.finish()?;
        Ok(Self {
            on_death,
            on_death_small,
            on_death_large,
            on_hatch,
            on_start_hatch,
            attachments,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        for id in [
            self.on_death,
            self.on_death_small,
            self.on_death_large,
            self.on_hatch,
            self.on_start_hatch,
        ] {
            w.u32(id);
        }
        w.u32(28);
        w.u32(self.attachments.len() as u32);
        for a in &self.attachments {
            w.i32(a.bone);
            w.i32(a.kind.raw());
            w.u32(a.structure);
            w.u32(a.random_creature);
            w.u32(a.resource);
            w.i32(a.level_min);
            w.i32(a.level_max);
            for c in a.color {
                w.f32(c);
            }
        }
        w.0
    }
}

// ---------------------------------------------------------------------------
// Advect map
// ---------------------------------------------------------------------------

/// `cAdvectResource`: a 128x128 field of `Vector3` currents, row-major with
/// the row index along map Y (SporeModder-FX `advect.py`: `vectors[y * 128 + x]`).
#[derive(Clone, Debug, PartialEq)]
pub struct AdvectMap {
    pub vectors: Vec<[f32; 3]>,
}

impl AdvectMap {
    pub const BYTES: usize = ADVECT_SIDE * ADVECT_SIDE * 12;

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != Self::BYTES {
            return Err(format!(
                "advect: {} bytes, expected {}",
                bytes.len(),
                Self::BYTES
            ));
        }
        let mut r = Reader::new(bytes);
        let mut vectors = Vec::with_capacity(ADVECT_SIDE * ADVECT_SIDE);
        for _ in 0..ADVECT_SIDE * ADVECT_SIDE {
            vectors.push([r.f32()?, r.f32()?, r.f32()?]);
        }
        r.finish()?;
        Ok(Self { vectors })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        for v in &self.vectors {
            for c in v {
                w.f32(*c);
            }
        }
        w.0
    }

    pub fn at(&self, x: usize, y: usize) -> [f32; 3] {
        self.vectors[(y % ADVECT_SIDE) * ADVECT_SIDE + (x % ADVECT_SIDE)]
    }

    /// Bilinear sample at map coordinates `u, v` in `[0, 1)`, wrapping (the
    /// map tiles). Returns the X/Y components.
    pub fn sample(&self, u: f32, v: f32) -> [f32; 2] {
        let fx = u.rem_euclid(1.0) * ADVECT_SIDE as f32;
        let fy = v.rem_euclid(1.0) * ADVECT_SIDE as f32;
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let top = lerp(self.at(x0, y0), self.at(x0 + 1, y0), tx);
        let bottom = lerp(self.at(x0, y0 + 1), self.at(x0 + 1, y0 + 1), tx);
        [
            top[0] + (bottom[0] - top[0]) * ty,
            top[1] + (bottom[1] - top[1]) * ty,
        ]
    }

    pub fn max_magnitude(&self) -> f32 {
        self.vectors
            .iter()
            .map(|v| (v[0] * v[0] + v[1] * v[1]).sqrt())
            .fold(0.0, f32::max)
    }
}

// ---------------------------------------------------------------------------
// cell_properties.prop graphs
// ---------------------------------------------------------------------------

/// `cell_properties.prop` (PatchData 0x00000000!0xC951EB63): the piecewise
/// linear graphs the Cell code reads through `G(name, x)` (RE 1B, 0xE837C0).
pub const CELL_PROPERTIES: u32 = 0xC951_EB63;
const PROP_TYPE: u32 = 0x00B1_B104;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellGraphs {
    pub graphs: BTreeMap<u32, Vec<[f32; 2]>>,
}

impl CellGraphs {
    pub const FLAGELLA_COUNT: u32 = 0xBBAB_1BF5;
    pub const CILIA_COUNT: u32 = 0x2EF6_8471;
    pub const JET_COUNT: u32 = 0x2C57_4746;
    pub const EYE_COUNT: u32 = 0x7754_0C34;
    pub const POISON_COUNT: u32 = 0x92BA_AE19;
    pub const ELECTRIC_RECHARGE_COUNT: u32 = 0xE501_5D0C;
    pub const ELECTRIC_RECHARGE_VS_SMALL_COUNT: u32 = 0x5A38_C4BA;
    pub const ELECTRIC_RANGE_COUNT: u32 = 0x03AB_589E;
    pub const ELECTRIC_JUMPS_COUNT: u32 = 0xAEE2_835E;
    pub const NPC_TURN_FACTOR: u32 = 0x1106_A9E2;
    pub const CILIA_GRAPH: u32 = 0x38EC_9850;
    pub const AWARENESS_DILATION: u32 = 0xD291_6EE5;
    pub const CAMERA_LOOKAHEAD: u32 = 0x7B41_F3E9;
    pub const CAMERA_LERPFACTOR: u32 = 0xCE31_F6A1;
    pub const MOUSE_ZOOM: u32 = 0x71CB_8A60;
    pub const ANIM_DILATION: u32 = 0x7AC2_E49F;
    pub const TIME_DILATION: u32 = 0x18B7_5C47;
    pub const EVO_POINTS: u32 = 0x7B02_FCB3;

    /// Every `vector2` array property of the PROP.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let list = crate::prop::parse(bytes)?;
        let mut graphs = BTreeMap::new();
        for (id, property) in list.0 {
            if property.kind != 0x30 || !property.array {
                continue;
            }
            let mut points = Vec::with_capacity(property.values.len());
            for value in &property.values {
                let x = f32::from_le_bytes(value.get(0..4).ok_or("short vector2")?.try_into().unwrap());
                let y = f32::from_le_bytes(value.get(4..8).ok_or("short vector2")?.try_into().unwrap());
                if !x.is_finite() || !y.is_finite() {
                    return Err(format!("non-finite graph point in {id:08X}"));
                }
                points.push([x, y]);
            }
            graphs.insert(id, points);
        }
        Ok(Self { graphs })
    }

    /// `G(name, x)`: below the first key the first y, above the last the last
    /// y, linear in between; a missing graph is 0.
    pub fn eval(&self, id: u32, x: f32) -> f32 {
        let Some(points) = self.graphs.get(&id).filter(|p| !p.is_empty()) else {
            return 0.0;
        };
        if x <= points[0][0] {
            return points[0][1];
        }
        for pair in points.windows(2) {
            let ([x0, y0], [x1, y1]) = (pair[0], pair[1]);
            if x <= x1 {
                let t = if x1 > x0 { (x - x0) / (x1 - x0) } else { 1.0 };
                return y0 + (y1 - y0) * t;
            }
        }
        points[points.len() - 1][1]
    }

    /// `eval`, or `default` when the graph is not loaded (headless tests).
    pub fn eval_or(&self, id: u32, x: f32, default: f32) -> f32 {
        if self.graphs.get(&id).is_some_and(|p| !p.is_empty()) {
            self.eval(id, x)
        } else {
            default
        }
    }
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

/// Every installed Cell record, keyed by instance ID (group 0), with the
/// localized cell names. Built once from the resource index.
#[derive(Resource, Clone, Default)]
pub struct CellDatabase {
    /// Instance ID of the Globals record (`CellGame`).
    pub globals_id: u32,
    pub globals: Option<CellGlobals>,
    pub worlds: BTreeMap<u32, CellWorld>,
    pub populates: BTreeMap<u32, CellPopulate>,
    pub cells: BTreeMap<u32, CellRecord>,
    pub loot_tables: BTreeMap<u32, CellLootTable>,
    pub look_tables: BTreeMap<u32, CellLookTable>,
    pub look_algorithms: BTreeMap<u32, CellLookAlgorithm>,
    pub effect_maps: BTreeMap<u32, CellEffectMap>,
    pub background_maps: BTreeMap<u32, CellBackgroundMap>,
    pub powers: BTreeMap<u32, CellPowers>,
    pub random_creatures: BTreeMap<u32, CellRandomCreature>,
    pub structures: BTreeMap<u32, CellStructure>,
    pub advect: BTreeMap<u32, Arc<AdvectMap>>,
    /// `cell_properties.prop` graphs (movement, perception, camera, zoom).
    pub graphs: CellGraphs,
    /// Locale table `0x04DC2F94` entries (`name.localeInstanceID` -> text).
    pub localized: HashMap<u32, String>,
    /// Decode errors (`type!instance: message`). Empty for the retail data.
    pub errors: Vec<String>,
}

/// All Cell resource types with their dump names.
pub const CELL_TYPES: [(u32, &str); 13] = [
    (TYPE_GLOBALS, "globals"),
    (TYPE_WORLD, "world"),
    (TYPE_POPULATE, "populate"),
    (TYPE_CELL, "cell"),
    (TYPE_LOOT_TABLE, "lootTable"),
    (TYPE_LOOK_TABLE, "lookTable"),
    (TYPE_LOOK_ALGORITHM, "lookAlgorithm"),
    (TYPE_EFFECT_MAP, "effectMap"),
    (TYPE_BACKGROUND_MAP, "backgroundMap"),
    (TYPE_POWERS, "powers"),
    (TYPE_RANDOM_CREATURE, "randomCreature"),
    (TYPE_STRUCTURE, "structure"),
    (TYPE_ADVECT, "advect"),
];

/// Read many records, opening each package once (the install is on a slow
/// disk; one open per record would dominate). Same decompression as
/// `dbpf::read_entry_bytes`.
pub fn read_many(index: &ResourceIndex, keys: &[ResourceKey]) -> Vec<(ResourceKey, Result<Vec<u8>, String>)> {
    let mut by_package: BTreeMap<PathBuf, Vec<(ResourceKey, dbpf::DbpfEntry)>> = BTreeMap::new();
    let mut out = Vec::with_capacity(keys.len());
    for key in keys {
        match index.records.get(key) {
            Some((path, entry)) => by_package
                .entry(path.clone())
                .or_default()
                .push((*key, entry.clone())),
            None => out.push((*key, Err(format!("missing resource {key:?}")))),
        }
    }
    for (path, mut entries) in by_package {
        entries.sort_by_key(|(_, e)| e.offset);
        let mut file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(error) => {
                for (key, _) in entries {
                    out.push((key, Err(format!("open {}: {error}", path.display()))));
                }
                continue;
            }
        };
        for (key, entry) in entries {
            let result = (|| {
                if entry.mem_size > 64 * 1024 * 1024 || entry.disk_size > 64 * 1024 * 1024 {
                    return Err("resource exceeds decode limit".to_string());
                }
                file.seek(SeekFrom::Start(entry.offset))
                    .map_err(|e| format!("seek: {e}"))?;
                let mut raw = vec![0u8; entry.disk_size as usize];
                file.read_exact(&mut raw).map_err(|e| format!("read: {e}"))?;
                if entry.is_compressed
                    && raw.len() >= 5
                    && (raw[0] == 0x10 || raw[0] == 0x50)
                    && raw[1] == 0xFB
                {
                    dbpf::qfs_decompress(&raw)
                } else {
                    Ok(raw)
                }
            })();
            out.push((key, result));
        }
    }
    out
}

impl CellDatabase {
    /// Load every Cell record from the install. Cheap: ~520 small records
    /// plus eight 192 KiB advect maps, read with one handle per package.
    ///
    /// Precedence is the resource index's package order (PatchData.package,
    /// Spore_Pack_03, DataEP1, BoosterPack, the rest; first definition wins).
    /// On this install Galactic Adventures' `Spore_EP1_Data.package` repeats
    /// 123 Cell records byte-for-byte and adds two (the nanite cell
    /// `B761DDF9` and its structure, referenced by no Cell world, populate or
    /// loot table), so the order does not change the stage either way; the
    /// installed test pins this.
    pub fn load(index: &ResourceIndex) -> Self {
        let wanted: Vec<u32> = CELL_TYPES.iter().map(|(t, _)| *t).collect();
        let mut keys: Vec<ResourceKey> = index
            .records
            .keys()
            .filter(|k| wanted.contains(&k.type_id))
            .copied()
            .collect();
        // Locale tables with the cell names, and the graphs PROP.
        keys.extend(
            index
                .records
                .keys()
                .filter(|k| {
                    (k.type_id == LOCALE_TEXT && k.instance_id == CELL_NAME_LOCALE_TABLE)
                        || (k.type_id == PROP_TYPE && k.instance_id == CELL_PROPERTIES && k.group_id == 0)
                })
                .copied(),
        );
        // Deterministic order: group 0 first so it wins duplicate instances.
        keys.sort_by_key(|k| (k.group_id != CELL_DATA_GROUP, k.group_id, k.type_id, k.instance_id));
        let mut records = read_many(index, &keys);
        records.sort_by_key(|(k, _)| (k.group_id != CELL_DATA_GROUP, k.group_id, k.type_id, k.instance_id));
        let mut db = Self::default();
        for (key, bytes) in records {
            match bytes {
                Ok(bytes) => {
                    if key.type_id == LOCALE_TEXT {
                        for (id, text) in crate::decode::parse_locale_table(&bytes) {
                            db.localized.entry(id).or_insert(text);
                        }
                    } else if key.type_id == PROP_TYPE {
                        match CellGraphs::parse(&bytes) {
                            Ok(graphs) => db.graphs = graphs,
                            Err(error) => db.errors.push(format!("cell_properties: {error}")),
                        }
                    } else {
                        db.insert(key, &bytes);
                    }
                }
                Err(error) => db.errors.push(format!(
                    "{:08X}!{:08X}.{:08X}: {error}",
                    key.group_id, key.instance_id, key.type_id
                )),
            }
        }
        db
    }

    /// Decode one record into the database (first instance wins).
    pub fn insert(&mut self, key: ResourceKey, bytes: &[u8]) {
        fn put<T>(map: &mut BTreeMap<u32, T>, id: u32, value: Result<T, String>) -> Result<(), String> {
            let value = value?;
            map.entry(id).or_insert(value);
            Ok(())
        }
        let id = key.instance_id;
        let result = match key.type_id {
            TYPE_GLOBALS => CellGlobals::parse(bytes).map(|g| {
                if self.globals.is_none() {
                    self.globals_id = id;
                    self.globals = Some(g);
                }
            }),
            TYPE_WORLD => put(&mut self.worlds, id, CellWorld::parse(bytes)),
            TYPE_POPULATE => put(&mut self.populates, id, CellPopulate::parse(bytes)),
            TYPE_CELL => put(&mut self.cells, id, CellRecord::parse(bytes)),
            TYPE_LOOT_TABLE => put(&mut self.loot_tables, id, CellLootTable::parse(bytes)),
            TYPE_LOOK_TABLE => put(&mut self.look_tables, id, CellLookTable::parse(bytes)),
            TYPE_LOOK_ALGORITHM => put(&mut self.look_algorithms, id, CellLookAlgorithm::parse(bytes)),
            TYPE_EFFECT_MAP => put(&mut self.effect_maps, id, CellEffectMap::parse(bytes)),
            TYPE_BACKGROUND_MAP => put(&mut self.background_maps, id, CellBackgroundMap::parse(bytes)),
            TYPE_POWERS => put(&mut self.powers, id, CellPowers::parse(bytes)),
            TYPE_RANDOM_CREATURE => put(&mut self.random_creatures, id, CellRandomCreature::parse(bytes)),
            TYPE_STRUCTURE => put(&mut self.structures, id, CellStructure::parse(bytes)),
            TYPE_ADVECT => put(&mut self.advect, id, AdvectMap::parse(bytes).map(Arc::new)),
            other => Err(format!("not a Cell resource type {other:08X}")),
        };
        if let Err(error) = result {
            self.errors.push(format!(
                "{:08X}!{:08X}.{:08X}: {error}",
                key.group_id, key.instance_id, key.type_id
            ));
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.globals.is_some() && !self.cells.is_empty()
    }

    pub fn cell(&self, id: u32) -> Option<&CellRecord> {
        self.cells.get(&id)
    }

    pub fn structure_of(&self, cell: &CellRecord) -> Option<&CellStructure> {
        self.structures.get(&cell.structure)
    }

    /// Display name of a cell: the localized text when the install has it,
    /// else the embedded name without its `PLACEHOLDER_` prefix.
    pub fn cell_name(&self, id: u32) -> Option<String> {
        let cell = self.cells.get(&id)?;
        if let Some(text) = self.localized.get(&cell.name_locale).filter(|_| cell.name_locale != 0) {
            return Some(text.trim().to_string());
        }
        let name = cell.name.trim_start_matches("PLACEHOLDER_").trim();
        (!name.is_empty()).then(|| name.to_string())
    }

    /// Record counts per type, in [`CELL_TYPES`] order.
    pub fn counts(&self) -> Vec<(&'static str, usize)> {
        vec![
            ("globals", self.globals.is_some() as usize),
            ("world", self.worlds.len()),
            ("populate", self.populates.len()),
            ("cell", self.cells.len()),
            ("lootTable", self.loot_tables.len()),
            ("lookTable", self.look_tables.len()),
            ("lookAlgorithm", self.look_algorithms.len()),
            ("effectMap", self.effect_maps.len()),
            ("backgroundMap", self.background_maps.len()),
            ("powers", self.powers.len()),
            ("randomCreature", self.random_creatures.len()),
            ("structure", self.structures.len()),
            ("advect", self.advect.len()),
        ]
    }

    /// Every intra-Cell reference whose target record is absent:
    /// `(owner, field, missing id)`. Effects, models and creatures live in
    /// other resource types and are not checked here.
    pub fn dangling_references(&self) -> Vec<(String, &'static str, u32)> {
        let mut out = Vec::new();
        let mut check = |owner: String, field: &'static str, id: u32, present: bool| {
            if id != 0 && !present {
                out.push((owner, field, id));
            }
        };
        if let Some(g) = &self.globals {
            let owner = format!("globals {:08X}", self.globals_id);
            for id in g.worlds.iter().chain(&g.world_backgrounds).chain([&g.world_random, &g.world_random_bg]) {
                check(owner.clone(), "world", *id, self.worlds.contains_key(id));
            }
            check(owner.clone(), "startCell", g.start_cell, self.cells.contains_key(&g.start_cell));
            check(owner.clone(), "effectMapEntry", g.effect_map_entry, self.effect_maps.contains_key(&g.effect_map_entry));
            check(
                owner.clone(),
                "backgroundMapEntry",
                g.background_map_entry,
                self.background_maps.contains_key(&g.background_map_entry),
            );
            check(owner, "keyLookAlgorithm", g.key_look_algorithm, self.look_algorithms.contains_key(&g.key_look_algorithm));
        }
        for (id, world) in &self.worlds {
            for level in &world.levels {
                check(format!("world {id:08X}"), "populate", level.populate, self.populates.contains_key(&level.populate));
            }
            for a in &world.advect {
                check(format!("world {id:08X}"), "advect", a.advect, self.advect.contains_key(&a.advect));
            }
        }
        for (id, p) in &self.populates {
            for m in &p.markers {
                let owner = format!("populate {id:08X}");
                check(owner.clone(), "distributeCell", m.distribute_cell, self.cells.contains_key(&m.distribute_cell));
                check(owner.clone(), "clusterCell", m.cluster_cell, self.cells.contains_key(&m.cluster_cell));
                check(owner, "encounterPopulate", m.encounter_populate, self.populates.contains_key(&m.encounter_populate));
            }
        }
        for (id, c) in &self.cells {
            let owner = format!("cell {id:08X}");
            check(owner.clone(), "structure", c.structure, self.structures.contains_key(&c.structure));
            for (field, cell) in [("break", c.break_cell), ("leak", c.leak), ("expel", c.expel), ("poison", c.poison)] {
                check(owner.clone(), field, cell, self.cells.contains_key(&cell));
            }
            for (field, table) in [("pieces", c.pieces), ("loot", c.loot), ("explosionTable", c.explosion_table)] {
                check(owner.clone(), field, table, self.loot_tables.contains_key(&table));
            }
            for ai in [&c.ai, &c.ai_easy, &c.ai_hard] {
                check(owner.clone(), "ai.spawnOutput", ai.spawn_output, self.loot_tables.contains_key(&ai.spawn_output));
                check(
                    owner.clone(),
                    "ai.digestionOutput",
                    ai.digestion_output,
                    self.loot_tables.contains_key(&ai.digestion_output),
                );
                check(
                    owner.clone(),
                    "ai.keyTransformation",
                    ai.key_transformation,
                    self.cells.contains_key(&ai.key_transformation),
                );
                check(owner.clone(), "ai.keyProjectile", ai.key_projectile, self.cells.contains_key(&ai.key_projectile));
            }
        }
        for (id, t) in &self.loot_tables {
            for e in &t.entries {
                let owner = format!("lootTable {id:08X}");
                if e.kind == LootEntryType::Cell {
                    check(owner.clone(), "cell", e.cell, self.cells.contains_key(&e.cell));
                }
                if e.kind == LootEntryType::Table {
                    check(owner, "table", e.table, self.loot_tables.contains_key(&e.table));
                }
            }
        }
        for (id, a) in &self.look_algorithms {
            for e in &a.entries {
                for (field, table) in [("player", e.player), ("npc", e.npc), ("epic", e.epic)] {
                    check(format!("lookAlgorithm {id:08X}"), field, table, self.look_tables.contains_key(&table));
                }
            }
        }
        for (id, s) in &self.structures {
            for a in &s.attachments {
                let owner = format!("structure {id:08X}");
                if a.kind == AttachmentType::Structure {
                    check(owner.clone(), "structure", a.structure, self.structures.contains_key(&a.structure));
                }
                if a.kind == AttachmentType::RandomCreature {
                    check(owner, "randomCreature", a.random_creature, self.random_creatures.contains_key(&a.random_creature));
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

// ---------------------------------------------------------------------------
// Readable dump (`--dump-cell-data <dir>`)
// ---------------------------------------------------------------------------

/// SporeModder-FX `reg_file.txt` names by lowercase FNV-1 hash (or the
/// explicit hash column). Optional: only the dump uses it.
#[derive(Default)]
pub struct NameRegistry(pub HashMap<u32, String>);

impl NameRegistry {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut names = HashMap::new();
        for line in text.lines() {
            let mut columns = line.split('\t');
            let name = columns.next().unwrap_or("").trim();
            if name.is_empty() || name.starts_with('#') {
                continue;
            }
            let id = columns
                .next()
                .map(str::trim)
                .and_then(|h| h.strip_prefix("0x").or_else(|| h.strip_prefix("0X")))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .unwrap_or_else(|| crate::world_objects::name_id(name));
            names.entry(id).or_insert_with(|| name.to_string());
        }
        Ok(Self(names))
    }

    pub fn name(&self, id: u32) -> String {
        self.0
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("0x{id:08X}"))
    }

    /// Replace every `"0xXXXXXXXX"` string in a JSON value with its name.
    fn resolve(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(s) => {
                if let Some(id) = s
                    .strip_prefix("0x")
                    .filter(|h| h.len() == 8)
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                {
                    if let Some(name) = self.0.get(&id) {
                        *s = name.clone();
                    }
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(|v| self.resolve(v)),
            serde_json::Value::Object(map) => map.values_mut().for_each(|v| self.resolve(v)),
            _ => {}
        }
    }
}

fn registry_candidates(install_root: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(root) = install_root {
        out.push(root.join("OpenSpore/target/format-reference/reg_file.txt"));
        out.push(root.join("OpenSpore/target/format-reference/sporemodder-fx/reg_file.txt"));
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    out.push(manifest.join("target/format-reference/reg_file.txt"));
    out.push(manifest.join("../../OpenSpore/target/format-reference/reg_file.txt"));
    out
}

fn dump_map<T: Serialize>(
    registry: &NameRegistry,
    map: &BTreeMap<u32, T>,
    extra: impl Fn(u32, &mut serde_json::Value),
) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for (id, value) in map {
        let mut json = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
        extra(*id, &mut json);
        registry.resolve(&mut json);
        out.insert(registry.name(*id), json);
    }
    serde_json::Value::Object(out)
}

/// Write one readable JSON file per Cell resource type into `output`, names
/// resolved through `reg_file.txt` when found (else the hex IDs stay).
pub fn run_dump(output: &Path) -> Result<(), String> {
    let install = crate::spore_install::discover()?;
    let index = ResourceIndex::load(&install.packages);
    let started = std::time::Instant::now();
    let db = CellDatabase::load(&index);
    let load_ms = started.elapsed().as_millis();
    let args: Vec<String> = std::env::args().collect();
    let explicit = args
        .iter()
        .position(|a| a == "--registry")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    let registry_path = explicit
        .into_iter()
        .chain(registry_candidates(Some(&install.root)))
        .find(|p| p.is_file());
    let registry = match &registry_path {
        Some(path) => NameRegistry::load(path)?,
        None => NameRegistry::default(),
    };
    std::fs::create_dir_all(output).map_err(|e| format!("{}: {e}", output.display()))?;
    let write = |name: &str, value: serde_json::Value| -> Result<(), String> {
        let path = output.join(format!("{name}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap())
            .map_err(|e| format!("{}: {e}", path.display()))
    };
    let mut globals = serde_json::Map::new();
    if let Some(g) = &db.globals {
        let mut json = serde_json::to_value(g).unwrap();
        registry.resolve(&mut json);
        globals.insert(registry.name(db.globals_id), json);
    }
    write("globals", serde_json::Value::Object(globals))?;
    let none = |_: u32, _: &mut serde_json::Value| {};
    write("world", dump_map(&registry, &db.worlds, none))?;
    write("populate", dump_map(&registry, &db.populates, none))?;
    write(
        "cell",
        dump_map(&registry, &db.cells, |id, json| {
            if let (Some(name), serde_json::Value::Object(map)) = (db.cell_name(id), &mut *json) {
                map.insert("displayName".into(), serde_json::Value::String(name));
            }
        }),
    )?;
    write("lootTable", dump_map(&registry, &db.loot_tables, none))?;
    write("lookTable", dump_map(&registry, &db.look_tables, none))?;
    write("lookAlgorithm", dump_map(&registry, &db.look_algorithms, none))?;
    write("effectMap", dump_map(&registry, &db.effect_maps, none))?;
    write("backgroundMap", dump_map(&registry, &db.background_maps, none))?;
    write("powers", dump_map(&registry, &db.powers, none))?;
    write("randomCreature", dump_map(&registry, &db.random_creatures, none))?;
    write("structure", dump_map(&registry, &db.structures, none))?;
    let mut advect = serde_json::Map::new();
    for (id, map) in &db.advect {
        let center = map.at(ADVECT_SIDE / 2, ADVECT_SIDE / 2);
        advect.insert(
            registry.name(*id),
            serde_json::json!({
                "side": ADVECT_SIDE,
                "maxMagnitude": map.max_magnitude(),
                "center": center,
            }),
        );
    }
    write("advect", serde_json::Value::Object(advect))?;
    let dangling: Vec<_> = db
        .dangling_references()
        .into_iter()
        .map(|(owner, field, id)| format!("{owner} {field} -> {}", registry.name(id)))
        .collect();
    write(
        "index",
        serde_json::json!({
            "counts": db.counts().into_iter().collect::<BTreeMap<_, _>>(),
            "errors": db.errors,
            "danglingReferences": dangling,
            "localizedNames": db.localized.len(),
            "registry": registry_path.map(|p| p.display().to_string()),
            "loadMilliseconds": load_ms,
        }),
    )?;
    println!(
        "Cell data: {} records in {load_ms} ms, {} errors, {} dangling references -> {}",
        db.counts().iter().map(|(_, n)| n).sum::<usize>(),
        db.errors.len(),
        db.dangling_references().len(),
        output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u32(bytes: &mut [u8], at: usize, v: u32) {
        bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put_f32(bytes: &mut [u8], at: usize, v: f32) {
        put_u32(bytes, at, v.to_bits());
    }

    fn ai(kind: AiType) -> CellAi {
        CellAi {
            kind,
            awareness_radius: 10.0,
            awareness_radius_food: 2.0,
            awareness_radius_predator: 3.0,
            movement_style: MovementStyle(MovementStyle::FLAGELLA | MovementStyle::JET),
            flocking: true,
            speed: 1.5,
            chase_speed: 2.5,
            wander_speed: 0.5,
            flee_speed: 3.0,
            fears_nearby_damage_radius: 4.0,
            fears_nearby_death_radius: 5.0,
            fears_nearby_damage_time: 6.0,
            fears_nearby_death_time: 7.0,
            protect_radius: 8.0,
            protect_time: 9.0,
            turn_factor: 1.25,
            axial_movement: true,
            spawn_time: 11.0,
            spawn_rest_time: 12.0,
            spawn_output: 0x1111,
            arc_length: 3.0,
            arc_length_secondary: 1.0,
            num_arcs: 3,
            key_transformation: 0x2222,
            key_projectile: 0x3333,
            food: FoodType::Solid,
            grow_count: 4,
            digestion_count: 2,
            digestion_time: 1.5,
            digestion_output: 0x4444,
            flee_time: 5.0,
            flee_rest_time: 1.0,
            chase_time: 10000.0,
            chase_rest_time: 2.0,
            chases_damage: true,
            fears_mouths: false,
            fears_weapons: true,
            fears_electric: false,
            fears_poison: true,
            fears_damage: false,
            ignores_food: true,
            awake_time: 3.0,
            sleep_time: 4.0,
            grow_amount: 1,
            hatch_duration: 10.0,
            poison_recharge: 0.5,
            electric_recharge: -1.0,
            electric_recharge_vs_small: -1.0,
            electric_discharge: 0.25,
        }
    }

    fn sample_cell() -> CellRecord {
        CellRecord {
            structure: 0xABCD,
            name: "PLACEHOLDER_Goldy".into(),
            name_locale: 0x2E26F6F8,
            hp: 3,
            fixed_orientation: true,
            flags: 2,
            cell_type: CellType::Unlock,
            unlock_type: UnlockType::Spike,
            density: Density::Solid,
            sound: CellSound::Creature,
            break_cell: 1,
            pieces: 2,
            leak: 3,
            expel: 4,
            loot: 5,
            explosion_table: 6,
            poison: 7,
            ai: ai(AiType::Eater),
            ai_hard: ai(AiType::Parts),
            ai_easy: ai(AiType::Sleeper),
            friend_group: 41,
            wont_attack_player: true,
            wont_attack_player_when_small: false,
            size: [0.25, 0.3],
            eat: CellEat {
                food_value: 6,
                hp_value: 1,
                bomb: false,
                poison_nova: true,
            },
            triggers_escape_mission: true,
        }
    }

    #[test]
    fn cell_record_round_trips_and_matches_modapi_offsets() {
        let cell = sample_cell();
        let bytes = cell.encode();
        assert_eq!(bytes.len(), CellRecord::SIZE);
        assert_eq!(CellRecord::parse(&bytes).unwrap(), cell);
        // Absolute ModAPI offsets (independent of the encoder's order).
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!(word(0xA4), 0x2E26F6F8, "name.localeInstanceID");
        assert_eq!(word(0xA8), 3, "hp");
        assert_eq!(word(0xB4), 6, "cellType unlock");
        assert_eq!(word(0xB8), 7, "unlockType spike");
        assert_eq!(word(0xD4), 5, "loot (SMFX order)");
        assert_eq!(word(0xD8), 6, "explosionTable");
        assert_eq!(word(0xE0), 0x1006, "ai.type");
        assert_eq!(word(0xE0 + 0x50), 0x1111, "ai.spawnOutput");
        assert_eq!(word(0x194), 0x1001, "ai_hard.type");
        assert_eq!(word(0x248), 0x1009, "ai_easy.type");
        assert_eq!(word(0x2FC), 41, "friendGroup");
        assert_eq!(bytes[0x300], 1, "wontAttackPlayer");
        assert_eq!(f32::from_le_bytes(bytes[0x304..0x308].try_into().unwrap()), 0.25);
        assert_eq!(word(0x30C), 6, "eat.foodValue");
        assert_eq!(bytes[0x315], 1, "eat.poisonNova");
        assert_eq!(bytes[0x318], 1, "triggersEscapeMission");
        // The difficulty selector picks the matching AI block.
        assert_eq!(cell.ai_for(0).kind, AiType::Sleeper);
        assert_eq!(cell.ai_for(1).kind, AiType::Eater);
        assert_eq!(cell.ai_for(2).kind, AiType::Parts);
    }

    #[test]
    fn cell_record_rejects_bad_sizes_enums_bools_and_floats() {
        let bytes = sample_cell().encode();
        assert!(CellRecord::parse(&bytes[..CellRecord::SIZE - 4]).is_err());
        let mut bad = bytes.clone();
        put_u32(&mut bad, 0xB4, 99);
        assert!(CellRecord::parse(&bad).unwrap_err().contains("cell type"));
        let mut bad = bytes.clone();
        put_u32(&mut bad, 0xE0, 0x2000);
        assert!(CellRecord::parse(&bad).unwrap_err().contains("AI type"));
        let mut bad = bytes.clone();
        bad[0x300] = 7;
        assert!(CellRecord::parse(&bad).unwrap_err().contains("bool"));
        let mut bad = bytes;
        put_f32(&mut bad, 0x304, f32::NAN);
        assert!(CellRecord::parse(&bad).is_err());
    }

    #[test]
    fn globals_round_trip_and_tail_offsets() {
        let mut bytes = vec![0u8; CellGlobals::SIZE];
        put_u32(&mut bytes, 0x04, 0x10);
        put_u32(&mut bytes, 0x34, 0x77);
        put_f32(&mut bytes, 0x44, 5.0);
        put_u32(&mut bytes, 0x84, 0x88);
        put_f32(&mut bytes, 0x88, 40.0);
        put_f32(&mut bytes, 0xB8, 10.0);
        put_u32(&mut bytes, 0xE4, 1);
        put_f32(&mut bytes, 0x10C, 4.0);
        put_f32(&mut bytes, 0x110, 3.5);
        let g = CellGlobals::parse(&bytes).unwrap();
        assert_eq!(g.worlds[0], 0x10);
        assert_eq!(g.start_cell, 0x77);
        assert_eq!(g.flow_multiplier, 5.0);
        assert_eq!(g.key_look_algorithm, 0x88);
        assert_eq!(g.beach_distance, 40.0);
        assert_eq!(g.start_level_no_creature_radius, 10.0);
        assert_eq!(g.eye_method, 1);
        assert_eq!(g.mate_trigger_distance, 4.0);
        assert_eq!(g.mate_spawn_distance, 3.5);
        assert_eq!(g.encode(), bytes);
        assert!(CellGlobals::parse(&bytes[..0x110]).is_err());
    }

    #[test]
    fn world_populate_loot_and_structure_round_trip() {
        let world = CellWorld {
            levels: vec![
                CellLevelEntry {
                    populate: 0x10,
                    start_tile: true,
                    player_size: CellScale(1),
                },
                CellLevelEntry {
                    populate: 0x10,
                    start_tile: false,
                    player_size: CellScale::NONE,
                },
            ],
            advect: vec![CellAdvectEntry {
                scale: CellScale(10),
                player_size: CellScale(2),
                strength: 3.5,
                variance: 0.0,
                period: 1.0,
                advect: 0x99,
            }],
        };
        let bytes = world.encode();
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 40);
        assert_eq!(CellWorld::parse(&bytes).unwrap(), world);
        let empty = CellWorld {
            levels: vec![],
            advect: vec![],
        };
        assert_eq!(CellWorld::parse(&empty.encode()).unwrap(), empty);

        let marker = CellMarker {
            kind: MarkerType::Cluster,
            field_0: 0,
            field_4: 0,
            field_8: 0,
            z_offset: 1.0,
            z_offset_max: 19.0,
            field_14: 0,
            distribute_cell: 0,
            cluster_cell: 0x55,
            encounter_populate: 0,
            plant_type: PlantType::Type3,
            count: -1.0,
            count_easy: 0.35,
            count_med: -1.0,
            count_hard: 12.0,
            size: CellScale(4),
            parts: 5,
            linear: 1,
            encounter_scale: 0.5,
        };
        let populate = CellPopulate {
            scale: CellScale(10),
            mask_texture: 0,
            markers: vec![marker.clone(), marker],
        };
        let bytes = populate.encode();
        assert_eq!(bytes.len(), 16 + 2 * CellMarker::SIZE);
        let back = CellPopulate::parse(&bytes).unwrap();
        assert_eq!(back, populate);
        let m = &back.markers[0];
        assert_eq!(m.cell(), Some(0x55));
        assert_eq!(m.count_for(0), Some(0.35));
        assert_eq!(m.count_for(1), None, "medium is unused (-1)");
        assert_eq!(m.count_for(2), Some(12.0));
        // Count (0xE5A910): own count first, then `count`; all -1 -> 1.
        let mut shared = m.clone();
        (shared.count, shared.count_easy, shared.count_hard) = (2.0, -1.0, 5.0);
        assert_eq!(shared.count_for(0), Some(2.0));
        assert_eq!(shared.count_for(2), Some(5.0), "the difficulty's own count wins");
        (shared.count, shared.count_hard) = (-1.0, -1.0);
        assert_eq!(shared.count_for(1), Some(1.0), "every count -1 -> 1");

        let loot = CellLootTable {
            entries: vec![
                CellLootEntry {
                    kind: LootEntryType::Cell,
                    cell: 0x77,
                    table: 0,
                    weight: 0.4,
                    count: 1,
                    count_delta: 0,
                    level_offset: 1,
                },
                CellLootEntry {
                    kind: LootEntryType::Nothing,
                    cell: 0,
                    table: 0,
                    weight: 0.6,
                    count: 0,
                    count_delta: 0,
                    level_offset: 0,
                },
            ],
            min_radius: 0.0,
            max_radius: 1.0,
            initial_alpha: 1.0,
            expel_force: 4.0,
            effect: 0,
            must_have_part: true,
            delay: 0.5,
        };
        assert_eq!(CellLootTable::parse(&loot.encode()).unwrap(), loot);

        let structure = CellStructure {
            on_death: 1,
            on_death_small: 2,
            on_death_large: 3,
            on_hatch: 4,
            on_start_hatch: 5,
            attachments: vec![CellAttachment {
                bone: -1,
                kind: AttachmentType::Effect,
                structure: 0,
                random_creature: 0,
                resource: 0xED3DAF17,
                level_min: -1,
                level_max: 10,
                color: [1.0, 0.5, 0.25],
            }],
        };
        let bytes = structure.encode();
        assert_eq!(bytes.len(), 28 + 40);
        assert_eq!(CellStructure::parse(&bytes).unwrap(), structure);
        let mut bad = bytes;
        put_u32(&mut bad, 20, 32);
        assert!(CellStructure::parse(&bad).is_err());
    }

    #[test]
    fn small_tables_round_trip() {
        let look = CellLookTable {
            entries: vec![
                CellLookEntry {
                    target: LookTarget::Cursor,
                    value: 10.0,
                },
                CellLookEntry {
                    target: LookTarget::Up,
                    value: 1.0,
                },
            ],
        };
        assert_eq!(CellLookTable::parse(&look.encode()).unwrap(), look);
        let algorithm = CellLookAlgorithm {
            entries: vec![CellLookAlgorithmEntry {
                subject: LookSubject::Npc,
                action: LookAction::MoveScared,
                player: 0,
                npc: 0x42,
                epic: 0,
            }],
        };
        let back = CellLookAlgorithm::parse(&algorithm.encode()).unwrap();
        assert_eq!(back, algorithm);
        assert_eq!(back.table_for(LookSubject::Npc, LookAction::MoveScared), Some(0x42));
        assert_eq!(back.table_for(LookSubject::Player, LookAction::MoveScared), None);
        let effects = CellEffectMap {
            entries: vec![CellEffectMapEntry {
                effect: 0x10,
                kind: EffectMapType::TileBackground,
                range: [-1.0, -1.0, 1.5, 1.9],
                field_18: 3,
            }],
        };
        assert_eq!(CellEffectMap::parse(&effects.encode()).unwrap(), effects);
        let background = CellBackgroundMap {
            entries: vec![
                CellBackgroundColor {
                    color: [0.0, 0.0, 0.0],
                    field_c: 0.0,
                },
                CellBackgroundColor {
                    color: [1.0, 0.5, 0.0],
                    field_c: 10.0,
                },
            ],
        };
        let back = CellBackgroundMap::parse(&background.encode()).unwrap();
        assert_eq!(back, background);
        let mid = back.color_at(5.0).unwrap();
        assert!((mid[0] - 0.5).abs() < 1e-4 && (mid[1] - 0.25).abs() < 1e-4 && mid[2] == 0.0, "{mid:?}");
        // The key of an entry gives that entry's colour (t = 1 within 2^-16 of the span).
        let top = back.color_at(10.0).unwrap();
        assert!((top[0] - 1.0).abs() < 1e-4 && (top[1] - 0.5).abs() < 1e-4, "{top:?}");
        // Outside the keys, and at the lowest key itself, the routine returns magenta.
        assert_eq!(back.color_at(50.0), Some([1.0, 0.0, 1.0]));
        assert_eq!(back.color_at(0.0), Some([1.0, 0.0, 1.0]));
        assert_eq!(back.color_at(-1.0), Some([1.0, 0.0, 1.0]));
        let powers = CellPowers {
            teleport_cost: 10,
            teleport_range: 10.0,
        };
        assert_eq!(CellPowers::parse(&powers.encode()).unwrap(), powers);
        let random = CellRandomCreature {
            entries: vec![CellRandomCreatureEntry {
                kind: RandomCreatureType::Pollinated,
                creature: 0,
                weight: 1.0,
                speed_min: -1,
                speed_max: 4,
                danger_min: 2,
                danger_max: 4,
            }],
        };
        assert_eq!(CellRandomCreature::parse(&random.encode()).unwrap(), random);
    }

    #[test]
    fn advect_maps_sample_bilinearly_and_wrap() {
        let mut vectors = vec![[0.0f32; 3]; ADVECT_SIDE * ADVECT_SIDE];
        vectors[0] = [1.0, -1.0, 0.0];
        vectors[1] = [3.0, 1.0, 0.0];
        let map = AdvectMap { vectors };
        let bytes = map.encode();
        assert_eq!(bytes.len(), AdvectMap::BYTES);
        let back = AdvectMap::parse(&bytes).unwrap();
        assert_eq!(back, map);
        assert_eq!(back.sample(0.0, 0.0), [1.0, -1.0]);
        let half = 0.5 / ADVECT_SIDE as f32;
        let mid = back.sample(half, 0.0);
        assert!((mid[0] - 2.0).abs() < 1e-5 && mid[1].abs() < 1e-5);
        assert_eq!(back.sample(1.0, 1.0), back.sample(0.0, 0.0), "the map tiles");
        assert!(AdvectMap::parse(&bytes[..12]).is_err());
    }

    #[test]
    fn graphs_are_piecewise_linear_and_clamped() {
        let mut graphs = CellGraphs::default();
        graphs.graphs.insert(1, vec![[0.0, 0.0], [2.0, 3.0], [4.0, 4.0]]);
        assert_eq!(graphs.eval(1, -1.0), 0.0);
        assert_eq!(graphs.eval(1, 1.0), 1.5);
        assert_eq!(graphs.eval(1, 3.0), 3.5);
        assert_eq!(graphs.eval(1, 9.0), 4.0);
        assert_eq!(graphs.eval(2, 1.0), 0.0, "missing graph");
    }

    #[test]
    fn scale_ladder_labels_offsets_and_bounds() {
        assert_eq!(CellScale(3).label(), "4");
        assert_eq!(CellScale(10).label(), "1K");
        assert_eq!(CellScale::NONE.label(), "none");
        assert_eq!(CellScale(4).nominal(), Some(10.0));
        assert_eq!(CellScale::NONE.nominal(), None);
        assert_eq!(CellScale(18).offset(5), CellScale(19));
        assert_eq!(CellScale(1).offset(-3), CellScale(0));
        assert_eq!(CellScale::NONE.offset(1), CellScale::NONE);
        assert!(CellScale::from_raw(20).is_err());
        assert_eq!(MovementStyle(5).label(), "flagella_jet");
        assert!(MovementStyle::from_raw(16).is_err());
    }

    #[test]
    fn database_resolves_references_and_names() {
        let mut db = CellDatabase::default();
        let key = |type_id, instance_id| ResourceKey {
            type_id,
            group_id: 0,
            instance_id,
        };
        let mut cell = sample_cell();
        cell.structure = 0x500;
        cell.loot = 0x600;
        cell.break_cell = 0;
        cell.pieces = 0;
        cell.leak = 0;
        cell.expel = 0;
        cell.explosion_table = 0;
        cell.poison = 0;
        for ai in [&mut cell.ai, &mut cell.ai_easy, &mut cell.ai_hard] {
            ai.spawn_output = 0;
            ai.digestion_output = 0;
            ai.key_transformation = 0;
            ai.key_projectile = 0;
        }
        db.insert(key(TYPE_CELL, 0x100), &cell.encode());
        db.insert(
            key(TYPE_STRUCTURE, 0x500),
            &CellStructure {
                on_death: 0,
                on_death_small: 0,
                on_death_large: 0,
                on_hatch: 0,
                on_start_hatch: 0,
                attachments: vec![],
            }
            .encode(),
        );
        db.insert(key(TYPE_CELL, 0x101), &[0u8; 4]);
        assert_eq!(db.errors.len(), 1, "a truncated record is reported");
        assert_eq!(db.cell_name(0x100).as_deref(), Some("Goldy"));
        db.localized.insert(0x2E26F6F8, "Goldy (localized)".into());
        assert_eq!(db.cell_name(0x100).as_deref(), Some("Goldy (localized)"));
        assert!(db.structure_of(db.cell(0x100).unwrap()).is_some());
        assert_eq!(
            db.dangling_references(),
            vec![("cell 00000100".to_string(), "loot", 0x600)]
        );
    }

    #[test]
    #[ignore = "requires original Spore installation"]
    fn installed_cell_data_decodes_completely_and_round_trips() {
        let install = crate::spore_install::discover().expect("original installation");
        let index = ResourceIndex::load(&install.packages);
        let db = CellDatabase::load(&index);
        assert!(db.errors.is_empty(), "decode errors: {:?}", db.errors);
        let counts: BTreeMap<_, _> = db.counts().into_iter().collect();
        assert_eq!(counts["globals"], 1);
        assert_eq!(counts["world"], 13);
        assert_eq!(counts["populate"], 22);
        // 223 from Data/PatchData.package + the GA nanite cell (see below).
        assert_eq!(counts["cell"], 224);
        assert_eq!(counts["lootTable"], 53);
        assert_eq!(counts["lookTable"], 10);
        assert_eq!(counts["lookAlgorithm"], 1);
        assert_eq!(counts["effectMap"], 1);
        assert_eq!(counts["backgroundMap"], 1);
        assert_eq!(counts["powers"], 1);
        assert_eq!(counts["randomCreature"], 10);
        assert_eq!(counts["structure"], 170);
        assert_eq!(counts["advect"], 8);
        // Precedence: Galactic Adventures (DataEP1/Spore_EP1_Data.package)
        // ships its own copy of the Cell data. Every key it shares with the
        // base PatchData.package is byte-identical, so package order cannot
        // change the stage; it only adds a nanite cell (cellType 7) and its
        // structure, which no world, populate or loot table references.
        let is_cell_type = |t: u32| CELL_TYPES.iter().any(|(ty, _)| *ty == t);
        for path in install.packages.iter().filter(|p| {
            p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("Spore_EP1_Data.package"))
        }) {
            let ep1 = dbpf::read_package_index(path).unwrap();
            let mut only_ep1 = Vec::new();
            for entry in ep1.entries.iter().filter(|e| is_cell_type(e.key.type_id)) {
                let (winner, _) = &index.records[&entry.key];
                if winner == path {
                    only_ep1.push((entry.key.type_id, entry.key.instance_id));
                } else {
                    assert_eq!(
                        dbpf::read_entry_bytes(path, entry).unwrap(),
                        index.read(entry.key).unwrap(),
                        "GA copy of {:?} must equal the base copy",
                        entry.key
                    );
                }
            }
            only_ep1.sort();
            assert_eq!(only_ep1, vec![(TYPE_STRUCTURE, 0x48F5_2C14), (TYPE_CELL, 0xB761_DDF9)]);
            assert_eq!(db.cells[&0xB761_DDF9].cell_type, CellType::Nanite);
        }
        // Every byte is accounted for: decode + encode reproduces each record.
        let keys: Vec<_> = index
            .records
            .keys()
            .filter(|k| CELL_TYPES.iter().any(|(t, _)| *t == k.type_id))
            .copied()
            .collect();
        let mut checked = 0;
        let mut mismatches = Vec::new();
        for (key, bytes) in read_many(&index, &keys) {
            let bytes = bytes.unwrap();
            let id = key.instance_id;
            let encoded = match key.type_id {
                TYPE_GLOBALS => db.globals.as_ref().unwrap().encode(),
                TYPE_WORLD => db.worlds[&id].encode(),
                TYPE_POPULATE => db.populates[&id].encode(),
                TYPE_CELL => db.cells[&id].encode(),
                TYPE_LOOT_TABLE => db.loot_tables[&id].encode(),
                TYPE_LOOK_TABLE => db.look_tables[&id].encode(),
                TYPE_LOOK_ALGORITHM => db.look_algorithms[&id].encode(),
                TYPE_EFFECT_MAP => db.effect_maps[&id].encode(),
                TYPE_BACKGROUND_MAP => db.background_maps[&id].encode(),
                TYPE_POWERS => db.powers[&id].encode(),
                TYPE_RANDOM_CREATURE => db.random_creatures[&id].encode(),
                TYPE_STRUCTURE => db.structures[&id].encode(),
                TYPE_ADVECT => db.advect[&id].encode(),
                _ => unreachable!(),
            };
            if encoded != bytes {
                let first = encoded.iter().zip(&bytes).position(|(a, b)| a != b);
                mismatches.push(format!(
                    "{:08X}.{:08X} first difference at {first:?} (len {} vs {})",
                    key.instance_id,
                    key.type_id,
                    encoded.len(),
                    bytes.len()
                ));
            }
            checked += 1;
        }
        assert!(mismatches.is_empty(), "records that do not round-trip: {mismatches:#?}");
        assert_eq!(checked, 515);
        // Facts the engine relies on.
        let g = db.globals.as_ref().unwrap();
        let id = crate::world_objects::name_id;
        assert_eq!(g.start_cell, id("cell_hero_L0"));
        assert_eq!(g.worlds[0], id("world_ice"));
        assert_eq!(g.world_backgrounds[0], id("world_ice_bg"));
        let ice = &db.worlds[&id("world_ice")];
        assert_eq!(ice.levels.len(), 20, "ten player sizes x (start tile, tile)");
        let sizes: Vec<_> = ice.levels.iter().step_by(2).map(|l| l.player_size.label()).collect();
        assert_eq!(sizes, ["1", "2", "4", "10", "20", "40", "100", "200", "400", "1K"]);
        assert!(ice.levels.iter().step_by(2).all(|l| l.start_tile));
        // loot (0xD4) holds creature part drops; explosionTable (0xD8) rock chunks.
        assert_eq!(db.cells[&id("cell_jawhead")].loot, id("loot_jawhead"));
        assert_eq!(db.cells[&0x6316_609B].explosion_table, id("loot_rock_chunks"));
        assert_eq!(db.cell_name(id("cell_goldy")).as_deref(), Some("Goldy"));
        let unlocks = db
            .cells
            .values()
            .filter(|c| c.cell_type == CellType::Unlock)
            .count();
        assert_eq!(unlocks, 15);
        let eaters = db.cells.values().filter(|c| c.ai.kind == AiType::Eater).count();
        assert_eq!(eaters, 71, "every real creature uses the eater AI");
        assert!(db.localized.len() > 100, "cell name locale table");
        // cell_properties.prop graphs (values as dumped by cell-re).
        assert_eq!(db.graphs.graphs.len(), 18);
        assert_eq!(db.graphs.eval(CellGraphs::FLAGELLA_COUNT, 2.0), 3.0);
        assert_eq!(db.graphs.eval(CellGraphs::FLAGELLA_COUNT, 3.0), 3.5);
        assert!((db.graphs.eval(CellGraphs::MOUSE_ZOOM, 480.0) - 0.7).abs() < 1e-6);
        assert_eq!(db.graphs.eval(CellGraphs::AWARENESS_DILATION, 0.1), 0.5);
        assert!((db.graphs.eval(CellGraphs::JET_COUNT, 2.0) - 0.02).abs() < 1e-6);
        // The only references that do not resolve inside the retail Cell data
        // (two advect maps the worlds name but the install lacks, and a test
        // cell whose digestionOutput names a cell instead of a loot table).
        let dangling = db.dangling_references();
        assert_eq!(
            dangling,
            vec![
                (
                    format!("cell {:08X}", 0x293A_F179u32),
                    "ai.digestionOutput",
                    id("cell_poison_L0")
                ),
                (
                    format!("world {:08X}", id("world_beach")),
                    "advect",
                    id("cell_diagonals")
                ),
                (
                    format!("world {:08X}", id("world_water")),
                    "advect",
                    id("cell_waves")
                ),
            ]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
        );
    }
}
