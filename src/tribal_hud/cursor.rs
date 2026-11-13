//! The Tribal context cursors: what the mouse pointer looks like over the world.
//!
//! Retail: `cTribeInputStrategy::SetHoverObjectCursorAndRollover` (0xCD00C0,
//! march2017) asks `ChooseInputActionID` (0xCCF130) for a `TribeInputAction` id
//! (0..=0x2F) of the hovered object, then switches over that id (jump table
//! 0xCD0320) to a cursor id and sets it with `cCursorManager::SetActiveCursor`.
//! The cursor ids are registered by the Tribe UI start-up (0xCCE790, one
//! `cCursorManager::Load(id, fileName, true, 0, 0)` per cursor, file names in
//! group `cursors~` 0x02393C07, resource type `cur` 0x02393756, instance = the
//! name's FNV hash); three more are loaded by their file's own hash (the
//! `cursor-ban-*` pair and `cursor_spyglass`). Verified by reading the code
//! image; the table below is the whole of 0xCD0320.
//!
//! The manual: "Objects that you can interact with in the world become
//! highlighted when you mouse over them, and your cursor changes to indicate
//! that you can act on it."
//!
//! What is retail here: the action ids (named by the ModAPI enum where it has
//! one), the id -> cursor table, the structure and the order of the object
//! rules of `ChooseInputActionID` for the objects OpenSpore has (see
//! [`choose_action`]), the cursor files and their hotspots. ADAPTED (marked
//! below): which OpenSpore object stands for which retail noun, the member
//! state test 0xDA8180 (traced, but its states have no OpenSpore counterpart),
//! the continent test, the rollover calls the same switch makes (the HUD's own
//! cards do that job). The click side (`DoActionGeneric`) is `super::click`;
//! [`hover_at`] is the one place both read, so a right click does what the
//! cursor shows.

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow},
};
use std::collections::HashMap;

use super::world::attitude;
use super::{TribalHudActive, TribalHudState};
use crate::{
    creature_hud::Diet,
    dbpf::ResourceKey,
    game::{dist_xz, Bush, Health, RunProgress},
    stages::GameStage,
    tribal::{self, RivalStatus},
    tribal_rts::{
        self as rts, ClickWorld, FishingSpot, MeatPile, Order, RivalMember, Selected, TribeUnit, WildAnimal,
    },
    tribal_tools::{ToolKind, ToolShack},
    world_objects::name_id,
};

/// Resource type `cur` and group `cursors~` of the cursor files.
pub const CURSOR_TYPE: u32 = 0x0239_3756;
pub const CURSOR_GROUP: u32 = 0x0239_3C07;

/// `Simulator::TribeInputAction` ids (0xCCF130 results). The ModAPI enum names
/// only 0x13..=0x1A (the tool pick-ups); the other names are ours, from what
/// the cursor of the id shows.
#[allow(dead_code)]
pub mod action {
    pub const NONE: i32 = 0;
    pub const NONE_RESET: i32 = 1;
    /// Keep the cursor that is set.
    pub const KEEP: i32 = 2;
    /// `strategy + 0xA0` = 1 / 2 (the stance ability modes, which pause the game
    /// and show the red arrow; the Tribal HUD stances do not use them, see
    /// the notes): no target / not a valid target.
    pub const MODE_ATTACK_IDLE: i32 = 3;
    pub const MODE_ATTACK_TARGET: i32 = 4;
    pub const MODE_SOCIAL_IDLE: i32 = 5;
    pub const MODE_SOCIAL_TARGET: i32 = 6;
    /// Over ground the members cannot reach (another continent).
    pub const UNREACHABLE: i32 = 7;
    /// Dragging the selection.
    pub const PAN: i32 = 8;
    pub const REPAIR: i32 = 9;
    /// Attack a member of another tribe (0xCCF8A3: the tribe is attackable ->
    /// 0xA, else 0x2A). The click is `Attack` (7).
    pub const ATTACK_TOOL: i32 = 0xA;
    pub const ATTACK_MEMBER: i32 = ATTACK_TOOL;
    /// Attack a living wild animal; the click is `Hunt` (10).
    pub const ATTACK_ANIMAL: i32 = 0xB;
    /// Raid the hut or a tool rack of another tribe (0xCCFBE4 hut, 0xCCF81D
    /// rack: attackable -> 0xC, else 0x2A). The click is `RaidHuts` (15).
    pub const ATTACK_OTHER: i32 = 0xC;
    pub const RAID_STRUCTURE: i32 = ATTACK_OTHER;
    pub const GIFT_DENIED: i32 = 0xD;
    pub const GIFT_TRIBE: i32 = 0xE;
    pub const NONE_TRIBE: i32 = 0xF;
    pub const GIFT_MEMBER: i32 = 0x10;
    pub const TOOL_FISH: i32 = 0x11;
    pub const TOOL_GATHER: i32 = 0x12;
    /// `ToolAttack1..3`, `ToolSocial1..3`, `ToolHeal`, `ToolFirepit` (ModAPI).
    pub const TOOL_ATTACK1: i32 = 0x13;
    pub const TOOL_ATTACK2: i32 = 0x14;
    pub const TOOL_ATTACK3: i32 = 0x15;
    pub const TOOL_SOCIAL1: i32 = 0x16;
    pub const TOOL_SOCIAL2: i32 = 0x17;
    pub const TOOL_SOCIAL3: i32 = 0x18;
    pub const TOOL_HEAL: i32 = 0x19;
    pub const TOOL_FIREPIT: i32 = 0x1A;
    pub const GATHER_BUNDLE: i32 = 0x1B;
    pub const GATHER_MEAT: i32 = 0x1C;
    pub const GATHER_EGG: i32 = 0x1D;
    pub const EAT_FRUIT: i32 = 0x1E;
    pub const EAT_MEAT: i32 = 0x1F;
    pub const EAT_BOTH: i32 = 0x20;
    pub const STEAL: i32 = 0x21;
    pub const FISH: i32 = 0x22;
    pub const GATHER_TREE: i32 = 0x23;
    pub const GATHER_FRUIT: i32 = 0x24;
    pub const UNREACHABLE_GATHER: i32 = 0x25;
    /// Over the HUD's buy-a-member / planner buttons or a hut (the card shows).
    pub const HUD: i32 = 0x26;
    pub const SOCIAL_MEMBER: i32 = 0x27;
    pub const HEAL_MEMBER: i32 = 0x28;
    pub const DOMESTICATE: i32 = 0x29;
    pub const DEFAULT_ROLLOVER: i32 = 0x2A;
    pub const SOCIAL_MEMBER_NO_CHIEF: i32 = 0x2B;
    pub const DEFAULT_ROLLOVER_2: i32 = 0x2C;
    pub const SOCIAL_MEMBER_SPECIAL: i32 = 0x2D;
    pub const DEFAULT_ROLLOVER_3: i32 = 0x2E;
    pub const GRAB: i32 = 0x2F;
}

/// What the switch of 0xCD0111 does for an action id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    /// `cursor-default` (0x1002), the cursor the switch starts from.
    Default,
    /// The cursor stays as it is.
    Keep,
    /// A file of `cursors~` by name.
    File(&'static str),
}

/// The cursor for an action id: the jump table 0xCD0320 read entry by entry
/// (VERIFIED). Ids above 0x2F are not reached (0xCD010B).
pub fn cursor_for(action: i32) -> Cursor {
    use action::*;
    match action {
        KEEP => Cursor::Keep,
        MODE_ATTACK_IDLE | MODE_SOCIAL_IDLE => Cursor::File("cursor-ban-mode"),
        MODE_ATTACK_TARGET => Cursor::File("cursor-ban-content"),
        MODE_SOCIAL_TARGET => Cursor::File("cursor_spyglass"),
        UNREACHABLE => Cursor::File("cursor-no-opt"),
        // 0x600CD69, registered with "cursor-camera-translate".
        PAN => Cursor::File("cursor-camera-translate"),
        REPAIR => Cursor::File("cursor_repair"),
        // 0x3570021, registered with "cursor_atk".
        ATTACK_TOOL | ATTACK_ANIMAL | ATTACK_OTHER => Cursor::File("cursor_atk"),
        GIFT_DENIED => Cursor::File("cursor-crg-give-grn-no"),
        GIFT_TRIBE | GIFT_MEMBER => Cursor::File("cursor-crg-give-grn"),
        TOOL_FISH => Cursor::File("cursor-trg-tool-pickup-fish"),
        TOOL_GATHER => Cursor::File("cursor-trg-tool-pickup-gather"),
        TOOL_ATTACK1 => Cursor::File("cursor_pickup-torch"),
        TOOL_ATTACK2 => Cursor::File("cursor_pickup-axe"),
        TOOL_ATTACK3 => Cursor::File("cursor_pickup-spears"),
        TOOL_SOCIAL1 => Cursor::File("cursor_pickup-horn"),
        TOOL_SOCIAL2 => Cursor::File("cursor_pickup-maracas"),
        TOOL_SOCIAL3 => Cursor::File("cursor_pickup-didgeridoo"),
        TOOL_HEAL => Cursor::File("cursor_pickup-heal"),
        TOOL_FIREPIT => Cursor::File("cursor-dance"),
        GATHER_BUNDLE | GATHER_MEAT | GATHER_EGG | GATHER_TREE | GATHER_FRUIT | UNREACHABLE_GATHER => {
            Cursor::File("cursor-gather")
        }
        EAT_FRUIT => Cursor::File("cursor-eat-fruit"),
        EAT_MEAT => Cursor::File("cursor-eat-carcass"),
        EAT_BOTH => Cursor::File("cursor-eat-omni"),
        STEAL => Cursor::File("cursor-steal"),
        FISH => Cursor::File("cursor-fish"),
        SOCIAL_MEMBER | SOCIAL_MEMBER_NO_CHIEF | SOCIAL_MEMBER_SPECIAL => Cursor::File("cursor_social"),
        HEAL_MEMBER => Cursor::File("cursor-heal"),
        DOMESTICATE => Cursor::File("cursor-trg-domesticate"),
        GRAB => Cursor::File("cursor-grab_open"),
        // 0, 1, 0xF, 0x26 and the rollover-only ids keep the default arrow.
        _ => Cursor::Default,
    }
}

/// The `ToolAttack1..ToolFirepit` id of a tool type (jump table 0xCCFD6C:
/// types 1..=10 -> 0x13 0x14 0x15 0x16 0x17 0x18 0x12 0x11 0x19 0x1A).
pub fn tool_action(kind: ToolKind) -> i32 {
    use action::*;
    match kind {
        ToolKind::SpikedClubs => TOOL_ATTACK1,
        ToolKind::StoneAxes => TOOL_ATTACK2,
        ToolKind::ThrowingSpears => TOOL_ATTACK3,
        ToolKind::WoodenHorns => TOOL_SOCIAL1,
        ToolKind::Maracas => TOOL_SOCIAL2,
        ToolKind::Aerophones => TOOL_SOCIAL3,
        ToolKind::FruitSticks => TOOL_GATHER,
        ToolKind::FishingSpears => TOOL_FISH,
        ToolKind::HealingRods => TOOL_HEAL,
        ToolKind::Campfire => TOOL_FIREPIT,
    }
}

/// What the pointer is over, in the retail nouns of `ChooseInputActionID`.
/// ADAPTED: OpenSpore's objects stand in for the nouns (a food bush for the
/// fruit / tree ornaments, a meat pile for a dead animal, the home food pile
/// for the player's food mat).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// Open ground the members can walk to.
    Ground,
    /// Ground the members cannot reach (retail: another continent, 7).
    OtherContinent,
    /// One of the player's own members (`kCreatureCitizen`, own).
    OwnMember { injured: bool },
    /// The player's tool rack (`kTribeTool`).
    OwnTool(ToolKind),
    /// The player's hut.
    OwnHut,
    /// The player's food mat.
    OwnFood,
    /// A food bush / fruit.
    Fruit,
    /// A dead animal.
    Carcass,
    /// An egg pile.
    Egg,
    /// A fish hot spot over water.
    FishSpot,
    /// A living wild animal; `tameable` = a grazer.
    Wild { tameable: bool },
    /// A rival tribe's member, hut or food mat (`village`).
    RivalMember,
    RivalHut,
    RivalFoodMat,
}

/// The state `ChooseInputActionID` reads besides the object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Context {
    /// The social stance (retail: the tribe game's stance word `[0xCD4230()+0x390]`
    /// is 0, VERIFIED by the setter 0xCD7340 playing `sfx_trg_mode_social`); the
    /// attack stance is 1, the start value.
    pub social: bool,
    /// The player's species can eat plants / meat (cTribe 0xC8E9E0 / 0xC8EA00:
    /// the species' herbivorous / carnivorous mouth counts).
    pub herbivore: bool,
    pub carnivore: bool,
    /// Attitude of the hovered tribe toward the player, 0 Furious .. 4
    /// Friendly (0xD00BD0).
    pub attitude: u8,
    /// A tribe that is no longer there (+0x556): nothing to do with it.
    pub defeated: bool,
    /// The chieftain is among the selected members (social orders).
    pub chieftain_selected: bool,
    /// The hovered member can take a gift (0xDA8180, VERIFIED: false while the
    /// member's behaviour flag 0x80 is set, in state 0x5218BB3 / 0x562C0FB or
    /// in state 0x462854E past stage 4). ADAPTED: OpenSpore's stand-in is "the
    /// member is not fighting"; the states have no counterpart.
    pub can_gift: bool,
    /// Another member than the hovered one carries the healing tool (type 9,
    /// 0xC22F20): an injured own member shows 0x28 (0xCCF9E0..0xCCFA23).
    pub healer_selected: bool,
    /// The hovered tribe's food mat holds food (0xCCFCBE: an empty mat of
    /// another tribe is nothing in either stance).
    pub mat_has_food: bool,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            social: false,
            herbivore: true,
            carnivore: true,
            attitude: 2,
            defeated: false,
            chieftain_selected: false,
            can_gift: true,
            healer_selected: false,
            mat_has_food: true,
        }
    }
}

/// `cTribeInputStrategy::ChooseInputActionID` (0xCCF130) for the objects
/// OpenSpore has. Rules, in the order of the code:
/// * ground: another continent 7, else 0;
/// * an own member: injured with a healer selected 0x28, else 0x2E;
/// * the player's tool rack: the pick-up id of its type (a damaged rack: 9 /
///   repair; OpenSpore racks have no damage state);
/// * the player's hut: 0x26 (the hut card shows);
/// * the player's food mat: eat cursor by diet (0xCCFC8B: no carnivorous mouth
///   -> fruit; carnivorous only -> carcass; both -> omni);
/// * fruit: a herbivorous mouth is needed, then the gather cursor (0x24);
/// * a dead animal: a carnivorous mouth, then gather (0x1C);
/// * eggs: gather (0x1D) in the attack stance only;
/// * fish spot over water: 0x22 (0xB1FB30 is a constant `true`);
/// * a living animal: attack stance 0xB, social stance domesticate 0x29;
/// * a rival tribe, social stance: attitude < 2 (Furious / Annoyed) offers a
///   gift (hut and food mat 0xE, a member 0x10 or the deny cursor 0xD), else a
///   member shows the social cursor (0x27 with the chieftain selected, 0x2B
///   without) and the hut / food mat the plain arrow (0xF);
/// * a rival tribe, attack stance: a member shows 0xA, a hut 0xC (both 0x2A
///   when the tribe is Friendly, 0xC8ED20), the food mat the steal cursor
///   (0x21) below Friendly and while it holds food;
/// * a defeated tribe: nothing (0).
pub fn choose_action(target: Target, c: &Context) -> i32 {
    use action::*;
    match target {
        Target::Ground => NONE,
        Target::OtherContinent => UNREACHABLE,
        Target::OwnMember { injured } => {
            if injured && c.healer_selected {
                HEAL_MEMBER
            } else {
                DEFAULT_ROLLOVER_3
            }
        }
        Target::OwnTool(kind) => tool_action(kind),
        Target::OwnHut => HUD,
        Target::OwnFood => {
            if !c.carnivore {
                EAT_FRUIT
            } else if c.herbivore {
                EAT_BOTH
            } else {
                EAT_MEAT
            }
        }
        Target::Fruit => {
            if c.herbivore {
                GATHER_FRUIT
            } else {
                NONE
            }
        }
        Target::Carcass => {
            if c.carnivore {
                GATHER_MEAT
            } else {
                NONE
            }
        }
        Target::Egg => {
            if c.social {
                NONE
            } else {
                GATHER_EGG
            }
        }
        Target::FishSpot => FISH,
        Target::Wild { tameable } => {
            if c.social {
                if tameable {
                    DOMESTICATE
                } else {
                    NONE
                }
            } else {
                ATTACK_ANIMAL
            }
        }
        Target::RivalMember => {
            if c.defeated {
                NONE
            } else if c.social {
                if c.attitude < 2 {
                    if c.can_gift {
                        GIFT_MEMBER
                    } else {
                        GIFT_DENIED
                    }
                } else if c.chieftain_selected {
                    SOCIAL_MEMBER
                } else {
                    SOCIAL_MEMBER_NO_CHIEF
                }
            } else if c.attitude != 4 {
                ATTACK_MEMBER
            } else {
                DEFAULT_ROLLOVER
            }
        }
        Target::RivalHut => {
            if c.defeated {
                NONE
            } else if c.social {
                if c.attitude < 2 {
                    GIFT_TRIBE
                } else {
                    NONE_TRIBE
                }
            } else if c.attitude != 4 {
                RAID_STRUCTURE
            } else {
                DEFAULT_ROLLOVER
            }
        }
        Target::RivalFoodMat => {
            if c.defeated || !c.mat_has_food {
                NONE
            } else if c.social {
                if c.attitude < 2 {
                    GIFT_TRIBE
                } else {
                    NONE_TRIBE
                }
            } else if c.attitude < 4 {
                STEAL
            } else {
                NONE
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The cursor files
// ---------------------------------------------------------------------------

/// A decoded `cur` file: straight RGBA, top row first, and its hotspot.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedCursor {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub hotspot: (u16, u16),
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Decode the first image of a Windows cursor file (an `ICONDIR` of type 2, the
/// entry's hotspot, a bottom-up DIB of 1 / 4 / 8 / 24 / 32 bits and the AND
/// mask). The installed cursors are 32x32, 32 bits with alpha.
pub fn decode_cursor(bytes: &[u8]) -> Option<DecodedCursor> {
    if u16_at(bytes, 0)? != 0 || !matches!(u16_at(bytes, 2)?, 1 | 2) || u16_at(bytes, 4)? == 0 {
        return None;
    }
    let hotspot = if u16_at(bytes, 2)? == 2 { (u16_at(bytes, 10)?, u16_at(bytes, 12)?) } else { (0, 0) };
    let at = u32_at(bytes, 18)? as usize;
    let header = u32_at(bytes, at)? as usize;
    if header < 40 {
        return None;
    }
    let width = u32_at(bytes, at + 4)?;
    let height = (u32_at(bytes, at + 8)? as i32).unsigned_abs() / 2;
    let bits = u16_at(bytes, at + 14)? as usize;
    let used = u32_at(bytes, at + 32)? as usize;
    if width == 0 || height == 0 || width > 256 || height > 256 || u32_at(bytes, at + 16)? != 0 {
        return None;
    }
    let colors = if bits <= 8 { if used == 0 { 1usize << bits } else { used } } else { 0 };
    let palette = at + header;
    let pixels = palette + colors * 4;
    let stride = (width as usize * bits).div_ceil(32) * 4;
    let mask = pixels + stride * height as usize;
    let mask_stride = (width as usize).div_ceil(32) * 4;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let mut any_alpha = false;
    for y in 0..height as usize {
        // Rows are stored bottom-up.
        let row = pixels + stride * (height as usize - 1 - y);
        for x in 0..width as usize {
            let (r, g, b, a) = match bits {
                32 => {
                    let p = bytes.get(row + x * 4..row + x * 4 + 4)?;
                    (p[2], p[1], p[0], p[3])
                }
                24 => {
                    let p = bytes.get(row + x * 3..row + x * 3 + 3)?;
                    (p[2], p[1], p[0], 255)
                }
                8 | 4 | 1 => {
                    let per = 8 / bits;
                    let byte = *bytes.get(row + x / per)?;
                    let shift = 8 - bits * (x % per + 1);
                    let index = ((byte >> shift) as usize) & ((1 << bits) - 1);
                    let p = bytes.get(palette + index * 4..palette + index * 4 + 4)?;
                    (p[2], p[1], p[0], 255)
                }
                _ => return None,
            };
            any_alpha |= a != 0;
            let out = (y * width as usize + x) * 4;
            rgba[out..out + 4].copy_from_slice(&[r, g, b, a]);
        }
    }
    // Without an alpha channel the AND mask (1 = transparent) cuts the shape; a
    // 32-bit image with alpha ignores it.
    if bits != 32 || !any_alpha {
        for y in 0..height as usize {
            let row = mask + mask_stride * (height as usize - 1 - y);
            for x in 0..width as usize {
                let byte = *bytes.get(row + x / 8)?;
                let transparent = (byte >> (7 - x % 8)) & 1 == 1;
                let out = (y * width as usize + x) * 4;
                rgba[out + 3] = if transparent { 0 } else { 255 };
            }
        }
    }
    Some(DecodedCursor { width, height, rgba, hotspot })
}

/// The resource key of a cursor file by name (instance = FNV hash of the name).
pub fn cursor_key(name: &str) -> ResourceKey {
    ResourceKey { type_id: CURSOR_TYPE, group_id: CURSOR_GROUP, instance_id: name_id(name) }
}

// ---------------------------------------------------------------------------
// The system
// ---------------------------------------------------------------------------

/// The decoded cursors by file name (`None` = the file could not be read: the
/// default arrow stays).
#[derive(Default)]
pub struct CursorCache {
    images: HashMap<&'static str, Option<(Handle<Image>, (u16, u16))>>,
}

/// The action under the pointer (read by the capture checks and the tests).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct HoverAction {
    pub action: i32,
}

/// The retail relationship / attitude of a tribe from the campaign.
fn village_attitude(run: &RunProgress, village: usize) -> (u8, bool) {
    match run.tribal_campaign.village(village) {
        Some(state) => (
            match state.status {
                RivalStatus::Allied => 4,
                _ => attitude(state.relations) as u8,
            },
            state.status == RivalStatus::Conquered,
        ),
        None => (2, false),
    }
}

/// What the pointer is over: the retail noun, the village of a rival object
/// and the entity of a member / animal / bush.
struct Hit {
    target: Target,
    village: Option<usize>,
    entity: Option<Entity>,
}

/// What the pointer is over (`world` built like a right click).
fn hovered(order: Order, p: Vec3, w: &ClickWorld, social: bool) -> Hit {
    let hit = |target, village, entity| Hit { target, village, entity };
    match order {
        Order::Move(_) => hit(Target::Ground, None, None),
        Order::Gather(e) => {
            if w.eggs.contains(&e) {
                hit(Target::Egg, None, Some(e))
            } else if w.meat.iter().any(|(meat, _)| *meat == e) {
                hit(Target::Carcass, None, Some(e))
            } else {
                hit(Target::Fruit, None, Some(e))
            }
        }
        Order::Domesticate(e) => hit(Target::Wild { tameable: true }, None, Some(e)),
        Order::Fish(_) => hit(Target::FishSpot, None, None),
        Order::Hunt(e) => match w.rivals.iter().find(|(rival, _)| *rival == e) {
            Some(&(_, village)) => hit(Target::RivalMember, Some(village), Some(e)),
            None => hit(
                Target::Wild { tameable: w.grazers.iter().any(|(g, _)| *g == e) && social },
                None,
                Some(e),
            ),
        },
        Order::Siege(v) => hit(Target::RivalHut, Some(v), None),
        Order::Raid(v) => hit(Target::RivalFoodMat, Some(v), None),
        // A carried gift ends at the other tribe's food mat (never a click result).
        Order::Gift(v) => hit(Target::RivalFoodMat, Some(v), None),
        Order::Perform(v) => {
            // The social stance reports a food mat and a hut alike.
            if p.distance(tribal::village_food_mat(v)) < p.distance(tribal::village_position(v)) {
                hit(Target::RivalFoodMat, Some(v), None)
            } else {
                hit(Target::RivalHut, Some(v), None)
            }
        }
        Order::Fetch(kind) => hit(Target::OwnTool(kind), None, None),
        Order::Home => {
            if p.distance(tribal::food_pile_position()) <= p.distance(tribal::home_position()) {
                hit(Target::OwnFood, None, None)
            } else {
                hit(Target::OwnHut, None, None)
            }
        }
    }
}

/// One row of the world query the pointer test reads (the same objects a right
/// click resolves, `tribal_rts::tribal_input`).
type Row<'a> = (
    Entity,
    &'a Transform,
    Option<&'a Bush>,
    Option<&'a MeatPile>,
    Option<&'a WildAnimal>,
    Option<&'a RivalMember>,
    Option<&'a ToolShack>,
    Option<&'a FishingSpot>,
    Option<&'a rts::livestock::EggPile>,
);

type Objects<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        Option<&'static Bush>,
        Option<&'static MeatPile>,
        Option<&'static WildAnimal>,
        Option<&'static RivalMember>,
        Option<&'static ToolShack>,
        Option<&'static FishingSpot>,
        Option<&'static rts::livestock::EggPile>,
    ),
    Without<TribeUnit>,
>;

/// Everything a pointer at a world point can land on, gathered like a right
/// click (`tribal_rts::tribal_input`'s `world()` and `command_at`).
pub fn click_world<'a>(rows: impl Iterator<Item = Row<'a>>, run: &RunProgress) -> ClickWorld {
    let mut w = ClickWorld::default();
    let allied = |v: usize| run.tribal_campaign.village(v).is_some_and(|s| s.status == RivalStatus::Allied);
    for (e, t, bush, meat, animal, rival, shack, spot, egg) in rows {
        let at = t.translation;
        if let Some(b) = bush {
            w.bushes.push((e, at, b.food_left));
        }
        if meat.is_some() {
            w.meat.push((e, at));
        }
        if animal.is_some() || rival.is_some_and(|r| !allied(r.village)) {
            w.foes.push((e, at));
        }
        if let Some(r) = rival {
            w.rivals.push((e, r.village));
            if allied(r.village) {
                w.allied_members.push((e, at, r.village));
            } else if r.mode != rts::RivalMode::Wander {
                w.busy.push(e);
            }
        }
        if egg.is_some() {
            w.eggs.push(e);
        }
        if animal.is_some_and(|a| !a.hostile) {
            w.grazers.push((e, at));
        }
        if let Some(s) = shack {
            w.shacks.push((s.0, at));
        }
        if spot.is_some() {
            w.spots.push(at);
        }
    }
    prepare(&mut w, run);
    w
}

/// Fill the village lists of `w` from the campaign: the hostile villages with
/// their food mats (the raid targets) and the allied ones. A right click and
/// the cursor both call this, so they read the same world.
pub fn prepare(w: &mut ClickWorld, run: &RunProgress) {
    w.hostile_villages.clear();
    w.food_mats.clear();
    w.allied_villages.clear();
    for v in 0..tribal::RIVAL_VILLAGES {
        match run.tribal_campaign.village(v).map(|s| s.status) {
            Some(RivalStatus::Hostile) => {
                w.hostile_villages.push((v, tribal::village_position(v)));
                w.food_mats.push((v, tribal::village_food_mat(v)));
            }
            Some(RivalStatus::Allied) => {
                w.allied_villages.push((v, tribal::village_position(v), tribal::village_food_mat(v)));
            }
            _ => {}
        }
    }
}

/// The selection the retail rules look at: the chieftain is among the
/// selected members and which of them carry the healing tool.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub chieftain: bool,
    pub healers: Vec<Entity>,
}

/// What the pointer is over and what the retail input makes of it: the
/// `TribeInputAction` id of `ChooseInputActionID`, the noun it came from and
/// the OpenSpore order of the object (what `tribal_rts::resolve_click` made of
/// the point), which `super::click` turns into the click's command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hover {
    pub action: i32,
    pub target: Target,
    /// Village of a rival object.
    pub village: Option<usize>,
    /// The member / animal / bush under the pointer.
    pub entity: Option<Entity>,
    pub order: Order,
    pub point: Vec3,
    /// Position of the hovered own member (the heal order's goal).
    pub member_at: Option<Vec3>,
}

/// Radius within which an own member is the hovered object on open ground
/// (ADAPTED: retail picks the object under the 3D pointer; bushes and animals
/// win over a member standing on them, as `resolve_click` orders them).
const MEMBER_PICK_RADIUS: f32 = 0.9;

/// The `TribeInputAction` of the world point `p` and what is there (the Tribal
/// layout scope must be entered): the one place the cursor and the right click
/// read, so a right click does what the cursor shows.
pub fn hover_at(p: Vec3, w: &ClickWorld, run: &RunProgress, diet: Diet, selection: &Selection) -> Hover {
    let social = run.tribal_campaign.friendly_stance;
    // Members, huts and food mats of allied tribes are objects of the retail
    // rules too (attitude Friendly); `resolve_click` knows only the hostile
    // ones, so a view of the world adds the allied ones.
    let view;
    let seen: &ClickWorld = if w.allied_members.is_empty() && w.allied_villages.is_empty() {
        w
    } else {
        let mut merged = w.clone();
        for &(e, at, village) in &w.allied_members {
            merged.foes.push((e, at));
            if !merged.rivals.iter().any(|(r, _)| *r == e) {
                merged.rivals.push((e, village));
            }
        }
        for &(v, hut, mat) in &w.allied_villages {
            merged.hostile_villages.push((v, hut));
            merged.food_mats.push((v, mat));
        }
        view = merged;
        &view
    };
    let order = rts::resolve_click(p, seen, social);
    let mut hit = hovered(order, p, seen, social);
    let mut member_at = None;
    if hit.target == Target::Ground {
        // An own member on open ground, else the ground itself: over water
        // the members cannot go (ADAPTED continent test of 0xB886E0).
        let member = w
            .members
            .iter()
            .filter(|(_, at, _)| dist_xz(p, *at) <= MEMBER_PICK_RADIUS)
            .min_by(|a, b| dist_xz(p, a.1).total_cmp(&dist_xz(p, b.1)));
        if let Some(&(e, at, frac)) = member {
            hit = Hit { target: Target::OwnMember { injured: frac < 1.0 }, village: None, entity: Some(e) };
            member_at = Some(at);
        } else if crate::tribal_planet::current().in_water(Vec2::new(p.x, p.z)) {
            hit.target = Target::OtherContinent;
        }
    }
    let (attitude, defeated) = hit.village.map_or((2, false), |v| village_attitude(run, v));
    let context = Context {
        social,
        herbivore: !matches!(diet, Diet::Carnivore),
        carnivore: !matches!(diet, Diet::Herbivore),
        attitude,
        defeated,
        chieftain_selected: selection.chieftain,
        can_gift: hit.entity.is_none_or(|e| !w.busy.contains(&e)),
        healer_selected: selection.healers.iter().any(|h| Some(*h) != hit.entity),
        mat_has_food: hit.village.is_none_or(|v| run.tribal_campaign.village_food(v) > 0.0),
    };
    Hover {
        action: choose_action(hit.target, &context),
        target: hit.target,
        village: hit.village,
        entity: hit.entity,
        order,
        point: p,
        member_at,
    }
}

/// The `TribeInputAction` of the world point `p`: what a right click there
/// would do, in the retail ids.
pub fn action_at(p: Vec3, w: &ClickWorld, run: &RunProgress, diet: Diet, chieftain_selected: bool) -> i32 {
    let selection = Selection { chieftain: chieftain_selected, healers: Vec::new() };
    hover_at(p, w, run, diet, &selection).action
}

/// Set the window cursor from the object under the pointer. Runs every frame in
/// the Tribal stage; the pointer over the HUD, no pointer or no retail file
/// leaves the default arrow.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
    spui: Option<Res<crate::spui::view::SpuiPointer>>,
    run: Res<RunProgress>,
    hud: Res<TribalHudState>,
    active: Res<TribalHudActive>,
    objects: Objects,
    units: Query<(Entity, &Transform, &TribeUnit, Option<&Selected>, Option<&Health>, Option<&crate::persistence::WorldId>)>,
    originals: Res<crate::Originals>,
    mut images: ResMut<Assets<Image>>,
    mut hover: ResMut<HoverAction>,
    mut cache: Local<CursorCache>,
    mut shown: Local<Option<Cursor>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    let mut action = action::NONE;
    'pick: {
        let Some((_, window)) = windows.iter().next() else { break 'pick };
        let Some(pointer) = window.cursor_position() else { break 'pick };
        if !active.0 || hud.paused || spui.as_deref().is_some_and(|s| s.over_ui) {
            break 'pick;
        }
        let Ok((camera, camera_transform)) = cameras.single() else { break 'pick };
        let Ok(ray) = camera.viewport_to_world(camera_transform, pointer) else { break 'pick };
        let Some(p) = rts::ground_hit(ray, surface.as_deref()) else { break 'pick };
        let mut w = click_world(objects.iter(), &run);
        w.members = units
            .iter()
            .filter_map(|(e, t, _, _, health, _)| {
                let frac = health.map_or(1.0, |h| h.frac());
                (frac > 0.0).then_some((e, t.translation, frac))
            })
            .collect();
        let selection = Selection {
            chieftain: units.iter().any(|(_, _, u, s, _, _)| s.is_some() && u.chieftain),
            healers: units
                .iter()
                .filter(|(_, _, _, s, _, id)| {
                    s.is_some()
                        && id.and_then(|id| run.tribal_campaign.tool_of(id.0)) == Some(ToolKind::HealingRods)
                })
                .map(|(e, ..)| e)
                .collect(),
        };
        action = hover_at(p, &w, &run, hud.diet, &selection).action;
    }
    if hover.action != action {
        hover.action = action;
    }
    let want = cursor_for(action);
    if want == Cursor::Keep || *shown == Some(want) {
        return;
    }
    let icon = match want {
        Cursor::Default | Cursor::Keep => CursorIcon::default(),
        Cursor::File(name) => {
            let entry = cache.images.entry(name).or_insert_with(|| {
                let bytes = originals.resource_index.read(cursor_key(name)).ok()?;
                let cur = decode_cursor(&bytes)?;
                let image = Image::new(
                    Extent3d { width: cur.width, height: cur.height, depth_or_array_layers: 1 },
                    TextureDimension::D2,
                    cur.rgba,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                );
                Some((images.add(image), cur.hotspot))
            });
            match entry {
                Some((handle, hotspot)) => CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
                    handle: handle.clone(),
                    hotspot: *hotspot,
                    ..default()
                })),
                None => CursorIcon::default(),
            }
        }
    };
    for (window, _) in &windows {
        commands.entity(window).insert(icon.clone());
    }
    *shown = Some(want);
}

/// Back to the default arrow on leaving the Tribal stage.
pub fn reset(
    mut commands: Commands,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut hover: ResMut<HoverAction>,
) {
    hover.action = 0;
    for window in &windows {
        commands.entity(window).insert(CursorIcon::default());
    }
}

pub struct TribalCursorPlugin;

impl Plugin for TribalCursorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HoverAction>()
            .add_systems(OnExit(GameStage::Tribal), reset)
            .add_systems(
                Update,
                update
                    .in_set(crate::GameSystems::Controls)
                    .run_if(in_state(GameStage::Tribal)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::action::*;

    fn ctx() -> Context {
        Context::default()
    }

    #[test]
    fn tool_types_map_to_the_pickup_ids_of_the_jump_table_0xccfd6c() {
        // Types 1..=10 (torch axe spear horn maraca didgeridoo gather fish heal fire).
        let ids = [
            ToolKind::SpikedClubs,
            ToolKind::StoneAxes,
            ToolKind::ThrowingSpears,
            ToolKind::WoodenHorns,
            ToolKind::Maracas,
            ToolKind::Aerophones,
            ToolKind::FruitSticks,
            ToolKind::FishingSpears,
            ToolKind::HealingRods,
            ToolKind::Campfire,
        ]
        .map(tool_action);
        assert_eq!(ids, [0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x12, 0x11, 0x19, 0x1A]);
        // The ModAPI's names agree for the attack, social, heal and fire tools.
        assert_eq!(crate::tribal_controls::tribe_input_action(ToolKind::HealingRods), TOOL_HEAL);
        assert_eq!(crate::tribal_controls::tribe_input_action(ToolKind::Campfire), TOOL_FIREPIT);
        assert_eq!(crate::tribal_controls::tribe_input_action(ToolKind::SpikedClubs), TOOL_ATTACK1);
    }

    #[test]
    fn the_cursor_table_follows_the_switch_at_0xcd0320() {
        assert_eq!(cursor_for(NONE), Cursor::Default);
        assert_eq!(cursor_for(NONE_RESET), Cursor::Default);
        assert_eq!(cursor_for(KEEP), Cursor::Keep);
        assert_eq!(cursor_for(3), Cursor::File("cursor-ban-mode"));
        assert_eq!(cursor_for(5), Cursor::File("cursor-ban-mode"));
        assert_eq!(cursor_for(4), Cursor::File("cursor-ban-content"));
        assert_eq!(cursor_for(6), Cursor::File("cursor_spyglass"));
        assert_eq!(cursor_for(7), Cursor::File("cursor-no-opt"));
        assert_eq!(cursor_for(9), Cursor::File("cursor_repair"));
        for id in [0xA, 0xB, 0xC] {
            assert_eq!(cursor_for(id), Cursor::File("cursor_atk"));
        }
        assert_eq!(cursor_for(0xD), Cursor::File("cursor-crg-give-grn-no"));
        for id in [0xE, 0x10] {
            assert_eq!(cursor_for(id), Cursor::File("cursor-crg-give-grn"));
        }
        assert_eq!(cursor_for(0xF), Cursor::Default);
        assert_eq!(cursor_for(0x1A), Cursor::File("cursor-dance"));
        for id in [0x1B, 0x1C, 0x1D, 0x23, 0x24, 0x25] {
            assert_eq!(cursor_for(id), Cursor::File("cursor-gather"));
        }
        assert_eq!(cursor_for(0x1E), Cursor::File("cursor-eat-fruit"));
        assert_eq!(cursor_for(0x1F), Cursor::File("cursor-eat-carcass"));
        assert_eq!(cursor_for(0x20), Cursor::File("cursor-eat-omni"));
        assert_eq!(cursor_for(0x21), Cursor::File("cursor-steal"));
        assert_eq!(cursor_for(0x22), Cursor::File("cursor-fish"));
        assert_eq!(cursor_for(0x26), Cursor::Default);
        for id in [0x27, 0x2B, 0x2D] {
            assert_eq!(cursor_for(id), Cursor::File("cursor_social"));
        }
        assert_eq!(cursor_for(0x28), Cursor::File("cursor-heal"));
        assert_eq!(cursor_for(0x29), Cursor::File("cursor-trg-domesticate"));
        for id in [0x2A, 0x2C, 0x2E] {
            assert_eq!(cursor_for(id), Cursor::Default);
        }
        assert_eq!(cursor_for(0x2F), Cursor::File("cursor-grab_open"));
    }

    #[test]
    fn own_objects_show_pickup_eat_and_hut_cursors() {
        assert_eq!(choose_action(Target::OwnTool(ToolKind::StoneAxes), &ctx()), TOOL_ATTACK2);
        assert_eq!(choose_action(Target::OwnHut, &ctx()), HUD);
        let diet = |herbivore, carnivore| Context { herbivore, carnivore, ..ctx() };
        assert_eq!(choose_action(Target::OwnFood, &diet(true, false)), EAT_FRUIT);
        assert_eq!(choose_action(Target::OwnFood, &diet(false, true)), EAT_MEAT);
        assert_eq!(choose_action(Target::OwnFood, &diet(true, true)), EAT_BOTH);
        assert_eq!(choose_action(Target::Ground, &ctx()), NONE);
    }

    #[test]
    fn gathering_follows_the_diet_and_the_stance() {
        let herbivore = Context { herbivore: true, carnivore: false, ..ctx() };
        let carnivore = Context { herbivore: false, carnivore: true, ..ctx() };
        assert_eq!(choose_action(Target::Fruit, &herbivore), GATHER_FRUIT);
        assert_eq!(choose_action(Target::Fruit, &carnivore), NONE);
        assert_eq!(choose_action(Target::Carcass, &carnivore), GATHER_MEAT);
        assert_eq!(choose_action(Target::Carcass, &herbivore), NONE);
        // Eggs are gathered with the attack stance only.
        assert_eq!(choose_action(Target::Egg, &Context { social: false, ..ctx() }), GATHER_EGG);
        assert_eq!(choose_action(Target::Egg, &Context { social: true, ..ctx() }), NONE);
        assert_eq!(choose_action(Target::FishSpot, &ctx()), FISH);
    }

    #[test]
    fn animals_are_attacked_or_domesticated_by_the_stance() {
        let attack = Context { social: false, ..ctx() };
        let social = Context { social: true, ..ctx() };
        assert_eq!(choose_action(Target::Wild { tameable: true }, &attack), ATTACK_ANIMAL);
        assert_eq!(choose_action(Target::Wild { tameable: true }, &social), DOMESTICATE);
        assert_eq!(choose_action(Target::Wild { tameable: false }, &social), NONE);
    }

    #[test]
    fn rival_tribes_follow_the_attitude_and_the_stance() {
        let social = |attitude, chief, gift| Context {
            social: true,
            attitude,
            chieftain_selected: chief,
            can_gift: gift,
            ..ctx()
        };
        let attack = |attitude| Context { social: false, attitude, ..ctx() };
        // Social stance: gifts below Cautious, the social cursor from Cautious.
        assert_eq!(choose_action(Target::RivalMember, &social(0, false, true)), GIFT_MEMBER);
        assert_eq!(choose_action(Target::RivalMember, &social(1, true, false)), GIFT_DENIED);
        assert_eq!(choose_action(Target::RivalMember, &social(2, true, true)), SOCIAL_MEMBER);
        assert_eq!(choose_action(Target::RivalMember, &social(3, false, true)), SOCIAL_MEMBER_NO_CHIEF);
        assert_eq!(choose_action(Target::RivalHut, &social(1, true, true)), GIFT_TRIBE);
        assert_eq!(choose_action(Target::RivalHut, &social(2, true, true)), NONE_TRIBE);
        assert_eq!(choose_action(Target::RivalFoodMat, &social(0, true, true)), GIFT_TRIBE);
        // Attack stance: attack unless Friendly; steal food below Friendly.
        assert_eq!(choose_action(Target::RivalMember, &attack(0)), ATTACK_TOOL);
        assert_eq!(choose_action(Target::RivalMember, &attack(4)), DEFAULT_ROLLOVER);
        // A hut (and a rack) is raided: 0xC, not the member's 0xA.
        assert_eq!(choose_action(Target::RivalHut, &attack(3)), ATTACK_OTHER);
        assert_eq!(choose_action(Target::RivalHut, &attack(4)), DEFAULT_ROLLOVER);
        assert_eq!(choose_action(Target::RivalFoodMat, &attack(2)), STEAL);
        assert_eq!(choose_action(Target::RivalFoodMat, &attack(4)), NONE);
        // A tribe that is gone is ignored.
        let gone = Context { defeated: true, ..attack(0) };
        assert_eq!(choose_action(Target::RivalHut, &gone), NONE);
    }

    #[test]
    fn the_extra_rules_of_the_retail_code_are_in() {
        // Own members: injured with a healer selected 0x28, else the plain arrow.
        let injured = Target::OwnMember { injured: true };
        let healthy = Target::OwnMember { injured: false };
        let healer = Context { healer_selected: true, ..ctx() };
        assert_eq!(choose_action(injured, &healer), HEAL_MEMBER);
        assert_eq!(choose_action(injured, &ctx()), DEFAULT_ROLLOVER_3);
        assert_eq!(choose_action(healthy, &healer), DEFAULT_ROLLOVER_3);
        // Ground the members cannot reach.
        assert_eq!(choose_action(Target::OtherContinent, &ctx()), UNREACHABLE);
        // An empty food mat of another tribe is nothing; a member busy in a
        // fight takes no gift.
        let empty = Context { mat_has_food: false, attitude: 1, social: true, ..ctx() };
        assert_eq!(choose_action(Target::RivalFoodMat, &empty), NONE);
        let busy = Context { can_gift: false, attitude: 0, social: true, ..ctx() };
        assert_eq!(choose_action(Target::RivalMember, &busy), GIFT_DENIED);
    }

    #[test]
    fn hover_at_reads_members_allies_and_the_diet_like_the_click_does() {
        let mut ecs = World::new();
        let member = ecs.spawn_empty().id();
        let healer = ecs.spawn_empty().id();
        let bush = ecs.spawn_empty().id();
        let mut run = RunProgress::default();
        run.tribal_campaign.ensure_ready();
        run.tribal_campaign.friendly_stance = false;
        let mut w = ClickWorld {
            members: vec![(member, Vec3::new(300.0, 0.0, 300.0), 0.4)],
            bushes: vec![(bush, Vec3::new(-300.0, 0.0, 300.0), 50.0)],
            ..default()
        };
        prepare(&mut w, &run);
        let sel = |healers: Vec<Entity>| Selection { chieftain: false, healers };
        let near = Vec3::new(300.2, 0.0, 300.0);
        // An injured member with another member carrying the healing tool.
        let h = hover_at(near, &w, &run, Diet::Omnivore, &sel(vec![healer]));
        assert_eq!((h.action, h.entity, h.member_at), (HEAL_MEMBER, Some(member), Some(Vec3::new(300.0, 0.0, 300.0))));
        // The member as its own healer does not count (0xCCF9ED).
        assert_eq!(hover_at(near, &w, &run, Diet::Omnivore, &sel(vec![member])).action, DEFAULT_ROLLOVER_3);
        // The bush: gathered by a herbivore, nothing for a carnivore (the diet gate).
        let bush_at = Vec3::new(-300.0, 0.0, 300.0);
        assert_eq!(hover_at(bush_at, &w, &run, Diet::Herbivore, &sel(vec![])).action, GATHER_FRUIT);
        assert_eq!(hover_at(bush_at, &w, &run, Diet::Omnivore, &sel(vec![])).action, GATHER_FRUIT);
        assert_eq!(hover_at(bush_at, &w, &run, Diet::Carnivore, &sel(vec![])).action, NONE);
        // An allied village: its hut shows the plain arrow in the attack stance
        // (0x2A) and the wrong-target one in the social stance (0xF).
        run.tribal_campaign.villages[0].appeared = true;
        run.tribal_campaign.villages[0].status = RivalStatus::Allied;
        prepare(&mut w, &run);
        assert_eq!(w.allied_villages.len(), 1);
        let hut = tribal::village_position(0);
        assert_eq!(hover_at(hut, &w, &run, Diet::Omnivore, &sel(vec![])).action, DEFAULT_ROLLOVER);
        run.tribal_campaign.friendly_stance = true;
        assert_eq!(hover_at(hut, &w, &run, Diet::Omnivore, &sel(vec![])).action, NONE_TRIBE);
        // An allied member in the social stance: the social cursor.
        let ally = ecs.spawn_empty().id();
        w.allied_members.push((ally, hut + Vec3::new(8.0, 0.0, 0.0), 0));
        let at = hut + Vec3::new(8.0, 0.0, 0.0);
        assert_eq!(hover_at(at, &w, &run, Diet::Omnivore, &Selection { chieftain: true, healers: vec![] }).action, SOCIAL_MEMBER);
        assert_eq!(hover_at(at, &w, &run, Diet::Omnivore, &sel(vec![])).action, SOCIAL_MEMBER_NO_CHIEF);
    }

    /// Build a 32-bit cursor file of `size` x `size` pixels by hand.
    fn cursor_file(size: u32, hotspot: (u16, u16), pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mask_stride = size.div_ceil(32) * 4;
        let image = 40 + size * size * 4 + mask_stride * size;
        let mut out = vec![0, 0, 2, 0, 1, 0, size as u8, size as u8, 0, 0];
        out.extend(hotspot.0.to_le_bytes());
        out.extend(hotspot.1.to_le_bytes());
        out.extend(image.to_le_bytes());
        out.extend(22u32.to_le_bytes());
        out.extend(40u32.to_le_bytes());
        out.extend(size.to_le_bytes());
        out.extend((size * 2).to_le_bytes());
        out.extend([1, 0, 32, 0]);
        out.extend([0; 24]);
        for y in (0..size).rev() {
            for x in 0..size {
                let [r, g, b, a] = pixel(x, y);
                out.extend([b, g, r, a]);
            }
        }
        out.extend(vec![0u8; (mask_stride * size) as usize]);
        out
    }

    #[test]
    fn a_32_bit_cursor_decodes_top_down_with_its_hotspot() {
        let bytes = cursor_file(4, (1, 2), |x, y| [x as u8 * 10, y as u8 * 10, 7, if y == 0 { 255 } else { 128 }]);
        let cur = decode_cursor(&bytes).expect("decodes");
        assert_eq!((cur.width, cur.height, cur.hotspot), (4, 4, (1, 2)));
        // The file stores the bottom row first; the decoded image starts at the top.
        assert_eq!(&cur.rgba[0..4], &[0, 0, 7, 255]);
        assert_eq!(&cur.rgba[(3 * 4 + 2) * 4..(3 * 4 + 2) * 4 + 4], &[20, 30, 7, 128]);
        assert!(decode_cursor(&bytes[..30]).is_none());
        assert!(decode_cursor(&[0u8; 4]).is_none());
    }

    #[test]
    fn a_cursor_without_alpha_is_cut_by_its_mask() {
        let mut bytes = cursor_file(2, (0, 0), |_, _| [200, 100, 50, 0]);
        // Mask rows (bottom-up, 4 bytes each): bottom row opaque, top row transparent.
        let n = bytes.len();
        bytes[n - 8..n - 4].copy_from_slice(&[0, 0, 0, 0]);
        bytes[n - 4..].copy_from_slice(&[0xC0, 0, 0, 0]);
        let cur = decode_cursor(&bytes).expect("decodes");
        assert_eq!(cur.rgba[3], 0, "top row transparent");
        assert_eq!(cur.rgba[2 * 4 + 3], 255, "bottom row opaque");
    }

    #[test]
    fn the_real_tribal_scene_names_what_the_pointer_is_over_in_the_retail_ids() {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<RunProgress>().enter_stage(GameStage::Tribal);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Tribal);
        for _ in 0..3 {
            app.update();
        }
        let mut query = app.world_mut().query_filtered::<Row, Without<TribeUnit>>();
        let _layout = crate::tribal_planet::scope_world(app.world());
        let (mut run, w) = {
            let world = app.world();
            let run = world.resource::<RunProgress>().clone();
            let w = click_world(query.iter(world), &run);
            (run, w)
        };
        let at = |run: &RunProgress, w: &ClickWorld, p: Vec3| action_at(p, w, run, Diet::Omnivore, false);
        // Home: the hut shows its card, the food pile is eaten by diet.
        assert_eq!(at(&run, &w, tribal::home_position()), HUD);
        assert_eq!(at(&run, &w, tribal::food_pile_position()), EAT_BOTH);
        assert_eq!(action_at(tribal::food_pile_position(), &w, &run, Diet::Herbivore, false), EAT_FRUIT);
        assert_eq!(action_at(tribal::food_pile_position(), &w, &run, Diet::Carnivore, false), EAT_MEAT);
        // Far from everything: the plain arrow (the red X over water).
        let far = Vec3::new(9000.0, 0.0, 9000.0);
        let over_water = crate::tribal_planet::current().in_water(Vec2::new(far.x, far.z));
        assert_eq!(at(&run, &w, far), if over_water { UNREACHABLE } else { NONE });
        // A built tool rack offers its pick-up cursor.
        if let Some(&(kind, p)) = w.shacks.first() {
            assert_eq!(at(&run, &w, p), tool_action(kind));
        }
        // A food bush is gathered with a herbivorous mouth only.
        if let Some(&(bush, p, _)) = w.bushes.iter().find(|(e, ..)| !w.eggs.contains(e)) {
            let _ = bush;
            let nearer = w.foes.iter().any(|(_, q)| q.distance(p) < 1.8) || w.meat.iter().any(|(_, q)| q.distance(p) < 1.6);
            if !nearer {
                assert_eq!(at(&run, &w, p), GATHER_FRUIT);
                assert_eq!(action_at(p, &w, &run, Diet::Carnivore, false), NONE);
            }
        }
        // A hostile tribe's hut and food mat, by stance and attitude.
        let hut = tribal::village_position(0);
        let mat = tribal::village_food_mat(0);
        let set = |run: &mut RunProgress, social: bool, relations: f32| {
            run.tribal_campaign.friendly_stance = social;
            let state = run.tribal_campaign.villages.get_mut(0).expect("village 0");
            state.appeared = true;
            state.status = RivalStatus::Hostile;
            state.relations = relations;
        };
        set(&mut run, false, 0.0);
        let w = {
            let world = app.world();
            click_world(query.iter(world), &run)
        };
        assert_eq!(at(&run, &w, hut), ATTACK_OTHER, "attack stance: the raid cursor over a hut");
        assert_eq!(at(&run, &w, mat), STEAL, "attack stance: steal food");
        set(&mut run, true, 0.0);
        assert_eq!(at(&run, &w, hut), NONE_TRIBE, "social stance, Cautious: the plain arrow");
        set(&mut run, true, -50.0);
        assert_eq!(at(&run, &w, hut), GIFT_TRIBE, "social stance, Annoyed: offer a gift");
        assert_eq!(at(&run, &w, mat), GIFT_TRIBE);
    }

    fn installed() -> Option<crate::asset_catalog::ResourceIndex> {
        let install = crate::spore_install::discover().ok()?;
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then_some(index)
    }

    #[test]
    fn every_cursor_the_table_names_is_an_installed_32x32_file() {
        let Some(index) = installed() else { return };
        let mut names: Vec<&str> = (0..=0x2F)
            .filter_map(|id| match cursor_for(id) {
                Cursor::File(name) => Some(name),
                _ => None,
            })
            .collect();
        names.sort_unstable();
        names.dedup();
        assert!(names.len() >= 25);
        for name in names {
            // "cursor-camera-translate" etc. are in the install under the same names.
            let bytes = index.read(cursor_key(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
            let cur = decode_cursor(&bytes).unwrap_or_else(|| panic!("{name} does not decode"));
            assert_eq!((cur.width, cur.height), (32, 32), "{name}");
            assert!(cur.rgba.chunks(4).any(|p| p[3] > 0), "{name} is blank");
            assert!(cur.hotspot.0 < 32 && cur.hotspot.1 < 32, "{name}");
        }
    }
}
